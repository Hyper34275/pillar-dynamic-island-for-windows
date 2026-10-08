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

pub mod wire;

pub use wire::*;

use tauri::AppHandle;

/// Where a question came from (for the reply's destination and logs).
pub fn origin_valid(origin: &str) -> bool {
    matches!(origin, "searchBar" | "island" | "center")
}

/// Run a question. Emits `assistant-update` (processing, then the answer) and `search-ready`, and
/// returns the final card. `origin` is `"searchBar"`, `"island"` or `"center"`.
pub async fn submit(app: AppHandle, text: String, origin: String) -> Result<AssistantCard, String> {
    let _ = (app, text, origin);
    Err("APP-040: smart search not available".into())
}

/// Answer a clarification ("באיזו תיבת דואר לחפש?") by option id. `remember` saves a mailbox choice
/// as the preference (never applied to mailboxes discovered later).
pub async fn choose(app: AppHandle, query_id: String, option_id: String, remember: bool) -> Result<AssistantCard, String> {
    let _ = (app, query_id, option_id, remember);
    Err("APP-041: search expired".into())
}

/// Continue a partial (time-budgeted) search for another budget (10 s for mail).
pub async fn extend(app: AppHandle, query_id: String) -> Result<AssistantCard, String> {
    let _ = (app, query_id);
    Err("APP-041: search expired".into())
}

/// Open one result (mail, event, note, file, app) after an explicit click.
pub async fn open_item(app: AppHandle, query_id: String, item_id: String) -> Result<(), String> {
    let _ = (app, query_id, item_id);
    Err("APP-041: search expired".into())
}

/// The full results of a query, for the Center.
pub fn results(query_id: &str) -> Result<SearchResults, String> {
    let _ = query_id;
    Err("APP-041: search expired".into())
}

/// The conversation so far (newest last), for the Center's chat page. Memory only.
pub fn history() -> Vec<SearchResults> {
    Vec::new()
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
    let _ = (app, query_id);
    Err("APP-041: search expired".into())
}

/// The user closed the card (Esc, swipe) or the search bar: a running search is not shown anymore.
#[tauri::command]
pub fn assistant_dismiss(query_id: String) {
    let _ = query_id;
}
