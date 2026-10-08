//! Mailboxes and mail search in the user's own running classic Outlook (smart search).
//!
//! Attach-only, like the calendar: one fresh STA thread per request (`outlook::run_on_outlook`),
//! a hard time budget, nothing cached that could keep OUTLOOK.EXE alive. Mail is read through
//! `Folder.GetTable` columns (subject, sender name, received, unread, EntryID): never Body,
//! SenderEmailAddress or Recipients. EntryIDs/StoreIDs stay in this module; callers get opaque keys.
//! Subjects, names and search terms are never logged.
//!
//! CONTRACT (used by `assistant`): every pub item below.

use chrono::{DateTime, Utc};

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

/// The mailboxes of the running profile, public folders skipped. Cached for 10 minutes; `force`
/// re-reads. Blocking (call through `rt::run_blocking`).
pub fn discover_mailboxes(force: bool) -> Result<Vec<MailboxInfo>, String> {
    let _ = force;
    Err("OUTLOOK-101: classic Outlook is not running".into())
}

/// The last discovery result without touching Outlook (for parsing: `intent::Known`).
pub fn cached_mailboxes() -> Vec<MailboxInfo> {
    Vec::new()
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

/// Search mail, tiered: Inbox and Sent Items of every mailbox first, then their subfolders, until
/// the budget is spent. Blocking. `cursor` = continue a previous partial search of the same query.
pub fn search_mail(query: &MailQuery, cursor: Option<MailCursor>) -> Result<MailSearchResult, String> {
    let _ = (query, cursor);
    Err("OUTLOOK-101: classic Outlook is not running".into())
}

/// Show one found mail in Outlook (explicit click only). Blocking.
pub fn open_mail(key: &str) -> Result<(), String> {
    let _ = key;
    Err("MAIL-104: mail no longer available".into())
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

/// A colleague's free/busy (no titles, no calendar permission needed) for `[from, to)`. Used when
/// their calendar is not open in the user's Outlook. Blocking, network (address book).
pub fn free_busy(name: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<FreeBusy, String> {
    let _ = (name, from, to);
    Err("OUTLOOK-101: classic Outlook is not running".into())
}
