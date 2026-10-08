//! Contract types of the offline intent engine (smart search). Shared by `intent` (understanding),
//! `assistant` (execution, answers) and their tests.
//!
//! CONTRACT: other modules depend on every public name here. Fields may be ADDED; nothing may be
//! renamed or removed without updating `assistant/`.

use chrono::{DateTime, Local};
// Types holding chrono values are not Serialize (chrono is built without its serde feature).
use serde::Serialize;

/// A capability id such as `"calendar.list_events"`. A string, not an enum, so a new capability is
/// one registry entry and one lexicon entry, not a change across the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct CapId(pub &'static str);

impl CapId {
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

/// The capabilities the spec asks for (section 5). `assistant` executes each one.
pub mod caps {
    use super::CapId;
    pub const CALENDAR_LIST_EVENTS: CapId = CapId("calendar.list_events");
    pub const CALENDAR_SEARCH_EVENTS: CapId = CapId("calendar.search_events");
    pub const CALENDAR_CHECK_AVAILABILITY: CapId = CapId("calendar.check_availability");
    pub const CALENDAR_RESOLVE_SHARED: CapId = CapId("calendar.resolve_shared_calendar");
    pub const EMAIL_DISCOVER_MAILBOXES: CapId = CapId("email.discover_mailboxes");
    pub const EMAIL_SEARCH: CapId = CapId("email.search");
    pub const EMAIL_OPEN: CapId = CapId("email.open");
    pub const NOTES_SEARCH: CapId = CapId("notes.search");
    pub const NOTES_OPEN: CapId = CapId("notes.open");
    pub const FILES_SEARCH: CapId = CapId("files.search");
    pub const FILES_OPEN: CapId = CapId("files.open");
    pub const APPS_SEARCH: CapId = CapId("apps.search");
    pub const APPS_LAUNCH: CapId = CapId("apps.launch");
    pub const CALCULATOR_EVALUATE: CapId = CapId("calculator.evaluate");
    // ---- explicit commands (intent/commands.rs). Every one is a `Confirm`: it is offered as a click. ----
    pub const WEB_SEARCH: CapId = CapId("web.search");
    pub const WEB_OPEN: CapId = CapId("web.open");
    pub const SYSTEM_OPEN_SETTINGS: CapId = CapId("system.open_settings");
    pub const FOLDERS_OPEN: CapId = CapId("folders.open");
    pub const NOTES_CREATE: CapId = CapId("notes.create");
    pub const MAIL_COMPOSE: CapId = CapId("mail.compose");
    pub const SYSTEM_LOCK: CapId = CapId("system.lock");
    // ---- the assistant itself (intent/commands.rs `talk`). Read-only: they are `Execute`d, never confirmed. ----
    /// "מה אתה יודע לעשות", "help": what can be asked, one example per ability.
    pub const ASSISTANT_HELP: CapId = CapId("assistant.help");
    /// "שלום", "היי", "בוקר טוב", "hello".
    pub const ASSISTANT_HELLO: CapId = CapId("assistant.hello");
    /// "תודה", "thanks".
    pub const ASSISTANT_THANKS: CapId = CapId("assistant.thanks");
    /// "מי אתה", "what are you".
    pub const ASSISTANT_ABOUT: CapId = CapId("assistant.about");

    /// Small talk and help: answered from fixed text, they say nothing about the user's data and are not
    /// remembered as the thread of a conversation ("תודה" between two questions keeps the follow-up alive).
    pub fn is_talk(cap: CapId) -> bool {
        matches!(cap, ASSISTANT_HELP | ASSISTANT_HELLO | ASSISTANT_THANKS | ASSISTANT_ABOUT)
    }

    pub const ALL: [CapId; 25] = [
        CALENDAR_LIST_EVENTS,
        CALENDAR_SEARCH_EVENTS,
        CALENDAR_CHECK_AVAILABILITY,
        CALENDAR_RESOLVE_SHARED,
        EMAIL_DISCOVER_MAILBOXES,
        EMAIL_SEARCH,
        EMAIL_OPEN,
        NOTES_SEARCH,
        NOTES_OPEN,
        FILES_SEARCH,
        FILES_OPEN,
        APPS_SEARCH,
        APPS_LAUNCH,
        CALCULATOR_EVALUATE,
        WEB_SEARCH,
        WEB_OPEN,
        SYSTEM_OPEN_SETTINGS,
        FOLDERS_OPEN,
        NOTES_CREATE,
        MAIL_COMPOSE,
        SYSTEM_LOCK,
        ASSISTANT_HELP,
        ASSISTANT_HELLO,
        ASSISTANT_THANKS,
        ASSISTANT_ABOUT,
    ];
}

/// What running a capability does to the user's machine. Only `Read` may run from a parse alone;
/// `Open` and `Launch` need an explicit click (never on a guess).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Sensitivity {
    Read,
    Open,
    Launch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Lang {
    #[default]
    He,
    En,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Grain {
    /// One calendar day (`from` = local midnight, `to` = next local midnight).
    Day,
    /// A Sunday-first week.
    Week,
    /// Any other stretch (several days, part of a day such as "אחרי הצהריים").
    Range,
    /// A moment ("בעוד שעה"): `from` = the moment, `to` = from + 1 hour.
    Instant,
}

/// A resolved time window, local time, `to` exclusive.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeSpec {
    pub from: DateTime<Local>,
    pub to: DateTime<Local>,
    pub grain: Grain,
}

/// Which slot a clarification asks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AskKind {
    /// "באיזו תיבת דואר לחפש?" (decided by `assistant`, which knows the mailboxes).
    Mailbox,
    /// The date is invalid or missing ("31.2").
    Date,
    /// An hour without AM/PM ("ב-3").
    Time,
    /// Several people named, or a name that matches more than one known calendar/person.
    Person,
    /// What should be searched at all ("מה?", "on what?"): no subject words were found.
    Content,
    /// Which kind of request this is (low confidence, or a follow-up without context).
    Intent,
}

/// What the engine decided to do with the text.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Decision {
    /// Run this read-only capability.
    Execute { cap: CapId },
    /// Two or more read-only capabilities are about equally likely: run all, merge, label sources.
    MultiSource { caps: Vec<CapId> },
    /// A required slot is missing or ambiguous. `cap` is the most likely capability, if any.
    Clarify { ask: AskKind, cap: Option<CapId> },
    /// A sensitive capability (open/launch). `assistant` first searches with the matching read
    /// capability and shows the candidate(s) for an explicit click. Never executed from text.
    Confirm { cap: CapId },
    /// Out of scope (weather, delete...) or nothing recognisable.
    NoMatch,
}

/// The values the engine extracted. Text slots keep the user's spelling (prefix-stripped where the
/// prefix is grammar, e.g. "מיובל" -> "יובל"), ready to show and to search with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slots {
    pub time: Option<TimeSpec>,
    /// A person whose calendar is meant ("מה יש לאיציק ביומן", "ומה אצל דנה").
    pub person: Option<String>,
    /// The mail sender ("המייל מיובל", "from Dana").
    pub sender: Option<String>,
    /// Search terms, AND-ed. Each inner list holds alternative spellings of one term, OR-ed
    /// (e.g. ["התקציב", "תקציב"]). Comes from "המילה X", "על X", "בנושא X", quotes, or the
    /// remaining content words.
    pub terms: Vec<Vec<String>>,
    /// A mailbox the user named, as the id of a `Known::mailboxes` entry.
    pub mailbox: Option<String>,
    /// The user explicitly asked for all mailboxes ("בכל התיבות").
    pub all_mailboxes: bool,
    /// "האחרון", "last", "latest": newest first, and the answer talks about the first hit.
    pub latest: bool,
    pub limit: Option<u32>,
    /// "שלא נקראו", "unread".
    pub unread: bool,
    /// File extension implied ("אקסל" -> "xlsx", "pdf"), without the dot, lowercase.
    pub file_ext: Option<String>,
    /// An app name ("אקסל", "Word", "מחשבון").
    pub app: Option<String>,
    /// A normalised arithmetic expression for `calculator.evaluate` (ASCII operators, '.' decimal).
    pub expr: Option<String>,
    /// "אותו", "it", "the first one": refers to the previous turn's results.
    pub refers_back: bool,
    // ---- explicit commands (`intent/commands.rs`); empty for every other capability ----
    /// What to search for, write in a note or translate, as typed (quotes stripped).
    pub query: Option<String>,
    /// Where a web search goes: "google" | "youtube" | "wikipedia" | "maps" | "waze" | "bing" | "translate".
    pub engine: Option<String>,
    /// Translate: the target language code ("en", "iw", ...); `None` = decided by the executor.
    pub lang_to: Option<String>,
    /// Translate: the source language code; `None` = automatic detection.
    pub lang_from: Option<String>,
    /// A built-in website (a key of `local::system_actions::SITES`).
    pub site: Option<String>,
    /// A typed http(s) address, already normalised.
    pub url: Option<String>,
    /// A Windows settings page (a key of `local::system_actions::SETTINGS`).
    pub setting: Option<String>,
    /// A folder (a key of `local::system_actions::FOLDERS`).
    pub folder: Option<String>,
    /// `mail.compose`: the recipient as typed (a name or an address).
    pub mail_to: Option<String>,
    /// `mail.compose`: the subject.
    pub mail_subject: Option<String>,
    // ---- language signals (`intent/entities.rs`); kept by follow-ups ----
    /// "המילה X", a quoted phrase, "בדיוק X": the terms are exact words. Whole-word hits first,
    /// and the search is never widened.
    pub exact_terms: bool,
    /// "התיבה המשותפת" with no mailbox named: only the shared mailboxes.
    pub shared_mailbox: bool,
}

/// One known name the engine can match against (real data, so nothing is invented).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownName {
    pub id: String,
    pub name: String,
}

/// What the machine actually has, for entity matching. Built by `assistant` from caches only
/// (never a blocking Outlook call just to parse).
#[derive(Clone, Debug, Default)]
pub struct Known {
    /// Discovered mailboxes (id = `outlook_mail::MailboxInfo::id`).
    pub mailboxes: Vec<KnownName>,
    /// Calendars in Outlook's Calendar module (id = `CalendarSourceDto::id`).
    pub calendars: Vec<KnownName>,
    /// People seen as organizers / senders recently, for name extraction.
    pub people: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Interpretation {
    pub decision: Decision,
    pub slots: Slots,
    /// 0..1, for logs and the UI's "did you mean" wording. Decisions are made on thresholds.
    pub confidence: f32,
    pub lang: Lang,
    /// The text continued the previous turn ("ומה לגבי יום חמישי?") and inherited its slots.
    pub follow_up: bool,
    /// The scored candidates, best first (for MultiSource and diagnostics).
    pub ranked: Vec<(CapId, f32)>,
}
