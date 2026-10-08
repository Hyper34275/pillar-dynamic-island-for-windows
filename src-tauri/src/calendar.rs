//! Calendar service: contract types, the connection state machine and the supervisor.
//!
//! The supervisor thread (`companyisland-calendar`) owns the [`Machine`], does the cheap
//! process discovery itself and sends Outlook reads to a dedicated STA worker
//! (`outlook::Worker`). It doubles as the watchdog: a read that does not answer within
//! [`WATCHDOG_MS`] makes it abandon the (hung) worker and start a fresh one later.
//!
//! The machine is pure (no clock, no IO): every input carries a [`Now`], so schedules
//! and backoff are unit-tested with a simulated clock and a fake source.
//!
//! Privacy: events live only in memory. Logs carry counts, status names and error codes.

use crate::calendar_diag::{self, OutlookDiag};
use crate::outlook::{self, Discovery};
use crate::settings::SettingsStore;
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, Utc};
use serde::{Serialize, Serializer};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

// =============================================================================
// Contract types (camelCase over IPC, see docs/ENTERPRISE_DESIGN.md section 1)
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CalendarStatus {
    Waiting,
    Connecting,
    Connected,
    NewOutlookOnly,
    ElevationMismatch,
    Unresponsive,
    Failed,
}

impl CalendarStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CalendarStatus::Waiting => "waiting",
            CalendarStatus::Connecting => "connecting",
            CalendarStatus::Connected => "connected",
            CalendarStatus::NewOutlookOnly => "newOutlookOnly",
            CalendarStatus::ElevationMismatch => "elevationMismatch",
            CalendarStatus::Unresponsive => "unresponsive",
            CalendarStatus::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BusyStatus {
    Free,
    Tentative,
    Busy,
    Oof,
    WorkingElsewhere,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseStatus {
    None,
    Organized,
    Tentative,
    Accepted,
    Declined,
    NotResponded,
}

fn serialize_iso<S: Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

fn serialize_iso_opt<S: Serializer>(t: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match t {
        Some(t) => serialize_iso(t, s),
        None => s.serialize_none(),
    }
}

/// What a calendar is to the user, decided from Outlook's navigation group type and the
/// folder's store, never from a (localized) display name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// The profile's default calendar.
    Primary,
    /// Another calendar in the user's own mailbox.
    Personal,
    /// Another person's (or a shared mailbox's) calendar.
    Shared,
    /// Other Calendars, rooms and the like.
    Other,
}

/// The Calendar navigation group a calendar sits in (`OlGroupType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceGroup {
    /// olMyFoldersGroup (1)
    My,
    /// olPeopleFoldersGroup (2): "Shared Calendars"
    Shared,
    /// olOtherFoldersGroup (3)
    Other,
    /// olRoomsGroup (5)
    Rooms,
    /// olCustomFoldersGroup (0) and anything else: a group the user made
    Custom,
    /// Not (yet) seen in the navigation pane (the default calendar before Outlook shows a window).
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceState {
    /// Active and read (now or recently).
    Ok,
    /// Discovered, not checked in Outlook: contributes nothing.
    NotSelected,
    /// Active but could not be read; `error_code` says why. The other calendars are unaffected.
    Unavailable,
    /// Active, not read yet (time budget of this sync); last known events are kept meanwhile.
    Pending,
}

/// One calendar the user has in Outlook's Calendar module. Discovery (all of them) is kept apart
/// from activity (the ones that contribute events).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSourceDto {
    /// sha256(StoreID|EntryID), 16 hex; the same id the events carry as `calendarId`.
    pub id: String,
    /// Display name as Outlook shows it (presentation only; never logged).
    pub name: String,
    pub group: SourceGroup,
    pub kind: SourceKind,
    /// Checked in Outlook (as last known; see [`SelectionOrigin`]).
    pub selected: bool,
    /// Contributes events: selected, or the primary calendar (always on).
    pub active: bool,
    /// Switched in the island; Outlook's own checkbox follows once Outlook shows its calendar.
    pub pending_in_outlook: bool,
    /// `#RRGGBB` the calendar has in Outlook's pane (for an active one, also an automatic color).
    pub color: Option<String>,
    pub state: SourceState,
    /// e.g. "CAL-SHARED-101"
    pub error_code: Option<String>,
    pub event_count: usize,
    pub last_read_unix_ms: Option<i64>,
}

/// Where the checked state of the last discovery came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionOrigin {
    /// Read from Outlook's Calendar navigation pane just now.
    Outlook,
    /// Outlook is not showing its calendar: the last selection read from it.
    Remembered,
    /// Nothing known yet: only the primary calendar.
    PrimaryOnly,
}

/// The calendars of the latest discovery. Memory only, like events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcesReport {
    pub sources: Vec<CalendarSourceDto>,
    pub selection: SelectionOrigin,
    /// Calendar navigation groups seen.
    pub groups: usize,
    /// The navigation pane's change notifications are connected.
    pub listener: bool,
    pub discovered_unix_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventDto {
    pub id: String,
    pub calendar_id: String,
    /// The source calendar's display name (presentation only).
    pub calendar_name: String,
    pub source_kind: SourceKind,
    /// Hash of the meeting's GlobalAppointmentID (the same in every calendar that shows it), read
    /// only while several calendars are active. Used to drop copies; never sent to the page.
    #[serde(skip)]
    pub meeting_key: Option<String>,
    pub subject: String,
    #[serde(serialize_with = "serialize_iso")]
    pub start_utc: DateTime<Utc>,
    #[serde(serialize_with = "serialize_iso")]
    pub end_utc: DateTime<Utc>,
    pub all_day: bool,
    pub location: Option<String>,
    pub organizer: Option<String>,
    pub is_recurring: bool,
    pub meeting_url: Option<String>,
    pub busy_status: BusyStatus,
    pub response_status: ResponseStatus,
    /// `#RRGGBB` of the first of the item's Outlook categories that has a color.
    pub color: Option<String>,
    /// `#RRGGBB` of its calendar in Outlook's pane, only while several calendars are active.
    pub calendar_color: Option<String>,
}

/// An unread meeting request in the default Inbox. Memory only, like events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingInviteDto {
    /// Hash of the request's EntryID; the EntryID itself never leaves the COM worker.
    pub id: String,
    pub subject: String,
    pub organizer: Option<String>,
    /// The requested meeting, when Outlook can tell (its tentative calendar entry).
    #[serde(serialize_with = "serialize_iso_opt")]
    pub start_utc: Option<DateTime<Utc>>,
    #[serde(serialize_with = "serialize_iso_opt")]
    pub end_utc: Option<DateTime<Utc>>,
    pub location: Option<String>,
    #[serde(serialize_with = "serialize_iso")]
    pub received_utc: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSnapshot {
    pub status: CalendarStatus,
    pub error_code: Option<String>,
    pub last_sync_unix_ms: Option<i64>,
    pub cached_count: usize,
    pub next_retry_unix_ms: Option<i64>,
    pub events: Vec<CalendarEventDto>,
    /// Newest first, at most [`MAX_INVITES`]; empty while invites are switched off.
    pub invites: Vec<MeetingInviteDto>,
    /// The calendars of the latest discovery (kept, like events, while Outlook is away).
    pub sources: Option<SourcesReport>,
}

impl Default for CalendarSnapshot {
    fn default() -> Self {
        CalendarSnapshot {
            status: CalendarStatus::Waiting,
            error_code: None,
            last_sync_unix_ms: None,
            cached_count: 0,
            next_retry_unix_ms: None,
            events: Vec::new(),
            invites: Vec::new(),
            sources: None,
        }
    }
}

/// Managed Tauri state: the latest published snapshot.
#[derive(Default)]
pub struct CalendarState(Mutex<CalendarSnapshot>);

impl CalendarState {
    /// Store `next`; true when it differs from what was stored (and so must be emitted).
    fn replace_if_changed(&self, next: &CalendarSnapshot) -> bool {
        let mut current = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if *current == *next {
            return false;
        }
        *current = next.clone();
        true
    }

    fn get(&self) -> CalendarSnapshot {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

// =============================================================================
// Source abstraction
// =============================================================================

pub const MAX_EVENTS: usize = 50;
/// Per-read event cap of a smart search read (the island's own range reads keep [`MAX_EVENTS`]).
pub const SEARCH_MAX_EVENTS: usize = 300;
pub const MAX_INVITES: usize = 10;
pub const HORIZON_HOURS: i64 = 48;

#[derive(Clone, Debug)]
pub struct FetchWindow {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Also read the unread meeting requests (the `meetingInvitesEnabled` setting).
    pub invites: bool,
    /// A day the user browses to, not the regular sync: every active calendar is read for it
    /// and nothing is cached or rediscovered.
    pub range: bool,
    /// Smart search only (needs `range`): read exactly these calendar ids, selected in Outlook or
    /// not, and nothing else. `None` = the active calendars (today's behaviour).
    pub only: Option<Vec<String>>,
    /// Smart search read: the larger [`SEARCH_MAX_EVENTS`] cap, and `Fetched::truncated` is reported.
    pub search: bool,
}

impl FetchWindow {
    pub fn starting_at(now: DateTime<Utc>) -> Self {
        FetchWindow {
            from: now,
            to: now + ChronoDuration::hours(HORIZON_HOURS),
            invites: false,
            range: false,
            only: None,
            search: false,
        }
    }

    /// Most events one read of this window may return.
    pub fn event_cap(&self) -> usize {
        if self.search {
            SEARCH_MAX_EVENTS
        } else {
            MAX_EVENTS
        }
    }
}

/// Whether a calendar other than the primary one is read for a window: the checked ones, or
/// (smart search) exactly the ids in `only`, checked or not. Pure.
pub fn reads_secondary(only: Option<&[String]>, id: &str, selected: bool) -> bool {
    match only {
        None => selected,
        Some(ids) => ids.iter().any(|i| i == id),
    }
}

/// Whether the primary calendar is read: always, unless `only` is set and does not list it. Pure.
pub fn reads_primary(only: Option<&[String]>, primary_id: &str) -> bool {
    match only {
        None => true,
        Some(ids) => ids.iter().any(|i| i == primary_id),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrKind {
    /// Outlook runs but is not (yet) in the running object table.
    NotInRot,
    /// Call rejected: the server is busy (modal dialog, startup).
    Busy,
    /// The server went away; every cached object must be dropped.
    Disconnected,
    /// Object model guard or policy blocked the read.
    Blocked,
    Failed,
}

#[derive(Clone, Debug)]
pub struct SourceError {
    pub kind: ErrKind,
    pub code: &'static str,
    /// HRESULT/stage text only; never item content.
    pub detail: String,
}

impl SourceError {
    pub fn new(kind: ErrKind, code: &'static str, detail: impl Into<String>) -> Self {
        SourceError { kind, code, detail: detail.into() }
    }
}

/// What one read returns: the events inside the window and, when asked for, the invites.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetched {
    pub events: Vec<CalendarEventDto>,
    pub invites: Vec<MeetingInviteDto>,
    /// The calendars discovered by this read (`None` for range reads and sources without discovery).
    pub sources: Option<SourcesReport>,
    /// Search reads only: events were cut by the cap, or a calendar's scan cap was reached.
    pub truncated: bool,
    /// Search reads only: calendars of `FetchWindow::only` that could not be read, (id, code).
    pub failed: Vec<(String, String)>,
}

impl From<Vec<CalendarEventDto>> for Fetched {
    fn from(events: Vec<CalendarEventDto>) -> Self {
        Fetched { events, invites: Vec::new(), sources: None, truncated: false, failed: Vec::new() }
    }
}

pub type FetchResult = Result<Fetched, SourceError>;

/// A calendar backend. `fetch` returns the events (all calendars it covers) inside the window.
pub trait CalendarSource {
    fn fetch(&mut self, window: &FetchWindow) -> FetchResult;

    /// Called on the worker thread whenever it is idle and has dispatched its messages. True
    /// when the source learned (from Outlook's own notifications) that its calendars changed
    /// and a read should follow soon.
    fn poll_changes(&mut self) -> bool {
        false
    }
}

/// Sort by start, drop duplicate ids, cap at [`MAX_EVENTS`].
pub fn normalize_events(events: Vec<CalendarEventDto>) -> Vec<CalendarEventDto> {
    normalize_events_capped(events, MAX_EVENTS).0
}

/// [`normalize_events`] with another cap; also says whether events were cut.
pub fn normalize_events_capped(mut events: Vec<CalendarEventDto>, cap: usize) -> (Vec<CalendarEventDto>, bool) {
    events.sort_by(|a, b| a.start_utc.cmp(&b.start_utc).then(a.end_utc.cmp(&b.end_utc)).then_with(|| a.id.cmp(&b.id)));
    events.dedup_by(|a, b| a.id == b.id);
    let cut = events.len() > cap;
    events.truncate(cap);
    (events, cut)
}

/// Drop the copies of a meeting that several active calendars show (your own calendar and a
/// colleague's shared one, say). Two events are the same meeting only when their
/// GlobalAppointmentID (hashed into `meeting_key`) AND their start AND end agree: occurrences
/// of one series share the id but not the times, and a subject is never an identity. Events
/// without a key are never merged. `events` must be in source priority order (primary
/// calendar first); the first copy wins, so a meeting keeps its id (and its reminder key)
/// from one sync to the next.
pub fn dedup_meetings(events: Vec<CalendarEventDto>) -> Vec<CalendarEventDto> {
    let mut seen = std::collections::HashSet::new();
    events
        .into_iter()
        .filter(|e| match &e.meeting_key {
            Some(key) => seen.insert((key.clone(), e.start_utc, e.end_utc)),
            None => true,
        })
        .collect()
}

// =============================================================================
// State machine
// =============================================================================

const DISCOVER_POLL_MS: u64 = 15_000;
const SYNC_INTERVAL_MS: u64 = 60_000;
const CONNECT_SCHEDULE_MS: [u64; 6] = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000];
const CONNECT_MAX_ATTEMPTS: u32 = 8;
const FAIL_SCHEDULE_MS: [u64; 7] = [5_000, 10_000, 20_000, 40_000, 60_000, 120_000, 300_000];
const BUSY_UNRESPONSIVE_AFTER: u32 = 3;
const ABANDONED_RETRY_MS: u64 = 300_000;
const MIN_REFRESH_GAP_MS: u64 = 2_000;
/// Wall clock running this far ahead of the monotonic clock means the PC slept.
const RESUME_SLACK_MS: i64 = 20_000;
pub const WATCHDOG_MS: u64 = 10_000;
/// Abandoned (hung) workers tolerated before the machine settles on `failed`.
pub const MAX_ABANDONED: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Cheap process-list check; decides whether an attach is worth trying.
    Discover,
    /// Attach (if needed), read the calendar, release.
    Fetch,
}

#[derive(Clone, Copy, Debug)]
pub struct Now {
    pub mono_ms: u64,
    pub unix_ms: i64,
    /// Local calendar day (days from CE), to resync at midnight.
    pub day: i32,
}

pub struct Machine {
    status: CalendarStatus,
    error_code: Option<&'static str>,
    events: Vec<CalendarEventDto>,
    invites: Vec<MeetingInviteDto>,
    sources: Option<SourcesReport>,
    last_sync_unix_ms: Option<i64>,
    next_retry_unix_ms: Option<i64>,
    action: Action,
    at: u64,
    /// No fetch before this instant (sync interval or backoff).
    fetch_not_before: u64,
    last_fetch_at: Option<u64>,
    connect_attempts: u32,
    fail_attempts: u32,
    busy_streak: u32,
    abandoned: u32,
    synced_day: Option<i32>,
    /// The Outlook process the current attempts and backoff belong to.
    outlook_pid: Option<u32>,
    last_tick: Option<(u64, i64)>,
    /// xorshift state for backoff jitter; `None` disables jitter (tests).
    rng: Option<u64>,
}

impl Machine {
    pub fn new(jitter_seed: Option<u64>) -> Self {
        Machine {
            status: CalendarStatus::Waiting,
            error_code: None,
            events: Vec::new(),
            invites: Vec::new(),
            sources: None,
            last_sync_unix_ms: None,
            next_retry_unix_ms: None,
            action: Action::Discover,
            at: 0,
            fetch_not_before: 0,
            last_fetch_at: None,
            connect_attempts: 0,
            fail_attempts: 0,
            busy_streak: 0,
            abandoned: 0,
            synced_day: None,
            outlook_pid: None,
            last_tick: None,
            rng: jitter_seed.map(|s| s | 1),
        }
    }

    /// The next thing to do and the monotonic instant it is due.
    pub fn next(&self) -> (Action, u64) {
        (self.action, self.at)
    }

    pub fn status(&self) -> CalendarStatus {
        self.status
    }

    pub fn error_code(&self) -> Option<&'static str> {
        self.error_code
    }

    pub fn abandoned(&self) -> u32 {
        self.abandoned
    }

    pub fn snapshot(&self, now_unix_ms: i64) -> CalendarSnapshot {
        let events: Vec<CalendarEventDto> =
            self.events.iter().filter(|e| e.end_utc.timestamp_millis() > now_unix_ms).cloned().collect();
        CalendarSnapshot {
            status: self.status,
            error_code: self.error_code.map(str::to_string),
            last_sync_unix_ms: self.last_sync_unix_ms,
            cached_count: events.len(),
            next_retry_unix_ms: self.next_retry_unix_ms,
            events,
            invites: self.invites.clone(),
            sources: self.sources.clone(),
        }
    }

    /// The invites of the latest successful read (replaced as a whole, like events).
    pub fn set_invites(&mut self, mut invites: Vec<MeetingInviteDto>) {
        invites.truncate(MAX_INVITES);
        self.invites = invites;
    }

    /// The calendars of the latest discovery. A read without one (a source that does not
    /// discover) keeps the last; like events, they stay while Outlook is away.
    pub fn set_sources(&mut self, sources: Option<SourcesReport>) {
        if sources.is_some() {
            self.sources = sources;
        }
    }

    fn jittered(&mut self, base_ms: u64) -> u64 {
        let Some(state) = self.rng.as_mut() else { return base_ms };
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        // +-20 %
        base_ms * (800 + x % 401) / 1000
    }

    fn fail_delay(&mut self) -> u64 {
        let idx = (self.fail_attempts.max(1) as usize - 1).min(FAIL_SCHEDULE_MS.len() - 1);
        self.jittered(FAIL_SCHEDULE_MS[idx])
    }

    fn connect_delay(&self) -> u64 {
        let idx = (self.connect_attempts.max(1) as usize - 1).min(CONNECT_SCHEDULE_MS.len() - 1);
        CONNECT_SCHEDULE_MS[idx]
    }

    /// Poll the process list soon, but never past the fetch gate.
    fn schedule_poll(&mut self, now: &Now) {
        self.action = Action::Discover;
        self.at = self.fetch_not_before.min(now.mono_ms + DISCOVER_POLL_MS).max(now.mono_ms);
    }

    fn retry_in(&mut self, now: &Now, delay_ms: u64) {
        self.fetch_not_before = now.mono_ms + delay_ms;
        self.next_retry_unix_ms = Some(now.unix_ms + delay_ms as i64);
        self.schedule_poll(now);
    }

    fn set_idle(&mut self, now: &Now, status: CalendarStatus, code: &'static str) {
        self.status = status;
        self.error_code = Some(code);
        self.next_retry_unix_ms = None;
        self.connect_attempts = 0;
        self.fail_attempts = 0;
        self.busy_streak = 0;
        // A different Outlook process is a fresh chance for a previously hung one.
        self.abandoned = 0;
        self.outlook_pid = None;
        self.fetch_not_before = now.mono_ms;
        self.action = Action::Discover;
        self.at = now.mono_ms + DISCOVER_POLL_MS;
    }

    fn fail(&mut self, now: &Now, status: CalendarStatus, code: &'static str, delay_ms: u64) {
        self.status = status;
        self.error_code = Some(code);
        self.retry_in(now, delay_ms);
    }

    /// Call before choosing the next action. Detects resume-from-sleep (wall clock ran ahead
    /// of the monotonic one) and the local date change. True when a resume was detected.
    pub fn on_tick(&mut self, now: &Now) -> bool {
        let resumed = self
            .last_tick
            .is_some_and(|(mono, unix)| (now.unix_ms - unix) - (now.mono_ms.saturating_sub(mono) as i64) > RESUME_SLACK_MS);
        self.last_tick = Some((now.mono_ms, now.unix_ms));
        if resumed {
            self.connect_attempts = 0;
            self.fail_attempts = 0;
            self.busy_streak = 0;
            self.fetch_not_before = now.mono_ms;
            self.action = Action::Discover;
            self.at = now.mono_ms;
            if self.next_retry_unix_ms.is_some() {
                self.next_retry_unix_ms = Some(now.unix_ms);
            }
        } else if self.status == CalendarStatus::Connected && self.synced_day.is_some_and(|d| d != now.day) {
            // One-shot: if the read answers "busy" the status stays connected, and the
            // backoff must not be overridden again on every tick.
            self.synced_day = None;
            self.fetch_not_before = self.fetch_not_before.min(now.mono_ms);
            if self.action == Action::Discover {
                self.at = self.at.min(now.mono_ms);
            }
        }
        resumed
    }

    /// Explicit user refresh: try now (rate-limited), unless the machine has given up on
    /// a hung Outlook.
    pub fn on_refresh(&mut self, now: &Now) {
        if self.abandoned >= MAX_ABANDONED {
            return;
        }
        self.connect_attempts = 0;
        self.fail_attempts = 0;
        self.busy_streak = 0;
        let earliest = self.last_fetch_at.map_or(now.mono_ms, |t| t + MIN_REFRESH_GAP_MS).max(now.mono_ms);
        self.fetch_not_before = earliest;
        if self.next_retry_unix_ms.is_some() {
            self.next_retry_unix_ms = Some(now.unix_ms + (earliest - now.mono_ms) as i64);
        }
        self.action = Action::Discover;
        self.at = now.mono_ms;
    }

    pub fn on_discovery(&mut self, now: &Now, discovery: Discovery) {
        match discovery {
            Discovery::Waiting => self.set_idle(now, CalendarStatus::Waiting, "OUTLOOK-101"),
            Discovery::NewOutlookOnly => self.set_idle(now, CalendarStatus::NewOutlookOnly, "OUTLOOK-104"),
            Discovery::ElevationMismatch => self.set_idle(now, CalendarStatus::ElevationMismatch, "OUTLOOK-103"),
            Discovery::Classic(pid) => {
                if self.outlook_pid.is_some_and(|known| known != pid) {
                    // Outlook was restarted: whatever the old process earned (backoff,
                    // hung-worker verdict) does not apply to the new one.
                    self.connect_attempts = 0;
                    self.fail_attempts = 0;
                    self.busy_streak = 0;
                    self.abandoned = 0;
                    self.fetch_not_before = now.mono_ms;
                    if matches!(self.status, CalendarStatus::Failed | CalendarStatus::Unresponsive) {
                        self.status = CalendarStatus::Connecting;
                        self.error_code = None;
                        self.next_retry_unix_ms = None;
                    }
                }
                self.outlook_pid = Some(pid);
                if matches!(
                    self.status,
                    CalendarStatus::Waiting | CalendarStatus::NewOutlookOnly | CalendarStatus::ElevationMismatch
                ) {
                    self.status = CalendarStatus::Connecting;
                    self.error_code = None;
                    self.next_retry_unix_ms = None;
                }
                if now.mono_ms >= self.fetch_not_before {
                    self.action = Action::Fetch;
                    self.at = now.mono_ms;
                } else {
                    self.schedule_poll(now);
                }
            }
        }
    }

    /// The process list could not be read (or our own identity could not be established).
    pub fn on_discovery_error(&mut self, now: &Now) {
        self.fail_attempts += 1;
        let delay = self.fail_delay();
        self.fail(now, CalendarStatus::Failed, "OUTLOOK-102", delay);
    }

    pub fn on_fetch_started(&mut self, now: &Now) {
        self.last_fetch_at = Some(now.mono_ms);
    }

    pub fn on_fetch_ok(&mut self, now: &Now, events: Vec<CalendarEventDto>) {
        self.events = normalize_events(events);
        self.status = CalendarStatus::Connected;
        self.error_code = None;
        self.next_retry_unix_ms = None;
        self.last_sync_unix_ms = Some(now.unix_ms);
        self.synced_day = Some(now.day);
        self.connect_attempts = 0;
        self.fail_attempts = 0;
        self.busy_streak = 0;
        self.abandoned = 0;
        self.fetch_not_before = now.mono_ms + SYNC_INTERVAL_MS;
        self.schedule_poll(now);
    }

    pub fn on_fetch_err(&mut self, now: &Now, err: &SourceError) {
        match err.kind {
            ErrKind::NotInRot | ErrKind::Disconnected => {
                self.connect_attempts += 1;
                if self.connect_attempts > CONNECT_MAX_ATTEMPTS {
                    self.fail_attempts += 1;
                    let delay = self.fail_delay();
                    self.fail(now, CalendarStatus::Failed, "OUTLOOK-102", delay);
                } else {
                    let delay = self.connect_delay();
                    self.status = CalendarStatus::Connecting;
                    self.error_code = None;
                    self.retry_in(now, delay);
                }
            }
            ErrKind::Busy => {
                self.busy_streak += 1;
                self.fail_attempts += 1;
                let delay = self.fail_delay();
                self.status = if self.busy_streak >= BUSY_UNRESPONSIVE_AFTER {
                    CalendarStatus::Unresponsive
                } else if self.status == CalendarStatus::Connected {
                    CalendarStatus::Connected
                } else {
                    CalendarStatus::Connecting
                };
                self.error_code = Some("OUTLOOK-105");
                self.retry_in(now, delay);
            }
            ErrKind::Blocked => {
                self.fail_attempts += 1;
                let delay = self.fail_delay();
                self.fail(now, CalendarStatus::Failed, "OUTLOOK-110", delay);
            }
            ErrKind::Failed => {
                self.fail_attempts += 1;
                let delay = self.fail_delay();
                self.fail(now, CalendarStatus::Failed, err.code, delay);
            }
        }
    }

    /// The watchdog abandoned a worker that did not answer in time.
    pub fn on_timeout(&mut self, now: &Now) {
        self.abandoned += 1;
        self.fail_attempts += 1;
        let mut delay = self.fail_delay();
        let status = if self.abandoned >= MAX_ABANDONED {
            delay = delay.max(ABANDONED_RETRY_MS);
            CalendarStatus::Failed
        } else {
            CalendarStatus::Unresponsive
        };
        self.fail(now, status, "OUTLOOK-109", delay);
    }
}

// =============================================================================
// Supervisor
// =============================================================================

enum Msg {
    Refresh,
    /// Outlook reported a change in its Calendar navigation pane (a calendar checked,
    /// unchecked, added or removed): read again soon.
    NavChanged,
    Shutdown,
    Reply { generation: u64, result: FetchResult },
    /// Read another stretch of the calendar (a day the user browses to) through the same worker.
    /// It changes nothing in the machine: the regular 48 h sync stays what reminders follow.
    Range { window: FetchWindow, reply: Sender<FetchResult> },
    /// The `calendarPrefetchDays` setting changed.
    PrefetchSettings,
}

/// Longest stretch one `calendar_get_range` may read.
const MAX_RANGE_DAYS: i64 = 7;

struct Control {
    tx: Sender<Msg>,
    done: Receiver<()>,
}

static CONTROL: Mutex<Option<Control>> = Mutex::new(None);

/// Longest the supervisor sleeps in one go, so sleep/resume and midnight are noticed.
const MAX_IDLE_WAIT_MS: u64 = 30_000;

enum Outcome {
    Done(FetchResult),
    TimedOut,
    Shutdown,
}

struct Supervisor {
    app: AppHandle,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    machine: Machine,
    worker: Option<outlook::Worker>,
    generation: u64,
    started: Instant,
    last_logged: (CalendarStatus, Option<&'static str>),
    /// Range reads asked for while the worker was busy; served before the next scheduled step.
    pending_ranges: Vec<(FetchWindow, Sender<FetchResult>)>,
    /// A navigation change arrived during a read that may have started before it.
    nav_changed_during_read: bool,
    /// Background schedule prefetch (lowest priority, see "Schedule prefetch").
    pf: PrefetchSched,
    /// A prefetch batch is on the worker right now.
    in_prefetch: bool,
    /// A refresh arrived during a prefetch batch (which does not satisfy it).
    refresh_after_prefetch: bool,
}

impl Supervisor {
    fn now(&self) -> Now {
        let local = Local::now();
        Now {
            mono_ms: self.started.elapsed().as_millis() as u64,
            unix_ms: local.timestamp_millis(),
            day: local.date_naive().num_days_from_ce(),
        }
    }

    fn publish(&mut self, now: &Now) {
        let snapshot = self.machine.snapshot(now.unix_ms);
        let state = self.app.state::<CalendarState>();
        if state.replace_if_changed(&snapshot) {
            let _ = self.app.emit("calendar-snapshot", &snapshot);
        }
        calendar_diag::set(OutlookDiag {
            status: snapshot.status.as_str().into(),
            error_code: snapshot.error_code.clone(),
            mode: match snapshot.status {
                CalendarStatus::Waiting => "none",
                CalendarStatus::NewOutlookOnly => "new",
                _ => "classic",
            }
            .into(),
            last_sync_unix_ms: snapshot.last_sync_unix_ms,
            cached_count: snapshot.cached_count,
        });
        let logged = (self.machine.status(), self.machine.error_code());
        if logged != self.last_logged {
            dlog!(
                "INFO",
                "calendar",
                "status {} -> {} code={} events={}",
                self.last_logged.0.as_str(),
                logged.0.as_str(),
                logged.1.unwrap_or("-"),
                snapshot.cached_count
            );
            self.last_logged = logged;
        }
    }

    fn drop_worker(&mut self, graceful: bool) {
        if let Some(worker) = self.worker.take() {
            if graceful {
                worker.shutdown();
            }
        }
    }

    fn discover(&mut self) {
        let now = self.now();
        match outlook::discover() {
            Ok(d) => {
                if !matches!(d, Discovery::Classic(_)) {
                    // Nothing to read: let the STA worker (and its apartment) go.
                    self.drop_worker(true);
                }
                self.machine.on_discovery(&now, d);
            }
            Err(e) => {
                dlog!("WARN", "calendar", "OUTLOOK-102 process discovery failed: {}", e);
                self.machine.on_discovery_error(&now);
            }
        }
    }

    fn reply_fn(&self, generation: u64) -> outlook::ReplyFn {
        let tx = self.tx.clone();
        Box::new(move |result| {
            let _ = tx.send(Msg::Reply { generation, result });
        })
    }

    fn notify_fn(&self) -> outlook::NotifyFn {
        let tx = self.tx.clone();
        Box::new(move || {
            let _ = tx.send(Msg::NavChanged);
        })
    }

    /// Outlook's calendar selection changed: read again now (rate-limited like a refresh).
    fn on_nav_changed(&mut self) {
        dlog!("INFO", "calendar", "calendar navigation changed in Outlook, re-reading");
        let now = self.now();
        self.machine.on_refresh(&now);
        // A calendar may have been added: read the schedule ahead again soon.
        if self.pf.armed {
            self.pf.due = Some(now.mono_ms + PREFETCH_NAV_MS);
            self.pf.round = None;
        }
    }

    /// Hand the window to the worker and wait up to the watchdog limit for its answer.
    fn run_fetch(&mut self, window: FetchWindow) -> Outcome {
        if self.worker.is_none() {
            self.generation += 1;
            let generation = self.generation;
            match outlook::Worker::spawn_outlook(generation, self.reply_fn(generation), self.notify_fn()) {
                Ok(w) => self.worker = Some(w),
                Err(e) => {
                    return Outcome::Done(Err(SourceError::new(ErrKind::Failed, "OUTLOOK-102", format!("worker spawn failed: {e}"))));
                }
            }
        }
        let generation = self.generation;
        if !self.worker.as_ref().is_some_and(|w| w.fetch(window)) {
            self.worker = None;
            return Outcome::Done(Err(SourceError::new(ErrKind::Disconnected, "OUTLOOK-102", "worker is gone")));
        }
        let deadline = Instant::now() + Duration::from_millis(WATCHDOG_MS);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(remaining) {
                Ok(Msg::Reply { generation: g, result }) if g == generation => return Outcome::Done(result),
                Ok(Msg::Reply { .. }) => {}
                Ok(Msg::Refresh) => {
                    // Already reading; the refresh is satisfied by this read, unless this is only
                    // a prefetch batch.
                    if self.in_prefetch {
                        self.refresh_after_prefetch = true;
                    }
                }
                Ok(Msg::PrefetchSettings) => self.pf.settings_changed = true,
                // This read may have looked at the navigation pane before the change.
                Ok(Msg::NavChanged) => self.nav_changed_during_read = true,
                Ok(Msg::Range { window, reply }) => self.pending_ranges.push((window, reply)),
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => return Outcome::Shutdown,
                Err(RecvTimeoutError::Timeout) => return Outcome::TimedOut,
            }
        }
    }

    /// Serve one range read. Only while connected: otherwise the answer is the current status code.
    fn fetch_range(&mut self, window: FetchWindow, reply: Sender<FetchResult>) -> bool {
        if self.machine.status() != CalendarStatus::Connected {
            let code = self.machine.error_code().unwrap_or("OUTLOOK-101");
            let _ = reply.send(Err(SourceError::new(ErrKind::Failed, code, "calendar not connected")));
            return true;
        }
        let search = window.search;
        match self.run_fetch(window) {
            Outcome::Shutdown => return false,
            Outcome::Done(result) => {
                let _ = reply.send(result);
            }
            Outcome::TimedOut => {
                // Same as a hung sync: abandon that worker and leave the calendar it was reading
                // out for a while, so repeating the question does not wait the watchdog again.
                self.drop_worker(false);
                if outlook::quarantine_reading_source() {
                    dlog!("WARN", "calendar", "CAL-SHARED-104 a calendar did not answer; skipped for a while");
                }
                let now = self.now();
                on_range_timeout(&mut self.machine, &now, search);
                dlog!("ERROR", "calendar", "OUTLOOK-109 watchdog: range read did not answer in {}ms", WATCHDOG_MS);
                let _ = reply.send(Err(SourceError::new(ErrKind::Busy, "OUTLOOK-109", "watchdog")));
            }
        }
        true
    }

    fn fetch(&mut self) -> bool {
        let started = self.now();
        self.machine.on_fetch_started(&started);
        let invites = self.app.state::<SettingsStore>().get().meeting_invites_enabled;
        let outcome = self.run_fetch(FetchWindow { invites, ..FetchWindow::starting_at(Utc::now()) });
        let now = self.now();
        match outcome {
            Outcome::Shutdown => return false,
            Outcome::Done(Ok(fetched)) => {
                dlog!(
                    "DEBUG",
                    "calendar",
                    "sync ok: {} events, {} invites in {}ms",
                    fetched.events.len(),
                    fetched.invites.len(),
                    now.mono_ms.saturating_sub(started.mono_ms)
                );
                self.machine.on_fetch_ok(&now, fetched.events);
                self.machine.set_invites(fetched.invites);
                self.machine.set_sources(fetched.sources);
            }
            Outcome::Done(Err(err)) => {
                dlog!("WARN", "calendar", "{} read failed ({:?}): {}", err.code, err.kind, err.detail);
                self.machine.on_fetch_err(&now, &err);
            }
            Outcome::TimedOut => {
                // A hung STA call cannot be cancelled: leave that thread (and its COM
                // references) alone and use a fresh worker for the next attempt.
                self.drop_worker(false);
                // A shared calendar that hung the read is left out for a while, so it cannot
                // take the user's own calendar down with it on every sync.
                if outlook::quarantine_reading_source() {
                    dlog!("WARN", "calendar", "CAL-SHARED-104 a calendar did not answer; skipped for a while");
                }
                self.machine.on_timeout(&now);
                dlog!(
                    "ERROR",
                    "calendar",
                    "OUTLOOK-109 watchdog: no answer in {}ms, abandoned worker generation {} (abandoned: {})",
                    WATCHDOG_MS,
                    self.generation,
                    self.machine.abandoned()
                );
            }
        }
        true
    }

    /// One scheduling step. False when the supervisor should stop.
    fn step(&mut self) -> bool {
        let now = self.now();
        if self.machine.on_tick(&now) {
            dlog!("INFO", "calendar", "resume from sleep detected, rechecking now");
        }
        self.publish(&now);
        if !self.pending_ranges.is_empty() {
            let (window, reply) = self.pending_ranges.remove(0);
            return self.fetch_range(window, reply);
        }
        if std::mem::take(&mut self.nav_changed_during_read) {
            self.on_nav_changed();
        }
        if std::mem::take(&mut self.pf.settings_changed) {
            self.on_prefetch_settings(&now);
        }
        let days = self.prefetch_days();
        self.prefetch_arm(&now, days);
        let (action, at) = self.machine.next();
        if at > now.mono_ms {
            let gate = PrefetchGate {
                connected: self.machine.status() == CalendarStatus::Connected,
                days,
                due: self.pf.due,
                now: now.mono_ms,
                sync_at: at,
                pending_ranges: self.pending_ranges.len(),
                work_waiting: self.nav_changed_during_read || self.pf.settings_changed,
            };
            if prefetch_may_run(&gate) {
                return self.prefetch_step(days);
            }
            let mut wait = (at - now.mono_ms).min(MAX_IDLE_WAIT_MS);
            if let Some(w) = self.prefetch_wake(&now, days) {
                wait = wait.min(w.max(1));
            }
            return match self.rx.recv_timeout(Duration::from_millis(wait)) {
                Ok(Msg::Refresh) => {
                    let now = self.now();
                    self.machine.on_refresh(&now);
                    true
                }
                Ok(Msg::NavChanged) => {
                    self.on_nav_changed();
                    true
                }
                Ok(Msg::Range { window, reply }) => {
                    self.pending_ranges.push((window, reply));
                    true
                }
                Ok(Msg::PrefetchSettings) => {
                    self.pf.settings_changed = true;
                    true
                }
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => false,
                Ok(Msg::Reply { .. }) | Err(RecvTimeoutError::Timeout) => true,
            };
        }
        match action {
            Action::Discover => {
                self.discover();
                true
            }
            Action::Fetch => self.fetch(),
        }
    }

    fn run(&mut self) {
        loop {
            match crate::debug_log::catch("calendar", || self.step()) {
                Some(true) => {}
                Some(false) => break,
                None => {
                    let now = self.now();
                    self.drop_worker(false);
                    self.machine.on_discovery_error(&now);
                }
            }
        }
        self.drop_worker(true);
    }
}

/// Start the calendar service (idempotent).
pub fn start(app: AppHandle) {
    let mut control = CONTROL.lock().unwrap_or_else(|e| e.into_inner());
    if control.is_some() {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let mut supervisor = Supervisor {
        app,
        tx: tx.clone(),
        rx,
        machine: Machine::new(Some(Utc::now().timestamp_nanos_opt().unwrap_or(1) as u64)),
        worker: None,
        generation: 0,
        started: Instant::now(),
        last_logged: (CalendarStatus::Waiting, None),
        pending_ranges: Vec::new(),
        nav_changed_during_read: false,
        pf: PrefetchSched::default(),
        in_prefetch: false,
        refresh_after_prefetch: false,
    };
    let spawned = std::thread::Builder::new().name("companyisland-calendar".into()).spawn(move || {
        supervisor.run();
        let _ = done_tx.send(());
    });
    match spawned {
        Ok(_) => *control = Some(Control { tx, done: done_rx }),
        Err(e) => dlog!("ERROR", "calendar", "OUTLOOK-102 cannot start calendar thread: {}", e),
    }
}

/// Stop the service and give the worker a moment to release COM cleanly.
pub fn stop() {
    let control = CONTROL.lock().unwrap_or_else(|e| e.into_inner()).take();
    prefetch_clear();
    if let Some(c) = control {
        let _ = c.tx.send(Msg::Shutdown);
        let _ = c.done.recv_timeout(Duration::from_secs(2));
    }
}

#[tauri::command]
pub fn calendar_get_snapshot(state: State<'_, CalendarState>) -> CalendarSnapshot {
    state.get()
}

#[tauri::command]
pub fn calendar_refresh() -> Result<(), String> {
    if let Some(c) = CONTROL.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = c.tx.send(Msg::Refresh);
    }
    Ok(())
}

/// `[from, to]` checked for a range read: both ISO instants, `to` after `from`, at most
/// [`MAX_RANGE_DAYS`] apart.
pub fn range_window(from_utc: &str, to_utc: &str) -> Result<FetchWindow, String> {
    let parse = |s: &str| {
        DateTime::parse_from_rfc3339(s).map(|t| t.with_timezone(&Utc)).map_err(|_| "OUTLOOK-108: invalid date".to_string())
    };
    let (from, to) = (parse(from_utc)?, parse(to_utc)?);
    if to <= from || to - from > ChronoDuration::days(MAX_RANGE_DAYS) {
        return Err("OUTLOOK-108: invalid range".into());
    }
    Ok(FetchWindow { from, to, invites: false, range: true, only: None, search: false })
}

/// Queue a range read at the supervisor; the answer arrives on the returned receiver.
fn send_range(window: FetchWindow) -> Result<Receiver<FetchResult>, String> {
    let (tx, rx) = mpsc::channel();
    let control = CONTROL.lock().unwrap_or_else(|e| e.into_inner());
    let c = control.as_ref().ok_or_else(|| "OUTLOOK-102: calendar service not running".to_string())?;
    c.tx.send(Msg::Range { window, reply: tx }).map_err(|_| "OUTLOOK-102: calendar service not running".to_string())?;
    Ok(rx)
}

/// Longest a caller waits for one range read: a sync in progress, then this read, each with the
/// watchdog's 10 s.
fn range_wait() -> Duration {
    Duration::from_millis(WATCHDOG_MS * 2 + 5_000)
}

/// The events of another stretch of the calendar (a day the user browses to), read on demand
/// through the same worker and watchdog as the regular sync. Nothing is cached in Rust.
#[tauri::command]
pub async fn calendar_get_range(from_utc: String, to_utc: String) -> Result<Vec<CalendarEventDto>, String> {
    let window = range_window(&from_utc, &to_utc)?;
    let rx = send_range(window)?;
    crate::rt::run_blocking("calendar_get_range", move || {
        match rx.recv_timeout(range_wait()) {
            Ok(Ok(fetched)) => Ok(fetched.events),
            Ok(Err(e)) => Err(format!("{}: range read failed", e.code)),
            Err(_) => Err("OUTLOOK-109: no answer".into()),
        }
    })
    .await
}

// =============================================================================
// Smart search (CONTRACT, used by `assistant`)
// =============================================================================

/// Longest stretch smart search may read in one call.
pub const MAX_QUERY_DAYS: i64 = 31;

/// Slack on [`MAX_QUERY_DAYS`] for a DST change: a calendar month that ends an hour later in local
/// time (Israel in October) is 31 days + 1 h of real time.
const QUERY_DST_SLACK_HOURS: i64 = 2;

/// A range read hit the watchdog. The worker is gone either way, but only the island's own range
/// reads count against the regular sync (the island shows them); a smart search read is the
/// user's question, so it must not push the island's calendar into backoff or "unresponsive".
fn on_range_timeout(machine: &mut Machine, now: &Now, search: bool) {
    if !search {
        machine.on_timeout(now);
    }
}

/// Split `[from, to)` into consecutive chunks of at most [`MAX_RANGE_DAYS`] (fixed 24 h days in
/// UTC, so a DST change inside the range never shifts a boundary). Pure.
pub fn chunk_range(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<(DateTime<Utc>, DateTime<Utc>)>, String> {
    if to <= from || to - from > ChronoDuration::days(MAX_QUERY_DAYS) + ChronoDuration::hours(QUERY_DST_SLACK_HOURS) {
        return Err("OUTLOOK-108: invalid range".into());
    }
    let step = ChronoDuration::days(MAX_RANGE_DAYS);
    let mut chunks = Vec::new();
    let mut start = from;
    while start < to {
        let end = (start + step).min(to);
        chunks.push((start, end));
        start = end;
    }
    Ok(chunks)
}

/// Events of `[from, to)` for smart search, read through the same worker and watchdog as the
/// island's own range reads. Longer stretches than [`MAX_RANGE_DAYS`] (up to 31 days) are read in
/// chunks. `only`: read exactly these calendar ids (ids from [`known_sources`]), also ones not
/// checked in Outlook; `None` = the active calendars, as the island shows them. Blocking.
///
/// A search read changes nothing: no selection, no snapshot, no reminders, no sync cache. The
/// per-read cap is [`SEARCH_MAX_EVENTS`]; `truncated` is set when it (or a calendar's scan cap)
/// was reached, or when a later chunk could not be read after earlier ones had been.
pub fn query_range(from: DateTime<Utc>, to: DateTime<Utc>, only: Option<Vec<String>>) -> Result<RangeRead, String> {
    let chunks = chunk_range(from, to)?;
    let mut parts = Vec::with_capacity(chunks.len());
    let mut incomplete = false;
    for (i, (start, end)) in chunks.into_iter().enumerate() {
        let window = FetchWindow { from: start, to: end, invites: false, range: true, only: only.clone(), search: true };
        let rx = send_range(window)?;
        match rx.recv_timeout(range_wait()) {
            Ok(Ok(fetched)) => parts.push(fetched),
            Ok(Err(e)) if i == 0 => return Err(format!("{}: range read failed", e.code)),
            Err(_) if i == 0 => return Err("OUTLOOK-109: no answer".into()),
            // Keep what the earlier chunks found.
            _ => {
                incomplete = true;
                break;
            }
        }
    }
    let mut read = merge_reads(parts, from, to);
    read.truncated |= incomplete;
    Ok(read)
}

/// Merge the chunk reads of one query: events inside `[from, to)` (a meeting that spans a chunk
/// boundary is read twice and kept once), sorted, capped at [`SEARCH_MAX_EVENTS`]. Pure.
pub fn merge_reads(parts: Vec<Fetched>, from: DateTime<Utc>, to: DateTime<Utc>) -> RangeRead {
    let mut truncated = false;
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut events = Vec::new();
    for part in parts {
        truncated |= part.truncated;
        for f in part.failed {
            if !failed.iter().any(|(id, _)| *id == f.0) {
                failed.push(f);
            }
        }
        // Inside = starts before `to` and ends after `from` (a zero-length one at `from` counts).
        events.extend(part.events.into_iter().filter(|e| e.start_utc < to && (e.end_utc > from || e.start_utc >= from)));
    }
    let (events, cut) = normalize_events_capped(events, SEARCH_MAX_EVENTS);
    RangeRead { events, truncated: truncated || cut, failed }
}

/// Organizer names seen in the island's snapshot (no Outlook call), distinct, in order of
/// appearance. For the assistant's name matching when a person's name is not a calendar's own
/// name. The names are only returned to the caller, never logged.
pub fn organizers_recent(app: &tauri::AppHandle) -> Vec<String> {
    use tauri::Manager;
    organizer_names(&app.state::<CalendarState>().get().events)
}

/// The distinct non-empty organizers of `events`, in order. Pure.
pub fn organizer_names(events: &[CalendarEventDto]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in events.iter().filter_map(|e| e.organizer.as_deref()).map(str::trim).filter(|n| !n.is_empty()) {
        if !out.iter().any(|o| o == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// Result of [`query_range`].
#[derive(Clone, Debug, PartialEq)]
pub struct RangeRead {
    pub events: Vec<CalendarEventDto>,
    /// Some events were cut (per-read cap) or some calendars could not be read in time.
    pub truncated: bool,
    /// Calendars of `only` that could not be read, with their code ("CAL-SHARED-101"...).
    pub failed: Vec<(String, String)>,
}

/// The calendars of the latest discovery, from the published snapshot (no Outlook call).
pub fn known_sources(app: &tauri::AppHandle) -> Vec<CalendarSourceDto> {
    use tauri::Manager;
    app.state::<CalendarState>().get().sources.map(|r| r.sources).unwrap_or_default()
}

// =============================================================================
// Schedule prefetch (memory only)
// =============================================================================
//
// The app downloads the next N days (setting `calendarPrefetchDays`, default 7) of the user's own
// calendar and of every calendar they have in Outlook's Calendar module, checked or not, whatever
// Outlook's Cached Exchange Mode says (Outlook fetches a folder it does not hold from the server
// when the object model reads it). Smart search answers from this copy while the asked window
// lies inside it; otherwise it reads live. The copy is never written to disk.
//
// The reads run through the supervisor and its watchdog, one small batch per scheduling step and
// only while nothing else wants the worker: a regular sync, or the island's own range read,
// always goes first. A timeout quarantines the calendar being read and never touches the
// island's state machine (`FetchWindow::search`).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};

/// Calendars read together by one batch.
pub const PREFETCH_BATCH: usize = 6;
/// A copy older than this is not answered from.
const PREFETCH_FRESH_MS: i64 = 30 * 60 * 1000;
/// First round after the calendar connects.
const PREFETCH_FIRST_MS: u64 = 20_000;
/// Rounds follow each other this far apart.
const PREFETCH_EVERY_MS: u64 = 15 * 60 * 1000;
/// After a navigation change (a calendar added) the next round starts this soon.
const PREFETCH_NAV_MS: u64 = 10_000;
/// After a changed setting.
const PREFETCH_SETTING_MS: u64 = 2_000;
/// Pause between two batches of one round.
const PREFETCH_PACE_MS: u64 = 500;
/// Retry when nothing could be planned yet (calendars not discovered).
const PREFETCH_RETRY_MS: u64 = 15_000;
/// A batch does not start when the regular sync is due within this time.
const PREFETCH_SYNC_GAP_MS: u64 = 2_000;
/// Events kept in memory over all calendars; the latest-starting ones go first.
pub const PREFETCH_MAX_EVENTS: usize = 2000;
/// Watchdog hits tolerated in one round, then it ends.
const PREFETCH_MAX_TIMEOUTS: u32 = 3;
/// Failed reads (Outlook busy or gone) tolerated in one round, then it ends.
const PREFETCH_MAX_ERRORS: u32 = 2;

static PREFETCH_DAYS: AtomicU32 = AtomicU32::new(0);

/// One calendar's share of the copy.
#[derive(Clone, Debug, PartialEq)]
pub struct PrefCal {
    pub id: String,
    /// Active in the island (checked, or the primary calendar): what a search without an explicit
    /// list is answered from.
    pub active: bool,
    pub events: Vec<CalendarEventDto>,
    pub truncated: bool,
    pub read_unix_ms: i64,
    /// Why the last round could not read it ("CAL-SHARED-104"...); `None` = read cleanly.
    pub error: Option<String>,
}

/// The process-wide copy. Replaced per calendar as batches finish.
#[derive(Debug)]
pub struct PrefetchCache {
    /// Bumped by every clear, so a round that started before it cannot store into the new copy.
    epoch: u64,
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// The calendars of the latest round, in reading order, with their active flag.
    planned: Vec<(String, bool)>,
    cals: Vec<PrefCal>,
    capped: bool,
    fetched_unix_ms: Option<i64>,
}

impl PrefetchCache {
    const fn new() -> Self {
        PrefetchCache { epoch: 0, window: None, planned: Vec::new(), cals: Vec::new(), capped: false, fetched_unix_ms: None }
    }

    fn clear(&mut self) {
        self.epoch += 1;
        self.window = None;
        self.planned.clear();
        self.cals.clear();
        self.capped = false;
        self.fetched_unix_ms = None;
    }

    /// Store finished calendars of a round. False (nothing stored) when the cache was cleared
    /// since the round began. A different window drops what was held for the old one.
    fn commit(
        &mut self,
        epoch: u64,
        window: (DateTime<Utc>, DateTime<Utc>),
        planned: &[(String, bool)],
        done: Vec<PrefCal>,
        now_ms: i64,
    ) -> bool {
        if epoch != self.epoch {
            return false;
        }
        if self.window != Some(window) {
            self.cals.clear();
            self.window = Some(window);
        }
        self.planned = planned.to_vec();
        for cal in done {
            self.cals.retain(|c| c.id != cal.id);
            self.cals.push(cal);
        }
        // Reading order; a calendar that left Outlook's list goes too.
        let order: HashMap<&str, usize> = self.planned.iter().enumerate().map(|(i, (id, _))| (id.as_str(), i)).collect();
        self.cals.retain(|c| order.contains_key(c.id.as_str()));
        self.cals.sort_by_key(|c| order[c.id.as_str()]);
        for c in self.cals.iter_mut() {
            c.active = self.planned.iter().any(|(id, a)| *id == c.id && *a);
        }
        self.capped = cap_prefetch_events(&mut self.cals, PREFETCH_MAX_EVENTS);
        self.fetched_unix_ms = Some(now_ms);
        true
    }
}

static PREFETCH: Mutex<PrefetchCache> = Mutex::new(PrefetchCache::new());

fn prefetch_cache() -> std::sync::MutexGuard<'static, PrefetchCache> {
    PREFETCH.lock().unwrap_or_else(|e| e.into_inner())
}

/// Keep at most `cap` events over all calendars, dropping the latest-starting first. True when
/// any were dropped. Pure.
pub fn cap_prefetch_events(cals: &mut [PrefCal], cap: usize) -> bool {
    let total: usize = cals.iter().map(|c| c.events.len()).sum();
    if total <= cap {
        return false;
    }
    let mut starts: Vec<DateTime<Utc>> = cals.iter().flat_map(|c| c.events.iter().map(|e| e.start_utc)).collect();
    starts.sort();
    // Events starting after the cut go; among those starting exactly at it, as many as still fit.
    let cut = starts[cap - 1];
    let mut at_cut = starts.iter().take(cap).filter(|s| **s == cut).count();
    for cal in cals.iter_mut() {
        let mut dropped = false;
        cal.events.retain(|e| {
            let keep = e.start_utc < cut || (e.start_utc == cut && at_cut > 0);
            if keep && e.start_utc == cut {
                at_cut -= 1;
            }
            dropped |= !keep;
            keep
        });
        cal.truncated |= dropped;
    }
    true
}

/// Forget everything held (profile change, stop, prefetch switched off).
pub fn prefetch_clear() {
    prefetch_cache().clear();
}

/// The window the copy covers: from local midnight today to local midnight `days` later
/// (DST-correct: both ends are calendar dates, not 24 h multiples). `days` 0 = off. Pure, with
/// the local midnight of a date supplied.
pub fn prefetch_window_with(
    today: chrono::NaiveDate,
    days: u32,
    midnight: impl Fn(chrono::NaiveDate) -> Option<DateTime<Utc>>,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    if days == 0 {
        return None;
    }
    let days = days.min(MAX_QUERY_DAYS as u32);
    let from = midnight(today)?;
    let to = midnight(today + ChronoDuration::days(days as i64))?;
    (to > from).then_some((from, to))
}

/// Midnight of a date in the PC's time zone. A zone whose clocks skip midnight starts the day at
/// the first hour that exists.
pub fn local_midnight(date: chrono::NaiveDate) -> Option<DateTime<Utc>> {
    use chrono::TimeZone;
    (0..3).find_map(|h| {
        let naive = date.and_hms_opt(h, 0, 0)?;
        Local.from_local_datetime(&naive).earliest().map(|t| t.with_timezone(&Utc))
    })
}

/// What a search over `[from, to)` can be answered with from `cache`, or `None` (read live): the
/// copy must cover the window completely, and every calendar asked for (`only`; without it, every
/// active calendar of the latest round) must have been read cleanly in the last 30 minutes.
/// Events are filtered to the window and to `only`. Pure.
pub fn prefetched_in(
    cache: &PrefetchCache,
    now_ms: i64,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    only: Option<&[String]>,
) -> Option<RangeRead> {
    let (win_from, win_to) = cache.window?;
    if to <= from || from < win_from || to > win_to {
        return None;
    }
    let wanted: Vec<&str> = match only {
        Some(ids) => ids.iter().map(String::as_str).collect(),
        None => cache.planned.iter().filter(|(_, a)| *a).map(|(id, _)| id.as_str()).collect(),
    };
    if wanted.is_empty() {
        return None;
    }
    let mut parts: Vec<&PrefCal> = Vec::new();
    for id in wanted {
        let cal = cache.cals.iter().find(|c| c.id == id)?;
        if cal.error.is_some() || now_ms.saturating_sub(cal.read_unix_ms) > PREFETCH_FRESH_MS {
            return None;
        }
        if !parts.iter().any(|p| p.id == cal.id) {
            parts.push(cal);
        }
    }
    // Reading order (the primary calendar first), so a meeting shown by several calendars keeps
    // the copy of the first.
    parts.sort_by_key(|p| cache.cals.iter().position(|c| c.id == p.id));
    let mut truncated = cache.capped;
    let mut events = Vec::new();
    for p in parts {
        truncated |= p.truncated;
        events.extend(
            p.events.iter().filter(|e| e.start_utc < to && (e.end_utc > from || e.start_utc >= from)).cloned(),
        );
    }
    let (events, cut) = normalize_events_capped(dedup_meetings(events), SEARCH_MAX_EVENTS);
    Some(RangeRead { events, truncated: truncated || cut, failed: Vec::new() })
}

/// Smart search: the events of `[from, to)` from the prefetched copy when it is fresh and
/// complete for that window, `None` when the question has to be read live.
pub fn prefetched(from: DateTime<Utc>, to: DateTime<Utc>, only: Option<&[String]>) -> Option<RangeRead> {
    prefetched_in(&prefetch_cache(), Utc::now().timestamp_millis(), from, to, only)
}

/// What the Island Center shows about the prefetch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchStatus {
    pub days: u32,
    pub from_unix_ms: Option<i64>,
    pub to_unix_ms: Option<i64>,
    pub fetched_unix_ms: Option<i64>,
    pub calendars_total: usize,
    pub calendars_read: usize,
    /// (calendar id, code) of the calendars the last round could not read.
    pub failed: Vec<(String, String)>,
}

fn prefetch_status_of(cache: &PrefetchCache, days: u32) -> PrefetchStatus {
    PrefetchStatus {
        days,
        from_unix_ms: cache.window.map(|w| w.0.timestamp_millis()),
        to_unix_ms: cache.window.map(|w| w.1.timestamp_millis()),
        fetched_unix_ms: cache.fetched_unix_ms,
        calendars_total: cache.planned.len(),
        calendars_read: cache.cals.iter().filter(|c| c.error.is_none()).count(),
        failed: cache.cals.iter().filter_map(|c| c.error.clone().map(|code| (c.id.clone(), code))).collect(),
    }
}

pub fn prefetch_status() -> PrefetchStatus {
    prefetch_status_of(&prefetch_cache(), PREFETCH_DAYS.load(Ordering::Relaxed))
}

#[tauri::command]
pub fn calendar_prefetch_status() -> PrefetchStatus {
    prefetch_status()
}

/// The `calendarPrefetchDays` setting changed: restart (or stop) the prefetch.
pub fn on_prefetch_settings_changed() {
    if let Some(c) = CONTROL.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = c.tx.send(Msg::PrefetchSettings);
    }
}

/// The calendars in reading order: those the last round did not reach (`priority`) first, then the
/// rest as listed. Ids in `priority` that are no longer listed are dropped. Pure.
pub fn prefetch_order(ids: &[String], priority: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(ids.len());
    for id in priority.iter().chain(ids.iter()) {
        if ids.contains(id) && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

/// One read of a round: some calendars over one stretch of days.
#[derive(Clone, Debug, PartialEq)]
pub struct PrefetchTask {
    pub group: usize,
    pub ids: Vec<String>,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub chunk: usize,
    /// A repeat of a read that came back partial; it is not repeated again.
    pub retry: bool,
}

#[derive(Default)]
struct CalAcc {
    events: Vec<CalendarEventDto>,
    truncated: bool,
    /// (chunk, code) of the stretches that could not be read.
    fails: Vec<(usize, String)>,
}

/// One pass over all calendars: the batches still to read and what they returned so far. Pure
/// (no clock, no Outlook), so batching, rotation and failure handling are unit-tested.
pub struct PrefetchRound {
    pub window: (DateTime<Utc>, DateTime<Utc>),
    pub epoch: u64,
    pub days: u32,
    planned: Vec<(String, bool)>,
    groups: Vec<Vec<String>>,
    tasks: VecDeque<PrefetchTask>,
    acc: HashMap<String, CalAcc>,
    errors: u32,
    timeouts: u32,
}

impl PrefetchRound {
    /// Plan a round: calendars (id, active) in the order of [`prefetch_order`], at most
    /// [`PREFETCH_BATCH`] per read, the window in chunks of at most [`MAX_RANGE_DAYS`] days.
    /// `None` when there is nothing to read or the window is unusable.
    pub fn new(
        calendars: &[(String, bool)],
        priority: &[String],
        window: (DateTime<Utc>, DateTime<Utc>),
        epoch: u64,
        days: u32,
    ) -> Option<Self> {
        let ids: Vec<String> = calendars.iter().map(|(id, _)| id.clone()).collect();
        let order = prefetch_order(&ids, priority);
        if order.is_empty() {
            return None;
        }
        let chunks = chunk_range(window.0, window.1).ok()?;
        let planned: Vec<(String, bool)> =
            order.iter().map(|id| (id.clone(), calendars.iter().any(|(i, a)| i == id && *a))).collect();
        let groups: Vec<Vec<String>> = order.chunks(PREFETCH_BATCH).map(|g| g.to_vec()).collect();
        let mut tasks = VecDeque::new();
        for (group, ids) in groups.iter().enumerate() {
            for (chunk, (from, to)) in chunks.iter().enumerate() {
                tasks.push_back(PrefetchTask { group, ids: ids.clone(), from: *from, to: *to, chunk, retry: false });
            }
        }
        Some(PrefetchRound {
            window,
            epoch,
            days,
            planned,
            groups,
            tasks,
            acc: HashMap::new(),
            errors: 0,
            timeouts: 0,
        })
    }

    pub fn planned(&self) -> &[(String, bool)] {
        &self.planned
    }

    pub fn pending_tasks(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_done(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn next_task(&mut self) -> Option<PrefetchTask> {
        self.tasks.pop_front()
    }

    fn fail(&mut self, id: &str, chunk: usize, code: &str) {
        let acc = self.acc.entry(id.to_string()).or_default();
        acc.fails.retain(|(c, _)| *c != chunk);
        acc.fails.push((chunk, code.to_string()));
    }

    /// The calendars of `task`'s group, when `task` was the group's last read: ready to store.
    fn finished_group(&self, task: &PrefetchTask, now_ms: i64) -> Vec<PrefCal> {
        if self.tasks.iter().any(|t| t.group == task.group) {
            return Vec::new();
        }
        self.groups[task.group]
            .iter()
            .filter_map(|id| {
                let acc = self.acc.get(id)?;
                let active = self.planned.iter().any(|(i, a)| i == id && *a);
                let mut events = acc.events.clone();
                events.sort_by(|a, b| a.start_utc.cmp(&b.start_utc).then_with(|| a.id.cmp(&b.id)));
                events.dedup_by(|a, b| a.id == b.id);
                let error = acc.fails.first().map(|(_, code)| code.clone());
                if error.is_some() {
                    events.clear();
                }
                Some(PrefCal { id: id.clone(), active, events, truncated: acc.truncated, read_unix_ms: now_ms, error })
            })
            .collect()
    }

    /// A read of `task` answered. Calendars it reported as failed are marked (and, when merely
    /// "pending" in Outlook's time budget, read once more at the end); the rest take its events.
    /// Returns the calendars that are complete now.
    pub fn on_result(&mut self, task: &PrefetchTask, fetched: Fetched, now_ms: i64) -> Vec<PrefCal> {
        let mut again: Vec<String> = Vec::new();
        for id in &task.ids {
            let acc = self.acc.entry(id.clone()).or_default();
            // A repeat that now succeeds clears the earlier failure of this stretch.
            acc.fails.retain(|(c, _)| *c != task.chunk);
            acc.truncated |= fetched.truncated;
        }
        for (id, code) in &fetched.failed {
            if !task.ids.contains(id) {
                continue;
            }
            self.fail(id, task.chunk, code);
            if code == "CAL-SHARED-105" && !task.retry {
                again.push(id.clone());
            }
        }
        for e in fetched.events {
            if let Some(acc) = self.acc.get_mut(&e.calendar_id) {
                if task.ids.contains(&e.calendar_id) {
                    acc.events.push(e);
                }
            }
        }
        if !again.is_empty() {
            self.tasks.push_back(PrefetchTask { ids: again, retry: true, ..task.clone() });
        }
        self.finished_group(task, now_ms)
    }

    /// A read of `task` failed as a whole (Outlook busy or gone). Its calendars are marked; a
    /// second such failure ends the round.
    pub fn on_error(&mut self, task: &PrefetchTask, code: &str, now_ms: i64) -> Vec<PrefCal> {
        for id in &task.ids {
            self.fail(id, task.chunk, code);
        }
        self.errors += 1;
        if self.errors >= PREFETCH_MAX_ERRORS {
            self.tasks.clear();
        }
        self.finished_group(task, now_ms)
    }

    /// The watchdog gave up on `task`. `culprit` is the calendar that was being read (now
    /// quarantined): it is marked failed, the others of the batch are read again without it. When
    /// no calendar can be blamed (the primary one, say) the round ends.
    pub fn on_timeout(&mut self, task: &PrefetchTask, culprit: Option<&str>, now_ms: i64) -> Vec<PrefCal> {
        self.timeouts += 1;
        match culprit.filter(|c| task.ids.iter().any(|i| i == c)) {
            Some(bad) => {
                self.fail(bad, task.chunk, "CAL-SHARED-104");
                let rest: Vec<String> = task.ids.iter().filter(|i| *i != bad).cloned().collect();
                if !rest.is_empty() {
                    self.tasks.push_front(PrefetchTask { ids: rest, retry: true, ..task.clone() });
                }
            }
            None => {
                for id in &task.ids {
                    self.fail(id, task.chunk, "OUTLOOK-109");
                }
                self.tasks.clear();
            }
        }
        if self.timeouts >= PREFETCH_MAX_TIMEOUTS {
            self.tasks.clear();
        }
        self.finished_group(task, now_ms)
    }

    /// The calendars to read first next round: those that failed, then those never reached.
    pub fn unreached(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (id, _) in &self.planned {
            if self.acc.get(id).map_or(true, |a| !a.fails.is_empty()) {
                out.push(id.clone());
            }
        }
        out
    }
}

/// What the supervisor needs to decide whether a prefetch batch may start now. Pure.
#[derive(Clone, Copy, Debug)]
pub struct PrefetchGate {
    pub connected: bool,
    pub days: u32,
    pub due: Option<u64>,
    pub now: u64,
    /// When the machine's next regular step (sync or discovery) is due.
    pub sync_at: u64,
    pub pending_ranges: usize,
    /// A navigation change or refresh is waiting to be handled.
    pub work_waiting: bool,
}

/// A prefetch batch goes only when it is due and nothing of higher priority wants the worker: no
/// island range read queued, no refresh waiting, and the regular sync not about to start.
pub fn prefetch_may_run(g: &PrefetchGate) -> bool {
    g.connected
        && g.days > 0
        && g.due.is_some_and(|d| d <= g.now)
        && g.pending_ranges == 0
        && !g.work_waiting
        && g.sync_at > g.now + PREFETCH_SYNC_GAP_MS
}

/// The supervisor's prefetch bookkeeping (monotonic ms).
#[derive(Default)]
struct PrefetchSched {
    due: Option<u64>,
    round: Option<PrefetchRound>,
    /// Calendars to read first next round.
    priority: Vec<String>,
    /// The first round of this connection is scheduled.
    armed: bool,
    settings_changed: bool,
}

impl Supervisor {
    fn prefetch_days(&self) -> u32 {
        let days = self.app.state::<SettingsStore>().get().calendar_prefetch_days;
        PREFETCH_DAYS.store(days, Ordering::Relaxed);
        days
    }

    /// The setting changed: drop the round in progress and start over (or clear when off).
    fn on_prefetch_settings(&mut self, now: &Now) {
        let days = self.prefetch_days();
        self.pf.round = None;
        self.pf.priority.clear();
        if days == 0 {
            prefetch_clear();
            self.pf.due = None;
        } else {
            self.pf.due = Some(now.mono_ms + PREFETCH_SETTING_MS);
            self.pf.armed = true;
        }
    }

    /// First round 20 s after the calendar connects; re-armed after Outlook went away.
    fn prefetch_arm(&mut self, now: &Now, days: u32) {
        if self.machine.status() != CalendarStatus::Connected {
            self.pf.armed = false;
            return;
        }
        if !self.pf.armed && days > 0 {
            self.pf.armed = true;
            self.pf.due = Some(now.mono_ms + PREFETCH_FIRST_MS);
        }
    }

    /// Milliseconds until the next prefetch batch is due, for the idle wait.
    fn prefetch_wake(&self, now: &Now, days: u32) -> Option<u64> {
        if days == 0 || self.machine.status() != CalendarStatus::Connected {
            return None;
        }
        self.pf.due.filter(|d| *d > now.mono_ms).map(|d| d - now.mono_ms)
    }

    /// Read one batch of the prefetch round (starting a round first when none is running).
    fn prefetch_step(&mut self, days: u32) -> bool {
        let now = self.now();
        if self.pf.round.is_none() {
            let calendars: Vec<(String, bool)> = known_sources(&self.app)
                .into_iter()
                .map(|s| {
                    let active = s.active || s.kind == SourceKind::Primary;
                    (s.id, active)
                })
                .collect();
            let window = prefetch_window_with(Local::now().date_naive(), days, local_midnight);
            let epoch = prefetch_cache().epoch;
            match window.and_then(|w| PrefetchRound::new(&calendars, &self.pf.priority, w, epoch, days)) {
                Some(round) => self.pf.round = Some(round),
                None => {
                    self.pf.due = Some(now.mono_ms + PREFETCH_RETRY_MS);
                    return true;
                }
            }
        }
        let Some(mut round) = self.pf.round.take() else { return true };
        let Some(task) = round.next_task() else {
            self.prefetch_finish(round, &now);
            return true;
        };
        let window = FetchWindow {
            from: task.from,
            to: task.to,
            invites: false,
            range: true,
            only: Some(task.ids.clone()),
            search: true,
        };
        self.in_prefetch = true;
        let outcome = self.run_fetch(window);
        self.in_prefetch = false;
        let after = self.now();
        let now_ms = after.unix_ms;
        let done = match outcome {
            Outcome::Shutdown => return false,
            Outcome::Done(Ok(fetched)) => round.on_result(&task, fetched, now_ms),
            Outcome::Done(Err(err)) => {
                dlog!("DEBUG", "calendar", "{} prefetch batch not read ({:?})", err.code, err.kind);
                round.on_error(&task, err.code, now_ms)
            }
            Outcome::TimedOut => {
                // Same as any hung read, but the island's state machine is not told.
                self.drop_worker(false);
                let culprit = outlook::quarantine_reading_source_id();
                if culprit.is_some() {
                    dlog!("WARN", "calendar", "CAL-SHARED-104 a calendar did not answer the prefetch; skipped for a while");
                }
                dlog!("WARN", "calendar", "OUTLOOK-109 watchdog: prefetch batch did not answer in {}ms", WATCHDOG_MS);
                round.on_timeout(&task, culprit.as_deref(), now_ms)
            }
        };
        if !done.is_empty() {
            let stored = prefetch_cache().commit(round.epoch, round.window, round.planned(), done, now_ms);
            if !stored {
                // Cleared meanwhile (profile change, switched off): this round is void.
                self.pf.priority.clear();
                return self.prefetch_after(after);
            }
        }
        if round.is_done() {
            self.prefetch_finish(round, &after);
        } else {
            self.pf.due = Some(after.mono_ms + PREFETCH_PACE_MS);
            self.pf.round = Some(round);
        }
        self.prefetch_after(after)
    }

    /// A refresh that arrived during the batch was not served by it: serve it now.
    fn prefetch_after(&mut self, now: Now) -> bool {
        if std::mem::take(&mut self.refresh_after_prefetch) {
            self.machine.on_refresh(&now);
        }
        true
    }

    fn prefetch_finish(&mut self, round: PrefetchRound, now: &Now) {
        let status = {
            let cache = prefetch_cache();
            prefetch_status_of(&cache, round.days)
        };
        dlog!(
            "INFO",
            "calendar",
            "prefetch round done: {}/{} calendars, {} days, {} failed",
            status.calendars_read,
            status.calendars_total,
            round.days,
            status.failed.len()
        );
        self.pf.priority = round.unreached();
        self.pf.due = Some(now.mono_ms + PREFETCH_EVERY_MS);
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::VecDeque;

    fn at(mono: u64) -> Now {
        Now { mono_ms: mono, unix_ms: 1_800_000_000_000 + mono as i64, day: 740_000 }
    }

    fn ev(id: &str, start_min: i64, len_min: i64) -> CalendarEventDto {
        let base = Utc.with_ymd_and_hms(2027, 1, 15, 8, 0, 0).unwrap();
        CalendarEventDto {
            id: id.into(),
            calendar_id: "cal".into(),
            calendar_name: "Calendar".into(),
            source_kind: SourceKind::Primary,
            meeting_key: None,
            subject: "s".into(),
            start_utc: base + ChronoDuration::minutes(start_min),
            end_utc: base + ChronoDuration::minutes(start_min + len_min),
            all_day: false,
            location: None,
            organizer: None,
            is_recurring: false,
            meeting_url: None,
            busy_status: BusyStatus::Busy,
            response_status: ResponseStatus::Accepted,
            color: None,
            calendar_color: None,
        }
    }

    fn at_utc(min: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 1, 15, 8, 0, 0).unwrap() + ChronoDuration::minutes(min)
    }

    #[test]
    fn range_reads_are_bounded_iso_windows() {
        let w = range_window("2026-10-11T21:00:00Z", "2026-10-12T21:00:00Z").unwrap();
        assert_eq!(w.to - w.from, ChronoDuration::hours(24));
        assert!(!w.invites);
        assert!(range_window("2026-10-11T21:00:00+03:00", "2026-10-18T21:00:00+03:00").is_ok());
        for (from, to) in [
            ("2026-10-12T00:00:00Z", "2026-10-11T00:00:00Z"),
            ("2026-10-12T00:00:00Z", "2026-10-12T00:00:00Z"),
            ("2026-10-01T00:00:00Z", "2026-10-09T00:00:00Z"),
            ("tomorrow", "2026-10-12T00:00:00Z"),
        ] {
            assert!(range_window(from, to).unwrap_err().starts_with("OUTLOOK-108"), "{from} {to}");
        }
    }

    #[test]
    fn invites_of_the_latest_read_reach_the_snapshot_capped() {
        let invite = |i: usize| MeetingInviteDto {
            id: format!("inv{i}"),
            subject: "s".into(),
            organizer: None,
            start_utc: None,
            end_utc: None,
            location: None,
            received_utc: at_utc(0),
        };
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("a", 0, 30)]);
        m.set_invites((0..15).map(invite).collect());
        let snapshot = m.snapshot(0);
        assert_eq!(snapshot.invites.len(), MAX_INVITES);
        assert_eq!(snapshot.invites[0].id, "inv0");
        m.set_invites(Vec::new());
        assert!(m.snapshot(0).invites.is_empty());
    }

    fn err(kind: ErrKind, code: &'static str) -> SourceError {
        SourceError::new(kind, code, "test")
    }

    /// Scripted source for driving the machine without Outlook.
    struct FakeSource {
        script: VecDeque<FetchResult>,
        calls: u32,
    }

    impl CalendarSource for FakeSource {
        fn fetch(&mut self, _window: &FetchWindow) -> FetchResult {
            self.calls += 1;
            self.script.pop_front().unwrap_or_else(|| Ok(Fetched::default()))
        }
    }

    /// Runs the machine's own schedule against a fake source and a simulated clock.
    struct Sim {
        machine: Machine,
        source: FakeSource,
        discovery: Discovery,
        clock: u64,
        trace: Vec<(u64, Action)>,
    }

    impl Sim {
        fn new(script: Vec<FetchResult>) -> Self {
            Sim {
                machine: Machine::new(None),
                source: FakeSource { script: script.into(), calls: 0 },
                discovery: Discovery::Classic(1),
                clock: 0,
                trace: Vec::new(),
            }
        }

        /// Execute every action due at or before `until`.
        fn run_until(&mut self, until: u64) {
            for _ in 0..10_000 {
                let (action, due) = self.machine.next();
                if due > until {
                    self.clock = until;
                    return;
                }
                self.clock = self.clock.max(due);
                let now = at(self.clock);
                self.trace.push((self.clock, action));
                match action {
                    Action::Discover => self.machine.on_discovery(&now, self.discovery),
                    Action::Fetch => {
                        self.machine.on_fetch_started(&now);
                        match self.source.fetch(&FetchWindow::starting_at(Utc::now())) {
                            Ok(fetched) => self.machine.on_fetch_ok(&now, fetched.events),
                            Err(e) => self.machine.on_fetch_err(&now, &e),
                        }
                    }
                }
            }
            panic!("machine did not settle (tight loop?)");
        }

        fn fetch_times(&self) -> Vec<u64> {
            self.trace.iter().filter(|(_, a)| *a == Action::Fetch).map(|(t, _)| *t).collect()
        }
    }

    #[test]
    fn serializes_to_the_contract_shape() {
        let snapshot = CalendarSnapshot {
            status: CalendarStatus::NewOutlookOnly,
            error_code: Some("OUTLOOK-104".into()),
            last_sync_unix_ms: Some(5),
            cached_count: 1,
            next_retry_unix_ms: None,
            events: vec![CalendarEventDto {
                busy_status: BusyStatus::WorkingElsewhere,
                response_status: ResponseStatus::NotResponded,
                color: Some("#3267B8".into()),
                calendar_color: None,
                ..ev("a", 0, 30)
            }],
            invites: vec![MeetingInviteDto {
                id: "0123456789abcdef".into(),
                subject: "Review".into(),
                organizer: None,
                start_utc: Some(at_utc(60)),
                end_utc: None,
                location: None,
                received_utc: at_utc(0),
            }],
            sources: None,
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["status"], "newOutlookOnly");
        assert_eq!(json["errorCode"], "OUTLOOK-104");
        assert_eq!(json["lastSyncUnixMs"], 5);
        assert_eq!(json["cachedCount"], 1);
        assert!(json["nextRetryUnixMs"].is_null());
        let e = &json["events"][0];
        assert_eq!(e["startUtc"], "2027-01-15T08:00:00Z");
        assert_eq!(e["endUtc"], "2027-01-15T08:30:00Z");
        assert_eq!(e["calendarId"], "cal");
        assert_eq!(e["allDay"], false);
        assert_eq!(e["isRecurring"], false);
        assert_eq!(e["busyStatus"], "workingElsewhere");
        assert_eq!(e["responseStatus"], "notResponded");
        assert!(e["meetingUrl"].is_null());
        assert_eq!(e["color"], "#3267B8");
        let i = &json["invites"][0];
        assert_eq!(i["startUtc"], "2027-01-15T09:00:00Z");
        assert!(i["endUtc"].is_null());
        assert_eq!(i["receivedUtc"], "2027-01-15T08:00:00Z");
        assert!(i.get("entryId").is_none());
        for s in [CalendarStatus::Waiting, CalendarStatus::ElevationMismatch, CalendarStatus::Unresponsive] {
            assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
        }
    }

    fn on(id: &str, cal: &str, kind: SourceKind, key: Option<&str>, start_min: i64) -> CalendarEventDto {
        CalendarEventDto {
            calendar_id: cal.into(),
            calendar_name: cal.into(),
            source_kind: kind,
            meeting_key: key.map(String::from),
            ..ev(id, start_min, 30)
        }
    }

    #[test]
    fn the_same_meeting_in_two_calendars_shows_once_from_the_first() {
        let merged = dedup_meetings(vec![
            on("p1", "mine", SourceKind::Primary, Some("g1"), 60),
            on("s1", "team", SourceKind::Shared, Some("g1"), 60),
            on("s2", "team", SourceKind::Shared, Some("g2"), 30),
        ]);
        let ids: Vec<_> = merged.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["p1", "s2"]);
    }

    #[test]
    fn dedup_never_merges_on_subject_or_across_occurrences() {
        // Same subject, no global id (plain appointments): both stay.
        let a = on("a", "mine", SourceKind::Primary, None, 60);
        let b = on("b", "team", SourceKind::Shared, None, 60);
        assert_eq!(dedup_meetings(vec![a, b]).len(), 2);
        // Same series id, different occurrences (daily recurrence): both stay.
        let monday = on("m", "team", SourceKind::Shared, Some("series"), 0);
        let tuesday = on("t", "team", SourceKind::Shared, Some("series"), 24 * 60);
        assert_eq!(dedup_meetings(vec![monday, tuesday]).len(), 2);
        // Same id and start but a moved end (an attendee copy not updated yet): both stay.
        let mut longer = on("l", "team", SourceKind::Shared, Some("g"), 60);
        longer.end_utc += ChronoDuration::minutes(15);
        assert_eq!(dedup_meetings(vec![on("p", "mine", SourceKind::Primary, Some("g"), 60), longer]).len(), 2);
    }

    #[test]
    fn meeting_keys_and_names_stay_out_of_the_wire_format_where_private() {
        let json = serde_json::to_value(on("a", "Support", SourceKind::Shared, Some("secret-goid-hash"), 0)).unwrap();
        assert!(json.get("meetingKey").is_none());
        assert_eq!(json["calendarName"], "Support");
        assert_eq!(json["sourceKind"], "shared");
    }

    fn report(n: usize) -> SourcesReport {
        SourcesReport {
            sources: (0..n)
                .map(|i| CalendarSourceDto {
                    id: format!("{i:016x}"),
                    name: format!("cal {i}"),
                    group: if i == 0 { SourceGroup::My } else { SourceGroup::Shared },
                    kind: if i == 0 { SourceKind::Primary } else { SourceKind::Shared },
                    selected: true,
                    active: true,
                    pending_in_outlook: false,
                    color: None,
                    state: SourceState::Ok,
                    error_code: None,
                    event_count: 0,
                    last_read_unix_ms: Some(1),
                })
                .collect(),
            selection: SelectionOrigin::Outlook,
            groups: 2,
            listener: true,
            discovered_unix_ms: 1,
        }
    }

    #[test]
    fn sources_survive_outlook_closing_like_events_do() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("a", 600, 30)]);
        m.set_sources(Some(report(3)));
        m.on_discovery(&at(1_000), Discovery::Waiting);
        let snapshot = m.snapshot(at(1_000).unix_ms);
        assert_eq!(snapshot.status, CalendarStatus::Waiting);
        assert_eq!(snapshot.sources.as_ref().map(|r| r.sources.len()), Some(3));
        // A read without discovery (range-style) keeps the last discovery.
        m.set_sources(None);
        assert!(m.snapshot(0).sources.is_some());
        let json = serde_json::to_value(m.snapshot(0)).unwrap();
        assert_eq!(json["sources"]["selection"], "outlook");
        assert_eq!(json["sources"]["sources"][1]["kind"], "shared");
        assert_eq!(json["sources"]["sources"][1]["state"], "ok");
    }

    #[test]
    fn normalize_sorts_dedups_and_caps() {
        let out = normalize_events(vec![ev("b", 30, 10), ev("a", 10, 10), ev("a", 10, 10)]);
        assert_eq!(out.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        let many: Vec<_> = (0..80).map(|i| ev(&format!("e{i:03}"), i, 5)).collect();
        assert_eq!(normalize_events(many).len(), MAX_EVENTS);
    }

    #[test]
    fn snapshot_diffing_only_reports_real_changes() {
        let state = CalendarState::default();
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("a", 0, 30)]);
        let s1 = m.snapshot(at(0).unix_ms);
        assert!(state.replace_if_changed(&s1));
        assert!(!state.replace_if_changed(&m.snapshot(at(0).unix_ms)));
        // Same events, same status: nothing to emit even though time passed.
        assert!(!state.replace_if_changed(&m.snapshot(at(5_000).unix_ms)));
        // A new sync timestamp is a change (contract: emitted after each successful sync).
        m.on_fetch_ok(&at(60_000), vec![ev("a", 0, 30)]);
        assert!(state.replace_if_changed(&m.snapshot(at(60_000).unix_ms)));
        // Events list changes.
        m.on_fetch_ok(&at(60_000), vec![ev("a", 0, 30), ev("b", 5, 5)]);
        assert!(state.replace_if_changed(&m.snapshot(at(60_000).unix_ms)));
        // Status change.
        m.on_discovery(&at(70_000), Discovery::Waiting);
        assert!(state.replace_if_changed(&m.snapshot(at(70_000).unix_ms)));
        assert_eq!(state.get().status, CalendarStatus::Waiting);
    }

    #[test]
    fn expired_events_leave_the_snapshot_but_stay_cached_until_next_sync() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("short", 0, 10), ev("long", 0, 600)]);
        let base = ev("x", 0, 0).start_utc.timestamp_millis();
        let snap = m.snapshot(base + 5 * 60_000);
        assert_eq!(snap.cached_count, 2);
        let snap = m.snapshot(base + 20 * 60_000);
        assert_eq!(snap.events.len(), 1);
        assert_eq!(snap.cached_count, 1);
        assert_eq!(snap.events[0].id, "long");
    }

    #[test]
    fn waiting_polls_every_15_seconds_and_never_fetches() {
        let mut sim = Sim::new(vec![]);
        sim.discovery = Discovery::Waiting;
        sim.run_until(60_000);
        assert!(sim.fetch_times().is_empty());
        let polls: Vec<u64> = sim.trace.iter().map(|(t, _)| *t).collect();
        assert_eq!(polls, [0, 15_000, 30_000, 45_000, 60_000]);
        assert_eq!(sim.machine.status(), CalendarStatus::Waiting);
        assert_eq!(sim.machine.error_code(), Some("OUTLOOK-101"));
    }

    #[test]
    fn idle_discoveries_map_to_contract_codes() {
        let mut m = Machine::new(None);
        m.on_discovery(&at(0), Discovery::NewOutlookOnly);
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::NewOutlookOnly, Some("OUTLOOK-104")));
        m.on_discovery(&at(0), Discovery::ElevationMismatch);
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::ElevationMismatch, Some("OUTLOOK-103")));
    }

    #[test]
    fn connected_resyncs_every_60_seconds_and_discovers_every_15() {
        let mut sim = Sim::new(vec![Ok(vec![ev("a", 0, 30)].into()), Ok(vec![ev("a", 0, 30)].into()), Ok(vec![].into())]);
        sim.run_until(125_000);
        assert_eq!(sim.fetch_times(), [0, 60_000, 120_000]);
        assert_eq!(sim.machine.status(), CalendarStatus::Connected);
        let discovers = sim.trace.iter().filter(|(_, a)| *a == Action::Discover).count();
        assert!(discovers >= 8, "expected a process check every 15 s, got {discovers}");
    }

    #[test]
    fn outlook_closing_is_noticed_within_one_poll() {
        let mut sim = Sim::new(vec![Ok(vec![ev("a", 0, 30)].into())]);
        sim.run_until(10_000);
        assert_eq!(sim.machine.status(), CalendarStatus::Connected);
        sim.discovery = Discovery::Waiting;
        sim.run_until(10_000 + DISCOVER_POLL_MS);
        assert_eq!(sim.machine.status(), CalendarStatus::Waiting);
        // The cache survives so reminders for already-known events keep working.
        assert_eq!(sim.machine.snapshot(at(0).unix_ms).events.len(), 1);
    }

    #[test]
    fn not_in_rot_retries_1_2_4_8_16_30_then_gives_up_to_failed() {
        let script = (0..20).map(|_| Err(err(ErrKind::NotInRot, "OUTLOOK-102"))).collect();
        let mut sim = Sim::new(script);
        sim.run_until(400_000);
        let t = sim.fetch_times();
        let gaps: Vec<u64> = t.windows(2).map(|w| w[1] - w[0]).take(9).collect();
        // 8 connecting retries (1,2,4,8,16,30,30,30 s); the 9th failure starts the failure schedule.
        assert_eq!(gaps[..8], [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000, 30_000]);
        assert_eq!(gaps[8], 5_000);
        assert_eq!(sim.machine.status(), CalendarStatus::Failed);
        assert_eq!(sim.machine.error_code(), Some("OUTLOOK-102"));
    }

    #[test]
    fn failure_backoff_schedule_and_reset_on_success() {
        let mut script: Vec<FetchResult> = (0..9).map(|_| Err(err(ErrKind::Failed, "OUTLOOK-108"))).collect();
        script.push(Ok(vec![].into()));
        script.push(Err(err(ErrKind::Failed, "OUTLOOK-108")));
        let mut sim = Sim::new(script);
        sim.run_until(2_000_000);
        let t = sim.fetch_times();
        let gaps: Vec<u64> = t.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(gaps[..8], [5_000, 10_000, 20_000, 40_000, 60_000, 120_000, 300_000, 300_000]);
        // Fetch #10 succeeds, #11 (one sync interval later) fails: the backoff restarts at 5 s.
        assert_eq!(gaps[8..11], [300_000, 60_000, 5_000]);
        assert_eq!(sim.machine.status(), CalendarStatus::Connected);
    }

    #[test]
    fn failure_counter_resets_after_success() {
        let mut m = Machine::new(None);
        for _ in 0..4 {
            m.on_fetch_err(&at(0), &err(ErrKind::Failed, "OUTLOOK-108"));
        }
        assert_eq!(m.fetch_not_before, 40_000);
        m.on_fetch_ok(&at(100_000), vec![]);
        m.on_fetch_err(&at(200_000), &err(ErrKind::Failed, "OUTLOOK-107"));
        assert_eq!(m.fetch_not_before, 205_000);
        assert_eq!(m.error_code(), Some("OUTLOOK-107"));
        assert_eq!(m.snapshot(0).next_retry_unix_ms, Some(at(200_000).unix_ms + 5_000));
    }

    #[test]
    fn jitter_stays_within_20_percent() {
        let mut m = Machine::new(Some(0x1234_5678));
        for _ in 0..200 {
            let d = m.jittered(10_000);
            assert!((8_000..=12_000).contains(&d), "{d}");
        }
    }

    #[test]
    fn busy_shows_unresponsive_only_when_it_persists() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![]);
        m.on_fetch_err(&at(1_000), &err(ErrKind::Busy, "OUTLOOK-105"));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Connected, Some("OUTLOOK-105")));
        m.on_fetch_err(&at(7_000), &err(ErrKind::Busy, "OUTLOOK-105"));
        assert_eq!(m.status(), CalendarStatus::Connected);
        m.on_fetch_err(&at(20_000), &err(ErrKind::Busy, "OUTLOOK-105"));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Unresponsive, Some("OUTLOOK-105")));
        m.on_fetch_ok(&at(40_000), vec![]);
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Connected, None));
    }

    #[test]
    fn blocked_is_failed_110() {
        let mut m = Machine::new(None);
        m.on_fetch_err(&at(0), &err(ErrKind::Blocked, "OUTLOOK-110"));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Failed, Some("OUTLOOK-110")));
    }

    #[test]
    fn disconnect_reattaches_through_connecting() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("a", 0, 30)]);
        m.on_fetch_err(&at(10_000), &err(ErrKind::Disconnected, "OUTLOOK-102"));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Connecting, None));
        assert_eq!(m.fetch_not_before, 11_000);
        assert_eq!(m.snapshot(0).events.len(), 1);
    }

    #[test]
    fn watchdog_timeouts_escalate_then_settle_on_failed() {
        let mut m = Machine::new(None);
        for i in 1..MAX_ABANDONED {
            m.on_timeout(&at(i as u64 * 1_000));
            assert_eq!((m.status(), m.error_code()), (CalendarStatus::Unresponsive, Some("OUTLOOK-109")));
        }
        let now = at(100_000);
        m.on_timeout(&now);
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Failed, Some("OUTLOOK-109")));
        assert!(m.fetch_not_before >= now.mono_ms + ABANDONED_RETRY_MS);
        // A refresh must not hammer a hung Outlook.
        let before = m.fetch_not_before;
        m.on_refresh(&at(101_000));
        assert_eq!(m.fetch_not_before, before);
        // A new Outlook process (process list says waiting) clears the verdict.
        m.on_discovery(&at(102_000), Discovery::Waiting);
        assert_eq!(m.abandoned(), 0);
    }

    #[test]
    fn a_search_range_timeout_leaves_the_regular_sync_state_alone() {
        let mut m = Machine::new(None);
        let before = (m.status(), m.error_code(), m.abandoned(), m.fail_attempts, m.fetch_not_before);
        for i in 0..(MAX_ABANDONED + 2) {
            on_range_timeout(&mut m, &at(i as u64 * 1_000), true);
        }
        assert_eq!(before, (m.status(), m.error_code(), m.abandoned(), m.fail_attempts, m.fetch_not_before));
        // The island's own range reads still count, as before.
        on_range_timeout(&mut m, &at(50_000), false);
        assert_eq!((m.status(), m.abandoned()), (CalendarStatus::Unresponsive, 1));
    }

    #[test]
    fn refresh_fetches_now_but_is_rate_limited() {
        let mut m = Machine::new(None);
        m.on_fetch_started(&at(0));
        m.on_fetch_ok(&at(0), vec![]);
        m.on_refresh(&at(500));
        m.on_discovery(&at(500), Discovery::Classic(1));
        // Within the 2 s gap the fetch waits for the gate.
        assert_eq!(m.next(), (Action::Discover, 2_000));
        m.on_discovery(&at(2_000), Discovery::Classic(1));
        assert_eq!(m.next(), (Action::Fetch, 2_000));
    }

    #[test]
    fn refresh_cuts_a_backoff_short() {
        let mut m = Machine::new(None);
        m.on_fetch_started(&at(0));
        for _ in 0..5 {
            m.on_fetch_err(&at(0), &err(ErrKind::Failed, "OUTLOOK-108"));
        }
        assert_eq!(m.fetch_not_before, 60_000);
        m.on_refresh(&at(10_000));
        assert_eq!(m.fetch_not_before, 10_000);
        m.on_discovery(&at(10_000), Discovery::Classic(1));
        assert_eq!(m.next(), (Action::Fetch, 10_000));
    }

    #[test]
    fn resume_from_sleep_rechecks_immediately() {
        let mut m = Machine::new(None);
        m.on_tick(&at(0));
        m.on_fetch_ok(&at(0), vec![]);
        m.on_fetch_err(&at(1_000), &err(ErrKind::Failed, "OUTLOOK-108"));
        assert!(m.fetch_not_before > 1_000);
        // 10 minutes of wall clock passed while the monotonic clock advanced 1 s.
        let woke = Now { mono_ms: 2_000, unix_ms: at(0).unix_ms + 600_000, day: 740_000 };
        assert!(m.on_tick(&woke));
        assert_eq!(m.next(), (Action::Discover, 2_000));
        assert_eq!(m.fetch_not_before, 2_000);
        // Ordinary ticks are not resumes.
        assert!(!m.on_tick(&Now { mono_ms: 17_000, unix_ms: woke.unix_ms + 15_000, day: 740_000 }));
    }

    #[test]
    fn local_date_change_triggers_a_resync() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![]);
        assert_eq!(m.next().1, 15_000);
        let next_day = Now { mono_ms: 5_000, unix_ms: at(5_000).unix_ms, day: 740_001 };
        m.on_tick(&next_day);
        assert_eq!(m.next(), (Action::Discover, 5_000));
        m.on_discovery(&next_day, Discovery::Classic(1));
        assert_eq!(m.next(), (Action::Fetch, 5_000));
    }

    #[test]
    fn midnight_resync_is_one_shot_so_a_busy_answer_keeps_its_backoff() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![]);
        let next_day = |mono: u64| Now { mono_ms: mono, unix_ms: at(mono).unix_ms, day: 740_001 };
        m.on_tick(&next_day(5_000));
        m.on_discovery(&next_day(5_000), Discovery::Classic(1));
        assert_eq!(m.next(), (Action::Fetch, 5_000));
        // The read is rejected as busy: status stays connected, retry is scheduled.
        m.on_fetch_err(&next_day(5_000), &err(ErrKind::Busy, "OUTLOOK-105"));
        assert_eq!(m.status(), CalendarStatus::Connected);
        let gate = m.fetch_not_before;
        assert!(gate > 5_000);
        m.on_tick(&next_day(6_000));
        assert_eq!(m.fetch_not_before, gate);
        assert!(m.next().1 > 6_000);
    }

    #[test]
    fn a_restarted_outlook_clears_backoff_and_the_hung_verdict() {
        let mut m = Machine::new(None);
        m.on_discovery(&at(0), Discovery::Classic(10));
        for i in 0..MAX_ABANDONED {
            m.on_timeout(&at(1_000 * (i as u64 + 1)));
        }
        assert_eq!(m.status(), CalendarStatus::Failed);
        // Same process: the verdict stands and the gate holds.
        m.on_discovery(&at(20_000), Discovery::Classic(10));
        assert_eq!(m.status(), CalendarStatus::Failed);
        assert!(m.fetch_not_before > 20_000);
        // Quick restart (the process list never showed "waiting"): new pid, try again now.
        m.on_discovery(&at(20_000), Discovery::Classic(11));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Connecting, None));
        assert_eq!(m.abandoned(), 0);
        assert_eq!(m.next(), (Action::Fetch, 20_000));
        m.on_refresh(&at(20_000));
        assert_eq!(m.fetch_not_before, 20_000);
    }

    #[test]
    fn simulated_run_with_fake_source_recovers_after_errors() {
        let mut sim = Sim::new(vec![
            Err(err(ErrKind::NotInRot, "OUTLOOK-102")),
            Err(err(ErrKind::Busy, "OUTLOOK-105")),
            Ok(vec![ev("a", 0, 30)].into()),
        ]);
        sim.run_until(40_000);
        assert_eq!(sim.machine.status(), CalendarStatus::Connected);
        assert_eq!(sim.source.calls, 3);
        assert_eq!(sim.machine.snapshot(0).events.len(), 1);
        assert!(sim.machine.snapshot(0).next_retry_unix_ms.is_none());
    }

    // ---- smart search reads ----

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_none_keeps_todays_selection_rules() {
        assert!(reads_secondary(None, "x", true));
        assert!(!reads_secondary(None, "x", false));
        assert!(reads_primary(None, "p"));
    }

    #[test]
    fn only_restricts_to_the_listed_ids_selected_or_not() {
        let only = ids(&["x", "p"]);
        assert!(reads_secondary(Some(&only), "x", false), "an unchecked calendar is read when asked for");
        assert!(!reads_secondary(Some(&only), "y", true), "a checked calendar not listed is left out");
        assert!(reads_primary(Some(&only), "p"));
        assert!(!reads_primary(Some(&ids(&["x"])), "p"), "the primary is included only when listed");
        assert!(!reads_primary(Some(&[]), "p"));
    }

    #[test]
    fn chunks_cover_the_range_without_gaps() {
        let from = Utc.with_ymd_and_hms(2027, 3, 20, 22, 0, 0).unwrap(); // spans the March DST change
        let to = from + ChronoDuration::days(20);
        let chunks = chunk_range(from, to).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].0, from);
        assert_eq!(chunks.last().unwrap().1, to);
        for w in chunks.windows(2) {
            assert_eq!(w[0].1, w[1].0);
        }
        assert!(chunks.iter().all(|(a, b)| *b - *a <= ChronoDuration::days(MAX_RANGE_DAYS) && b > a));
        assert_eq!(chunks[2].1 - chunks[2].0, ChronoDuration::days(6));
    }

    #[test]
    fn exact_multiples_and_limits_of_chunking() {
        let from = at_utc(0);
        assert_eq!(chunk_range(from, from + ChronoDuration::days(7)).unwrap().len(), 1);
        assert_eq!(chunk_range(from, from + ChronoDuration::days(14)).unwrap().len(), 2);
        assert_eq!(chunk_range(from, from + ChronoDuration::hours(1)).unwrap().len(), 1);
        assert_eq!(chunk_range(from, from + ChronoDuration::days(31)).unwrap().len(), 5);
        assert_eq!(chunk_range(from, from + ChronoDuration::days(31) + ChronoDuration::hours(2) + ChronoDuration::seconds(1)), Err("OUTLOOK-108: invalid range".into()));
        assert_eq!(chunk_range(from, from), Err("OUTLOOK-108: invalid range".into()));
        assert_eq!(chunk_range(from, from - ChronoDuration::days(1)), Err("OUTLOOK-108: invalid range".into()));
    }

    #[test]
    fn a_whole_october_month_in_israel_is_accepted_when_dst_ends_inside_it() {
        // 2026-10-01 00:00+03:00 .. 2026-11-01 00:00+02:00 (fixed offsets): 31 days + 1 h.
        let from = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+03:00").unwrap().with_timezone(&Utc);
        let to = chrono::DateTime::parse_from_rfc3339("2026-11-01T00:00:00+02:00").unwrap().with_timezone(&Utc);
        assert_eq!(to - from, ChronoDuration::days(31) + ChronoDuration::hours(1));
        let chunks = chunk_range(from, to).unwrap();
        assert_eq!(chunks[0].0, from);
        assert_eq!(chunks.last().unwrap().1, to);
    }

    #[test]
    fn the_islands_range_check_still_stops_at_seven_days() {
        assert!(range_window("2027-01-15T00:00:00Z", "2027-01-22T00:00:00Z").is_ok());
        assert!(range_window("2027-01-15T00:00:00Z", "2027-01-22T00:00:01Z").is_err());
        let w = range_window("2027-01-15T00:00:00Z", "2027-01-16T00:00:00Z").unwrap();
        assert!(w.range && w.only.is_none() && !w.search);
        assert_eq!(w.event_cap(), MAX_EVENTS);
    }

    #[test]
    fn merge_keeps_one_copy_of_a_meeting_read_in_two_chunks() {
        let a = Fetched { events: vec![ev("m", 0, 600), ev("a", 10, 30)], ..Fetched::default() };
        let b = Fetched { events: vec![ev("m", 0, 600), ev("b", 700, 30)], ..Fetched::default() };
        let read = merge_reads(vec![a, b], at_utc(0), at_utc(2_000));
        let order: Vec<&str> = read.events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(order, ["m", "a", "b"]);
        assert!(!read.truncated);
    }

    #[test]
    fn merge_drops_events_outside_the_range() {
        let before = ev("old", -120, 30);
        let at_end = ev("late", 100, 30);
        let inside = ev("in", 50, 30);
        let read = merge_reads(vec![Fetched { events: vec![before, at_end, inside], ..Fetched::default() }], at_utc(0), at_utc(100));
        assert_eq!(read.events.len(), 1);
        assert_eq!(read.events[0].id, "in");
    }

    #[test]
    fn merge_reports_truncation_and_failures_once() {
        let a = Fetched { truncated: true, failed: vec![("c1".into(), "CAL-SHARED-101".into())], ..Fetched::default() };
        let b = Fetched { failed: vec![("c1".into(), "CAL-SHARED-101".into()), ("c2".into(), "CAL-SHARED-104".into())], ..Fetched::default() };
        let read = merge_reads(vec![a, b], at_utc(0), at_utc(100));
        assert!(read.truncated);
        assert_eq!(read.failed, vec![("c1".to_string(), "CAL-SHARED-101".to_string()), ("c2".to_string(), "CAL-SHARED-104".to_string())]);
    }

    #[test]
    fn merge_caps_at_the_search_limit_and_says_so() {
        let many: Vec<CalendarEventDto> = (0..SEARCH_MAX_EVENTS + 5).map(|i| ev(&format!("e{i}"), i as i64, 1)).collect();
        let read = merge_reads(vec![Fetched { events: many, ..Fetched::default() }], at_utc(0), at_utc(10_000));
        assert_eq!(read.events.len(), SEARCH_MAX_EVENTS);
        assert!(read.truncated);
    }

    #[test]
    fn capped_normalizing_reports_a_cut_and_the_old_cap_is_unchanged() {
        let many: Vec<CalendarEventDto> = (0..120).map(|i| ev(&format!("e{i}"), i as i64, 1)).collect();
        let (kept, cut) = normalize_events_capped(many.clone(), 100);
        assert_eq!((kept.len(), cut), (100, true));
        let (kept, cut) = normalize_events_capped(many.clone(), 120);
        assert_eq!((kept.len(), cut), (120, false));
        assert_eq!(normalize_events(many).len(), MAX_EVENTS);
    }

    #[test]
    fn organizer_names_are_distinct_and_trimmed() {
        let mut a = ev("a", 0, 10);
        a.organizer = Some(" Itzik Levi ".into());
        let mut b = ev("b", 20, 10);
        b.organizer = Some("Itzik Levi".into());
        let mut c = ev("c", 40, 10);
        c.organizer = Some("  ".into());
        let d = ev("d", 60, 10);
        let mut e = ev("e", 80, 10);
        e.organizer = Some("Dana".into());
        assert_eq!(organizer_names(&[a, b, c, d, e]), vec!["Itzik Levi".to_string(), "Dana".to_string()]);
    }

    #[test]
    fn query_range_without_a_service_or_with_a_bad_range_is_an_error() {
        let from = at_utc(0);
        assert!(query_range(from, from, None).unwrap_err().contains("OUTLOOK-108"));
        assert!(query_range(from, from + ChronoDuration::days(32), None).unwrap_err().contains("OUTLOOK-108"));
    }
}

#[cfg(test)]
mod prefetch_tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    fn t(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 3, day, hour, 0, 0).unwrap()
    }

    fn ids(n: usize) -> Vec<(String, bool)> {
        (0..n).map(|i| (format!("c{i}"), i == 0)).collect()
    }

    fn event(cal: &str, id: &str, start: DateTime<Utc>, len_h: i64) -> CalendarEventDto {
        CalendarEventDto {
            id: id.into(),
            calendar_id: cal.into(),
            calendar_name: "n".into(),
            source_kind: SourceKind::Shared,
            meeting_key: None,
            subject: "s".into(),
            start_utc: start,
            end_utc: start + ChronoDuration::hours(len_h),
            all_day: false,
            location: None,
            organizer: None,
            is_recurring: false,
            meeting_url: None,
            busy_status: BusyStatus::Busy,
            response_status: ResponseStatus::Accepted,
            color: None,
            calendar_color: None,
        }
    }

    fn fetched(events: Vec<CalendarEventDto>) -> Fetched {
        Fetched { events, ..Fetched::default() }
    }

    fn window(days: i64) -> (DateTime<Utc>, DateTime<Utc>) {
        (t(1, 0), t(1, 0) + ChronoDuration::days(days))
    }

    fn cal(id: &str, active: bool, events: Vec<CalendarEventDto>, read_ms: i64) -> PrefCal {
        PrefCal { id: id.into(), active, events, truncated: false, read_unix_ms: read_ms, error: None }
    }

    fn cache_with(cals: Vec<PrefCal>, days: i64) -> PrefetchCache {
        let planned: Vec<(String, bool)> = cals.iter().map(|c| (c.id.clone(), c.active)).collect();
        let mut cache = PrefetchCache::new();
        assert!(cache.commit(0, window(days), &planned, cals, 1_000));
        cache
    }

    #[test]
    fn window_is_local_midnight_to_midnight_dst_correct() {
        let day = NaiveDate::from_ymd_opt(2027, 3, 24).unwrap();
        // A zone at +2 that moves to +3 on 26 March 2027 (like Israel): the 7 days are 167 h long.
        let midnight = |d: NaiveDate| {
            let offset = if d >= NaiveDate::from_ymd_opt(2027, 3, 26).unwrap() { 3 } else { 2 };
            Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()) - ChronoDuration::hours(offset))
        };
        let (from, to) = prefetch_window_with(day, 7, midnight).unwrap();
        assert_eq!(from, Utc.with_ymd_and_hms(2027, 3, 23, 22, 0, 0).unwrap());
        assert_eq!(to, Utc.with_ymd_and_hms(2027, 3, 30, 21, 0, 0).unwrap());
        assert_eq!(to - from, ChronoDuration::hours(167));
        assert!(chunk_range(from, to).is_ok());
    }

    #[test]
    fn window_zero_is_off_and_31_days_is_the_most_and_chunks() {
        let day = NaiveDate::from_ymd_opt(2027, 10, 20).unwrap();
        let utc_midnight = |d: NaiveDate| Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()));
        assert!(prefetch_window_with(day, 0, utc_midnight).is_none());
        let (from, to) = prefetch_window_with(day, 31, utc_midnight).unwrap();
        assert_eq!(to - from, ChronoDuration::days(31));
        // More than the setting allows is cut to 31 days.
        let (_, to99) = prefetch_window_with(day, 99, utc_midnight).unwrap();
        assert_eq!(to99, to);
        assert_eq!(chunk_range(from, to).unwrap().len(), 5);
        // An unusable midnight means no window.
        assert!(prefetch_window_with(day, 7, |_| None).is_none());
        // The real local midnight exists for a plain date.
        assert!(local_midnight(day).is_some());
    }

    #[test]
    fn plan_batches_calendars_by_six_and_days_by_seven() {
        let round = PrefetchRound::new(&ids(14), &[], window(30), 0, 30).unwrap();
        // 3 groups (6 + 6 + 2) x 5 chunks (7+7+7+7+2 days)
        assert_eq!(round.pending_tasks(), 15);
        let mut round = round;
        let mut seen_groups = Vec::new();
        while let Some(task) = round.next_task() {
            assert!(task.ids.len() <= PREFETCH_BATCH);
            assert!(task.to - task.from <= ChronoDuration::days(MAX_RANGE_DAYS));
            seen_groups.push((task.group, task.ids.len()));
        }
        assert_eq!(seen_groups.first(), Some(&(0, 6)));
        assert_eq!(seen_groups.last(), Some(&(2, 2)));
        // group-major: a group's chunks follow each other
        assert!(seen_groups.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(PrefetchRound::new(&[], &[], window(7), 0, 7).is_none());
    }

    #[test]
    fn rotation_reads_unreached_calendars_first() {
        let all: Vec<String> = (0..8).map(|i| format!("c{i}")).collect();
        let order = prefetch_order(&all, &["c6".into(), "c7".into(), "gone".into()]);
        assert_eq!(&order[..3], ["c6", "c7", "c0"]);
        assert_eq!(order.len(), 8);
        assert_eq!(prefetch_order(&all, &[]), all);
        let round = PrefetchRound::new(&ids(8), &["c7".into()], window(7), 0, 7).unwrap();
        assert_eq!(round.planned()[0].0, "c7");
        // the active flag follows the calendar, not the position
        assert!(round.planned().iter().find(|(id, _)| id == "c0").unwrap().1);
        assert!(!round.planned()[0].1);
    }

    #[test]
    fn unreached_after_an_ended_round_are_the_failed_and_never_read() {
        let mut round = PrefetchRound::new(&ids(8), &[], window(7), 0, 7).unwrap();
        let task = round.next_task().unwrap();
        let mut f = fetched(vec![event("c0", "e", t(2, 9), 1)]);
        f.failed.push(("c3".into(), "CAL-SHARED-101".into()));
        let done = round.on_result(&task, f, 5);
        assert_eq!(done.len(), 6);
        assert_eq!(done.iter().find(|c| c.id == "c3").unwrap().error.as_deref(), Some("CAL-SHARED-101"));
        assert!(done.iter().find(|c| c.id == "c0").unwrap().error.is_none());
        // c6 and c7 (second group) were not reached
        let unreached = round.unreached();
        assert!(unreached.contains(&"c3".to_string()) && unreached.contains(&"c6".to_string()));
        assert!(!unreached.contains(&"c0".to_string()));
    }

    #[test]
    fn pending_calendars_are_read_once_more_and_a_good_repeat_clears_them() {
        let mut round = PrefetchRound::new(&ids(2), &[], window(7), 0, 7).unwrap();
        let task = round.next_task().unwrap();
        let mut f = fetched(vec![event("c0", "a", t(2, 9), 1)]);
        f.failed.push(("c1".into(), "CAL-SHARED-105".into()));
        assert!(round.on_result(&task, f, 5).is_empty(), "the group waits for its repeat");
        let retry = round.next_task().unwrap();
        assert!(retry.retry);
        assert_eq!(retry.ids, ["c1"]);
        let done = round.on_result(&retry, fetched(vec![event("c1", "b", t(3, 9), 1)]), 9);
        assert!(done.iter().all(|c| c.error.is_none()));
        assert_eq!(done.iter().find(|c| c.id == "c1").unwrap().events.len(), 1);
        assert!(round.is_done());
        // a repeat that is pending again is not repeated a third time
        let mut round = PrefetchRound::new(&ids(2), &[], window(7), 0, 7).unwrap();
        let task = round.next_task().unwrap();
        let mut f = fetched(vec![]);
        f.failed.push(("c1".into(), "CAL-SHARED-105".into()));
        round.on_result(&task, f.clone(), 5);
        let retry = round.next_task().unwrap();
        let done = round.on_result(&retry, f, 6);
        assert_eq!(done.iter().find(|c| c.id == "c1").unwrap().error.as_deref(), Some("CAL-SHARED-105"));
        assert!(round.is_done());
    }

    #[test]
    fn a_timeout_blames_the_quarantined_calendar_and_rereads_the_others() {
        let mut round = PrefetchRound::new(&ids(3), &[], window(7), 0, 7).unwrap();
        let task = round.next_task().unwrap();
        assert!(round.on_timeout(&task, Some("c2"), 5).is_empty());
        let again = round.next_task().unwrap();
        assert_eq!(again.ids, ["c0", "c1"]);
        let done = round.on_result(&again, fetched(vec![]), 6);
        assert_eq!(done.iter().find(|c| c.id == "c2").unwrap().error.as_deref(), Some("CAL-SHARED-104"));
        assert!(done.iter().filter(|c| c.id != "c2").all(|c| c.error.is_none()));
        // nobody to blame (the primary calendar): the batch fails and the round ends
        let mut round = PrefetchRound::new(&ids(8), &[], window(7), 0, 7).unwrap();
        let task = round.next_task().unwrap();
        let done = round.on_timeout(&task, None, 5);
        assert!(done.iter().all(|c| c.error.as_deref() == Some("OUTLOOK-109")));
        assert!(round.is_done());
        // a culprit that is not in the batch is no culprit
        let mut round = PrefetchRound::new(&ids(8), &[], window(7), 0, 7).unwrap();
        let task = round.next_task().unwrap();
        round.on_timeout(&task, Some("c7"), 5);
        assert!(round.is_done());
    }

    #[test]
    fn too_many_timeouts_or_errors_end_the_round() {
        let mut round = PrefetchRound::new(&ids(30), &[], window(7), 0, 7).unwrap();
        for _ in 0..PREFETCH_MAX_TIMEOUTS {
            let task = round.next_task().unwrap();
            let first = task.ids[0].clone();
            round.on_timeout(&task, Some(&first), 1);
        }
        assert!(round.is_done());
        let mut round = PrefetchRound::new(&ids(30), &[], window(7), 0, 7).unwrap();
        let task = round.next_task().unwrap();
        round.on_error(&task, "OUTLOOK-105", 1);
        assert!(!round.is_done());
        let task = round.next_task().unwrap();
        round.on_error(&task, "OUTLOOK-105", 1);
        assert!(round.is_done());
    }

    #[test]
    fn prefetched_needs_coverage_freshness_and_every_requested_calendar() {
        let now = 1_000_000;
        let cache = cache_with(
            vec![
                cal("c0", true, vec![event("c0", "a", t(2, 9), 1)], now),
                cal("c1", false, vec![event("c1", "b", t(3, 9), 1)], now),
            ],
            7,
        );
        let only = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // inside the window
        let read = prefetched_in(&cache, now, t(1, 0), t(8, 0), Some(&only(&["c0", "c1"]))).unwrap();
        assert_eq!(read.events.len(), 2);
        assert!(read.failed.is_empty() && !read.truncated);
        // partly outside the window, or reaching before it
        assert!(prefetched_in(&cache, now, t(1, 0), t(8, 1), Some(&only(&["c0"]))).is_none());
        assert!(prefetched_in(&cache, now, t(1, 0) - ChronoDuration::hours(1), t(2, 0), Some(&only(&["c0"]))).is_none());
        assert!(prefetched_in(&cache, now, t(2, 0), t(2, 0), Some(&only(&["c0"]))).is_none());
        // a calendar that was never read
        assert!(prefetched_in(&cache, now, t(1, 0), t(3, 0), Some(&only(&["c0", "zzz"]))).is_none());
        assert!(prefetched_in(&cache, now, t(1, 0), t(3, 0), Some(&[])).is_none());
        // fresh for 30 minutes
        assert!(prefetched_in(&cache, now + PREFETCH_FRESH_MS, t(1, 0), t(3, 0), Some(&only(&["c0"]))).is_some());
        assert!(prefetched_in(&cache, now + PREFETCH_FRESH_MS + 1, t(1, 0), t(3, 0), Some(&only(&["c0"]))).is_none());
        // an empty cache answers nothing
        assert!(prefetched_in(&PrefetchCache::new(), now, t(1, 0), t(3, 0), None).is_none());
    }

    #[test]
    fn prefetched_filters_to_the_window_and_to_only() {
        let now = 5;
        let cache = cache_with(
            vec![
                cal(
                    "c0",
                    true,
                    vec![event("c0", "before", t(1, 8), 1), event("c0", "in", t(2, 9), 1), event("c0", "after", t(4, 9), 1)],
                    now,
                ),
                cal("c1", false, vec![event("c1", "other", t(2, 10), 1)], now),
            ],
            7,
        );
        let read = prefetched_in(&cache, now, t(2, 0), t(3, 0), Some(&["c0".to_string()])).unwrap();
        let got: Vec<_> = read.events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(got, ["in"]);
        // a meeting running into the window counts
        let read = prefetched_in(&cache, now, t(1, 8) + ChronoDuration::minutes(30), t(2, 0), Some(&["c0".to_string()])).unwrap();
        assert_eq!(read.events.len(), 1);
        // without a list: the active calendars of the round only
        let read = prefetched_in(&cache, now, t(2, 0), t(3, 0), None).unwrap();
        let got: Vec<_> = read.events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(got, ["in"]);
    }

    #[test]
    fn a_failed_calendar_blocks_only_the_questions_that_need_it() {
        let now = 5;
        let mut bad = cal("c1", false, vec![], now);
        bad.error = Some("CAL-SHARED-101".into());
        let cache = cache_with(vec![cal("c0", true, vec![event("c0", "a", t(2, 9), 1)], now), bad], 7);
        assert!(prefetched_in(&cache, now, t(1, 0), t(3, 0), Some(&["c0".to_string()])).is_some());
        assert!(prefetched_in(&cache, now, t(1, 0), t(3, 0), Some(&["c0".to_string(), "c1".to_string()])).is_none());
        let status = prefetch_status_of(&cache, 7);
        assert_eq!((status.calendars_total, status.calendars_read), (2, 1));
        assert_eq!(status.failed, vec![("c1".to_string(), "CAL-SHARED-101".to_string())]);
        assert_eq!(status.from_unix_ms, Some(t(1, 0).timestamp_millis()));
        assert_eq!(status.to_unix_ms, Some(t(8, 0).timestamp_millis()));
        assert_eq!(status.fetched_unix_ms, Some(1_000));
    }

    #[test]
    fn status_serializes_camel_case() {
        let json =
            serde_json::to_value(PrefetchStatus { days: 7, failed: vec![("a".into(), "b".into())], ..Default::default() }).unwrap();
        assert_eq!(json["days"], 7);
        assert!(json["fromUnixMs"].is_null());
        assert_eq!(json["calendarsTotal"], 0);
        assert_eq!(json["failed"][0][1], "b");
    }

    #[test]
    fn the_memory_cap_drops_the_latest_starting_events_first() {
        let mk = |cal: &str, n: usize, h0: i64| -> Vec<CalendarEventDto> {
            (0..n).map(|i| event(cal, &format!("{cal}{i}"), t(1, 0) + ChronoDuration::hours(h0 + i as i64), 1)).collect()
        };
        let mut cals = vec![cal("a", true, mk("a", 5, 0), 1), cal("b", false, mk("b", 5, 100), 1)];
        assert!(!cap_prefetch_events(&mut cals, 10));
        assert!(cap_prefetch_events(&mut cals, 7));
        assert_eq!(cals[0].events.len(), 5);
        assert_eq!(cals[1].events.len(), 2);
        assert!(cals[1].truncated && !cals[0].truncated);
        assert_eq!(cals[1].events[1].id, "b1");
        // ties at the cut keep exactly the cap
        let same = |c: &str| (0..4).map(|i| event(c, &format!("{c}{i}"), t(2, 0), 1)).collect::<Vec<_>>();
        let mut cals = vec![cal("a", true, same("a"), 1), cal("b", true, same("b"), 1)];
        assert!(cap_prefetch_events(&mut cals, 5));
        assert_eq!(cals.iter().map(|c| c.events.len()).sum::<usize>(), 5);
    }

    #[test]
    fn commit_caps_marks_truncated_and_a_clear_voids_older_rounds() {
        let many: Vec<_> = (0..(PREFETCH_MAX_EVENTS + 50))
            .map(|i| event("c0", &format!("e{i}"), t(1, 0) + ChronoDuration::minutes(i as i64), 1))
            .collect();
        let mut cache = PrefetchCache::new();
        let planned = vec![("c0".to_string(), true)];
        assert!(cache.commit(0, window(7), &planned, vec![cal("c0", true, many, 10)], 10));
        assert_eq!(cache.cals[0].events.len(), PREFETCH_MAX_EVENTS);
        let read = prefetched_in(&cache, 10, t(1, 0), t(8, 0), Some(&["c0".to_string()])).unwrap();
        assert!(read.truncated);
        assert_eq!(read.events.len(), SEARCH_MAX_EVENTS);
        // a round that began before a clear cannot store into the cleared copy
        cache.clear();
        assert!(!cache.commit(0, window(7), &planned, vec![cal("c0", true, vec![], 11)], 11));
        assert!(cache.cals.is_empty() && cache.window.is_none());
        let epoch = cache.epoch;
        assert!(cache.commit(epoch, window(7), &planned, vec![cal("c0", true, vec![], 11)], 11));
        // another window drops what the old one held; a calendar gone from Outlook goes too
        let two = vec![("c0".to_string(), true), ("c1".to_string(), false)];
        assert!(cache.commit(epoch, window(7), &two, vec![cal("c1", false, vec![], 12)], 12));
        assert_eq!(cache.cals.len(), 2);
        assert!(cache.commit(epoch, window(14), &planned, vec![cal("c0", true, vec![], 13)], 13));
        assert_eq!(cache.cals.len(), 1);
    }

    #[test]
    fn a_pending_regular_sync_or_range_read_preempts_a_batch() {
        let ok = PrefetchGate {
            connected: true,
            days: 7,
            due: Some(100),
            now: 100,
            sync_at: 100 + PREFETCH_SYNC_GAP_MS + 1,
            pending_ranges: 0,
            work_waiting: false,
        };
        assert!(prefetch_may_run(&ok));
        assert!(!prefetch_may_run(&PrefetchGate { pending_ranges: 1, ..ok }));
        assert!(!prefetch_may_run(&PrefetchGate { work_waiting: true, ..ok }));
        assert!(!prefetch_may_run(&PrefetchGate { sync_at: 100 + PREFETCH_SYNC_GAP_MS, ..ok }));
        assert!(!prefetch_may_run(&PrefetchGate { sync_at: 100, ..ok }));
        assert!(!prefetch_may_run(&PrefetchGate { connected: false, ..ok }));
        assert!(!prefetch_may_run(&PrefetchGate { days: 0, ..ok }));
        assert!(!prefetch_may_run(&PrefetchGate { due: Some(101), ..ok }));
        assert!(!prefetch_may_run(&PrefetchGate { due: None, ..ok }));
    }

    #[test]
    fn a_prefetch_timeout_never_escalates_the_island() {
        // The prefetch reads are search reads, and a search read's timeout leaves the machine alone.
        let mut m = Machine::new(None);
        let now = Now { mono_ms: 0, unix_ms: 0, day: 1 };
        m.on_fetch_ok(&now, vec![]);
        on_range_timeout(&mut m, &now, true);
        assert_eq!(m.status(), CalendarStatus::Connected);
        assert_eq!(m.abandoned(), 0);
    }

    #[test]
    fn dedup_keeps_the_first_calendars_copy_of_a_meeting() {
        let now = 5;
        let mut a = event("c0", "mine", t(2, 9), 1);
        a.meeting_key = Some("g".into());
        let mut b = event("c1", "theirs", t(2, 9), 1);
        b.meeting_key = Some("g".into());
        let cache = cache_with(vec![cal("c0", true, vec![a], now), cal("c1", false, vec![b], now)], 7);
        let read = prefetched_in(&cache, now, t(2, 0), t(3, 0), Some(&["c1".to_string(), "c0".to_string()])).unwrap();
        assert_eq!(read.events.len(), 1);
        assert_eq!(read.events[0].id, "mine");
    }
}
