//! Mailboxes and mail search in the user's own running classic Outlook (smart search).
//!
//! Attach-only, like the calendar: one fresh STA thread per request (`outlook::run_on_outlook`),
//! a hard time budget, nothing cached that could keep OUTLOOK.EXE alive. Mail is read through
//! `Folder.GetTable` columns (subject, sender name, received, unread, EntryID): never Body,
//! SenderEmailAddress or Recipients. EntryIDs/StoreIDs stay in this module; callers get opaque keys.
//! Subjects, names and search terms are never logged.
//!
//! CONTRACT (used by `assistant`): every pub item below.
//!
//! Layout of the file: public types, pure helpers (sanitiser, DASL filter builder, store
//! classification, error mapping, work-unit planning, budgeted driver, free/busy parser; all
//! unit-tested), process state (discovery cache, quarantine, key map), the COM code, the public
//! functions, tests.
//!
//! Error codes (MAIL family; raw HRESULTs never leave this module):
//! - MAIL-101 no permission for the mailbox / folder
//! - MAIL-102 mailbox or folder gone / invalid
//! - MAIL-103 mailbox unreachable (offline, not synchronised)
//! - MAIL-104 the mail is no longer available (open)
//! - MAIL-105 not finished within the time budget
//! - MAIL-106 mailbox skipped: it hung recently (quarantined for 15 minutes)
//! - MAIL-107 the search filter was rejected
//! - MAIL-108 any other read failure
//! - MAIL-109 free/busy not available
//!
//! Live findings (classic Outlook 16, non-Exchange profile; see the `live_` tests):
//! - `Folder.GetTable(filter)` with a DASL `@SQL=` filter works with Hebrew `LIKE '%..%'`.
//! - Table DATE columns are UTC (`TABLE_DATES_ARE_UTC`), unlike every Item property (local).

use crate::com::{self, ComError, ComResult, Dispatch};
use crate::outlook::{bool_prop, hash16, i32_prop, optional, run_on_outlook, str_prop};
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDateTime, TimeZone, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailboxKind {
    /// The profile's default store (ExchangeStoreType 0, or the default store of a non-Exchange profile).
    Primary,
    /// Another Exchange mailbox in the profile: delegate or auto-mapped shared (ExchangeStoreType 1).
    Shared,
    /// An additional Exchange mailbox added to the account (ExchangeStoreType 4).
    Additional,
    /// An online archive (detected only when reliable; otherwise `Shared`/`Additional`).
    Archive,
    /// A PST/OST data file that is not a mailbox (IsDataFileStore and not Exchange).
    DataFile,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailboxAccess {
    Ok,
    /// Opening its Inbox was refused (no permission).
    Denied,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailboxAvailability {
    Ok,
    /// Network / not synced / server unreachable.
    Offline,
    /// Removed or invalid.
    Gone,
    /// Did not answer within the discovery budget.
    Timeout,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MailboxInfo {
    /// `hash16("store|" + StoreID)`: stable per mailbox, safe to show/persist.
    pub id: String,
    /// Display name as Outlook shows it (presentation only, never logged).
    pub name: String,
    pub kind: MailboxKind,
    pub access: MailboxAccess,
    pub availability: MailboxAvailability,
    /// Cached Exchange mode, when known.
    pub cached: Option<bool>,
    /// Windows/Exchange instant search index available for this store.
    pub instant_search: Option<bool>,
}

impl MailboxInfo {
    /// Can be searched now.
    pub fn searchable(&self) -> bool {
        self.access == MailboxAccess::Ok && self.availability == MailboxAvailability::Ok
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MailQuery {
    /// Mailbox ids to search; empty = every searchable mailbox.
    pub mailboxes: Vec<String>,
    /// AND of groups; each group is alternative spellings OR-ed. Matched against subject (and the
    /// indexed body where the store has instant search).
    pub terms: Vec<Vec<String>>,
    /// Sender name alternatives (OR-ed), matched against the sender's display name / address.
    pub sender: Vec<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub unread_only: bool,
    /// Max hits returned (newest first).
    pub limit: usize,
    /// Time budget for this call. The spec: 10 s for the first round, +10 s per extension.
    pub budget_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MailHit {
    /// Opaque, resolvable by [`open_mail`] for a while (memory only).
    pub key: String,
    pub mailbox_id: String,
    pub subject: String,
    /// Sender display name.
    pub from: String,
    pub received: Option<DateTime<Utc>>,
    pub unread: bool,
    /// Folder display name ("Inbox", "Sent Items", a subfolder).
    pub folder: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MailboxOutcome {
    pub mailbox_id: String,
    /// Every folder of this mailbox in scope was searched.
    pub complete: bool,
    /// e.g. "MAIL-101" (no permission) - the other mailboxes are unaffected.
    pub error: Option<String>,
}

/// Where a budgeted search stopped. Opaque; pass it back to continue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MailCursor {
    pub(crate) pending: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MailSearchResult {
    /// Newest first, at most `limit`.
    pub hits: Vec<MailHit>,
    pub per_mailbox: Vec<MailboxOutcome>,
    /// Some folders were not searched (budget). `cursor` continues.
    pub partial: bool,
    pub cursor: Option<MailCursor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreeBusyStatus {
    Free,
    Tentative,
    Busy,
    Oof,
    WorkingElsewhere,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FreeBusyBlock {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub status: FreeBusyStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FreeBusy {
    /// The name resolved to exactly one person in the address book.
    pub resolved: bool,
    /// The resolved display name (presentation only).
    pub display_name: Option<String>,
    /// Non-free blocks inside the asked window, merged.
    pub blocks: Vec<FreeBusyBlock>,
}

// =============================================================================
// Pure: constants, sanitiser, DASL filter
// =============================================================================

const MAX_STORES: usize = 20;
const MAX_TERM_GROUPS: usize = 6;
const MAX_ALTERNATIVES: usize = 6;
const MAX_TERM_CHARS: usize = 40;
const MAX_SENDERS: usize = 6;
const ROWS_PER_UNIT: usize = 25;
/// Rows read from one table at most (rows skipped by the date re-check count).
const MAX_ROWS_SCANNED: usize = 400;
const MAX_FOLDERS: usize = 200;
const MAX_DEPTH: usize = 4;
const DEFAULT_LIMIT: usize = 20;
const DEFAULT_BUDGET_MS: u64 = 10_000;
const MIN_BUDGET_MS: u64 = 1_000;
const MAX_BUDGET_MS: u64 = 60_000;
const DISCOVERY_BUDGET_MS: u64 = 8_000;
/// Extra time the caller waits beyond the worker's own budget before giving the thread up.
const GRACE_SECS: u64 = 5;
const DISCOVERY_CACHE_SECS: u64 = 600;
const QUARANTINE_SECS: u64 = 900;
const KEY_CAP: usize = 500;
const MAX_NAME_CHARS: usize = 120;

/// Table DATE columns are UTC (live probe, see the module header). Item properties are local.
const TABLE_DATES_ARE_UTC: bool = true;

/// OlDefaultFolders
const FOLDER_DELETED: i32 = 3;
const FOLDER_OUTBOX: i32 = 4;
const FOLDER_SENT: i32 = 5;
const FOLDER_INBOX: i32 = 6;
const FOLDER_DRAFTS: i32 = 16;
const FOLDER_JUNK: i32 = 23;

const P_SUBJECT: &str = "urn:schemas:httpmail:subject";
const P_FROMNAME: &str = "urn:schemas:httpmail:fromname";
const P_SENDERNAME: &str = "urn:schemas:httpmail:sendername";
const P_RECEIVED: &str = "urn:schemas:httpmail:datereceived";
const P_READ: &str = "urn:schemas:httpmail:read";
const P_BODY: &str = "urn:schemas:httpmail:textdescription";

/// One search term made safe for a DASL string literal: control characters and the LIKE /
/// property-syntax characters `% _ [ ] " ;` are removed, whitespace is collapsed, at most 40
/// characters are kept. The single quote is kept here and doubled by [`like_literal`].
pub fn sanitize_term(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut pending_space = false;
    for c in raw.chars() {
        if c.is_control() || matches!(c, '%' | '_' | '[' | ']' | '"' | ';') {
            // A stripped separator must not glue two words together.
            pending_space = !out.is_empty();
            continue;
        }
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    let clipped: String = out.chars().take(MAX_TERM_CHARS).collect();
    let clipped = clipped.trim().to_string();
    (!clipped.is_empty()).then_some(clipped)
}

/// The content of a single-quoted DASL literal.
fn like_literal(term: &str) -> String {
    term.replace('\'', "''")
}

/// The query after sanitising: at most 6 groups of at most 6 alternatives, no duplicates.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FilterSpec {
    pub groups: Vec<Vec<String>>,
    pub senders: Vec<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub unread_only: bool,
}

pub(crate) fn sanitize_query(q: &MailQuery) -> FilterSpec {
    let mut groups = Vec::new();
    for group in q.terms.iter().take(MAX_TERM_GROUPS * 2) {
        let mut alts: Vec<String> = Vec::new();
        for raw in group {
            if let Some(t) = sanitize_term(raw) {
                if !alts.iter().any(|a| a.to_lowercase() == t.to_lowercase()) {
                    alts.push(t);
                }
            }
            if alts.len() >= MAX_ALTERNATIVES {
                break;
            }
        }
        if !alts.is_empty() {
            groups.push(alts);
        }
        if groups.len() >= MAX_TERM_GROUPS {
            break;
        }
    }
    let mut senders: Vec<String> = Vec::new();
    for raw in &q.sender {
        if let Some(t) = sanitize_term(raw) {
            if !senders.iter().any(|a| a.to_lowercase() == t.to_lowercase()) {
                senders.push(t);
            }
        }
        if senders.len() >= MAX_SENDERS {
            break;
        }
    }
    FilterSpec { groups, senders, since: q.since, until: q.until, unread_only: q.unread_only }
}

impl FilterSpec {
    /// Nothing to filter on: every mail matches (newest first).
    fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.senders.is_empty() && self.since.is_none() && self.until.is_none() && !self.unread_only
    }
}

/// DASL date literal. Dates are given in UTC; the Rust-side re-check of every row is the
/// authority, the filter only narrows the table.
fn dasl_date(d: &DateTime<Utc>) -> String {
    d.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The `@SQL=` filter for `Folder.GetTable`. `body` adds the indexed full-text clause
/// (`ci_phrasematch`) for stores whose instant search is on. `None` = no filter at all.
/// `dates` puts the received range into the filter too.
pub(crate) fn build_filter(spec: &FilterSpec, body: bool, dates: bool) -> Option<String> {
    let mut clauses: Vec<String> = Vec::new();
    for group in &spec.groups {
        let mut alts: Vec<String> = Vec::new();
        for t in group {
            let lit = like_literal(t);
            alts.push(format!("\"{P_SUBJECT}\" LIKE '%{lit}%'"));
            if body {
                alts.push(format!("\"{P_BODY}\" ci_phrasematch '{lit}'"));
            }
        }
        clauses.push(format!("({})", alts.join(" OR ")));
    }
    if !spec.senders.is_empty() {
        let mut alts: Vec<String> = Vec::new();
        for s in &spec.senders {
            let lit = like_literal(s);
            alts.push(format!("\"{P_FROMNAME}\" LIKE '%{lit}%'"));
            alts.push(format!("\"{P_SENDERNAME}\" LIKE '%{lit}%'"));
        }
        clauses.push(format!("({})", alts.join(" OR ")));
    }
    if dates {
        if let Some(since) = &spec.since {
            clauses.push(format!("\"{P_RECEIVED}\" >= '{}'", dasl_date(since)));
        }
        if let Some(until) = &spec.until {
            clauses.push(format!("\"{P_RECEIVED}\" < '{}'", dasl_date(until)));
        }
    }
    if spec.unread_only {
        clauses.push(format!("\"{P_READ}\" = 0"));
    }
    if clauses.is_empty() {
        None
    } else {
        Some(format!("@SQL={}", clauses.join(" AND ")))
    }
}

// =============================================================================
// Pure: store classification and error mapping
// =============================================================================

/// `None` for public folders (ExchangeStoreType 2): never searched.
pub(crate) fn classify_store(exchange_type: Option<i32>, is_data_file: bool, is_default: bool, name: &str) -> Option<MailboxKind> {
    let lower = name.to_lowercase();
    let archive_name = lower.starts_with("online archive") || lower.starts_with("in-place archive") || name.starts_with("ארכיון מקוון");
    match exchange_type {
        Some(2) => None,
        Some(0) => Some(MailboxKind::Primary),
        Some(4) => Some(if archive_name { MailboxKind::Archive } else { MailboxKind::Additional }),
        Some(1) => Some(if archive_name { MailboxKind::Archive } else { MailboxKind::Shared }),
        _ if is_default => Some(MailboxKind::Primary),
        _ if is_data_file => Some(MailboxKind::DataFile),
        _ => Some(MailboxKind::Other),
    }
}

/// How opening a store / folder failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreFailure {
    Denied,
    Offline,
    Gone,
    Other,
}

pub(crate) fn classify_failure(e: &ComError) -> StoreFailure {
    match e.effective() as u32 {
        // E_ACCESSDENIED (= MAPI_E_NO_ACCESS)
        0x8007_0005 => StoreFailure::Denied,
        // MAPI_E_NOT_FOUND, MAPI_E_INVALID_ENTRYID, MAPI_E_OBJECT_DELETED
        0x8004_010F | 0x8004_0107 | 0x8004_0117 => StoreFailure::Gone,
        // MAPI_E_NETWORK_ERROR, MAPI_E_LOGON_FAILED, MAPI_E_UNCONFIGURED, MAPI_E_FAILONEPROVIDER,
        // MAPI_E_END_OF_SESSION, MAPI_E_TIMEOUT
        0x8004_0115 | 0x8004_0111 | 0x8004_011C | 0x8004_011D | 0x8004_0200 | 0x8004_0401 => StoreFailure::Offline,
        _ => StoreFailure::Other,
    }
}

/// Access / availability for a store from the outcome of opening its Inbox.
pub(crate) fn access_from_failure(f: StoreFailure) -> (MailboxAccess, MailboxAvailability) {
    match f {
        StoreFailure::Denied => (MailboxAccess::Denied, MailboxAvailability::Ok),
        StoreFailure::Offline => (MailboxAccess::Unknown, MailboxAvailability::Offline),
        StoreFailure::Gone => (MailboxAccess::Unknown, MailboxAvailability::Gone),
        // Unknown failure: not searchable now; "offline" is the least misleading availability.
        StoreFailure::Other => (MailboxAccess::Unknown, MailboxAvailability::Offline),
    }
}

pub(crate) fn failure_code(f: StoreFailure) -> &'static str {
    match f {
        StoreFailure::Denied => "MAIL-101",
        StoreFailure::Gone => "MAIL-102",
        StoreFailure::Offline => "MAIL-103",
        StoreFailure::Other => "MAIL-108",
    }
}

/// A failure either ends the whole call (Outlook busy / gone) or belongs to one mailbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Fatal(&'static str),
    Mailbox(&'static str),
}

pub(crate) fn classify_com(e: &ComError) -> Failure {
    if e.is_busy() {
        Failure::Fatal("OUTLOOK-105")
    } else if e.is_disconnected() {
        Failure::Fatal("OUTLOOK-102")
    } else {
        Failure::Mailbox(failure_code(classify_failure(e)))
    }
}


// =============================================================================
// Pure: folders to skip
// =============================================================================

/// System mail folders that hold no user mail, by name (English and Hebrew Outlook). Folders
/// that Outlook exposes through `GetDefaultFolder` (Deleted, Junk, Drafts, Outbox) are skipped
/// by EntryID instead, which does not depend on the language.
pub(crate) fn is_system_folder_name(name: &str) -> bool {
    const NAMES: [&str; 14] = [
        "sync issues",
        "conflicts",
        "local failures",
        "server failures",
        "rss feeds",
        "rss subscriptions",
        "conversation history",
        "quick step settings",
        "yammer root",
        "outbox",
        "בעיות סנכרון",
        "התנגשויות",
        "כשלים מקומיים",
        "כשלים בשרת",
    ];
    let lower = name.trim().to_lowercase();
    NAMES.iter().any(|n| lower == *n) || lower.starts_with("הזנות rss") || lower.starts_with("היסטוריית שיחות")
}

/// Should the walk go into / search this folder? `default_item_type` is `Folder.DefaultItemType`
/// (0 = mail).
pub(crate) fn skip_folder(default_item_type: Option<i32>, name: &str, is_excluded_default: bool) -> bool {
    is_excluded_default || default_item_type != Some(0) || is_system_folder_name(name)
}

// =============================================================================
// Pure: work units, planning, the budgeted driver
// =============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkUnit {
    /// Tier 1: a default folder (Inbox 6, Sent 5) of a mailbox.
    Special { mailbox: String, folder: i32 },
    /// Tier 2 planning: list the mailbox's folders and queue a `Folder` unit per mail folder.
    Expand { mailbox: String },
    /// Tier 2: one folder, by its ids (kept only in memory, inside the cursor).
    Folder { mailbox: String, entry: String, store: String, name: String },
}

impl WorkUnit {
    pub fn mailbox(&self) -> &str {
        match self {
            WorkUnit::Special { mailbox, .. } | WorkUnit::Expand { mailbox } | WorkUnit::Folder { mailbox, .. } => mailbox,
        }
    }

    pub fn tier(&self) -> u8 {
        match self {
            WorkUnit::Special { .. } => 1,
            _ => 2,
        }
    }

    pub fn encode(&self) -> String {
        match self {
            WorkUnit::Special { mailbox, folder } => format!("S|{mailbox}|{folder}"),
            WorkUnit::Expand { mailbox } => format!("X|{mailbox}"),
            WorkUnit::Folder { mailbox, entry, store, name } => format!("F|{mailbox}|{entry}|{store}|{name}"),
        }
    }

    pub fn decode(s: &str) -> Option<WorkUnit> {
        let mut it = s.splitn(5, '|');
        match it.next()? {
            "S" => {
                let mailbox = it.next()?.to_string();
                let folder = it.next()?.parse().ok()?;
                Some(WorkUnit::Special { mailbox, folder })
            }
            "X" => Some(WorkUnit::Expand { mailbox: it.next()?.to_string() }),
            "F" => {
                let mailbox = it.next()?.to_string();
                let entry = it.next()?.to_string();
                let store = it.next()?.to_string();
                let name = it.next().unwrap_or("").to_string();
                Some(WorkUnit::Folder { mailbox, entry, store, name })
            }
            _ => None,
        }
    }
}

/// Tier 1 for every mailbox (Inbox, Sent), then one Expand per mailbox for tier 2. Mailbox order
/// is kept: the one the user chose first is searched first.
pub(crate) fn plan_units(mailboxes: &[String]) -> VecDeque<WorkUnit> {
    let mut units = VecDeque::new();
    for m in mailboxes {
        units.push_back(WorkUnit::Special { mailbox: m.clone(), folder: FOLDER_INBOX });
        units.push_back(WorkUnit::Special { mailbox: m.clone(), folder: FOLDER_SENT });
    }
    for m in mailboxes {
        units.push_back(WorkUnit::Expand { mailbox: m.clone() });
    }
    units
}

pub(crate) trait Clock {
    fn now_ms(&self) -> u64;
}

pub(crate) struct RealClock(pub Instant);

impl Clock for RealClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}

/// What running one unit produced.
#[derive(Default)]
pub(crate) struct UnitResult {
    pub hits: Vec<MailHit>,
    /// New units for the front of the queue (tier 2 folders found by an Expand).
    pub follow: Vec<WorkUnit>,
    /// The mailbox failed (code): its remaining units are dropped, the others go on.
    pub mailbox_error: Option<&'static str>,
    /// Outlook is busy / gone: the whole call ends.
    pub fatal: Option<&'static str>,
}

pub(crate) trait UnitRunner {
    fn run(&mut self, unit: &WorkUnit, remaining_ms: u64) -> UnitResult;
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Progress {
    pub hits: Vec<MailHit>,
    pub pending: VecDeque<WorkUnit>,
    /// Mailbox id -> first error code.
    pub errors: Vec<(String, &'static str)>,
    pub scope: Vec<String>,
    /// Mailbox being worked on (for quarantining a hang).
    pub current: Option<String>,
    /// Tier of the last unit that ran.
    pub last_tier: Option<u8>,
}

impl Progress {
    pub fn new(scope: Vec<String>, pending: VecDeque<WorkUnit>) -> Self {
        Progress { hits: Vec::new(), pending, errors: Vec::new(), scope, current: None, last_tier: None }
    }

    pub fn fail_mailbox(&mut self, mailbox: &str, code: &'static str) {
        if !self.errors.iter().any(|(m, _)| m == mailbox) {
            self.errors.push((mailbox.to_string(), code));
        }
        self.pending.retain(|u| u.mailbox() != mailbox);
    }

    pub fn outcomes(&self) -> Vec<MailboxOutcome> {
        self.scope
            .iter()
            .map(|m| {
                let error = self.errors.iter().find(|(e, _)| e == m).map(|(_, c)| c.to_string());
                let complete = error.is_none() && !self.pending.iter().any(|u| u.mailbox() == m);
                MailboxOutcome { mailbox_id: m.clone(), complete, error }
            })
            .collect()
    }
}

/// Run units in order until the queue is empty, the budget is spent, or a finished tier already
/// holds `limit` hits (the rest stays in the queue for a continuation). The budget is checked
/// before every unit; the runner checks it between rows.
pub(crate) fn drive(
    progress: &mut Progress,
    runner: &mut dyn UnitRunner,
    clock: &dyn Clock,
    budget_ms: u64,
    limit: usize,
    on_unit_done: &mut dyn FnMut(&Progress),
) -> Result<(), &'static str> {
    while let Some(unit) = progress.pending.front().cloned() {
        let now = clock.now_ms();
        if now >= budget_ms {
            break;
        }
        // A tier that is complete and already enough: stop before starting the next one.
        if progress.hits.len() >= limit {
            if let Some(prev_tier) = progress.last_tier {
                if unit.tier() > prev_tier {
                    break;
                }
            }
        }
        progress.pending.pop_front();
        progress.current = Some(unit.mailbox().to_string());
        let result = runner.run(&unit, budget_ms - now);
        if let Some(code) = result.fatal {
            progress.pending.push_front(unit);
            return Err(code);
        }
        progress.hits.extend(result.hits);
        for u in result.follow.into_iter().rev() {
            progress.pending.push_front(u);
        }
        if let Some(code) = result.mailbox_error {
            progress.fail_mailbox(unit.mailbox(), code);
        }
        progress.last_tier = Some(unit.tier());
        on_unit_done(progress);
    }
    progress.current = None;
    Ok(())
}

/// Merge hit lists from several rounds: newest first (unknown dates last), no duplicate keys,
/// at most `limit`.
pub(crate) fn merge_hits(mut hits: Vec<MailHit>, limit: usize) -> Vec<MailHit> {
    hits.sort_by(|a, b| b.received.cmp(&a.received).then_with(|| a.key.cmp(&b.key)));
    let mut seen = HashSet::new();
    hits.retain(|h| seen.insert(h.key.clone()));
    hits.truncate(limit);
    hits
}

// =============================================================================
// Pure: free/busy
// =============================================================================

fn free_busy_status(c: char) -> Option<FreeBusyStatus> {
    match c {
        '1' => Some(FreeBusyStatus::Tentative),
        '2' => Some(FreeBusyStatus::Busy),
        '3' => Some(FreeBusyStatus::Oof),
        '4' => Some(FreeBusyStatus::WorkingElsewhere),
        // 0 = free; anything else is not a status and is treated as free.
        _ => None,
    }
}

/// `FreeBusy` string -> non-free blocks inside `[from, to)`. Character `i` covers
/// `[start + i * slot, start + (i + 1) * slot)`; `start` is local midnight of the first day.
/// Neighbouring slots of the same status merge.
pub(crate) fn parse_free_busy(s: &str, start: DateTime<Utc>, slot_minutes: i64, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<FreeBusyBlock> {
    let mut blocks: Vec<FreeBusyBlock> = Vec::new();
    if slot_minutes <= 0 {
        return blocks;
    }
    for (i, c) in s.chars().enumerate() {
        let Some(status) = free_busy_status(c) else { continue };
        let slot_start = start + ChronoDuration::minutes(slot_minutes * i as i64);
        let slot_end = slot_start + ChronoDuration::minutes(slot_minutes);
        let (a, b) = (slot_start.max(from), slot_end.min(to));
        if a >= b {
            continue;
        }
        match blocks.last_mut() {
            Some(last) if last.status == status && last.end == a => last.end = b,
            _ => blocks.push(FreeBusyBlock { start: a, end: b, status }),
        }
    }
    blocks
}

// =============================================================================
// Process state: discovery cache, key map, quarantine
// =============================================================================

/// Hit key -> (EntryID, StoreID), memory only, bounded.
pub(crate) struct KeyMap {
    cap: usize,
    map: HashMap<String, (String, String)>,
    order: VecDeque<String>,
}

impl KeyMap {
    pub fn new(cap: usize) -> Self {
        KeyMap { cap, map: HashMap::new(), order: VecDeque::new() }
    }

    pub fn insert(&mut self, key: String, entry: String, store: String) {
        if self.map.insert(key.clone(), (entry, store)).is_none() {
            self.order.push_back(key);
        }
        while self.map.len() > self.cap {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<(String, String)> {
        self.map.get(key).cloned()
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// Mailboxes that hung a worker: skipped until the instant stored here.
#[derive(Default)]
pub(crate) struct Quarantine {
    until: HashMap<String, Instant>,
}

impl Quarantine {
    pub fn add(&mut self, mailbox: &str, now: Instant) {
        self.until.insert(mailbox.to_string(), now + Duration::from_secs(QUARANTINE_SECS));
    }

    pub fn is_quarantined(&self, mailbox: &str, now: Instant) -> bool {
        self.until.get(mailbox).is_some_and(|until| now < *until)
    }
}

struct State {
    profile: String,
    mailboxes: Option<(Instant, Vec<MailboxInfo>)>,
    /// mailbox id -> StoreID
    store_ids: HashMap<String, String>,
    keys: KeyMap,
    quarantine: Quarantine,
}

impl State {
    fn new() -> Self {
        State { profile: String::new(), mailboxes: None, store_ids: HashMap::new(), keys: KeyMap::new(KEY_CAP), quarantine: Quarantine::default() }
    }

    /// A different profile than the one the caches were built for: forget everything but the
    /// quarantine (it is keyed by store hashes, which differ per profile anyway).
    fn note_profile(&mut self, profile: &str) {
        if self.profile != profile {
            if !self.profile.is_empty() {
                self.mailboxes = None;
                self.store_ids.clear();
                self.keys.clear();
            }
            self.profile = profile.to_string();
        }
    }
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::new())).lock().unwrap_or_else(|p| p.into_inner())
}

static BUSY: AtomicBool = AtomicBool::new(false);

/// At most one discovery / search / free-busy at a time; a second caller is told OUTLOOK-105.
struct BusyGuard;

impl BusyGuard {
    fn acquire() -> Result<BusyGuard, String> {
        if BUSY.swap(true, Ordering::AcqRel) {
            Err("OUTLOOK-105: another mail operation is running".into())
        } else {
            Ok(BusyGuard)
        }
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

fn clamp_budget(ms: u64) -> u64 {
    if ms == 0 {
        DEFAULT_BUDGET_MS
    } else {
        ms.clamp(MIN_BUDGET_MS, MAX_BUDGET_MS)
    }
}

fn wait_secs(budget_ms: u64) -> u64 {
    budget_ms.div_ceil(1000) + GRACE_SECS
}

// =============================================================================
// COM: reading
// =============================================================================

fn clip_name(s: &str) -> String {
    s.trim().chars().take(MAX_NAME_CHARS).collect()
}

/// A Table DATE cell -> UTC.
fn table_date(serial: f64) -> Option<DateTime<Utc>> {
    if TABLE_DATES_ARE_UTC {
        com::date_to_naive(serial).map(|n| Utc.from_utc_datetime(&n))
    } else {
        com::date_to_utc(serial)
    }
}

fn profile_hash(session: &mut Dispatch) -> String {
    let name = str_prop(session, "CurrentProfileName").ok().flatten().unwrap_or_default();
    hash16(&format!("profile|{name}"))
}

/// Read one store's header and probe its Inbox.
fn read_store(store: &mut Dispatch, default_store_id: &str, budget_over: bool, current: &Mutex<Option<String>>) -> ComResult<Option<(MailboxInfo, String)>> {
    let store_id = str_prop(store, "StoreID")?.unwrap_or_default();
    if store_id.is_empty() {
        return Ok(None);
    }
    let id = hash16(&format!("store|{store_id}"));
    let name = clip_name(&str_prop(store, "DisplayName")?.unwrap_or_default());
    let exchange_type = i32_prop(store, "ExchangeStoreType")?;
    let is_data_file = bool_prop(store, "IsDataFileStore")?;
    let Some(kind) = classify_store(exchange_type, is_data_file, store_id == default_store_id, &name) else {
        return Ok(None);
    };
    let mut info = MailboxInfo {
        id: id.clone(),
        name,
        kind,
        access: MailboxAccess::Unknown,
        availability: MailboxAvailability::Timeout,
        cached: None,
        instant_search: None,
    };
    let quarantined = state().quarantine.is_quarantined(&id, Instant::now());
    if quarantined || budget_over {
        return Ok(Some((info, store_id)));
    }
    *current.lock().unwrap_or_else(|p| p.into_inner()) = Some(id);
    info.cached = optional(store.get("IsCachedExchange"))?.and_then(|v| com::variant_bool(&v));
    info.instant_search = optional(store.get("IsInstantSearchEnabled"))?.and_then(|v| com::variant_bool(&v));
    match store.call_object("GetDefaultFolder", vec![com::variant_from_i32(FOLDER_INBOX)]) {
        Ok(Some(_)) => {
            info.access = MailboxAccess::Ok;
            info.availability = MailboxAvailability::Ok;
        }
        Ok(None) => {
            info.availability = MailboxAvailability::Gone;
        }
        Err(e) => match classify_com(&e) {
            Failure::Fatal(_) => return Err(e),
            Failure::Mailbox(_) => {
                let (access, availability) = access_from_failure(classify_failure(&e));
                info.access = access;
                info.availability = availability;
            }
        },
    }
    Ok(Some((info, store_id)))
}

/// All stores of the session. Public folders are skipped, at most 20 stores are read, and a
/// store is only touched while the budget lasts. Results are published to `shared` per store so
/// a hang later still leaves the earlier stores usable.
fn discover_in(session: &mut Dispatch, clock: &dyn Clock, budget_ms: u64, shared: &Mutex<Vec<MailboxInfo>>, current: &Mutex<Option<String>>) -> ComResult<Vec<MailboxInfo>> {
    let profile = profile_hash(session);
    state().note_profile(&profile);
    let default_store_id = match session.get_object("DefaultStore") {
        Ok(mut d) => str_prop(&mut d, "StoreID").ok().flatten().unwrap_or_default(),
        Err(e) if matches!(classify_com(&e), Failure::Fatal(_)) => return Err(e),
        Err(_) => String::new(),
    };
    let mut stores = session.get_object("Stores")?;
    let count = i32_prop(&mut stores, "Count")?.unwrap_or(0).max(0) as usize;
    let mut out: Vec<MailboxInfo> = Vec::new();
    for i in 1..=count.min(MAX_STORES) {
        let Some(mut store) = stores.call_object("Item", vec![com::variant_from_i32(i as i32)])? else { continue };
        let over = clock.now_ms() >= budget_ms;
        if let Some((info, store_id)) = read_store(&mut store, &default_store_id, over, current)? {
            state().store_ids.insert(info.id.clone(), store_id);
            out.push(info);
            *shared.lock().unwrap_or_else(|p| p.into_inner()) = out.clone();
        }
    }
    *current.lock().unwrap_or_else(|p| p.into_inner()) = None;
    Ok(out)
}

/// Column order of the search table.
const COL_ENTRY: i32 = 1;
const COL_SUBJECT: i32 = 2;
const COL_FROM: i32 = 3;
const COL_RECEIVED: i32 = 4;
const COL_READ: i32 = 5;

struct OutlookRunner<'a> {
    session: Dispatch,
    spec: &'a FilterSpec,
    limit: usize,
    /// mailbox id -> instant search known to be on
    instant: HashMap<String, bool>,
    /// After a filter with dates was rejected, stop trying it.
    dates_in_filter: bool,
    clock: &'a dyn Clock,
    budget_ms: u64,
}

impl OutlookRunner<'_> {
    fn store(&mut self, mailbox: &str) -> ComResult<Dispatch> {
        let store_id = state().store_ids.get(mailbox).cloned();
        let Some(store_id) = store_id else { return Err(ComError::new("GetStoreFromID", 0x8004_010F_u32 as i32)) };
        self.session
            .call_object("GetStoreFromID", vec![com::variant_from_str(&store_id)])?
            .ok_or_else(|| ComError::new("GetStoreFromID", com::E_NOOBJECT))
    }

    /// Search one folder. `store_id` is the folder's store (for the key map).
    fn search_folder(&mut self, mailbox: &str, folder: &mut Dispatch, store_id: &str, name: &str, deadline_ms: u64) -> ComResult<Vec<MailHit>> {
        let body = self.instant.get(mailbox).copied().unwrap_or(false);
        let mut attempts: Vec<(bool, bool)> = Vec::new();
        if body {
            attempts.push((true, self.dates_in_filter));
        }
        attempts.push((false, self.dates_in_filter));
        if self.dates_in_filter {
            attempts.push((false, false));
        }
        let mut last_err: Option<ComError> = None;
        let mut table = None;
        for (with_body, with_dates) in attempts {
            let args = match build_filter(self.spec, with_body, with_dates) {
                Some(f) => vec![com::variant_from_str(&f)],
                None => Vec::new(),
            };
            match folder.call_object("GetTable", args) {
                Ok(Some(t)) => {
                    table = Some(t);
                    break;
                }
                Ok(None) => {}
                Err(e) => match classify_com(&e) {
                    Failure::Fatal(_) => return Err(e),
                    Failure::Mailbox(_) => {
                        // Denied / gone / offline will not get better with another filter.
                        if classify_failure(&e) != StoreFailure::Other {
                            return Err(e);
                        }
                        last_err = Some(e);
                    }
                },
            }
        }
        let Some(mut table) = table else {
            return Err(last_err.unwrap_or_else(|| ComError::new("GetTable", com::E_NOOBJECT)));
        };
        {
            let mut columns = table.get_object("Columns")?;
            columns.call("RemoveAll", Vec::new())?;
            for p in ["EntryID", P_SUBJECT, P_FROMNAME, P_RECEIVED, P_READ] {
                // Property names are runtime strings here; Dispatch caches DISPIDs per static name,
                // so "Add" is the key and the argument is the property.
                columns.call("Add", vec![com::variant_from_str(p)])?;
            }
        }
        let sorted = table.call("Sort", vec![com::variant_from_str("[ReceivedTime]"), com::variant_from_bool(true)]).is_ok();

        let mut hits = Vec::new();
        let mut scanned = 0usize;
        while hits.len() < ROWS_PER_UNIT && scanned < MAX_ROWS_SCANNED {
            if self.clock.now_ms() >= deadline_ms {
                break;
            }
            if bool_prop(&mut table, "EndOfTable")? {
                break;
            }
            let Some(mut row) = table.call_object("GetNextRow", Vec::new())? else { break };
            scanned += 1;
            let cell = |row: &mut Dispatch, n: i32| row.call("Item", vec![com::variant_from_i32(n)]);
            let entry = optional(cell(&mut row, COL_ENTRY))?.as_ref().and_then(com::variant_string).unwrap_or_default();
            if entry.is_empty() {
                continue;
            }
            let received = optional(cell(&mut row, COL_RECEIVED))?.as_ref().and_then(com::variant_date).and_then(table_date);
            // The date is re-checked here whatever the filter did (its date format is locale
            // sensitive). The table is sorted newest first, so older than `since` ends it.
            if let (Some(since), Some(r)) = (self.spec.since, received) {
                if r < since {
                    if sorted {
                        break;
                    }
                    continue;
                }
            }
            if let (Some(until), Some(r)) = (self.spec.until, received) {
                if r >= until {
                    continue;
                }
            }
            let subject = optional(cell(&mut row, COL_SUBJECT))?.as_ref().and_then(com::variant_string).unwrap_or_default();
            let from = optional(cell(&mut row, COL_FROM))?.as_ref().and_then(com::variant_string).unwrap_or_default();
            let read = optional(cell(&mut row, COL_READ))?.as_ref().and_then(com::variant_bool).unwrap_or(true);
            if self.spec.unread_only && read {
                continue;
            }
            let key = hash16(&format!("mail|{store_id}|{entry}"));
            state().keys.insert(key.clone(), entry, store_id.to_string());
            hits.push(MailHit {
                key,
                mailbox_id: mailbox.to_string(),
                subject: clip_name(&subject),
                from: clip_name(&from),
                received,
                unread: !read,
                folder: clip_name(name),
            });
        }
        let _ = (self.limit, self.budget_ms);
        Ok(hits)
    }

    fn run_special(&mut self, mailbox: &str, code: i32, deadline_ms: u64) -> ComResult<Vec<MailHit>> {
        let mut store = self.store(mailbox)?;
        let store_id = state().store_ids.get(mailbox).cloned().unwrap_or_default();
        let mut folder = store
            .call_object("GetDefaultFolder", vec![com::variant_from_i32(code)])?
            .ok_or_else(|| ComError::new("GetDefaultFolder", 0x8004_010F_u32 as i32))?;
        let name = str_prop(&mut folder, "Name")?.unwrap_or_default();
        self.search_folder(mailbox, &mut folder, &store_id, &name, deadline_ms)
    }

    /// Walk the store's folders (depth <= 4, <= 200) and queue every mail folder not yet covered.
    fn run_expand(&mut self, mailbox: &str, deadline_ms: u64) -> ComResult<Vec<WorkUnit>> {
        let mut store = self.store(mailbox)?;
        let store_id = state().store_ids.get(mailbox).cloned().unwrap_or_default();
        let mut excluded = HashSet::new();
        for code in [FOLDER_DELETED, FOLDER_JUNK, FOLDER_DRAFTS, FOLDER_OUTBOX] {
            // A store without such a folder (or one we may not open) simply has nothing to exclude.
            if let Ok(Some(mut f)) = store.call_object("GetDefaultFolder", vec![com::variant_from_i32(code)]) {
                if let Some(id) = str_prop(&mut f, "EntryID").ok().flatten() {
                    excluded.insert(id);
                }
            }
        }
        let mut done = HashSet::new();
        for code in [FOLDER_INBOX, FOLDER_SENT] {
            if let Ok(Some(mut f)) = store.call_object("GetDefaultFolder", vec![com::variant_from_i32(code)]) {
                if let Some(id) = str_prop(&mut f, "EntryID").ok().flatten() {
                    done.insert(id);
                }
            }
        }
        let mut root = store.call_object("GetRootFolder", Vec::new())?.ok_or_else(|| ComError::new("GetRootFolder", com::E_NOOBJECT))?;
        let mut units = Vec::new();
        let mut visited = 0usize;
        self.walk(mailbox, &store_id, &mut root, 0, &excluded, &done, &mut units, &mut visited, deadline_ms)?;
        Ok(units)
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        mailbox: &str,
        store_id: &str,
        parent: &mut Dispatch,
        depth: usize,
        excluded: &HashSet<String>,
        done: &HashSet<String>,
        units: &mut Vec<WorkUnit>,
        visited: &mut usize,
        deadline_ms: u64,
    ) -> ComResult<()> {
        if depth >= MAX_DEPTH {
            return Ok(());
        }
        let mut folders = parent.get_object("Folders")?;
        let count = i32_prop(&mut folders, "Count")?.unwrap_or(0).max(0);
        for i in 1..=count {
            if *visited >= MAX_FOLDERS || self.clock.now_ms() >= deadline_ms {
                return Ok(());
            }
            let Some(mut f) = folders.call_object("Item", vec![com::variant_from_i32(i)])? else { continue };
            *visited += 1;
            let entry = str_prop(&mut f, "EntryID")?.unwrap_or_default();
            let name = str_prop(&mut f, "Name")?.unwrap_or_default();
            let kind = i32_prop(&mut f, "DefaultItemType")?;
            if entry.is_empty() || skip_folder(kind, &name, excluded.contains(&entry)) {
                continue;
            }
            if !done.contains(&entry) {
                units.push(WorkUnit::Folder { mailbox: mailbox.to_string(), entry: entry.clone(), store: store_id.to_string(), name: clip_name(&name) });
            }
            self.walk(mailbox, store_id, &mut f, depth + 1, excluded, done, units, visited, deadline_ms)?;
        }
        Ok(())
    }

    fn run_folder(&mut self, mailbox: &str, entry: &str, store_id: &str, name: &str, deadline_ms: u64) -> ComResult<Vec<MailHit>> {
        let mut folder = self
            .session
            .call_object("GetFolderFromID", vec![com::variant_from_str(entry), com::variant_from_str(store_id)])?
            .ok_or_else(|| ComError::new("GetFolderFromID", 0x8004_010F_u32 as i32))?;
        self.search_folder(mailbox, &mut folder, store_id, name, deadline_ms)
    }
}

impl UnitRunner for OutlookRunner<'_> {
    fn run(&mut self, unit: &WorkUnit, remaining_ms: u64) -> UnitResult {
        let deadline = self.clock.now_ms() + remaining_ms;
        let mailbox = unit.mailbox().to_string();
        let outcome: ComResult<(Vec<MailHit>, Vec<WorkUnit>)> = match unit {
            WorkUnit::Special { folder, .. } => self.run_special(&mailbox, *folder, deadline).map(|h| (h, Vec::new())),
            WorkUnit::Expand { .. } => self.run_expand(&mailbox, deadline).map(|u| (Vec::new(), u)),
            WorkUnit::Folder { entry, store, name, .. } => self.run_folder(&mailbox, entry, store, name, deadline).map(|h| (h, Vec::new())),
        };
        match outcome {
            Ok((hits, follow)) => UnitResult { hits, follow, ..Default::default() },
            Err(e) => match classify_com(&e) {
                Failure::Fatal(code) => UnitResult { fatal: Some(code), ..Default::default() },
                Failure::Mailbox(code) => {
                    // No Sent Items / unreadable subfolder is not a failure of the mailbox; only
                    // its Inbox or the planning of its folders is.
                    let optional_unit = matches!(unit, WorkUnit::Folder { .. }) || matches!(unit, WorkUnit::Special { folder, .. } if *folder != FOLDER_INBOX);
                    let benign = optional_unit && classify_failure(&e) != StoreFailure::Offline;
                    if benign {
                        UnitResult::default()
                    } else {
                        let code = if matches!(unit, WorkUnit::Special { .. } | WorkUnit::Folder { .. }) && code == "MAIL-108" && classify_failure(&e) == StoreFailure::Other {
                            "MAIL-107"
                        } else {
                            code
                        };
                        UnitResult { mailbox_error: Some(code), ..Default::default() }
                    }
                }
            },
        }
    }
}

// =============================================================================
// Public API
// =============================================================================

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The mailboxes of the running profile, public folders skipped. Cached for 10 minutes; `force`
/// re-reads. Blocking (call through `rt::run_blocking`).
pub fn discover_mailboxes(force: bool) -> Result<Vec<MailboxInfo>, String> {
    if !force {
        let st = state();
        if let Some((at, list)) = &st.mailboxes {
            if at.elapsed() < Duration::from_secs(DISCOVERY_CACHE_SECS) {
                return Ok(list.clone());
            }
        }
    }
    let _busy = BusyGuard::acquire()?;
    let started = Instant::now();
    let shared: Arc<Mutex<Vec<MailboxInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let current: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let (s2, c2) = (shared.clone(), current.clone());
    let result = run_on_outlook("companyisland-mail-discover", wait_secs(DISCOVERY_BUDGET_MS), move |_pid| {
        let mut app = Dispatch::get_active("Outlook.Application")?;
        let mut session = app.get_object("Session")?;
        discover_in(&mut session, &RealClock(started), DISCOVERY_BUDGET_MS, &s2, &c2)
    });
    match result {
        Ok(list) => {
            state().mailboxes = Some((Instant::now(), list.clone()));
            crate::dlog!("INFO", "outlook_mail", "discovery: {} mailboxes in {} ms", list.len(), started.elapsed().as_millis());
            Ok(list)
        }
        Err(e) if e.starts_with("OUTLOOK-109") => {
            // A store hung the worker: quarantine it and hand back what was read before.
            let hung = lock(&current).clone();
            let mut list = lock(&shared).clone();
            if let Some(id) = &hung {
                state().quarantine.add(id, Instant::now());
                match list.iter_mut().find(|m| &m.id == id) {
                    Some(m) => m.availability = MailboxAvailability::Timeout,
                    None => {}
                }
            }
            crate::dlog!("WARN", "outlook_mail", "MAIL-105 discovery timed out after {} mailboxes", list.len());
            if list.is_empty() {
                Err(e)
            } else {
                Ok(list)
            }
        }
        Err(e) => Err(e),
    }
}

/// The last discovery result without touching Outlook (for parsing: `intent::Known`).
pub fn cached_mailboxes() -> Vec<MailboxInfo> {
    state().mailboxes.as_ref().map(|(_, l)| l.clone()).unwrap_or_default()
}

fn decode_cursor(cursor: &MailCursor) -> VecDeque<WorkUnit> {
    cursor.pending.iter().filter_map(|s| WorkUnit::decode(s)).collect()
}

fn result_from(progress: &Progress, limit: usize) -> MailSearchResult {
    let partial = !progress.pending.is_empty();
    MailSearchResult {
        hits: merge_hits(progress.hits.clone(), limit),
        per_mailbox: progress.outcomes(),
        partial,
        cursor: partial.then(|| MailCursor { pending: progress.pending.iter().map(WorkUnit::encode).collect() }),
    }
}

/// Which mailboxes a query covers: the asked ones that can be searched, or all that can.
pub(crate) fn scope_of(asked: &[String], known: &[MailboxInfo]) -> Vec<String> {
    known
        .iter()
        .filter(|m| m.searchable() && (asked.is_empty() || asked.contains(&m.id)))
        .map(|m| m.id.clone())
        .collect()
}

/// Search mail, tiered: Inbox and Sent Items of every mailbox first, then their subfolders, until
/// the budget is spent. Blocking. `cursor` = continue a previous partial search of the same query.
pub fn search_mail(query: &MailQuery, cursor: Option<MailCursor>) -> Result<MailSearchResult, String> {
    let _busy = BusyGuard::acquire()?;
    let started = Instant::now();
    let budget_ms = clamp_budget(query.budget_ms);
    let limit = if query.limit == 0 { DEFAULT_LIMIT } else { query.limit };
    let spec = sanitize_query(query);
    let asked = query.mailboxes.clone();

    let cached: Option<Vec<MailboxInfo>> = {
        let st = state();
        st.mailboxes.as_ref().filter(|(at, _)| at.elapsed() < Duration::from_secs(DISCOVERY_CACHE_SECS)).map(|(_, l)| l.clone())
    };
    let resume = cursor.as_ref().map(decode_cursor);

    let shared: Arc<Mutex<Progress>> = Arc::new(Mutex::new(Progress::default()));
    let sh = shared.clone();
    let spec2 = spec.clone();
    let result = run_on_outlook("companyisland-mail-search", wait_secs(budget_ms), move |_pid| {
        let mut app = Dispatch::get_active("Outlook.Application")?;
        let mut session = app.get_object("Session")?;
        let clock = RealClock(started);
        let current: Mutex<Option<String>> = Mutex::new(None);
        let discovery_sink: Mutex<Vec<MailboxInfo>> = Mutex::new(Vec::new());
        let known = match cached {
            Some(list) => {
                let profile = profile_hash(&mut session);
                state().note_profile(&profile);
                list
            }
            None => {
                let list = discover_in(&mut session, &clock, DISCOVERY_BUDGET_MS.min(budget_ms / 2).max(MIN_BUDGET_MS), &discovery_sink, &current)?;
                state().mailboxes = Some((Instant::now(), list.clone()));
                list
            }
        };
        let (scope, pending) = match resume {
            Some(units) => {
                let mut scope: Vec<String> = Vec::new();
                for u in &units {
                    if !scope.iter().any(|m| m == u.mailbox()) {
                        scope.push(u.mailbox().to_string());
                    }
                }
                (scope, units)
            }
            None => {
                let scope = scope_of(&asked, &known);
                let pending = plan_units(&scope);
                (scope, pending)
            }
        };
        let mut progress = Progress::new(scope, pending);
        let instant: HashMap<String, bool> = known.iter().map(|m| (m.id.clone(), m.instant_search == Some(true))).collect();
        let mut runner = OutlookRunner {
            session,
            spec: &spec2,
            limit,
            instant,
            dates_in_filter: false,
            clock: &clock,
            budget_ms,
        };
        let mut publish = |p: &Progress| *lock(&sh) = p.clone();
        publish(&progress);
        let r = drive(&mut progress, &mut runner, &clock, budget_ms, limit, &mut publish);
        publish(&progress);
        // Re-raise a fatal code as the COM error class `run_on_outlook` maps back to it.
        r.map_err(|code| ComError::new(code, if code == "OUTLOOK-105" { 0x8001_0001_u32 as i32 } else { 0x8001_0108_u32 as i32 }))
    });
    let mut progress = lock(&shared).clone();
    let out = match result {
        Ok(()) => Ok(result_from(&progress, limit)),
        Err(e) if e.starts_with("OUTLOOK-109") => {
            // A call into Outlook never returned: quarantine that mailbox, keep what was found.
            if let Some(m) = progress.current.clone() {
                state().quarantine.add(&m, Instant::now());
                progress.fail_mailbox(&m, "MAIL-105");
            }
            Ok(result_from(&progress, limit))
        }
        Err(e) => Err(e),
    };
    if let Ok(r) = &out {
        crate::dlog!(
            "INFO",
            "outlook_mail",
            "search: {} hits, {} mailboxes, partial={}, {} ms",
            r.hits.len(),
            r.per_mailbox.len(),
            r.partial,
            started.elapsed().as_millis()
        );
    }
    out
}

/// Show one found mail in Outlook (explicit click only). Blocking.
pub fn open_mail(key: &str) -> Result<(), String> {
    let Some((entry, store)) = state().keys.get(key) else {
        return Err("MAIL-104: mail no longer available".into());
    };
    let found = run_on_outlook("companyisland-mail-open", 10, move |pid| {
        let mut app = Dispatch::get_active("Outlook.Application")?;
        let mut session = app.get_object("Session")?;
        let item = session.call_object("GetItemFromID", vec![com::variant_from_str(&entry), com::variant_from_str(&store)]);
        let mut item = match item {
            Ok(Some(i)) => i,
            Ok(None) => return Ok(false),
            Err(e) if matches!(classify_failure(&e), StoreFailure::Gone) => return Ok(false),
            Err(e) => return Err(e),
        };
        // Only real mail is shown; a key only ever points at an item of a mail folder, so this
        // is a safeguard against a reused EntryID.
        let class = str_prop(&mut item, "MessageClass")?.unwrap_or_default();
        if !is_mail_class(&class) {
            return Ok(false);
        }
        // The click was on our (never-activated) window, so Windows lets this process hand the
        // foreground on to the user's own Outlook for this one window.
        unsafe {
            let _ = AllowSetForegroundWindow(pid);
        }
        item.call("Display", vec![com::variant_from_bool(false)])?;
        Ok(true)
    })?;
    if found {
        Ok(())
    } else {
        Err("MAIL-104: mail no longer available".into())
    }
}

/// Mail and its report / receipt variants. Meeting requests are `IPM.Schedule.*` and also open
/// from a mail folder, so the whole `IPM.` family except obviously non-mail items is allowed.
pub(crate) fn is_mail_class(class: &str) -> bool {
    let c = class.to_ascii_uppercase();
    c.starts_with("IPM.NOTE") || c.starts_with("IPM.SCHEDULE") || c.starts_with("REPORT.IPM.NOTE") || c.starts_with("IPM.RECALL") || c.starts_with("IPM.OOF")
}

fn clean_person_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control()).collect();
    s.trim().chars().take(80).collect()
}

/// A colleague's free/busy (no titles, no calendar permission needed) for `[from, to)`. Used when
/// their calendar is not open in the user's Outlook. Blocking, network (address book).
pub fn free_busy(name: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<FreeBusy, String> {
    let name = clean_person_name(name);
    if name.is_empty() || to <= from {
        return Err("MAIL-109: free/busy not available".into());
    }
    let _busy = BusyGuard::acquire()?;
    // Slot 0 is local midnight of the first day (Outlook counts the string from there).
    let first_day = from.with_timezone(&Local).date_naive();
    let midnight: NaiveDateTime = first_day.and_hms_opt(0, 0, 0).ok_or("MAIL-109: free/busy not available")?;
    let start_utc = com::resolve_local(midnight, |n| Local.from_local_datetime(n).map(|d| d.with_timezone(&Utc)))
        .ok_or("MAIL-109: free/busy not available")?;
    let start_date = com::naive_to_date(midnight).ok_or("MAIL-109: free/busy not available")?;
    let raw = run_on_outlook("companyisland-mail-freebusy", 25, move |_pid| {
        let mut app = Dispatch::get_active("Outlook.Application")?;
        let mut session = app.get_object("Session")?;
        let mut recipient = session
            .call_object("CreateRecipient", vec![com::variant_from_str(&name)])?
            .ok_or_else(|| ComError::new("CreateRecipient", com::E_NOOBJECT))?;
        // Resolve() against the address book; an ambiguous or unknown name stays unresolved.
        let _ = optional(recipient.call("Resolve", Vec::new()))?;
        if !bool_prop(&mut recipient, "Resolved")? {
            return Ok((false, None, String::new()));
        }
        let display = str_prop(&mut recipient, "Name")?.map(|n| clip_name(&n));
        let fb = recipient.call("FreeBusy", vec![com::variant_from_date(start_date)?, com::variant_from_i32(30), com::variant_from_bool(true)])?;
        Ok((true, display, com::variant_string(&fb).unwrap_or_default()))
    })?;
    let (resolved, display_name, digits) = raw;
    if !resolved {
        return Ok(FreeBusy { resolved: false, display_name: None, blocks: Vec::new() });
    }
    if digits.is_empty() {
        return Err("MAIL-109: free/busy not available".into());
    }
    Ok(FreeBusy { resolved: true, display_name, blocks: parse_free_busy(&digits, start_utc, 30, from, to) })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    fn q(terms: &[&[&str]]) -> MailQuery {
        MailQuery { terms: terms.iter().map(|g| g.iter().map(|s| s.to_string()).collect()).collect(), ..Default::default() }
    }

    // ---- sanitiser -------------------------------------------------------------------------

    #[test]
    fn sanitize_strips_like_and_syntax_characters() {
        assert_eq!(sanitize_term("50%_off[x]\"y\";z").as_deref(), Some("50 off x y z"));
        assert_eq!(sanitize_term("a\u{0}b\tc\nd").as_deref(), Some("a b c d"));
        assert_eq!(sanitize_term("  %%  "), None);
        assert_eq!(sanitize_term(""), None);
    }

    #[test]
    fn sanitize_keeps_hebrew_and_clips_length() {
        assert_eq!(sanitize_term("  תקציב   שנתי ").as_deref(), Some("תקציב שנתי"));
        let long = "א".repeat(100);
        assert_eq!(sanitize_term(&long).unwrap().chars().count(), 40);
    }

    #[test]
    fn sanitize_query_caps_groups_and_alternatives() {
        let many: Vec<String> = (0..20).map(|i| format!("w{i}")).collect();
        let query = MailQuery { terms: vec![many.clone(); 10], sender: many, ..Default::default() };
        let spec = sanitize_query(&query);
        assert_eq!(spec.groups.len(), 6);
        assert!(spec.groups.iter().all(|g| g.len() == 6));
        assert_eq!(spec.senders.len(), 6);
    }

    #[test]
    fn sanitize_query_dedups_and_drops_empty_groups() {
        let spec = sanitize_query(&q(&[&["Budget", "budget", "%%"], &["%"], &[]]));
        assert_eq!(spec.groups, vec![vec!["Budget".to_string()]]);
    }

    // ---- filter ----------------------------------------------------------------------------

    #[test]
    fn filter_subject_or_within_group_and_across_groups() {
        let spec = sanitize_query(&q(&[&["תקציב", "budget"], &["2026"]]));
        let f = build_filter(&spec, false, false).unwrap();
        assert_eq!(
            f,
            "@SQL=(\"urn:schemas:httpmail:subject\" LIKE '%תקציב%' OR \"urn:schemas:httpmail:subject\" LIKE '%budget%') AND (\"urn:schemas:httpmail:subject\" LIKE '%2026%')"
        );
    }

    #[test]
    fn filter_body_clause_only_when_asked() {
        let spec = sanitize_query(&q(&[&["תקציב"]]));
        assert!(!build_filter(&spec, false, false).unwrap().contains("ci_phrasematch"));
        let f = build_filter(&spec, true, false).unwrap();
        assert!(f.contains("\"urn:schemas:httpmail:textdescription\" ci_phrasematch 'תקציב'"));
    }

    #[test]
    fn filter_sender_unread_and_dates() {
        let query = MailQuery {
            sender: vec!["יובל".into()],
            unread_only: true,
            since: Some(utc(2026, 10, 1, 0, 0)),
            until: Some(utc(2026, 10, 8, 12, 30)),
            ..Default::default()
        };
        let spec = sanitize_query(&query);
        let f = build_filter(&spec, false, true).unwrap();
        assert!(f.contains("\"urn:schemas:httpmail:fromname\" LIKE '%יובל%'"));
        assert!(f.contains("\"urn:schemas:httpmail:sendername\" LIKE '%יובל%'"));
        assert!(f.contains("\"urn:schemas:httpmail:datereceived\" >= '2026-10-01 00:00:00'"));
        assert!(f.contains("\"urn:schemas:httpmail:datereceived\" < '2026-10-08 12:30:00'"));
        assert!(f.ends_with("\"urn:schemas:httpmail:read\" = 0"));
        let without_dates = build_filter(&spec, false, false).unwrap();
        assert!(!without_dates.contains("datereceived"));
    }

    #[test]
    fn filter_empty_query_has_no_filter() {
        assert!(sanitize_query(&MailQuery::default()).is_empty());
        assert_eq!(build_filter(&sanitize_query(&MailQuery::default()), true, true), None);
    }

    #[test]
    fn filter_resists_injection() {
        let attacks = ["x' OR 1=1 --", "x\") OR (\"a\"=\"a", "%' ; DROP", "a'; b", "\\'", "x\u{0}' OR '"];
        for a in attacks {
            let spec = sanitize_query(&q(&[&[a]]));
            let f = build_filter(&spec, true, false).unwrap();
            // Every quote that is not a delimiter is doubled, and no property quote can be forged.
            for lit in f.split("LIKE '%").skip(1) {
                let inner = &lit[..lit.find("%'").unwrap()];
                assert!(!inner.replace("''", "").contains('\''), "unescaped quote in {inner:?}");
                assert!(!inner.contains('"') && !inner.contains(';') && !inner.contains('%'));
            }
            assert_eq!(f.matches('"').count() % 2, 0);
            assert!(!f.contains(';'));
        }
    }

    #[test]
    fn filter_doubles_single_quotes() {
        let spec = sanitize_query(&q(&[&["o'brien"]]));
        assert!(build_filter(&spec, false, false).unwrap().contains("LIKE '%o''brien%'"));
    }

    // ---- stores and errors ------------------------------------------------------------------

    #[test]
    fn store_kinds() {
        assert_eq!(classify_store(Some(2), false, false, "Public Folders"), None);
        assert_eq!(classify_store(Some(0), false, true, "me"), Some(MailboxKind::Primary));
        assert_eq!(classify_store(Some(1), false, false, "Team"), Some(MailboxKind::Shared));
        assert_eq!(classify_store(Some(4), false, false, "Other"), Some(MailboxKind::Additional));
        assert_eq!(classify_store(Some(1), false, false, "Online Archive - me@x"), Some(MailboxKind::Archive));
        assert_eq!(classify_store(Some(1), false, false, "ארכיון מקוון - יובל"), Some(MailboxKind::Archive));
        assert_eq!(classify_store(Some(3), true, false, "backup.pst"), Some(MailboxKind::DataFile));
        assert_eq!(classify_store(Some(3), true, true, "Outlook Data File"), Some(MailboxKind::Primary));
        assert_eq!(classify_store(None, false, true, "x"), Some(MailboxKind::Primary));
        assert_eq!(classify_store(None, false, false, "x"), Some(MailboxKind::Other));
    }

    fn com_err(hr: u32) -> ComError {
        ComError::new("t", hr as i32)
    }

    #[test]
    fn failure_mapping() {
        assert_eq!(classify_failure(&com_err(0x8007_0005)), StoreFailure::Denied);
        assert_eq!(classify_failure(&com_err(0x8004_0115)), StoreFailure::Offline);
        assert_eq!(classify_failure(&com_err(0x8004_011D)), StoreFailure::Offline);
        assert_eq!(classify_failure(&com_err(0x8004_0401)), StoreFailure::Offline);
        assert_eq!(classify_failure(&com_err(0x8004_010F)), StoreFailure::Gone);
        assert_eq!(classify_failure(&com_err(0x8004_0107)), StoreFailure::Gone);
        assert_eq!(classify_failure(&com_err(0x8000_4005)), StoreFailure::Other);
        assert_eq!(access_from_failure(StoreFailure::Denied), (MailboxAccess::Denied, MailboxAvailability::Ok));
        assert_eq!(access_from_failure(StoreFailure::Gone).1, MailboxAvailability::Gone);
        assert_eq!(access_from_failure(StoreFailure::Offline).1, MailboxAvailability::Offline);
        assert_eq!(failure_code(StoreFailure::Denied), "MAIL-101");
        assert_eq!(failure_code(StoreFailure::Gone), "MAIL-102");
        assert_eq!(failure_code(StoreFailure::Offline), "MAIL-103");
    }

    #[test]
    fn only_busy_and_disconnected_are_fatal() {
        assert_eq!(classify_com(&com_err(0x8001_0001)), Failure::Fatal("OUTLOOK-105"));
        assert_eq!(classify_com(&com_err(0x8001_010A)), Failure::Fatal("OUTLOOK-105"));
        assert_eq!(classify_com(&com_err(0x8001_0108)), Failure::Fatal("OUTLOOK-102"));
        // Access denied is the mailbox's problem, not Outlook's.
        assert_eq!(classify_com(&com_err(0x8007_0005)), Failure::Mailbox("MAIL-101"));
        assert_eq!(classify_com(&com_err(0x8004_0115)), Failure::Mailbox("MAIL-103"));
    }

    // ---- folders and planning ------------------------------------------------------------------

    #[test]
    fn skip_rules() {
        assert!(!skip_folder(Some(0), "Projects", false));
        assert!(skip_folder(Some(0), "Deleted Items", true));
        assert!(skip_folder(Some(1), "Calendar", false));
        assert!(skip_folder(Some(2), "Contacts", false));
        assert!(skip_folder(None, "Weird", false));
        for n in ["Sync Issues", "Conflicts", "Local Failures", "Server Failures", "RSS Feeds", "Conversation History", "בעיות סנכרון", "התנגשויות", "הזנות RSS"] {
            assert!(skip_folder(Some(0), n, false), "{n}");
        }
        assert!(!skip_folder(Some(0), "Conflicts with budget", false));
    }

    #[test]
    fn planning_puts_tier_one_first_in_mailbox_order() {
        let units = plan_units(&["a".into(), "b".into()]);
        let enc: Vec<String> = units.iter().map(WorkUnit::encode).collect();
        assert_eq!(enc, vec!["S|a|6", "S|a|5", "S|b|6", "S|b|5", "X|a", "X|b"]);
        assert!(plan_units(&[]).is_empty());
    }

    #[test]
    fn units_round_trip_even_with_separators_in_the_name() {
        let u = WorkUnit::Folder { mailbox: "m".into(), entry: "E1".into(), store: "S1".into(), name: "a|b|c".into() };
        assert_eq!(WorkUnit::decode(&u.encode()), Some(u));
        assert_eq!(WorkUnit::decode("X|m"), Some(WorkUnit::Expand { mailbox: "m".into() }));
        assert_eq!(WorkUnit::decode("S|m|x"), None);
        assert_eq!(WorkUnit::decode("Z|m"), None);
        assert_eq!(WorkUnit::decode(""), None);
    }

    #[test]
    fn scope_uses_only_searchable_asked_mailboxes() {
        let mk = |id: &str, access, availability| MailboxInfo {
            id: id.into(),
            name: id.into(),
            kind: MailboxKind::Shared,
            access,
            availability,
            cached: None,
            instant_search: None,
        };
        let known = vec![
            mk("a", MailboxAccess::Ok, MailboxAvailability::Ok),
            mk("b", MailboxAccess::Denied, MailboxAvailability::Ok),
            mk("c", MailboxAccess::Ok, MailboxAvailability::Offline),
            mk("d", MailboxAccess::Ok, MailboxAvailability::Ok),
        ];
        assert_eq!(scope_of(&[], &known), vec!["a", "d"]);
        assert_eq!(scope_of(&["d".into(), "b".into()], &known), vec!["d"]);
    }

    // ---- driver ----------------------------------------------------------------------------------

    struct FakeClock(std::cell::Cell<u64>);
    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0.get()
        }
    }

    fn hit(key: &str, mailbox: &str, minute: u32) -> MailHit {
        MailHit {
            key: key.into(),
            mailbox_id: mailbox.into(),
            subject: "s".into(),
            from: "f".into(),
            received: Some(utc(2026, 10, 8, 10, minute)),
            unread: false,
            folder: "Inbox".into(),
        }
    }

    /// Scripted runner: each unit costs `cost` ms of the fake clock.
    struct Fake<'a> {
        clock: &'a FakeClock,
        cost: u64,
        ran: Vec<String>,
        script: Box<dyn FnMut(&WorkUnit) -> UnitResult + 'a>,
    }
    impl UnitRunner for Fake<'_> {
        fn run(&mut self, unit: &WorkUnit, _remaining_ms: u64) -> UnitResult {
            self.ran.push(unit.encode());
            self.clock.0.set(self.clock.0.get() + self.cost);
            (self.script)(unit)
        }
    }

    fn run_drive(units: VecDeque<WorkUnit>, scope: &[&str], budget: u64, limit: usize, cost: u64, script: impl FnMut(&WorkUnit) -> UnitResult) -> (Progress, Vec<String>, Result<(), &'static str>) {
        let clock = FakeClock(std::cell::Cell::new(0));
        let mut p = Progress::new(scope.iter().map(|s| s.to_string()).collect(), units);
        let mut fake = Fake { clock: &clock, cost, ran: Vec::new(), script: Box::new(script) };
        let r = drive(&mut p, &mut fake, &clock, budget, limit, &mut |_| {});
        let ran = fake.ran.clone();
        (p, ran, r)
    }

    #[test]
    fn driver_runs_everything_and_expands_in_front() {
        let units = plan_units(&["a".into()]);
        let (p, ran, r) = run_drive(units, &["a"], 10_000, 100, 10, |u| match u {
            WorkUnit::Expand { mailbox } => UnitResult {
                follow: vec![
                    WorkUnit::Folder { mailbox: mailbox.clone(), entry: "e1".into(), store: "s".into(), name: "P".into() },
                    WorkUnit::Folder { mailbox: mailbox.clone(), entry: "e2".into(), store: "s".into(), name: "Q".into() },
                ],
                ..Default::default()
            },
            WorkUnit::Special { folder: 6, .. } => UnitResult { hits: vec![hit("k1", "a", 1)], ..Default::default() },
            _ => UnitResult::default(),
        });
        assert_eq!(r, Ok(()));
        assert_eq!(ran, vec!["S|a|6", "S|a|5", "X|a", "F|a|e1|s|P", "F|a|e2|s|Q"]);
        assert!(p.pending.is_empty());
        let o = p.outcomes();
        assert!(o[0].complete && o[0].error.is_none());
        assert_eq!(p.hits.len(), 1);
    }

    #[test]
    fn driver_stops_on_budget_and_leaves_a_cursor() {
        let units = plan_units(&["a".into(), "b".into()]);
        let (p, ran, _) = run_drive(units, &["a", "b"], 25, 100, 10, |_| UnitResult::default());
        // 10 ms per unit, 25 ms budget: units start at 0, 10, 20; the one at 30 does not.
        assert_eq!(ran.len(), 3);
        assert_eq!(p.pending.len(), 3);
        let o = p.outcomes();
        assert!(!o[0].complete && !o[1].complete);
    }

    #[test]
    fn driver_continuation_picks_up_where_it_stopped() {
        let units = plan_units(&["a".into()]);
        let (p1, ran1, _) = run_drive(units, &["a"], 15, 100, 10, |_| UnitResult::default());
        assert_eq!(ran1, vec!["S|a|6", "S|a|5"]);
        let encoded: Vec<String> = p1.pending.iter().map(WorkUnit::encode).collect();
        let resumed: VecDeque<WorkUnit> = encoded.iter().filter_map(|s| WorkUnit::decode(s)).collect();
        let (p2, ran2, _) = run_drive(resumed, &["a"], 15, 100, 10, |_| UnitResult::default());
        assert_eq!(ran2, vec!["X|a"]);
        assert!(p2.pending.is_empty());
    }

    #[test]
    fn driver_with_zero_budget_runs_nothing() {
        let (p, ran, r) = run_drive(plan_units(&["a".into()]), &["a"], 0, 10, 1, |_| UnitResult::default());
        assert!(ran.is_empty() && r.is_ok());
        assert_eq!(p.pending.len(), 3);
    }

    #[test]
    fn driver_stops_after_a_finished_tier_that_has_enough() {
        let units = plan_units(&["a".into()]);
        let (p, ran, _) = run_drive(units, &["a"], 10_000, 2, 1, |u| match u {
            WorkUnit::Special { .. } => UnitResult { hits: vec![hit(&u.encode(), "a", 1), hit(&format!("{}x", u.encode()), "a", 2)], ..Default::default() },
            _ => UnitResult::default(),
        });
        // Both tier-1 units run (the tier is not finished after the first), tier 2 does not.
        assert_eq!(ran, vec!["S|a|6", "S|a|5"]);
        assert_eq!(p.pending.len(), 1);
    }

    #[test]
    fn driver_mailbox_error_drops_only_that_mailbox() {
        let units = plan_units(&["a".into(), "b".into()]);
        let (p, ran, r) = run_drive(units, &["a", "b"], 10_000, 100, 1, |u| {
            if u.mailbox() == "a" {
                UnitResult { mailbox_error: Some("MAIL-101"), ..Default::default() }
            } else {
                UnitResult::default()
            }
        });
        assert_eq!(r, Ok(()));
        assert_eq!(ran, vec!["S|a|6", "S|b|6", "S|b|5", "X|b"]);
        let o = p.outcomes();
        assert_eq!(o[0], MailboxOutcome { mailbox_id: "a".into(), complete: false, error: Some("MAIL-101".into()) });
        assert!(o[1].complete);
    }

    #[test]
    fn driver_fatal_aborts_and_keeps_the_unit() {
        let (p, ran, r) = run_drive(plan_units(&["a".into()]), &["a"], 10_000, 10, 1, |u| {
            if matches!(u, WorkUnit::Special { folder: 5, .. }) {
                UnitResult { fatal: Some("OUTLOOK-102"), ..Default::default() }
            } else {
                UnitResult { hits: vec![hit("k", "a", 1)], ..Default::default() }
            }
        });
        assert_eq!(r, Err("OUTLOOK-102"));
        assert_eq!(ran.len(), 2);
        assert_eq!(p.hits.len(), 1);
        assert_eq!(p.pending.front().map(WorkUnit::encode).as_deref(), Some("S|a|5"));
    }

    #[test]
    fn merge_sorts_dedups_and_limits() {
        let mut older = hit("k1", "a", 5);
        older.received = Some(utc(2026, 10, 1, 9, 0));
        let mut undated = hit("k9", "a", 0);
        undated.received = None;
        let merged = merge_hits(vec![older.clone(), hit("k2", "b", 30), hit("k3", "b", 10), hit("k2", "b", 30), undated], 3);
        let keys: Vec<&str> = merged.iter().map(|h| h.key.as_str()).collect();
        assert_eq!(keys, vec!["k2", "k3", "k1"]);
        assert_eq!(merge_hits(vec![], 5), vec![]);
    }

    #[test]
    fn result_has_a_cursor_only_when_partial() {
        let mut p = Progress::new(vec!["a".into()], VecDeque::new());
        assert!(result_from(&p, 5).cursor.is_none());
        p.pending.push_back(WorkUnit::Expand { mailbox: "a".into() });
        let r = result_from(&p, 5);
        assert!(r.partial);
        assert_eq!(r.cursor.unwrap().pending, vec!["X|a".to_string()]);
    }

    // ---- state ---------------------------------------------------------------------------------

    #[test]
    fn key_map_is_bounded_and_evicts_the_oldest() {
        let mut k = KeyMap::new(3);
        for i in 0..5 {
            k.insert(format!("k{i}"), format!("e{i}"), "s".into());
        }
        assert_eq!(k.len(), 3);
        assert!(k.get("k0").is_none() && k.get("k1").is_none());
        assert_eq!(k.get("k4"), Some(("e4".into(), "s".into())));
        k.insert("k4".into(), "e4b".into(), "s".into());
        assert_eq!(k.len(), 3);
        assert_eq!(k.get("k4").unwrap().0, "e4b");
        k.clear();
        assert_eq!(k.len(), 0);
    }

    #[test]
    fn quarantine_expires_after_fifteen_minutes() {
        let mut qn = Quarantine::default();
        let now = Instant::now();
        assert!(!qn.is_quarantined("m", now));
        qn.add("m", now);
        assert!(qn.is_quarantined("m", now + Duration::from_secs(899)));
        assert!(!qn.is_quarantined("m", now + Duration::from_secs(901)));
        assert!(!qn.is_quarantined("other", now));
    }

    #[test]
    fn profile_change_clears_caches() {
        let mut s = State::new();
        s.note_profile("p1");
        s.store_ids.insert("m".into(), "S".into());
        s.keys.insert("k".into(), "e".into(), "s".into());
        s.mailboxes = Some((Instant::now(), Vec::new()));
        s.note_profile("p1");
        assert!(s.mailboxes.is_some() && s.keys.len() == 1);
        s.note_profile("p2");
        assert!(s.mailboxes.is_none() && s.keys.len() == 0 && s.store_ids.is_empty());
    }

    #[test]
    fn busy_guard_allows_one_at_a_time() {
        // The static is shared with other tests, so only the relation is asserted.
        let first = BusyGuard::acquire();
        if let Ok(g) = first {
            assert_eq!(BusyGuard::acquire().err().as_deref().map(|e| &e[..11]), Some("OUTLOOK-105"));
            drop(g);
            assert!(BusyGuard::acquire().is_ok());
        }
    }

    #[test]
    fn budget_and_wait_helpers() {
        assert_eq!(clamp_budget(0), 10_000);
        assert_eq!(clamp_budget(10), 1_000);
        assert_eq!(clamp_budget(20_000), 20_000);
        assert_eq!(clamp_budget(10_000_000), 60_000);
        assert_eq!(wait_secs(10_000), 15);
        assert_eq!(wait_secs(1_500), 7);
    }

    #[test]
    fn mail_classes() {
        assert!(is_mail_class("IPM.Note"));
        assert!(is_mail_class("IPM.Note.SMIME"));
        assert!(is_mail_class("IPM.Schedule.Meeting.Request"));
        assert!(!is_mail_class("IPM.Appointment"));
        assert!(!is_mail_class("IPM.Contact"));
        assert!(!is_mail_class(""));
    }

    // ---- free/busy -----------------------------------------------------------------------------

    #[test]
    fn free_busy_merges_and_clips() {
        let start = utc(2026, 10, 8, 0, 0);
        // 00:00-00:30 free, 00:30-01:30 busy (2 slots), 01:30-02:00 tentative, 02:00-02:30 busy.
        let s = "0221" .to_string() + "2";
        let blocks = parse_free_busy(&s, start, 30, utc(2026, 10, 8, 0, 0), utc(2026, 10, 8, 3, 0));
        assert_eq!(
            blocks,
            vec![
                FreeBusyBlock { start: utc(2026, 10, 8, 0, 30), end: utc(2026, 10, 8, 1, 30), status: FreeBusyStatus::Busy },
                FreeBusyBlock { start: utc(2026, 10, 8, 1, 30), end: utc(2026, 10, 8, 2, 0), status: FreeBusyStatus::Tentative },
                FreeBusyBlock { start: utc(2026, 10, 8, 2, 0), end: utc(2026, 10, 8, 2, 30), status: FreeBusyStatus::Busy },
            ]
        );
    }

    #[test]
    fn free_busy_clips_to_the_window_and_knows_all_digits() {
        let start = utc(2026, 10, 8, 0, 0);
        let blocks = parse_free_busy("01234", start, 30, utc(2026, 10, 8, 0, 45), utc(2026, 10, 8, 2, 15));
        let statuses: Vec<_> = blocks.iter().map(|b| b.status).collect();
        assert_eq!(statuses, vec![FreeBusyStatus::Tentative, FreeBusyStatus::Busy, FreeBusyStatus::Oof, FreeBusyStatus::WorkingElsewhere]);
        assert_eq!(blocks[0].start, utc(2026, 10, 8, 0, 45));
        assert_eq!(blocks[3].end, utc(2026, 10, 8, 2, 15));
    }

    #[test]
    fn free_busy_free_empty_and_garbage() {
        let start = utc(2026, 10, 8, 0, 0);
        let (f, t) = (utc(2026, 10, 8, 0, 0), utc(2026, 10, 9, 0, 0));
        assert!(parse_free_busy("0000", start, 30, f, t).is_empty());
        assert!(parse_free_busy("", start, 30, f, t).is_empty());
        assert!(parse_free_busy("x?9", start, 30, f, t).is_empty());
        assert!(parse_free_busy("222", start, 0, f, t).is_empty());
        // Entirely before the window.
        assert!(parse_free_busy("22", start, 30, utc(2026, 10, 8, 5, 0), t).is_empty());
        // Different statuses never merge.
        assert_eq!(parse_free_busy("23", start, 30, f, t).len(), 2);
    }

    #[test]
    fn person_name_is_cleaned() {
        assert_eq!(clean_person_name("  איציק\u{0}\n "), "איציק");
        assert_eq!(clean_person_name(&"x".repeat(200)).len(), 80);
    }

    #[test]
    fn table_dates_are_read_as_utc() {
        // 2026-10-08 12:00:00 as a DATE serial.
        let naive = chrono::NaiveDate::from_ymd_opt(2026, 10, 8).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let serial = com::naive_to_date(naive).unwrap();
        assert_eq!(table_date(serial), Some(utc(2026, 10, 8, 12, 0)));
    }

    // ---- live, read-only probes (run with -- --ignored --nocapture) -----------------------------

    #[test]
    #[ignore]
    fn live_discover_mailboxes() {
        let t = Instant::now();
        let list = match discover_mailboxes(true) {
            Ok(l) => l,
            Err(e) => {
                println!("discover failed: {}", &e[..e.len().min(60)]);
                return;
            }
        };
        println!("stores: {} in {} ms", list.len(), t.elapsed().as_millis());
        for m in &list {
            println!("  kind={:?} access={:?} avail={:?} cached={:?} instant={:?}", m.kind, m.access, m.availability, m.cached, m.instant_search);
        }
        let t2 = Instant::now();
        let again = discover_mailboxes(false).unwrap();
        println!("second call (cache): {} stores in {} ms", again.len(), t2.elapsed().as_millis());
        assert_eq!(list.len(), again.len());
    }

    /// Does the Hebrew LIKE really filter, and are Table dates UTC? Compares the newest hit's
    /// Table date with the same item's `ReceivedTime` (local). Prints booleans and minutes only.
    #[test]
    #[ignore]
    fn live_hebrew_filter_and_date_zone() {
        if discover_mailboxes(true).is_err() {
            println!("no Outlook");
            return;
        }
        let query = MailQuery { terms: vec![vec!["תקציב".into(), "דוח".into(), "פגישה".into()]], limit: 10, budget_ms: 10_000, ..Default::default() };
        let r = search_mail(&query, None).unwrap();
        let wanted: Vec<String> = query.terms[0].clone();
        let matching = r.hits.iter().filter(|h| wanted.iter().any(|w| h.subject.contains(w.as_str()))).count();
        println!("hebrew hits={} whose subject contains a term={}", r.hits.len(), matching);
        let r = search_mail(&MailQuery { limit: 3, budget_ms: 10_000, ..Default::default() }, None).unwrap();
        let Some(h) = r.hits.first() else { return };
        let (entry, store) = state().keys.get(&h.key).unwrap();
        let table_utc = h.received.unwrap();
        let item_utc = run_on_outlook("companyisland-mail-probe", 10, move |_| {
            let mut app = Dispatch::get_active("Outlook.Application")?;
            let mut session = app.get_object("Session")?;
            let mut item = session
                .call_object("GetItemFromID", vec![com::variant_from_str(&entry), com::variant_from_str(&store)])?
                .ok_or_else(|| ComError::new("x", com::E_NOOBJECT))?;
            let v = item.get("ReceivedTime")?;
            Ok(com::variant_date(&v).and_then(com::date_to_utc))
        })
        .unwrap()
        .unwrap();
        println!("table date minus item ReceivedTime (as local->utc) = {} min; local offset now = {} min", (table_utc - item_utc).num_minutes(), Local::now().offset().local_minus_utc() / 60);
        assert_eq!((table_utc - item_utc).num_minutes(), 0);
    }

    #[test]
    #[ignore]
    fn live_search_counts() {
        let list = match discover_mailboxes(true) {
            Ok(l) => l,
            Err(e) => {
                println!("discover failed: {}", &e[..e.len().min(60)]);
                return;
            }
        };
        println!("searchable: {}/{}", list.iter().filter(|m| m.searchable()).count(), list.len());
        for (label, query) in [
            ("no filter", MailQuery { limit: 5, budget_ms: 10_000, ..Default::default() }),
            ("hebrew term", MailQuery { terms: vec![vec!["תקציב".into(), "דוח".into()]], limit: 5, budget_ms: 10_000, ..Default::default() }),
            ("latin term", MailQuery { terms: vec![vec!["report".into(), "meeting".into()]], limit: 5, budget_ms: 10_000, ..Default::default() }),
            ("sender hebrew", MailQuery { sender: vec!["יובל".into()], limit: 5, budget_ms: 10_000, ..Default::default() }),
            ("last 7 days", MailQuery { since: Some(Utc::now() - ChronoDuration::days(7)), limit: 50, budget_ms: 10_000, ..Default::default() }),
            ("unread", MailQuery { unread_only: true, limit: 50, budget_ms: 10_000, ..Default::default() }),
        ] {
            let t = Instant::now();
            match search_mail(&query, None) {
                Ok(r) => {
                    let newest = r.hits.first().and_then(|h| h.received);
                    let drift = newest.map(|n| (Utc::now() - n).num_minutes());
                    let folders: HashSet<&str> = r.hits.iter().map(|h| h.folder.as_str()).collect();
                    println!(
                        "{label}: hits={} partial={} complete={}/{} errors={} folders={} newest_age_min={:?} in {} ms",
                        r.hits.len(),
                        r.partial,
                        r.per_mailbox.iter().filter(|m| m.complete).count(),
                        r.per_mailbox.len(),
                        r.per_mailbox.iter().filter(|m| m.error.is_some()).count(),
                        folders.len(),
                        drift,
                        t.elapsed().as_millis()
                    );
                    if let Some(c) = r.cursor {
                        let t = Instant::now();
                        let more = search_mail(&query, Some(c)).unwrap();
                        println!("  continuation: hits={} partial={} in {} ms", more.hits.len(), more.partial, t.elapsed().as_millis());
                    }
                }
                Err(e) => println!("{label}: error {}", &e[..e.len().min(60)]),
            }
        }
    }
}
