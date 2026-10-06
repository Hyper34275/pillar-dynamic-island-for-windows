//! Classic Outlook (COM) provider. Attach-only, read-only, default calendar only.
//!
//! Discovery (cheap, no COM) looks for Outlook processes that belong to *our* Windows
//! session and *our* user before anything is attached, so another user's Outlook on the
//! same machine is never touched and a missing Outlook costs one process-list read.
//! Attaching uses `GetActiveObject` (the running object table): nothing is ever launched,
//! no profile is selected and `Logon` is never called.
//!
//! Every read is a full attach -> read -> release cycle. Holding Outlook object references
//! between syncs would keep OUTLOOK.EXE alive after the user closes it.
//!
//! Per item only EntryID (hashed), Subject, Start, End, Location, Organizer (display
//! name), AllDayEvent, IsRecurring, BusyStatus, ResponseStatus and Categories (names, only
//! to look up their color) are read. Never Body, Recipients, Attachments,
//! SenderEmailAddress, UserProperties or GetOrganizer. The master category list is read for
//! names and colors only.
//!
//! When meeting invitations are on, the default Inbox is filtered (no date in the filter) to
//! unread meeting requests; of each, only EntryID (hashed), Subject, ReceivedTime and, of
//! the tentative calendar entry Outlook made for it, Start, End, Location and Organizer.
//!
//! The one thing that is not a read is [`open_calendar`]: on a click in the island it shows
//! the user's own (running) Outlook on its calendar.

use crate::calendar::{
    BusyStatus, CalendarEventDto, CalendarSource, ErrKind, FetchResult, FetchWindow, Fetched, MeetingInviteDto, ResponseStatus,
    SourceError, MAX_INVITES,
};
use crate::com::{self, ComApartment, ComError, ComResult, Dispatch, MessageFilterGuard};
use crate::debug_log::hash_id;
use chrono::{DateTime, Local, NaiveDateTime, Timelike, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, SYSTEMTIME};
use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx, DATE_SHORTDATE, TIME_NOSECONDS};
use windows::Win32::Security::{
    GetLengthSid, GetTokenInformation, TokenElevation, TokenUser, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, SetEvent, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG,
    MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT,
};

const PROG_ID: &str = "Outlook.Application";
const OL_FOLDER_CALENDAR: i32 = 9;
const OL_FOLDER_INBOX: i32 = 6;
const MAX_SCANNED: usize = 500;
const MAX_TEXT_CHARS: usize = 200;
const MAX_LOCATION_CHARS: usize = 4_096;
/// Far above any real master category list; bounds the read.
const MAX_CATEGORIES: i32 = 200;
/// The category colors are re-read this often, not on every sync.
const CATEGORY_REFRESH_SECS: u64 = 300;
const MEETING_REQUEST_CLASS: &str = "IPM.Schedule.Meeting.Request";
/// Unread requests looked at per sync (newest first).
const MAX_INVITES_SCANNED: usize = 30;
/// An unread request older than this is not news any more.
const INVITE_MAX_AGE_HOURS: i64 = 72;

// =============================================================================
// Process discovery
// =============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub session: u32,
    pub sid: Vec<u8>,
    pub elevated: bool,
}

#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub session: u32,
    /// `None` when the token could not be opened (typically a higher-integrity process).
    pub sid: Option<Vec<u8>>,
    pub elevated: Option<bool>,
    pub exe: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discovery {
    /// No Outlook of ours is running (normal; informational).
    Waiting,
    /// Only the "new" Outlook (olk.exe) runs: no COM automation.
    NewOutlookOnly,
    /// Classic Outlook runs at a different integrity level than we do.
    ElevationMismatch,
    /// Classic Outlook runs in our session, as our user, at our elevation. Carries its
    /// process id so a restarted Outlook is told apart from the one that failed before.
    Classic(u32),
}

fn is_exe(p: &ProcInfo, name: &str) -> bool {
    p.exe.eq_ignore_ascii_case(name)
}

/// Pure classification of the process list. Only processes in our session count; a
/// process that runs as someone else is ignored, one we cannot inspect is treated as
/// an elevation problem (we could not attach to it either way).
pub fn classify(me: &Identity, procs: &[ProcInfo]) -> Discovery {
    let ours = || procs.iter().filter(|p| p.session == me.session);
    let same_user = |p: &ProcInfo| p.sid.as_deref() == Some(me.sid.as_slice());

    let classic: Vec<&ProcInfo> = ours().filter(|p| is_exe(p, "outlook.exe")).collect();
    if let Some(p) = classic.iter().filter(|p| same_user(p) && p.elevated == Some(me.elevated)).min_by_key(|p| p.pid) {
        return Discovery::Classic(p.pid);
    }
    if classic.iter().any(|p| p.sid.is_none() || (same_user(p) && p.elevated != Some(me.elevated))) {
        return Discovery::ElevationMismatch;
    }
    if ours().any(|p| is_exe(p, "olk.exe") && same_user(p)) {
        return Discovery::NewOutlookOnly;
    }
    Discovery::Waiting
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// SID bytes and elevation of the user a token belongs to.
fn token_identity(token: HANDLE) -> Option<(Vec<u8>, bool)> {
    unsafe {
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        if len == 0 {
            return None;
        }
        // u64 storage keeps the TOKEN_USER buffer pointer-aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), len, &mut len).ok()?;
        let sid = (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid;
        if sid.0.is_null() {
            return None;
        }
        let sid_len = GetLengthSid(sid) as usize;
        let sid_bytes = std::slice::from_raw_parts(sid.0 as *const u8, sid_len).to_vec();

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .ok()?;
        Some((sid_bytes, elevation.TokenIsElevated != 0))
    }
}

fn process_token_identity(process: HANDLE) -> Option<(Vec<u8>, bool)> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let token = OwnedHandle(token);
        token_identity(token.0)
    }
}

pub fn current_identity() -> Result<Identity, String> {
    unsafe {
        let mut session = 0u32;
        ProcessIdToSessionId(GetCurrentProcessId(), &mut session).map_err(|e| format!("session id: {e}"))?;
        let (sid, elevated) =
            process_token_identity(GetCurrentProcess()).ok_or_else(|| "own token is unreadable".to_string())?;
        Ok(Identity { session, sid, elevated })
    }
}

fn inspect(pid: u32, exe: String) -> ProcInfo {
    let mut session = u32::MAX;
    unsafe {
        let _ = ProcessIdToSessionId(pid, &mut session);
    }
    let identity = unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .ok()
            .map(OwnedHandle)
            .and_then(|h| process_token_identity(h.0))
    };
    ProcInfo { pid, session, sid: identity.as_ref().map(|i| i.0.clone()), elevated: identity.map(|i| i.1), exe }
}

/// Outlook-related processes on the machine (any session), inspected.
fn enumerate_outlook_processes() -> Result<Vec<ProcInfo>, String> {
    let mut found = Vec::new();
    unsafe {
        let snapshot = OwnedHandle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| format!("snapshot: {e}"))?);
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot.0, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if exe.eq_ignore_ascii_case("outlook.exe") || exe.eq_ignore_ascii_case("olk.exe") {
                found.push(inspect(entry.th32ProcessID, exe));
            }
            more = Process32NextW(snapshot.0, &mut entry).is_ok();
        }
    }
    Ok(found)
}

pub fn discover() -> Result<Discovery, String> {
    let me = current_identity()?;
    Ok(classify(&me, &enumerate_outlook_processes()?))
}

// =============================================================================
// Pure helpers: hashing, URL, enum mapping, Restrict filter
// =============================================================================

fn hash16(input: &str) -> String {
    Sha256::digest(input.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Stable event id: first 16 hex of sha256(EntryID + "|" + startUtc). The raw EntryID
/// never leaves this module.
pub fn event_id(entry_id: &str, start_utc: &DateTime<Utc>) -> String {
    hash16(&format!("{}|{}", entry_id, start_utc.format("%Y-%m-%dT%H:%M:%SZ")))
}

/// Join links recognised in a Location. Teams (work, personal, government clouds), Skype for
/// Business / Lync Online (`meet.lync.com`) and Skype (`join.skype.com`), Cisco Webex (which Jabber
/// meetings run on) and Webex Teams, Zoom, Google Meet.
const MEETING_HOSTS: [&str; 10] = [
    "teams.microsoft.com",
    "teams.live.com",
    "teams.microsoft.us",
    "lync.com",
    "join.skype.com",
    "webex.com",
    "ciscospark.com",
    "zoom.us",
    "zoomgov.com",
    "meet.google.com",
];
const MAX_URL_LEN: usize = 2_048;

/// Where the meeting add-ins put the join link (MAPI named properties in PS_PUBLIC_STRINGS):
/// Teams, then Skype for Business / Lync (whose on-premises link is on the company's own host).
/// Teams and Skype meetings carry their link in the body, which is never read; these are not.
const ONLINE_MEETING_PROPS: [&str; 2] = [
    "http://schemas.microsoft.com/mapi/string/{00020329-0000-0000-C000-000000000046}/SkypeTeamsMeetingUrl",
    "http://schemas.microsoft.com/mapi/string/{00020329-0000-0000-C000-000000000046}/OnlineMeetingExternalLink",
];

/// A join link taken from a meeting property: one plain https URL, any host, no credentials,
/// nothing that could break out of a command line.
pub fn meeting_link(value: &str) -> Option<String> {
    let url = value.trim();
    if url.len() > MAX_URL_LEN || !url.get(..8).is_some_and(|s| s.eq_ignore_ascii_case("https://")) {
        return None;
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '<' | '>' | '`')) {
        return None;
    }
    let authority_end = url[8..].find(['/', '?', '#']).map_or(url.len(), |i| 8 + i);
    let authority = &url[8..authority_end];
    (!authority.is_empty() && !authority.contains('@')).then(|| url.to_string())
}

fn is_meeting_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    MEETING_HOSTS.iter().any(|d| host == *d || host.strip_suffix(d).is_some_and(|rest| rest.ends_with('.')))
}

/// First http(s) URL in `location` whose host is a known meeting provider.
pub fn find_meeting_url(location: &str) -> Option<String> {
    let lower = location.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("http") {
        let start = from + pos;
        from = start + 4;
        let rest = &location[start..];
        let scheme_len = if lower[start..].starts_with("https://") {
            8
        } else if lower[start..].starts_with("http://") {
            7
        } else {
            continue;
        };
        let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']')).unwrap_or(rest.len());
        let url = rest[..end].trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'));
        let authority_end = url[scheme_len..].find(['/', '?', '#']).map_or(url.len(), |i| scheme_len + i);
        let authority = &url[scheme_len..authority_end];
        if authority.contains('@') || url.len() > MAX_URL_LEN {
            continue;
        }
        let host = authority.split(':').next().unwrap_or("");
        if is_meeting_host(host) {
            return Some(url.to_string());
        }
    }
    None
}

fn busy_status(v: i32) -> BusyStatus {
    match v {
        0 => BusyStatus::Free,
        1 => BusyStatus::Tentative,
        3 => BusyStatus::Oof,
        4 => BusyStatus::WorkingElsewhere,
        _ => BusyStatus::Busy,
    }
}

fn response_status(v: i32) -> ResponseStatus {
    match v {
        1 => ResponseStatus::Organized,
        2 => ResponseStatus::Tentative,
        3 => ResponseStatus::Accepted,
        4 => ResponseStatus::Declined,
        5 => ResponseStatus::NotResponded,
        _ => ResponseStatus::None,
    }
}

fn clip(s: &str) -> String {
    s.chars().take(MAX_TEXT_CHARS).collect()
}

fn non_empty(s: Option<String>, max_chars: usize) -> Option<String> {
    s.map(|s| s.trim().chars().take(max_chars).collect::<String>()).filter(|s| !s.is_empty())
}

/// `OlCategoryColor` 1..=25 -> `#RRGGBB`, close to how Outlook paints each category and
/// readable on the island's black (Outlook's own "Black" is lifted to a dark gray).
/// 0 (`olCategoryColorNone`) and unknown values have no color.
pub fn category_color(ol_color: i32) -> Option<&'static str> {
    const COLORS: [&str; 25] = [
        "#E7484F", // 1 red
        "#F7881D", // 2 orange
        "#F2B661", // 3 peach
        "#F9D33C", // 4 yellow
        "#5FBE7D", // 5 green
        "#4BC1B5", // 6 teal
        "#A5B85A", // 7 olive
        "#4A9BE8", // 8 blue
        "#8C6CE0", // 9 purple
        "#C94F7C", // 10 maroon
        "#9EB0C6", // 11 steel
        "#6B7F99", // 12 dark steel
        "#ABABAB", // 13 gray
        "#7A7A7A", // 14 dark gray
        "#4A4A4A", // 15 black
        "#B0262D", // 16 dark red
        "#C25E10", // 17 dark orange
        "#A97A2B", // 18 dark peach
        "#B39A16", // 19 dark yellow
        "#2E7D47", // 20 dark green
        "#23807A", // 21 dark teal
        "#6D7B33", // 22 dark olive
        "#2B5FA8", // 23 dark blue
        "#5B3FA6", // 24 dark purple
        "#8E2F56", // 25 dark maroon
    ];
    usize::try_from(ol_color).ok()?.checked_sub(1).and_then(|i| COLORS.get(i)).copied()
}

/// Category name (trimmed, lowercased) -> its color.
type CategoryColors = HashMap<String, &'static str>;

/// The color of the first of an item's categories that has one. Outlook joins the names with
/// the list separator ("," or ";" depending on the locale); names can contain neither.
pub fn first_category_color(categories: &str, colors: &CategoryColors) -> Option<String> {
    categories
        .split([',', ';'])
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .find_map(|name| colors.get(&name).map(|c| c.to_string()))
}

/// What is read from one appointment.
struct RawItem {
    entry_id: String,
    subject: String,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    location: Option<String>,
    organizer: Option<String>,
    all_day: bool,
    recurring: bool,
    busy: i32,
    response: i32,
    /// Already resolved from the item's categories.
    color: Option<String>,
    /// From a meeting add-in's property; only looked up when the Location has no join link.
    online_link: Option<String>,
}

fn build_event(raw: RawItem, calendar_id: &str) -> CalendarEventDto {
    let id_seed = if raw.entry_id.is_empty() { format!("noid:{}", raw.subject) } else { raw.entry_id };
    let meeting_url = raw.location.as_deref().and_then(find_meeting_url).or(raw.online_link);
    CalendarEventDto {
        id: event_id(&id_seed, &raw.start),
        calendar_id: calendar_id.to_string(),
        subject: clip(&raw.subject),
        start_utc: raw.start,
        end_utc: raw.end,
        all_day: raw.all_day,
        location: raw.location.as_deref().map(clip),
        organizer: raw.organizer,
        is_recurring: raw.recurring,
        meeting_url,
        busy_status: busy_status(raw.busy),
        response_status: response_status(raw.response),
        color: raw.color,
    }
}

/// Jet filter for "still running or upcoming within the window". `fmt` renders a local
/// civil time the way Outlook will parse it.
pub fn restrict_filter(
    from_local: NaiveDateTime,
    to_local: NaiveDateTime,
    fmt: impl Fn(&NaiveDateTime) -> Option<String>,
) -> Option<String> {
    Some(format!("[End] >= '{}' AND [Start] <= '{}'", fmt(&from_local)?, fmt(&to_local)?))
}

/// Outlook parses Restrict dates with the user's regional settings.
fn locale_format(dt: &NaiveDateTime) -> Option<String> {
    use chrono::{Datelike, Timelike};
    let st = SYSTEMTIME {
        wYear: dt.year() as u16,
        wMonth: dt.month() as u16,
        wDay: dt.day() as u16,
        wHour: dt.hour() as u16,
        wMinute: dt.minute() as u16,
        ..Default::default()
    };
    let mut date = [0u16; 80];
    let mut time = [0u16; 80];
    let (dn, tn) = unsafe {
        (
            GetDateFormatEx(PCWSTR::null(), DATE_SHORTDATE, Some(&st), PCWSTR::null(), Some(&mut date), PCWSTR::null()),
            GetTimeFormatEx(PCWSTR::null(), TIME_NOSECONDS, Some(&st), PCWSTR::null(), Some(&mut time)),
        )
    };
    if dn <= 1 || tn <= 1 {
        return None;
    }
    // Returned lengths include the terminator. Bidi marks in RTL locales would break parsing.
    let clean = |buf: &[u16], n: i32| -> String {
        String::from_utf16_lossy(&buf[..(n - 1) as usize])
            .chars()
            .filter(|c| !matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}'))
            .collect()
    };
    Some(format!("{} {}", clean(&date, dn), clean(&time, tn)))
}

fn us_format(dt: &NaiveDateTime) -> Option<String> {
    Some(dt.format("%m/%d/%Y %I:%M %p").to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestrictMode {
    Locale,
    Fallback,
}

impl RestrictMode {
    fn other(self) -> Self {
        match self {
            RestrictMode::Locale => RestrictMode::Fallback,
            RestrictMode::Fallback => RestrictMode::Locale,
        }
    }
}

/// How an item returned by `Restrict` relates to the requested window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    Inside,
    /// Ended just before the window (the filter has minute resolution): drop quietly.
    Edge,
    /// Far outside the window: a correct filter cannot return it, so Outlook
    /// parsed the dates differently than we wrote them.
    Stray,
}

fn fit(start: DateTime<Utc>, end: DateTime<Utc>, window: &FetchWindow) -> Fit {
    if start > window.to || end + chrono::Duration::minutes(1) < window.from {
        Fit::Stray
    } else if end < window.from {
        Fit::Edge
    } else {
        Fit::Inside
    }
}

/// Result of reading one restricted collection.
struct Scan {
    events: Vec<CalendarEventDto>,
    stray: bool,
}

impl Scan {
    /// Trustworthy evidence that this date format works: it returned events and nothing impossible.
    fn proves_format(&self) -> bool {
        !self.stray && !self.events.is_empty()
    }
}

fn is_transport(e: &SourceError) -> bool {
    matches!(e.kind, ErrKind::Busy | ErrKind::Disconnected)
}

/// Decide between two reads of the same window made with different date formats. A
/// wrongly parsed filter can silently return nothing (or a different day), so an empty or
/// stray first read gets a second opinion. Returns the events and whether the second
/// format is the one that proved itself. Items are always re-checked against the window,
/// so a wrong choice can hide events but never show a wrong one.
fn choose(first: Result<Scan, SourceError>, second: Result<Scan, SourceError>) -> (Result<Vec<CalendarEventDto>, SourceError>, bool) {
    match (first, second) {
        (_, Err(e)) if is_transport(&e) => (Err(e), false),
        (_, Ok(b)) if b.proves_format() => (Ok(b.events), true),
        (Ok(a), _) => (Ok(a.events), false),
        (Err(_), Ok(b)) => (Ok(b.events), false),
        (Err(_), Err(e)) => (Err(e), false),
    }
}

// =============================================================================
// Outlook COM source
// =============================================================================

fn map_com(stage_code: &'static str, e: ComError) -> SourceError {
    let kind = if e.is_busy() {
        ErrKind::Busy
    } else if e.is_disconnected() {
        ErrKind::Disconnected
    } else if e.is_blocked() {
        ErrKind::Blocked
    } else {
        ErrKind::Failed
    };
    let code = match kind {
        ErrKind::Busy => "OUTLOOK-105",
        ErrKind::Blocked => "OUTLOOK-110",
        ErrKind::Disconnected => "OUTLOOK-102",
        _ => stage_code,
    };
    SourceError::new(kind, code, e.to_string())
}

fn attach_error(e: ComError) -> SourceError {
    if e.hr == com::MK_E_UNAVAILABLE {
        return SourceError::new(ErrKind::NotInRot, "OUTLOOK-102", e.to_string());
    }
    map_com("OUTLOOK-102", e)
}

/// Keep real transport/policy failures; a property that is merely unavailable is `None`.
fn optional<T>(r: ComResult<T>) -> ComResult<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_disconnected() || e.is_busy() || e.is_blocked() => Err(e),
        Err(_) => Ok(None),
    }
}

fn str_prop(d: &mut Dispatch, name: &'static str) -> ComResult<Option<String>> {
    Ok(optional(d.get(name))?.and_then(|v| com::variant_string(&v)))
}

fn i32_prop(d: &mut Dispatch, name: &'static str) -> ComResult<Option<i32>> {
    Ok(optional(d.get(name))?.and_then(|v| com::variant_i32(&v)))
}

fn bool_prop(d: &mut Dispatch, name: &'static str) -> ComResult<bool> {
    Ok(optional(d.get(name))?.and_then(|v| com::variant_bool(&v)).unwrap_or(false))
}

fn date_prop(d: &mut Dispatch, name: &'static str) -> ComResult<Option<DateTime<Utc>>> {
    Ok(optional(d.get(name))?.and_then(|v| com::variant_date(&v)).and_then(com::date_to_utc))
}

/// `OlMeetingStatus`: olMeetingCanceled (5) and olMeetingReceivedAndCanceled (7) are meetings the
/// organizer called off; Outlook can leave them on the calendar until the user deletes them.
fn is_canceled(meeting_status: i32) -> bool {
    matches!(meeting_status, 5 | 7)
}

/// Only a lost or busy Outlook is an error here: anything else (no add-in, the property is not
/// set, a guard) just means there is nothing to read.
fn lenient<T>(r: ComResult<T>) -> ComResult<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_disconnected() || e.is_busy() => Err(e),
        Err(_) => Ok(None),
    }
}

/// The join link a Teams or Skype for Business add-in stored on the item, if any.
fn online_meeting_link(item: &mut Dispatch) -> ComResult<Option<String>> {
    let Some(mut accessor) = lenient(item.get_object("PropertyAccessor"))? else {
        return Ok(None);
    };
    for prop in ONLINE_MEETING_PROPS {
        let value = lenient(accessor.call("GetProperty", vec![com::variant_from_str(prop)]))?;
        if let Some(link) = value.as_ref().and_then(com::variant_string).as_deref().and_then(meeting_link) {
            return Ok(Some(link));
        }
    }
    Ok(None)
}

fn read_item(item: &mut Dispatch, colors: &CategoryColors) -> ComResult<Option<RawItem>> {
    let (Some(start), Some(end)) = (date_prop(item, "Start")?, date_prop(item, "End")?) else {
        return Ok(None);
    };
    // A number, never content. A canceled meeting is neither listed nor reminded about.
    if is_canceled(i32_prop(item, "MeetingStatus")?.unwrap_or(0)) {
        return Ok(None);
    }
    // Without any colored category in the profile there is nothing to look up.
    let color = if colors.is_empty() {
        None
    } else {
        str_prop(item, "Categories")?.and_then(|names| first_category_color(&names, colors))
    };
    // Kept long enough to find a whole join URL; shortened for the DTO in `build_event`.
    let location = non_empty(str_prop(item, "Location")?, MAX_LOCATION_CHARS);
    let online_link = if location.as_deref().and_then(find_meeting_url).is_some() { None } else { online_meeting_link(item)? };
    Ok(Some(RawItem {
        entry_id: str_prop(item, "EntryID")?.unwrap_or_default(),
        subject: str_prop(item, "Subject")?.unwrap_or_default(),
        start,
        end,
        location,
        organizer: non_empty(str_prop(item, "Organizer")?, MAX_TEXT_CHARS),
        all_day: bool_prop(item, "AllDayEvent")?,
        recurring: bool_prop(item, "IsRecurring")?,
        busy: i32_prop(item, "BusyStatus")?.unwrap_or(2),
        response: i32_prop(item, "ResponseStatus")?.unwrap_or(0),
        color,
        online_link,
    }))
}

/// The profile's master category list: name -> color, for the categories that have one.
fn read_category_colors(session: &mut Dispatch) -> ComResult<CategoryColors> {
    let mut colors = CategoryColors::new();
    let Some(mut categories) = optional(session.get_object("Categories"))? else {
        return Ok(colors);
    };
    let count = i32_prop(&mut categories, "Count")?.unwrap_or(0).clamp(0, MAX_CATEGORIES);
    for index in 1..=count {
        let Some(mut category) = optional(categories.call_object("Item", vec![com::variant_from_i32(index)]))?.flatten() else {
            continue;
        };
        let (Some(name), Some(color)) = (str_prop(&mut category, "Name")?, i32_prop(&mut category, "Color")?) else {
            continue;
        };
        if let Some(hex) = category_color(color) {
            colors.insert(name.trim().to_lowercase(), hex);
        }
    }
    Ok(colors)
}

/// What one unread meeting request turned out to be.
enum InviteRead {
    /// The invite and the request's EntryID (kept in this module, for answering it).
    Invite(MeetingInviteDto, String),
    /// Older than [`INVITE_MAX_AGE_HOURS`]; the list is sorted newest first, so stop here.
    TooOld,
    Skip,
}

/// `OlResponseStatus` values that mean the user already answered: tentative, accepted, declined.
fn is_answered(response_status: i32) -> bool {
    matches!(response_status, 2..=4)
}

/// invite id -> the request's EntryID, from the latest read. Memory only, never logged or sent
/// to the page: the page answers an invite by its hashed id.
static INVITE_ENTRY_IDS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn remember_invites(entries: Vec<(String, String)>) {
    *INVITE_ENTRY_IDS.lock().unwrap_or_else(|e| e.into_inner()) = entries;
}

fn invite_entry_id(id: &str) -> Option<String> {
    INVITE_ENTRY_IDS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(i, _)| i == id).map(|(_, entry)| entry.clone())
}

fn read_invite(item: &mut Dispatch, now: DateTime<Utc>) -> ComResult<InviteRead> {
    let Some(received) = date_prop(item, "ReceivedTime")? else {
        return Ok(InviteRead::Skip);
    };
    if received < now - chrono::Duration::hours(INVITE_MAX_AGE_HOURS) {
        return Ok(InviteRead::TooOld);
    }
    let entry_id = str_prop(item, "EntryID")?.unwrap_or_default();
    if entry_id.is_empty() {
        return Ok(InviteRead::Skip);
    }
    let subject = str_prop(item, "Subject")?.unwrap_or_default();
    // The meeting itself: the tentative entry Outlook put on the calendar when the request
    // arrived. `false` = look it up only, never add one.
    let appointment = optional(item.call_object("GetAssociatedAppointment", vec![com::variant_from_bool(false)]))?.flatten();
    let (start_utc, end_utc, location, organizer) = match appointment {
        Some(mut a) => {
            // Answered already (from Outlook, a phone or the island): not pending any more.
            if is_answered(i32_prop(&mut a, "ResponseStatus")?.unwrap_or(0)) {
                return Ok(InviteRead::Skip);
            }
            (
                date_prop(&mut a, "Start")?,
                date_prop(&mut a, "End")?,
                non_empty(str_prop(&mut a, "Location")?, MAX_TEXT_CHARS),
                non_empty(str_prop(&mut a, "Organizer")?, MAX_TEXT_CHARS),
            )
        }
        None => (None, None, None, None),
    };
    let dto = MeetingInviteDto {
        id: invite_id(&entry_id),
        subject: clip(&subject),
        organizer,
        start_utc,
        end_utc,
        location,
        received_utc: received,
    };
    Ok(InviteRead::Invite(dto, entry_id))
}

fn invite_id(entry_id: &str) -> String {
    hash16(&format!("invite|{entry_id}"))
}

/// The newest unread meeting requests in the default Inbox. The filter has no date in it, so
/// unlike the calendar's it cannot be misparsed in any locale.
fn read_invites(session: &mut Dispatch, now: DateTime<Utc>) -> ComResult<Vec<MeetingInviteDto>> {
    let mut invites = Vec::new();
    let Some(mut inbox) = session.call_object("GetDefaultFolder", vec![com::variant_from_i32(OL_FOLDER_INBOX)])? else {
        return Ok(invites);
    };
    let mut items = inbox.get_object("Items")?;
    let filter = format!("[MessageClass] = '{MEETING_REQUEST_CLASS}' AND [UnRead] = True");
    let Some(mut requests) = items.call_object("Restrict", vec![com::variant_from_str(&filter)])? else {
        return Ok(invites);
    };
    requests.call("Sort", vec![com::variant_from_str("[ReceivedTime]"), com::variant_from_bool(true)])?;
    let mut entries = Vec::new();
    let mut next = requests.call_object("GetFirst", Vec::new())?;
    let mut scanned = 0;
    while let Some(mut item) = next.take() {
        scanned += 1;
        match read_invite(&mut item, now)? {
            InviteRead::Invite(invite, entry_id) => {
                entries.push((invite.id.clone(), entry_id));
                invites.push(invite);
            }
            InviteRead::TooOld => break,
            InviteRead::Skip => {}
        }
        drop(item);
        if scanned >= MAX_INVITES_SCANNED || invites.len() >= MAX_INVITES {
            break;
        }
        next = requests.call_object("GetNext", Vec::new())?;
    }
    remember_invites(entries);
    Ok(invites)
}

#[derive(Default)]
pub struct OutlookSource {
    pub last_restrict_mode: Option<RestrictMode>,
    category_colors: CategoryColors,
    categories_read_at: Option<Instant>,
}

impl OutlookSource {
    /// Re-read the category colors when they are older than [`CATEGORY_REFRESH_SECS`]. A
    /// failure keeps the last good colors; only a lost or busy Outlook fails the sync.
    fn refresh_category_colors(&mut self, session: &mut Dispatch) -> Result<(), SourceError> {
        if self.categories_read_at.is_some_and(|at| at.elapsed().as_secs() < CATEGORY_REFRESH_SECS) {
            return Ok(());
        }
        match read_category_colors(session) {
            Ok(colors) => self.category_colors = colors,
            Err(e) if e.is_disconnected() || e.is_busy() => return Err(map_com("OUTLOOK-107", e)),
            Err(e) => dlog!("WARN", "outlook", "category colors not read: {}", e),
        }
        self.categories_read_at = Some(Instant::now());
        Ok(())
    }

    /// Read the items of `items` that Outlook returns for the window when the dates are
    /// written in `mode`'s format. Every item is re-checked against the window.
    fn scan(&self, items: &mut Dispatch, window: &FetchWindow, mode: RestrictMode, calendar_id: &str) -> Result<Scan, SourceError> {
        let from = window.from.with_timezone(&Local).naive_local().with_second(0).unwrap_or_default();
        let to = window.to.with_timezone(&Local).naive_local();
        let fmt: fn(&NaiveDateTime) -> Option<String> = match mode {
            RestrictMode::Locale => locale_format,
            RestrictMode::Fallback => us_format,
        };
        let filter = restrict_filter(from, to, fmt)
            .ok_or_else(|| SourceError::new(ErrKind::Failed, "OUTLOOK-108", "date format unavailable"))?;
        let mut restricted = items
            .call_object("Restrict", vec![com::variant_from_str(&filter)])
            .map_err(|e| map_com("OUTLOOK-108", e))?
            .ok_or_else(|| SourceError::new(ErrKind::Failed, "OUTLOOK-108", "Restrict returned nothing"))?;

        let mut scan = Scan { events: Vec::new(), stray: false };
        let mut next = restricted.call_object("GetFirst", Vec::new()).map_err(|e| map_com("OUTLOOK-108", e))?;
        let mut scanned = 0;
        while let Some(mut item) = next.take() {
            scanned += 1;
            if let Some(raw) = read_item(&mut item, &self.category_colors).map_err(|e| map_com("OUTLOOK-108", e))? {
                match fit(raw.start, raw.end, window) {
                    Fit::Inside => scan.events.push(build_event(raw, calendar_id)),
                    Fit::Edge => {}
                    Fit::Stray => {
                        scan.stray = true;
                        if raw.start > window.to {
                            // Sorted by start: everything after this is later still.
                            break;
                        }
                    }
                }
            }
            drop(item);
            if scanned >= MAX_SCANNED || scan.events.len() >= crate::calendar::MAX_EVENTS {
                break;
            }
            next = restricted.call_object("GetNext", Vec::new()).map_err(|e| map_com("OUTLOOK-108", e))?;
        }
        Ok(scan)
    }

    fn read_calendar(&mut self, folder: &mut Dispatch, window: &FetchWindow) -> Result<Vec<CalendarEventDto>, SourceError> {
        let calendar_id = {
            let store = str_prop(folder, "StoreID").map_err(|e| map_com("OUTLOOK-107", e))?.unwrap_or_default();
            let entry = str_prop(folder, "EntryID").map_err(|e| map_com("OUTLOOK-107", e))?.unwrap_or_default();
            hash16(&format!("{store}|{entry}"))
        };
        let mut items = folder.get_object("Items").map_err(|e| map_com("OUTLOOK-107", e))?;
        // Order matters: sort first, then expand recurrences, then restrict.
        items.call("Sort", vec![com::variant_from_str("[Start]")]).map_err(|e| map_com("OUTLOOK-108", e))?;
        items.put("IncludeRecurrences", com::variant_from_bool(true)).map_err(|e| map_com("OUTLOOK-108", e))?;

        // The format that worked last time goes first; the other only runs when the first
        // read was empty, impossible or rejected. A calendar that is empty for the next
        // 48 h therefore costs one extra Restrict per sync, and nothing otherwise.
        let primary = self.last_restrict_mode.unwrap_or(RestrictMode::Locale);
        let first = self.scan(&mut items, window, primary, &calendar_id);
        match &first {
            Ok(scan) if scan.proves_format() => {
                self.last_restrict_mode = Some(primary);
                return first.map(|s| s.events);
            }
            Err(e) if is_transport(e) => return first.map(|s| s.events),
            _ => {}
        }
        let second = self.scan(&mut items, window, primary.other(), &calendar_id);
        let (result, second_proved) = choose(first, second);
        if second_proved {
            self.last_restrict_mode = Some(primary.other());
            dlog!("INFO", "outlook", "Restrict date format switched to {:?}", primary.other());
        }
        result
    }
}

impl CalendarSource for OutlookSource {
    fn fetch(&mut self, window: &FetchWindow) -> FetchResult {
        let mut app = Dispatch::get_active(PROG_ID).map_err(attach_error)?;
        let mut session = app.get_object("Session").map_err(|e| map_com("OUTLOOK-106", e))?;
        // The profile name is only ever hashed for the log.
        if let Ok(Some(profile)) = str_prop(&mut session, "CurrentProfileName") {
            dlog!("DEBUG", "outlook", "attached, profile {}", hash_id(&profile));
        }
        let default_calendar = session
            .call_object("GetDefaultFolder", vec![com::variant_from_i32(OL_FOLDER_CALENDAR)])
            .map_err(|e| map_com("OUTLOOK-107", e))?
            .ok_or_else(|| SourceError::new(ErrKind::Failed, "OUTLOOK-107", "no default calendar"))?;
        self.refresh_category_colors(&mut session)?;
        // Multi-calendar-ready: today the list holds only the default calendar.
        let mut calendars = vec![default_calendar];
        let mut events = Vec::new();
        for folder in calendars.iter_mut() {
            events.extend(self.read_calendar(folder, window)?);
        }
        // The calendar is what matters: a request that cannot be read only costs its popup.
        let invites = if window.invites {
            read_invites(&mut session, Utc::now()).unwrap_or_else(|e| {
                dlog!("WARN", "outlook", "meeting requests not read: {}", e);
                Vec::new()
            })
        } else {
            Vec::new()
        };
        Ok(Fetched { events: crate::calendar::normalize_events(events), invites })
    }
}

// =============================================================================
// Actions on a click in the island: open the calendar, answer an invitation
// =============================================================================

/// `OlWindowState`
const OL_MINIMIZED: i32 = 1;
const OL_NORMAL_WINDOW: i32 = 2;
/// The whole open (attach, switch folder, navigate) must answer within this.
const OPEN_TIMEOUT_SECS: u64 = 10;
/// Answering may wait on an Outlook security prompt the user has to click.
const RESPOND_TIMEOUT_SECS: u64 = 60;

/// Run `action` against the user's own running classic Outlook, on a fresh STA thread (pool
/// threads may already be MTA), giving up after `timeout_secs`. Attach-only: an Outlook that
/// is not running is never started.
fn run_on_outlook<T: Send + 'static>(
    thread: &'static str,
    timeout_secs: u64,
    action: impl FnOnce(u32) -> ComResult<T> + Send + 'static,
) -> Result<T, String> {
    let pid = match discover()? {
        Discovery::Classic(pid) => pid,
        Discovery::Waiting => return Err("OUTLOOK-101: classic Outlook is not running".into()),
        Discovery::NewOutlookOnly => return Err("OUTLOOK-104: only the new Outlook is running".into()),
        Discovery::ElevationMismatch => return Err("OUTLOOK-103: Outlook runs with different permissions".into()),
    };
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(thread.into())
        .spawn(move || {
            let result = crate::debug_log::catch("outlook", || {
                let apartment = ComApartment::init_sta().map_err(|e| format!("OUTLOOK-102: {e}"))?;
                let _filter = MessageFilterGuard::register(&apartment).ok();
                action(pid).map_err(|e| {
                    let code = if e.is_blocked() {
                        "OUTLOOK-110"
                    } else if e.is_busy() {
                        "OUTLOOK-105"
                    } else if e.is_disconnected() {
                        "OUTLOOK-102"
                    } else {
                        "OUTLOOK-108"
                    };
                    format!("{code}: {e}")
                })
            })
            .unwrap_or_else(|| Err("OUTLOOK-108: internal error".into()));
            let _ = tx.send(result);
        })
        .map_err(|e| format!("OUTLOOK-102: cannot start the Outlook thread: {e}"))?;
    rx.recv_timeout(std::time::Duration::from_secs(timeout_secs))
        .unwrap_or_else(|_| Err("OUTLOOK-109: Outlook did not answer".into()))
}

fn show_calendar(pid: u32, at: Option<NaiveDateTime>) -> ComResult<()> {
    let mut app = Dispatch::get_active(PROG_ID)?;
    let mut session = app.get_object("Session")?;
    let mut calendar = session
        .call_object("GetDefaultFolder", vec![com::variant_from_i32(OL_FOLDER_CALENDAR)])?
        .ok_or_else(|| ComError::new("GetDefaultFolder", com::E_NOOBJECT))?;
    let mut explorer = match app.call_object("ActiveExplorer", Vec::new())? {
        Some(mut explorer) => {
            explorer.put_ref("CurrentFolder", &calendar)?;
            explorer
        }
        // Outlook runs without a main window (e.g. only a mail was open): give it one.
        None => {
            let mut explorer = calendar
                .call_object("GetExplorer", Vec::new())?
                .ok_or_else(|| ComError::new("GetExplorer", com::E_NOOBJECT))?;
            explorer.call("Display", Vec::new())?;
            explorer
        }
    };
    if i32_prop(&mut explorer, "WindowState")? == Some(OL_MINIMIZED) {
        explorer.put("WindowState", com::variant_from_i32(OL_NORMAL_WINDOW))?;
    }
    // The click was on our (never-activated) window, so Windows lets this process hand the
    // foreground on: to the user's own Outlook, for this one activation.
    unsafe {
        let _ = AllowSetForegroundWindow(pid);
    }
    explorer.call("Activate", Vec::new())?;
    if let Some(date) = at.and_then(com::naive_to_date) {
        // Only a calendar view has GoToDate; any other view simply stays where it is.
        if let Ok(mut view) = explorer.get_object("CurrentView") {
            if let Err(e) = com::variant_from_date(date).and_then(|d| view.call("GoToDate", vec![d])) {
                dlog!("DEBUG", "outlook", "GoToDate not applied: {}", e);
            }
        }
    }
    Ok(())
}

/// Show the user's classic Outlook on its calendar, on the day of `at` when given. Only ever
/// from an explicit click in the island.
pub fn open_calendar(at: Option<DateTime<Utc>>) -> Result<(), String> {
    let local = at.map(|t| t.with_timezone(&Local).naive_local());
    run_on_outlook("companyisland-outlook-open", OPEN_TIMEOUT_SECS, move |pid| show_calendar(pid, local))
}

/// An answer to a meeting request (`OlMeetingResponse`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteResponse {
    Accept,
    Tentative,
    Decline,
}

impl InviteResponse {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "accept" => Some(InviteResponse::Accept),
            "tentative" => Some(InviteResponse::Tentative),
            "decline" => Some(InviteResponse::Decline),
            _ => None,
        }
    }

    fn ol_value(self) -> i32 {
        match self {
            InviteResponse::Tentative => 2,
            InviteResponse::Accept => 3,
            InviteResponse::Decline => 4,
        }
    }
}

/// Answer the meeting request `entry_id` exactly as Outlook's own buttons do: respond on its
/// calendar entry and send the reply to the organizer (only when a reply was asked for).
/// Returns whether a reply was sent. Nothing about the meeting is logged.
fn answer_invite(entry_id: &str, response: InviteResponse) -> ComResult<bool> {
    let mut app = Dispatch::get_active(PROG_ID)?;
    let mut session = app.get_object("Session")?;
    let mut request = session
        .call_object("GetItemFromID", vec![com::variant_from_str(entry_id)])?
        .ok_or_else(|| ComError::new("GetItemFromID", com::E_NOOBJECT))?;
    // The id only ever comes from `read_invites`, but the item must still be a request.
    if !str_prop(&mut request, "MessageClass")?.unwrap_or_default().starts_with(MEETING_REQUEST_CLASS) {
        return Err(ComError::new("MessageClass", com::E_NOOBJECT));
    }
    // `true`: put it on the calendar if it is not there yet, as answering from Outlook does.
    let mut appointment = request
        .call_object("GetAssociatedAppointment", vec![com::variant_from_bool(true)])?
        .ok_or_else(|| ComError::new("GetAssociatedAppointment", com::E_NOOBJECT))?;
    // Unknown counts as "asked for": a reply too many is better than a silent organizer.
    let reply_wanted = optional(appointment.get("ResponseRequested"))?.and_then(|v| com::variant_bool(&v)).unwrap_or(true);
    let reply = appointment.call_object(
        "Respond",
        vec![com::variant_from_i32(response.ol_value()), com::variant_from_bool(true), com::variant_from_bool(false)],
    )?;
    match reply {
        Some(mut reply) if reply_wanted => {
            reply.call("Send", Vec::new())?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Answer a pending invitation (by the id the page got in the snapshot). Only ever from an
/// explicit click in the island.
pub fn respond_invite(id: &str, response: InviteResponse) -> Result<bool, String> {
    let entry_id = invite_entry_id(id).ok_or_else(|| "OUTLOOK-108: the invitation is no longer pending".to_string())?;
    let sent = run_on_outlook("companyisland-outlook-respond", RESPOND_TIMEOUT_SECS, move |_| answer_invite(&entry_id, response))?;
    // Answered: it is not pending any more, whatever the next read finds.
    INVITE_ENTRY_IDS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(i, _)| i != id);
    Ok(sent)
}

#[tauri::command]
pub async fn outlook_respond_invite(id: String, response: String) -> Result<(), String> {
    let response = InviteResponse::parse(&response).ok_or_else(|| "OUTLOOK-108: unknown response".to_string())?;
    let result = crate::rt::run_blocking("outlook_respond_invite", move || respond_invite(&id, response)).await;
    match &result {
        Ok(sent) => {
            dlog!("INFO", "outlook", "invitation answered from the island ({:?}, reply sent: {})", response, sent);
            // The calendar entry changed: read it again now rather than in a minute.
            let _ = crate::calendar::calendar_refresh();
        }
        Err(e) => dlog!("WARN", "outlook", "answering an invitation failed: {}", e),
    }
    result.map(|_| ())
}

/// The Teams desktop app's own link for a Teams web join link (`msteams:/l/meetup-join/...`).
pub fn teams_app_link(url: &str) -> Option<String> {
    const WEB: &str = "https://teams.microsoft.com/";
    url.get(..WEB.len()).filter(|p| p.eq_ignore_ascii_case(WEB)).map(|_| format!("msteams:/{}", &url[WEB.len()..]))
}

/// Only links the island itself found: an https link from a meeting property, or a link on a
/// known meeting host from a Location.
fn openable_meeting_link(url: &str) -> bool {
    meeting_link(url).is_some() || find_meeting_url(url).as_deref() == Some(url)
}

fn shell_open(target: &str) -> bool {
    let result = unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            &windows::core::HSTRING::from("open"),
            &windows::core::HSTRING::from(target),
            None,
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    result.0 as isize > 32
}

/// Join a meeting: Teams links go straight to the Teams app when it is installed, anything else
/// (and Teams without the app) to the link's default handler (browser, Webex, Zoom, Skype). The
/// link itself is never logged.
#[tauri::command]
pub async fn open_meeting_url(url: String) -> Result<(), String> {
    if !openable_meeting_link(&url) {
        return Err("APP-020: not a meeting link".into());
    }
    crate::rt::run_blocking("open_meeting_url", move || {
        crate::rt::ensure_com_initialized();
        let app = teams_app_link(&url);
        if app.as_deref().is_some_and(shell_open) {
            dlog!("INFO", "outlook", "meeting joined in the Teams app");
            return Ok(());
        }
        if shell_open(&url) {
            dlog!("INFO", "outlook", "meeting link opened (teams link: {})", app.is_some());
            Ok(())
        } else {
            Err("APP-020: the meeting link could not be opened".into())
        }
    })
    .await
}

#[tauri::command]
pub async fn outlook_open_calendar(start_utc: Option<String>) -> Result<(), String> {
    let at = start_utc
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc));
    let result = crate::rt::run_blocking("outlook_open_calendar", move || open_calendar(at)).await;
    match &result {
        Ok(()) => dlog!("INFO", "outlook", "calendar opened from the island (dated: {})", at.is_some()),
        Err(e) => dlog!("WARN", "outlook", "open calendar failed: {}", e),
    }
    result
}

// =============================================================================
// STA worker thread
// =============================================================================

pub type ReplyFn = Box<dyn Fn(FetchResult) + Send>;

enum Job {
    Fetch(FetchWindow),
    Shutdown,
}

/// Auto-reset event the supervisor signals to wake the STA thread.
struct Waker(HANDLE);

// SAFETY: an event HANDLE may be used from any thread.
unsafe impl Send for Waker {}
unsafe impl Sync for Waker {}

impl Waker {
    fn new() -> std::io::Result<Self> {
        unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .map(Waker)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn signal(&self) {
        unsafe {
            let _ = SetEvent(self.0);
        }
    }
}

impl Drop for Waker {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn pump_messages() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Sleep until signalled while still dispatching window messages, which an STA needs
/// for COM callbacks and cross-apartment calls.
fn wait_for_work(waker: &Waker) {
    unsafe {
        MsgWaitForMultipleObjectsEx(Some(&[waker.0]), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
    }
}

fn worker_main<S: CalendarSource>(make_source: impl FnOnce() -> S, jobs: Receiver<Job>, waker: Arc<Waker>, reply: ReplyFn) {
    // Declaration order is drop order in reverse: the source (and every COM object it
    // owns) goes before the message filter and the apartment.
    let apartment = ComApartment::init_sta();
    let filter = apartment.as_ref().ok().map(|a| MessageFilterGuard::register(a));
    if let Err(e) = &apartment {
        dlog!("ERROR", "outlook", "OUTLOOK-102 STA init failed: {}", e);
    }
    if let Some(Err(e)) = &filter {
        dlog!("WARN", "outlook", "message filter not registered: {}", e);
    }
    let mut source = apartment.is_ok().then(make_source);
    loop {
        pump_messages();
        match jobs.try_recv() {
            Ok(Job::Fetch(window)) => {
                let result = match source.as_mut() {
                    // A panic must answer too; otherwise the supervisor would wait out the
                    // watchdog and count a hung Outlook.
                    Some(s) => crate::debug_log::catch("outlook", || s.fetch(&window)).unwrap_or_else(|| {
                        Err(SourceError::new(ErrKind::Failed, "OUTLOOK-108", "internal error while reading"))
                    }),
                    None => Err(SourceError::new(ErrKind::Failed, "OUTLOOK-102", "COM apartment unavailable")),
                };
                reply(result);
            }
            Ok(Job::Shutdown) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => wait_for_work(&waker),
        }
    }
}

/// Handle to one worker generation. Dropping it (the watchdog path included) asks the
/// thread to exit without waiting for it: a hung thread is left alone, and if it ever
/// returns it releases its COM objects and exits.
pub struct Worker {
    jobs: Sender<Job>,
    waker: Arc<Waker>,
}

impl Worker {
    pub fn spawn<S: CalendarSource + 'static>(
        generation: u64,
        make_source: impl FnOnce() -> S + Send + 'static,
        reply: ReplyFn,
    ) -> std::io::Result<Worker> {
        let (jobs_tx, jobs_rx) = mpsc::channel();
        let waker = Arc::new(Waker::new()?);
        let thread_waker = Arc::clone(&waker);
        std::thread::Builder::new().name(format!("companyisland-outlook-{generation}")).spawn(move || {
            crate::debug_log::catch("outlook", || worker_main(make_source, jobs_rx, thread_waker, reply));
        })?;
        Ok(Worker { jobs: jobs_tx, waker })
    }

    pub fn spawn_outlook(generation: u64, reply: ReplyFn) -> std::io::Result<Worker> {
        Worker::spawn(generation, OutlookSource::default, reply)
    }

    /// False when the worker thread is gone.
    pub fn fetch(&self, window: FetchWindow) -> bool {
        let sent = self.jobs.send(Job::Fetch(window)).is_ok();
        self.waker.signal();
        sent
    }

    pub fn shutdown(&self) {
        let _ = self.jobs.send(Job::Shutdown);
        self.waker.signal();
    }
}

impl Drop for Worker {
    /// Closing the channel alone would not wake a thread parked in `wait_for_work`; it
    /// would sit in its apartment forever. A hung thread finds the request when it returns.
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};
    use std::time::Duration;

    fn me() -> Identity {
        Identity { session: 2, sid: vec![1, 5, 0, 0, 0, 0, 0, 5, 21, 7], elevated: false }
    }

    fn proc(exe: &str, session: u32, sid: Option<&[u8]>, elevated: Option<bool>) -> ProcInfo {
        ProcInfo { pid: 100, session, sid: sid.map(<[u8]>::to_vec), elevated, exe: exe.into() }
    }

    #[test]
    fn nothing_running_is_waiting() {
        assert_eq!(classify(&me(), &[]), Discovery::Waiting);
    }

    #[test]
    fn our_classic_outlook_is_attachable_and_case_insensitive() {
        let p = proc("OUTLOOK.EXE", 2, Some(&me().sid), Some(false));
        assert_eq!(classify(&me(), &[p]), Discovery::Classic(100));
        let p = proc("outlook.exe", 2, Some(&me().sid), Some(false));
        assert_eq!(classify(&me(), &[p]), Discovery::Classic(100));
    }

    #[test]
    fn other_sessions_and_other_users_are_never_ours() {
        let other_session = proc("OUTLOOK.EXE", 3, Some(&me().sid), Some(false));
        assert_eq!(classify(&me(), &[other_session]), Discovery::Waiting);
        let other_user = proc("OUTLOOK.EXE", 2, Some(&[9, 9, 9]), Some(false));
        assert_eq!(classify(&me(), &[other_user]), Discovery::Waiting);
        // Their new Outlook does not make ours "new outlook only" either.
        let other_new = proc("olk.exe", 2, Some(&[9, 9, 9]), Some(false));
        assert_eq!(classify(&me(), &[other_new]), Discovery::Waiting);
    }

    #[test]
    fn only_canceled_meeting_statuses_are_hidden() {
        // 0 non-meeting, 1 meeting, 3 received: shown. 5 canceled, 7 received and canceled: hidden.
        for shown in [0, 1, 3] {
            assert!(!is_canceled(shown), "{shown}");
        }
        assert!(is_canceled(5) && is_canceled(7));
    }

    #[test]
    fn elevation_difference_is_a_mismatch_both_ways() {
        let elevated_outlook = proc("OUTLOOK.EXE", 2, Some(&me().sid), Some(true));
        assert_eq!(classify(&me(), &[elevated_outlook.clone()]), Discovery::ElevationMismatch);
        let elevated_me = Identity { elevated: true, ..me() };
        let normal_outlook = proc("OUTLOOK.EXE", 2, Some(&me().sid), Some(false));
        assert_eq!(classify(&elevated_me, &[normal_outlook]), Discovery::ElevationMismatch);
        assert_eq!(classify(&elevated_me, &[elevated_outlook]), Discovery::Classic(100));
    }

    #[test]
    fn uninspectable_outlook_in_our_session_is_a_mismatch() {
        let hidden = proc("OUTLOOK.EXE", 2, None, None);
        assert_eq!(classify(&me(), &[hidden]), Discovery::ElevationMismatch);
    }

    #[test]
    fn new_outlook_only_when_no_classic() {
        let new = proc("olk.exe", 2, Some(&me().sid), Some(false));
        assert_eq!(classify(&me(), &[new.clone()]), Discovery::NewOutlookOnly);
        let classic = proc("OUTLOOK.EXE", 2, Some(&me().sid), Some(false));
        assert_eq!(classify(&me(), &[new, classic]), Discovery::Classic(100));
    }

    #[test]
    fn classic_carries_the_lowest_matching_pid() {
        let mut later = proc("OUTLOOK.EXE", 2, Some(&me().sid), Some(false));
        later.pid = 300;
        let mut earlier = later.clone();
        earlier.pid = 200;
        let mut other_user = proc("OUTLOOK.EXE", 2, Some(&[9, 9, 9]), Some(false));
        other_user.pid = 50;
        assert_eq!(classify(&me(), &[later, other_user, earlier]), Discovery::Classic(200));
    }

    #[test]
    fn own_identity_is_readable() {
        let id = current_identity().unwrap();
        assert!(!id.sid.is_empty());
    }

    #[test]
    fn event_id_is_stable_16_hex_and_depends_on_start() {
        let t1 = Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 10, 7, 9, 0, 0).unwrap();
        let a = event_id("00000000ABCDEF", &t1);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, event_id("00000000ABCDEF", &t1));
        assert_ne!(a, event_id("00000000ABCDEF", &t2));
        assert_ne!(a, event_id("00000000ABCDE0", &t1));
        assert_eq!(a, hash16("00000000ABCDEF|2026-10-06T09:00:00Z"));
    }

    #[test]
    fn meeting_urls_for_known_providers() {
        let cases = [
            ("Microsoft Teams Meeting https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc/0?context=%7b%7d", true),
            ("https://us02web.zoom.us/j/123456789?pwd=abc", true),
            ("Join: http://zoom.us/j/1", true),
            ("https://company.webex.com/meet/jdoe", true),
            ("https://meet.google.com/abc-defg-hij", true),
            ("https://teams.live.com/meet/9876", true),
            ("Skype Meeting https://meet.lync.com/contoso/jdoe/AB12CD34", true),
            ("https://join.skype.com/aBcDeFgHiJ", true),
            ("Cisco Webex https://contoso.webex.com/contoso/j.php?MTID=m123", true),
            ("https://web.ciscospark.com/meet/jdoe", true),
            ("https://dod.teams.microsoft.us/l/meetup-join/1", true),
            ("https://www.skype.com/en/", false),
            ("https://lync.com.evil.example/x", false),
            ("https://evil.example.com/teams.microsoft.com", false),
            ("https://teams.microsoft.com.evil.example/x", false),
            ("https://nozoom.us/j/1", false),
            ("https://user@zoom.us/j/1", false),
            ("Room 4.12", false),
            ("", false),
        ];
        for (input, expected) in cases {
            assert_eq!(find_meeting_url(input).is_some(), expected, "{input}");
        }
    }

    #[test]
    fn meeting_url_extraction_boundaries() {
        assert_eq!(
            find_meeting_url("Link (https://zoom.us/j/55). Thanks").as_deref(),
            Some("https://zoom.us/j/55")
        );
        assert_eq!(
            find_meeting_url("see https://example.com/x then https://meet.google.com/aaa-bbbb-ccc, ok").as_deref(),
            Some("https://meet.google.com/aaa-bbbb-ccc")
        );
        assert_eq!(find_meeting_url("HTTPS://ZOOM.US/J/1").as_deref(), Some("HTTPS://ZOOM.US/J/1"));
        assert!(find_meeting_url(&format!("https://zoom.us/{}", "a".repeat(3000))).is_none());
    }

    #[test]
    fn enum_mapping_matches_the_outlook_object_model() {
        assert_eq!(busy_status(0), BusyStatus::Free);
        assert_eq!(busy_status(1), BusyStatus::Tentative);
        assert_eq!(busy_status(2), BusyStatus::Busy);
        assert_eq!(busy_status(3), BusyStatus::Oof);
        assert_eq!(busy_status(4), BusyStatus::WorkingElsewhere);
        assert_eq!(busy_status(99), BusyStatus::Busy);
        assert_eq!(response_status(0), ResponseStatus::None);
        assert_eq!(response_status(1), ResponseStatus::Organized);
        assert_eq!(response_status(2), ResponseStatus::Tentative);
        assert_eq!(response_status(3), ResponseStatus::Accepted);
        assert_eq!(response_status(4), ResponseStatus::Declined);
        assert_eq!(response_status(5), ResponseStatus::NotResponded);
        assert_eq!(response_status(-1), ResponseStatus::None);
    }

    #[test]
    fn category_colors_follow_ol_category_color() {
        assert_eq!(category_color(1), Some("#E7484F"));
        assert_eq!(category_color(8), Some("#4A9BE8"));
        assert_eq!(category_color(25), Some("#8E2F56"));
        for none in [0, -1, 26, 1000] {
            assert_eq!(category_color(none), None, "{none}");
        }
        for i in 1..=25 {
            let hex = category_color(i).unwrap();
            assert!(hex.len() == 7 && hex.starts_with('#') && hex[1..].chars().all(|c| c.is_ascii_hexdigit()), "{hex}");
        }
    }

    #[test]
    fn the_first_colored_category_wins_whatever_the_separator() {
        let colors: CategoryColors = [("red category".to_string(), "#E7484F"), ("project x".to_string(), "#4A9BE8")].into();
        assert_eq!(first_category_color("Red Category, Project X", &colors).as_deref(), Some("#E7484F"));
        assert_eq!(first_category_color("Uncolored; project x", &colors).as_deref(), Some("#4A9BE8"));
        assert_eq!(first_category_color("  PROJECT X  ", &colors).as_deref(), Some("#4A9BE8"));
        assert_eq!(first_category_color("", &colors), None);
        assert_eq!(first_category_color("Other", &colors), None);
    }

    #[test]
    fn meeting_property_links_are_plain_https() {
        let teams = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0?context=%7b%7d";
        assert_eq!(meeting_link(teams).as_deref(), Some(teams));
        // Skype for Business on-premises: the company's own host is fine from a meeting property.
        assert!(meeting_link("https://meet.contoso.com/jdoe/AB12CD34").is_some());
        assert_eq!(meeting_link("  https://meet.contoso.com/x  ").as_deref(), Some("https://meet.contoso.com/x"));
        for bad in [
            "http://meet.contoso.com/x",
            "conf:sip:jdoe@contoso.com",
            "https://user@evil.example/x",
            "https:///nohost",
            "https://a.example/x y",
            "https://a.example/\"x",
            "javascript:alert(1)",
        ] {
            assert!(meeting_link(bad).is_none(), "{bad}");
        }
        assert!(meeting_link(&format!("https://a.example/{}", "x".repeat(3000))).is_none());
    }

    #[test]
    fn a_location_link_wins_over_a_property_link() {
        let start = Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, 0).unwrap();
        let raw = |location: Option<&str>, online_link: Option<&str>| RawItem {
            entry_id: "id".into(),
            subject: "s".into(),
            start,
            end: start + chrono::Duration::minutes(30),
            location: location.map(String::from),
            organizer: None,
            all_day: false,
            recurring: false,
            busy: 2,
            response: 3,
            color: None,
            online_link: online_link.map(String::from),
        };
        let teams = "https://teams.microsoft.com/l/meetup-join/1";
        assert_eq!(build_event(raw(Some("Microsoft Teams Meeting"), Some(teams)), "c").meeting_url.as_deref(), Some(teams));
        assert_eq!(build_event(raw(Some("https://zoom.us/j/1"), Some(teams)), "c").meeting_url.as_deref(), Some("https://zoom.us/j/1"));
        assert_eq!(build_event(raw(None, None), "c").meeting_url, None);
    }

    #[test]
    fn teams_links_go_to_the_teams_app() {
        assert_eq!(
            teams_app_link("https://teams.microsoft.com/l/meetup-join/19%3a1/0").as_deref(),
            Some("msteams:/l/meetup-join/19%3a1/0")
        );
        assert_eq!(teams_app_link("HTTPS://TEAMS.MICROSOFT.COM/l/x").as_deref(), Some("msteams:/l/x"));
        assert_eq!(teams_app_link("https://zoom.us/j/1"), None);
        assert_eq!(teams_app_link("https://teams.microsoft.com.evil.example/x"), None);
    }

    #[test]
    fn only_links_the_island_found_can_be_opened() {
        assert!(openable_meeting_link("https://zoom.us/j/1"));
        assert!(openable_meeting_link("http://zoom.us/j/1"));
        assert!(openable_meeting_link("https://meet.contoso.com/jdoe/1"));
        assert!(!openable_meeting_link("http://evil.example/x"));
        assert!(!openable_meeting_link("file:///C:/Windows/System32/cmd.exe"));
        assert!(!openable_meeting_link("calc.exe"));
        assert!(!openable_meeting_link("https://zoom.us/j/1 && calc"));
    }

    #[test]
    fn invite_answers_map_to_ol_meeting_response() {
        assert_eq!(InviteResponse::parse("accept").map(InviteResponse::ol_value), Some(3));
        assert_eq!(InviteResponse::parse("tentative").map(InviteResponse::ol_value), Some(2));
        assert_eq!(InviteResponse::parse("decline").map(InviteResponse::ol_value), Some(4));
        assert_eq!(InviteResponse::parse("Accept"), None);
        assert!(is_answered(2) && is_answered(3) && is_answered(4));
        assert!(!is_answered(0) && !is_answered(1) && !is_answered(5));
    }

    #[test]
    fn answering_needs_an_invite_from_the_latest_read() {
        remember_invites(vec![("abc".into(), "ENTRY".into())]);
        assert_eq!(invite_entry_id("abc").as_deref(), Some("ENTRY"));
        assert_eq!(invite_entry_id("other"), None);
        assert!(respond_invite("other", InviteResponse::Accept).unwrap_err().starts_with("OUTLOOK-108"));
        remember_invites(Vec::new());
    }

    #[test]
    fn invite_ids_are_stable_and_hide_the_entry_id() {
        let id = invite_id("SECRET-ENTRY-ID");
        assert_eq!(id, invite_id("SECRET-ENTRY-ID"));
        assert_ne!(id, invite_id("OTHER"));
        assert_eq!(id.len(), 16);
        assert!(!id.contains("SECRET"));
    }

    #[test]
    fn build_event_hides_entry_id_and_extracts_the_url() {
        let start = Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, 0).unwrap();
        let dto = build_event(
            RawItem {
                entry_id: "SECRET-ENTRY-ID".into(),
                subject: "x".repeat(500),
                start,
                end: start + chrono::Duration::minutes(30),
                location: Some("https://zoom.us/j/9".into()),
                organizer: None,
                all_day: false,
                recurring: true,
                busy: 2,
                response: 3,
                color: None,
                online_link: None,
            },
            "cal",
        );
        assert!(!dto.id.contains("SECRET"));
        assert_eq!(dto.id, event_id("SECRET-ENTRY-ID", &start));
        assert_eq!(dto.subject.chars().count(), MAX_TEXT_CHARS);
        assert_eq!(dto.meeting_url.as_deref(), Some("https://zoom.us/j/9"));
        assert!(dto.is_recurring);
    }

    #[test]
    fn long_join_urls_survive_while_the_location_text_is_clipped() {
        let start = Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, 0).unwrap();
        let url = format!("https://teams.microsoft.com/l/meetup-join/{}", "a".repeat(400));
        let dto = build_event(
            RawItem {
                entry_id: "id".into(),
                subject: "s".into(),
                start,
                end: start + chrono::Duration::minutes(30),
                location: non_empty(Some(format!("  {url}  ")), MAX_LOCATION_CHARS),
                organizer: None,
                all_day: false,
                recurring: false,
                busy: 2,
                response: 3,
                color: None,
                online_link: None,
            },
            "cal",
        );
        assert_eq!(dto.meeting_url.as_deref(), Some(url.as_str()));
        assert_eq!(dto.location.as_deref().map(|l| l.chars().count()), Some(MAX_TEXT_CHARS));
    }

    fn window() -> FetchWindow {
        let from = Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, 30).unwrap();
        FetchWindow { from, to: from + chrono::Duration::hours(48), invites: false }
    }

    #[test]
    fn fit_separates_edge_rounding_from_a_misparsed_filter() {
        let w = window();
        let m = chrono::Duration::minutes;
        assert_eq!(fit(w.from - m(30), w.from + m(30), &w), Fit::Inside);
        assert_eq!(fit(w.from - m(30), w.from - chrono::Duration::seconds(20), &w), Fit::Edge);
        assert_eq!(fit(w.from - m(60), w.from - m(5), &w), Fit::Stray);
        assert_eq!(fit(w.to + m(1), w.to + m(30), &w), Fit::Stray);
        assert_eq!(fit(w.to - m(1), w.to + m(30), &w), Fit::Inside);
    }

    fn scan_of(count: usize, stray: bool) -> Result<Scan, SourceError> {
        let start = window().from;
        let events = (0..count)
            .map(|i| {
                build_event(
                    RawItem {
                        entry_id: format!("e{i}"),
                        subject: String::new(),
                        start,
                        end: start + chrono::Duration::minutes(30),
                        location: None,
                        organizer: None,
                        all_day: false,
                        recurring: false,
                        busy: 2,
                        response: 3,
                        color: None,
                        online_link: None,
                    },
                    "cal",
                )
            })
            .collect();
        Ok(Scan { events, stray })
    }

    fn failed() -> Result<Scan, SourceError> {
        Err(SourceError::new(ErrKind::Failed, "OUTLOOK-108", "rejected"))
    }

    #[test]
    fn empty_first_read_defers_to_a_format_that_finds_events() {
        let (r, switched) = choose(scan_of(0, false), scan_of(2, false));
        assert_eq!((r.unwrap().len(), switched), (2, true));
        // Rejected or misparsed (stray) first reads are treated the same way.
        assert!(matches!(choose(failed(), scan_of(1, false)), (Ok(e), true) if e.len() == 1));
        assert!(matches!(choose(scan_of(0, true), scan_of(3, false)), (Ok(e), true) if e.len() == 3));
    }

    #[test]
    fn a_genuinely_empty_calendar_stays_empty_and_keeps_the_format() {
        let (r, switched) = choose(scan_of(0, false), scan_of(0, false));
        assert_eq!((r.unwrap().len(), switched), (0, false));
        // The second format also returned impossible items: not evidence, keep the first read.
        let (r, switched) = choose(scan_of(1, true), scan_of(2, true));
        assert_eq!((r.unwrap().len(), switched), (1, false));
    }

    #[test]
    fn choose_surfaces_transport_errors_and_total_failure() {
        let busy = Err(SourceError::new(ErrKind::Busy, "OUTLOOK-105", "busy"));
        assert_eq!(choose(scan_of(0, false), busy).0.unwrap_err().kind, ErrKind::Busy);
        let gone = Err(SourceError::new(ErrKind::Disconnected, "OUTLOOK-102", "gone"));
        assert_eq!(choose(failed(), gone).0.unwrap_err().kind, ErrKind::Disconnected);
        assert_eq!(choose(failed(), failed()).0.unwrap_err().code, "OUTLOOK-108");
        // First rejected, second readable but empty: an empty (not failed) calendar.
        assert_eq!(choose(failed(), scan_of(0, false)).0.unwrap().len(), 0);
    }

    #[test]
    fn a_panicking_read_answers_with_an_error_instead_of_hanging_the_watchdog() {
        struct Boom;
        impl CalendarSource for Boom {
            fn fetch(&mut self, _: &FetchWindow) -> FetchResult {
                panic!("simulated failure");
            }
        }
        let (tx, rx) = mpsc::channel();
        let worker = Worker::spawn(9, || Boom, Box::new(move |r| tx.send(r).unwrap())).unwrap();
        assert!(worker.fetch(FetchWindow::starting_at(Utc::now())));
        let reply = rx.recv_timeout(Duration::from_secs(5)).expect("worker answers even after a panic");
        assert_eq!(reply.unwrap_err().code, "OUTLOOK-108");
        // The worker survives and keeps answering.
        assert!(worker.fetch(FetchWindow::starting_at(Utc::now())));
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn restrict_filter_uses_the_injected_format() {
        let from = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap().and_hms_opt(9, 5, 0).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap().and_hms_opt(9, 5, 0).unwrap();
        let f = restrict_filter(from, to, |d| Some(d.format("%d/%m/%Y %H:%M").to_string())).unwrap();
        assert_eq!(f, "[End] >= '06/10/2026 09:05' AND [Start] <= '08/10/2026 09:05'");
        let us = restrict_filter(from, to, us_format).unwrap();
        assert_eq!(us, "[End] >= '10/06/2026 09:05 AM' AND [Start] <= '10/08/2026 09:05 AM'");
        assert!(restrict_filter(from, to, |_| None).is_none());
    }

    #[test]
    fn locale_format_renders_without_bidi_marks() {
        let dt = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap().and_hms_opt(14, 5, 0).unwrap();
        let s = locale_format(&dt).expect("locale formatting works on this machine");
        assert!(!s.contains('\u{200F}') && !s.contains('\u{200E}'));
        assert!(s.contains(' '));
        assert!(s.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn worker_round_trip_with_a_fake_source_and_clean_shutdown() {
        struct Echo;
        impl CalendarSource for Echo {
            fn fetch(&mut self, _: &FetchWindow) -> FetchResult {
                Err(SourceError::new(ErrKind::NotInRot, "OUTLOOK-102", "fake"))
            }
        }
        let (tx, rx) = mpsc::channel();
        let worker = Worker::spawn(7, || Echo, Box::new(move |r| tx.send(r).unwrap())).unwrap();
        for _ in 0..3 {
            assert!(worker.fetch(FetchWindow::starting_at(Utc::now())));
            let reply = rx.recv_timeout(Duration::from_secs(5)).expect("worker answers");
            assert_eq!(reply.unwrap_err().kind, ErrKind::NotInRot);
        }
        worker.shutdown();
        std::thread::sleep(Duration::from_millis(200));
        assert!(!worker.fetch(FetchWindow::starting_at(Utc::now())));
    }

    #[test]
    fn dropping_an_idle_worker_ends_its_thread_and_releases_the_source() {
        struct Tracked(mpsc::Sender<()>);
        impl CalendarSource for Tracked {
            fn fetch(&mut self, _: &FetchWindow) -> FetchResult {
                Ok(Fetched::default())
            }
        }
        impl Drop for Tracked {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (released_tx, released_rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel();
        let worker = Worker::spawn(8, move || Tracked(released_tx), Box::new(move |r| reply_tx.send(r).unwrap())).unwrap();
        assert!(worker.fetch(FetchWindow::starting_at(Utc::now())));
        reply_rx.recv_timeout(Duration::from_secs(5)).expect("worker answers").unwrap();
        // The thread is now parked in its wait; dropping the handle alone must wake it.
        drop(worker);
        released_rx.recv_timeout(Duration::from_secs(5)).expect("worker thread exits and drops its source");
    }

    /// Live, read-only check against an Outlook that is ALREADY running. Prints counts only.
    /// Run: `cargo test outlook_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn outlook_live() {
        use windows::Win32::System::Threading::GetProcessHandleCount;
        fn handles() -> u32 {
            let mut n = 0;
            unsafe {
                let _ = GetProcessHandleCount(GetCurrentProcess(), &mut n);
            }
            n
        }

        let handles_before_discovery = handles();
        let discovery = discover().expect("process discovery");
        for _ in 0..200 {
            discover().expect("process discovery");
        }
        println!(
            "live: discovery = {discovery:?}; 200 discoveries, process handles before={} after={}",
            handles_before_discovery,
            handles()
        );
        let apartment = ComApartment::init_sta().expect("STA");
        let _filter = MessageFilterGuard::register(&apartment).expect("message filter");
        let mut source = OutlookSource::default();
        let window = FetchWindow { invites: true, ..FetchWindow::starting_at(Utc::now()) };
        if !matches!(discovery, Discovery::Classic(_)) {
            // Attach-only: with no Outlook running this must fail fast and launch nothing.
            let started = std::time::Instant::now();
            let handles_before = handles();
            let probe = source.fetch(&window);
            println!(
                "live: status=waiting-path-only; attach probe -> {:?} ms={}; connected path NOT exercised",
                probe.as_ref().err().map(|e| (e.kind, e.code)),
                started.elapsed().as_millis()
            );
            for batch in 1..=4 {
                for _ in 0..200 {
                    let _ = source.fetch(&window);
                }
                println!("live: failed attach probes x{}, process handles before={} now={}", batch * 200, handles_before, handles());
            }
            return;
        }

        let started = std::time::Instant::now();
        let outcome = source.fetch(&window);
        let ms = started.elapsed().as_millis();
        match &outcome {
            Ok(Fetched { events, invites }) => println!(
                "live: status=connected errorCode=- events={} allDay={} recurring={} withMeetingUrl={} colored={} invites={} restrict={:?} ms={}",
                events.len(),
                events.iter().filter(|e| e.all_day).count(),
                events.iter().filter(|e| e.is_recurring).count(),
                events.iter().filter(|e| e.meeting_url.is_some()).count(),
                events.iter().filter(|e| e.color.is_some()).count(),
                invites.len(),
                source.last_restrict_mode,
                ms
            ),
            Err(e) => println!("live: status=error errorCode={} kind={:?} detail={} ms={}", e.code, e.kind, e.detail, ms),
        }
        // `fetch` swallows these two on purpose; here their errors are shown (HRESULTs only).
        {
            let mut app = Dispatch::get_active(PROG_ID).expect("attach");
            let mut session = app.get_object("Session").expect("session");
            println!(
                "live: colored categories={:?} unread meeting requests={:?}",
                read_category_colors(&mut session).map(|c| c.len()).map_err(|e| e.to_string()),
                read_invites(&mut session, Utc::now()).map(|v| v.len()).map_err(|e| e.to_string())
            );
        }

        let before = handles();
        let loop_started = std::time::Instant::now();
        let mut ok = 0;
        for _ in 0..200 {
            if source.fetch(&window).is_ok() {
                ok += 1;
            }
        }
        println!(
            "live: 200 attach/read/release cycles ok={} in {}ms; process handles before={} after={}",
            ok,
            loop_started.elapsed().as_millis(),
            before,
            handles()
        );
        drop(source);
    }
}
