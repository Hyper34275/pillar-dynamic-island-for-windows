//! Smart search orchestration: text in (search bar overlay, island, Center chat) -> `intent` ->
//! data sources -> an answer card for the island + full results for the Island Center.
//!
//! CONTRACT (used by `search_bar`, `center_ipc`, the island and the Center):
//! - Tauri commands `assistant_submit`, `assistant_choose`, `assistant_extend`, `assistant_open_item`,
//!   `assistant_open_center`, `assistant_dismiss`.
//! - Tauri event `assistant-update`, payload [`AssistantCard`] (to every webview).
//! - Pipe event `search-ready`, payload `{"queryId": "..."}` (to the Center), and the pub fns
//!   [`submit`], [`choose`], [`extend`], [`open_item`], [`results`], [`history`] that the pipe
//!   commands call.
//! Every id that reaches a page or the Center is opaque (minted here); paths, EntryIDs and AUMIDs
//! stay in Rust. Query text and results are memory only and never logged.
//!
//! Layout: `flow` (conversation state, store use, the steps of one question), `exec` (capabilities
//! against the [`exec::Sources`] trait), `policy` (mailbox plan, typed replies), `answer`
//! (wording), `avail` (free slots), `store` (last 20 queries, 30 minutes), `prefs` (saved mailbox
//! choice). The real sources are [`Live`] below.

mod actions;
mod answer;
mod avail;
mod exec;
mod flow;
mod policy;
mod prefs;
mod store;
pub mod wire;

pub use wire::*;

use crate::calendar::{CalendarSourceDto, RangeRead};
use crate::local::{AppHit, FileSearch, NoteHit};
use crate::outlook_mail::{FreeBusy, MailCursor, MailQuery, MailSearchResult, MailboxInfo};
use crate::{calendar, center, center_ipc, intent, local, outlook, outlook_mail, rt, window};
use chrono::{DateTime, Local, Utc};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};

const MAX_QUESTION_CHARS: usize = 500;

/// Where a question came from (for the reply's destination and logs).
pub fn origin_valid(origin: &str) -> bool {
    matches!(origin, "searchBar" | "island" | "center")
}

fn engine() -> &'static flow::Engine {
    static ENGINE: OnceLock<flow::Engine> = OnceLock::new();
    ENGINE.get_or_init(flow::Engine::new)
}

fn now_ms() -> i64 {
    Local::now().timestamp_millis()
}

fn new_query_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    flow::mint_query_id(COUNTER.fetch_add(1, Ordering::Relaxed), nanos)
}

/// Set when smart search was switched off: the stored queries are gone and stay unreadable until
/// the next accepted question (`enabled` clears it).
static OFF: AtomicBool = AtomicBool::new(false);

const OFF_ERROR: &str = "APP-040: smart search is off";

/// Smart search was switched off in the settings: forget every stored query and the conversation
/// context, and refuse to show anything until a question is accepted again.
pub fn on_disabled() {
    OFF.store(true, Ordering::SeqCst);
    engine().clear();
}

fn enabled(app: &AppHandle) -> Result<(), String> {
    if app.state::<crate::settings::SettingsStore>().get().ai_search_enabled {
        OFF.store(false, Ordering::SeqCst);
        Ok(())
    } else {
        Err(OFF_ERROR.into())
    }
}

/// The ids Center pages accept (same rule as note ids).
fn valid_query_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn emit_card(app: &AppHandle, card: &AssistantCard) {
    // A dismissed query is still stored, just not shown anymore.
    if engine().is_dismissed(&card.query_id) {
        return;
    }
    if let Err(e) = app.emit("assistant-update", card) {
        crate::dlog!("WARN", "assistant", "emit assistant-update failed: {}", e);
    }
}

/// Emit the final card and tell the Center a result is ready.
fn publish(app: &AppHandle, card: AssistantCard) -> AssistantCard {
    emit_card(app, &card);
    center_ipc::broadcast("search-ready", &serde_json::json!({ "queryId": card.query_id }));
    card
}

/// Run `work` on a blocking thread; a failure still replaces the processing card with an error.
async fn run_query<F>(app: &AppHandle, name: &'static str, query_id: &str, query: &str, lang: intent::Lang, work: F) -> Result<AssistantCard, String>
where
    F: FnOnce(&Live) -> Result<AssistantCard, String> + Send + 'static,
{
    let live = Live { app: app.clone() };
    match rt::run_blocking(name, move || work(&live)).await {
        Ok(_) if OFF.load(Ordering::SeqCst) => {
            // Switched off while this search ran: its result must not outlive the switch.
            engine().clear();
            emit_card(app, &flow::error_card(query_id, query, lang, "APP-040", now_ms()));
            Err(OFF_ERROR.into())
        }
        Ok(card) => Ok(publish(app, card)),
        Err(e) => {
            let code = answer::code_of(&e, "APP-001");
            let card = flow::error_card(query_id, query, lang, &code, now_ms());
            emit_card(app, &card);
            Err(e)
        }
    }
}

/// Run a question. Emits `assistant-update` (processing, then the answer) and `search-ready`, and
/// returns the final card. `origin` is `"searchBar"`, `"island"` or `"center"`.
pub async fn submit(app: AppHandle, text: String, origin: String) -> Result<AssistantCard, String> {
    if !origin_valid(&origin) {
        return Err("APP-042: invalid question".into());
    }
    let text = text.trim().to_string();
    if text.is_empty() || text.chars().count() > MAX_QUESTION_CHARS {
        return Err("APP-042: invalid question".into());
    }
    enabled(&app)?;
    let eng = engine();
    let route = eng.route(&text, now_ms());
    let (query_id, query, lang, option) = match route {
        flow::Route::Choose { query_id, option_id } => {
            let (query, lang) = eng.resume(&query_id, now_ms()).unwrap_or_else(|| (String::new(), intent::detect_lang(&text)));
            (query_id, query, lang, Some(option_id))
        }
        flow::Route::New => (new_query_id(), text.clone(), intent::detect_lang(&text), None),
    };
    // At once, before any Outlook call: the island wakes and shows that the question was heard.
    emit_card(&app, &flow::processing_card(&query_id, &query, lang, now_ms()));
    window::show(&app);
    let id = query_id.clone();
    run_query(&app, "assistant_submit", &query_id, &query, lang, move |live| match option {
        Some(option_id) => eng.choose(live, &id, &option_id, false, Local::now()),
        None => Ok(eng.submit(live, &text, &id, Local::now())),
    })
    .await
}

/// Answer a clarification ("באיזו תיבת דואר לחפש?") by option id. `remember` saves a mailbox choice
/// as the preference (never applied to mailboxes discovered later).
pub async fn choose(app: AppHandle, query_id: String, option_id: String, remember: bool) -> Result<AssistantCard, String> {
    enabled(&app)?;
    let eng = engine();
    let (query, lang) = eng.resume(&query_id, now_ms()).ok_or("APP-041: search expired")?;
    emit_card(&app, &flow::processing_card(&query_id, &query, lang, now_ms()));
    let id = query_id.clone();
    run_query(&app, "assistant_choose", &query_id, &query, lang, move |live| eng.choose(live, &id, &option_id, remember, Local::now())).await
}

/// Continue a partial (time-budgeted) search for another budget (10 s for mail).
pub async fn extend(app: AppHandle, query_id: String) -> Result<AssistantCard, String> {
    enabled(&app)?;
    let eng = engine();
    let (query, lang) = eng.resume(&query_id, now_ms()).ok_or("APP-041: search expired")?;
    emit_card(&app, &flow::processing_card(&query_id, &query, lang, now_ms()));
    let id = query_id.clone();
    run_query(&app, "assistant_extend", &query_id, &query, lang, move |live| eng.extend(live, &id, Local::now())).await
}

/// Open one result (mail, event, note, file, app) after an explicit click.
pub async fn open_item(app: AppHandle, query_id: String, item_id: String) -> Result<(), String> {
    enabled(&app)?;
    let live = Live { app };
    rt::run_blocking("assistant_open_item", move || engine().open(&live, &query_id, &item_id, now_ms())).await
}

/// The full results of a query, for the Center. Nothing is shown while smart search is off.
pub fn results(query_id: &str) -> Result<SearchResults, String> {
    if OFF.load(Ordering::SeqCst) {
        return Err(OFF_ERROR.into());
    }
    engine().results(query_id, now_ms())
}

/// The conversation so far (newest last), for the Center's chat page. Memory only.
pub fn history() -> Vec<SearchResults> {
    if OFF.load(Ordering::SeqCst) {
        return Vec::new();
    }
    engine().history(now_ms())
}

#[tauri::command]
pub async fn assistant_submit(app: AppHandle, text: String, origin: String) -> Result<AssistantCard, String> {
    submit(app, text, origin).await
}

#[tauri::command]
pub async fn assistant_choose(app: AppHandle, query_id: String, option_id: String, remember: bool) -> Result<AssistantCard, String> {
    choose(app, query_id, option_id, remember).await
}

#[tauri::command]
pub async fn assistant_extend(app: AppHandle, query_id: String) -> Result<AssistantCard, String> {
    extend(app, query_id).await
}

#[tauri::command]
pub async fn assistant_open_item(app: AppHandle, query_id: String, item_id: String) -> Result<(), String> {
    open_item(app, query_id, item_id).await
}

/// "הצג את כל התוצאות": the Island Center on its smart search page for this query.
#[tauri::command]
pub async fn assistant_open_center(app: AppHandle, query_id: String) -> Result<(), String> {
    if !valid_query_id(&query_id) {
        return Err("APP-042: invalid question".into());
    }
    rt::run_blocking("assistant_open_center", move || center::open(&app, &format!("search:{query_id}"))).await
}

/// The user closed the card (Esc, swipe) or the search bar: a running search is not shown anymore.
#[tauri::command]
pub fn assistant_dismiss(query_id: String) {
    engine().dismiss(&query_id);
}

// =============================================================================
// The real data sources
// =============================================================================

/// At most this many organizer names go to the intent parser.
const MAX_PEOPLE: usize = 200;

fn cap_people(mut people: Vec<String>) -> Vec<String> {
    people.truncate(MAX_PEOPLE);
    people
}

/// [`exec::Sources`] over the running app: Outlook, the calendar worker, local search.
struct Live {
    app: AppHandle,
}

impl exec::Sources for Live {
    fn cached_mailboxes(&self) -> Vec<MailboxInfo> {
        outlook_mail::cached_mailboxes()
    }
    fn discover_mailboxes(&self, force: bool) -> Result<Vec<MailboxInfo>, String> {
        outlook_mail::discover_mailboxes(force)
    }
    fn search_mail(&self, query: &MailQuery, cursor: Option<MailCursor>) -> Result<MailSearchResult, String> {
        outlook_mail::search_mail(query, cursor)
    }
    fn open_mail(&self, key: &str) -> Result<(), String> {
        outlook_mail::open_mail(key)
    }
    fn free_busy(&self, name: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<FreeBusy, String> {
        outlook_mail::free_busy(name, from, to)
    }
    fn calendars(&self) -> Vec<CalendarSourceDto> {
        calendar::known_sources(&self.app)
    }
    fn people(&self) -> Vec<String> {
        cap_people(calendar::organizers_recent(&self.app))
    }
    fn query_range(&self, from: DateTime<Utc>, to: DateTime<Utc>, only: Option<Vec<String>>) -> Result<RangeRead, String> {
        calendar::query_range(from, to, only)
    }
    fn prefetched(&self, from: DateTime<Utc>, to: DateTime<Utc>, only: Option<&[String]>) -> Option<RangeRead> {
        calendar::prefetched(from, to, only)
    }
    fn open_event(&self, start: DateTime<Utc>) -> Result<(), String> {
        outlook::open_calendar(Some(start))
    }
    fn search_files(&self, terms: &[Vec<String>], ext: Option<&str>, limit: usize, budget_ms: u64) -> Result<FileSearch, String> {
        local::search_files(terms, ext, limit, budget_ms)
    }
    fn open_file(&self, key: &str) -> Result<(), String> {
        local::open_file(key)
    }
    fn search_apps(&self, names: &[String], limit: usize) -> Result<Vec<AppHit>, String> {
        local::search_apps(names, limit)
    }
    fn launch_app(&self, key: &str) -> Result<(), String> {
        local::launch_app(key)
    }
    fn search_notes(&self, terms: &[Vec<String>], latest: bool, limit: usize) -> Result<Vec<NoteHit>, String> {
        local::search_notes(terms, latest, limit)
    }
    fn open_note(&self, id: &str) -> Result<(), String> {
        center::open(&self.app, &format!("note:{id}"))
    }
    fn load_prefs(&self) -> Option<prefs::Prefs> {
        prefs::load()
    }
    fn save_prefs(&self, p: &prefs::Prefs) -> Result<(), String> {
        prefs::save(p)
    }
    fn run_action(&self, action: &actions::Action) -> Result<(), String> {
        actions::perform(&self.app, action)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        assert!(origin_valid("searchBar"));
        assert!(origin_valid("island"));
        assert!(origin_valid("center"));
        assert!(!origin_valid("web"));
        assert!(!origin_valid(""));
    }

    #[test]
    fn query_id_rule_matches_center_pages() {
        assert!(valid_query_id("0123456789abcdef"));
        assert!(valid_query_id("a_b-C"));
        assert!(!valid_query_id(""));
        assert!(!valid_query_id("a b"));
        assert!(!valid_query_id("../x"));
        assert!(!valid_query_id(&"a".repeat(65)));
    }

    #[test]
    fn minted_ids_are_16_hex_and_differ() {
        let a = flow::mint_query_id(1, 1_000);
        let b = flow::mint_query_id(2, 1_000);
        assert_eq!(a.len(), 16);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(valid_query_id(&a));
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn people_are_capped() {
        let many: Vec<String> = (0..500).map(|i| format!("p{i}")).collect();
        assert_eq!(cap_people(many).len(), MAX_PEOPLE);
        assert_eq!(cap_people(vec!["Dana".into()]), vec!["Dana".to_string()]);
    }

    #[test]
    fn nothing_is_readable_after_switching_off() {
        // the only test that touches the process-wide engine and the off flag
        let ms = now_ms();
        on_disabled();
        assert!(history().is_empty());
        assert_eq!(results("anything").unwrap_err().split(':').next(), Some("APP-040"));
        assert!(engine().history(ms).is_empty());
        OFF.store(false, Ordering::SeqCst);
        // back on: the lookup reaches the (empty) store and reports an expired search
        assert_eq!(results("anything").unwrap_err().split(':').next(), Some("APP-041"));
    }
}
