//! Wire shapes of smart search (camelCase JSON). Mirrored in `src/lib/ipc.ts` (island, search bar)
//! and `center/CompanyIsland.Center.Core/Models.cs` (Center). Fields may be added, never renamed.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CardPhase {
    /// The question is being worked on (shown only while it really is: no artificial delay).
    Processing,
    /// The answer (possibly partial: see `partial`).
    Answer,
    /// A clarification with `choices`.
    Choices,
    /// Nothing could be done; `error_code` says why.
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Event,
    Mail,
    Note,
    File,
    App,
    Calc,
    /// A free slot / busy summary line of an availability answer.
    Slot,
    /// Information only (e.g. a colleague's busy blocks without titles).
    Info,
    /// A confirmed-by-click command: search the web, open a site / setting / folder, save a note, open a
    /// new mail window, lock the PC (see `assistant::actions`). The Center reads `kind` as a plain string,
    /// so an older Center shows it as information with its "פתח" button.
    Action,
}

/// One result line. `id` is opaque and only meaningful together with the query id.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantItem {
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    pub subtitle: Option<String>,
    /// Unix ms: event start, mail received, note/file modified.
    pub time: Option<i64>,
    /// Unix ms: event end.
    pub end_time: Option<i64>,
    /// `#RRGGBB` (calendar colour, category...).
    pub accent: Option<String>,
    /// Clicking opens it (mail, event, note, file reveal, app launch).
    pub openable: bool,
    /// Mail: unread. Others: false.
    pub unread: bool,
    /// Mail: the mailbox display name. Event: the calendar name. File: the folder ("Documents").
    pub source: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChoiceKind {
    /// One discovered mailbox.
    Mailbox,
    /// "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן."
    AllMailboxes,
    /// Any other option (a person, a date, a capability).
    Option,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub kind: ChoiceKind,
    /// The user's saved preference: shown first / highlighted, never chosen automatically when a
    /// mailbox appeared since it was saved.
    pub preferred: bool,
}

/// The island's card for one query (also the Tauri `assistant-update` payload).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantCard {
    pub query_id: String,
    /// The question as typed (memory only).
    pub query: String,
    pub phase: CardPhase,
    pub lang: crate::intent::Lang,
    /// Headline: "מחר יש לאיציק 3 פגישות", "נמצאו 12 מיילים".
    pub title: String,
    /// One or two short lines under the headline (times, the first hit...), may be empty.
    pub summary: String,
    /// The question of a clarification.
    pub question: Option<String>,
    pub choices: Vec<Choice>,
    /// The first few results (the island shows at most 3).
    pub items: Vec<AssistantItem>,
    /// All results found (the Center shows them all).
    pub total: u32,
    /// The search stopped at its time budget; [`super::extend`] can continue it.
    pub partial: bool,
    pub can_extend: bool,
    /// e.g. "OUTLOOK-101" for phase Error, or for a source that failed while others answered.
    pub error_code: Option<String>,
    /// Which sources answered: "calendar", "mail", "notes", "files", "apps", "calc".
    pub sources: Vec<String>,
    /// Unix ms when the query was received.
    pub created_at: i64,
    /// The query continued the previous one (a follow-up / refinement).
    pub follow_up: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultGroup {
    /// "calendar" | "mail" | "notes" | "files" | "apps" | "calc" | "availability"
    pub kind: String,
    pub title: String,
    /// Mail groups: one per mailbox.
    pub mailbox: Option<MailboxRef>,
    pub items: Vec<AssistantItem>,
    pub truncated: bool,
    /// This source failed (the others may still have answered).
    pub error_code: Option<String>,
}

/// Everything about one query, for the Center (pipe command `searchResults`, chat history).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub card: AssistantCard,
    pub groups: Vec<ResultGroup>,
}
