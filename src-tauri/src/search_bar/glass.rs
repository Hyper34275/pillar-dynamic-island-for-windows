//! State that belongs to the centre "Spotlight glass" bar and nothing else: the backdrop picture the
//! page asked for, the height of the sheet (for the click-through region), and which questions were
//! asked from the open glass (their answers live in the sheet, not in the island).
//!
//! Everything here is memory only. The picture is dropped when the bar closes; the question ids (a
//! bounded list, ids only, no text) are tied to the session that asked them and only decide where an
//! answer goes. Nothing is logged or written to disk.

use super::layout::SPOT_FIELD_DIP;
use super::snapshot::GlassBackdrop;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

/// Event carrying a `GlassBackdrop` to the search page, sent just before the window shows.
pub const BACKDROP_EVENT: &str = "search-bar-backdrop";

/// Questions remembered as asked from the glass. A session asks a handful; this only bounds the list.
const MAX_QUERIES: usize = 16;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// =============================================================================
// Where an answer card goes
// =============================================================================

/// Where one `assistant-update` card is delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardRoute {
    /// Every webview, the island among them (today's behaviour): a question from the island, the
    /// Center, the taskbar-anchored or floating bar, or one the glass has already forgotten.
    Everyone,
    /// Only the search page: the question came from the open glass bar, so the answer is shown in its
    /// sheet, next to the question, and the island stays out of it.
    GlassOnly,
    /// The question came from the glass but the sheet was closed before the answer arrived: the
    /// island shows it as it always did (and wakes up to do so).
    IslandAfterGlass,
}

/// The island suppression rule. `asked_here`: the question's id was noted when it was asked from the
/// glass bar. `glass_open`: the glass bar is open right now.
pub fn route_for(asked_here: bool, glass_open: bool) -> CardRoute {
    match (asked_here, glass_open) {
        (false, _) => CardRoute::Everyone,
        (true, true) => CardRoute::GlassOnly,
        (true, false) => CardRoute::IslandAfterGlass,
    }
}

/// The questions asked from a glass bar, each with the session (one open of the bar) that asked it.
static QUERIES: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());
static SESSION: Mutex<u64> = Mutex::new(0);

/// A question was just asked from the open glass bar (or answered there).
pub fn note_query(query_id: &str) {
    let session = *lock(&SESSION);
    let mut ids = lock(&QUERIES);
    ids.retain(|(id, _)| id != query_id);
    ids.push((query_id.to_string(), session));
    let extra = ids.len().saturating_sub(MAX_QUERIES);
    ids.drain(..extra);
}

/// `(asked from some glass bar, asked from the bar that is open now)` for a question.
pub fn query_standing(query_id: &str) -> (bool, bool) {
    let session = *lock(&SESSION);
    match lock(&QUERIES).iter().find(|(id, _)| id == query_id) {
        Some((_, asked_in)) => (true, *asked_in == session),
        None => (false, false),
    }
}

/// Where the card of one question goes. `glass_open`: a glass bar is open right now. A question
/// asked from an earlier bar than the open one counts as asked with no bar open.
pub fn route_of(query_id: &str, glass_open: bool) -> CardRoute {
    let (asked_from_glass, asked_here) = query_standing(query_id);
    route_for(asked_from_glass, glass_open && asked_here)
}

/// A new glass session: the sheet of the last one is gone, so an answer that is still on its way
/// from it is the island's (the ids stay noted: forgetting them would leave that answer unshown).
pub fn new_session() {
    let mut session = lock(&SESSION);
    *session = session.wrapping_add(1);
}

// =============================================================================
// The backdrop picture
// =============================================================================

static BACKDROP: Mutex<Option<GlassBackdrop>> = Mutex::new(None);

/// Keep the picture for the page's pull (`search_bar_backdrop`) and push it as an event. The page
/// that is not loaded yet (the very first open) pulls; the page that is loaded but hidden receives
/// the event before the window shows.
pub fn publish_backdrop(app: &AppHandle, backdrop: &GlassBackdrop) {
    *lock(&BACKDROP) = Some(backdrop.clone());
    if let Err(e) = app.emit_to(super::WINDOW_LABEL, BACKDROP_EVENT, backdrop) {
        dlog!("WARN", "search_bar", "WIN-507 emit search-bar-backdrop failed: {}", e);
    }
}

pub fn current_backdrop() -> Option<GlassBackdrop> {
    lock(&BACKDROP).clone()
}

/// The bar closed: the picture of the user's screen goes with it.
pub fn clear_backdrop() {
    *lock(&BACKDROP) = None;
}

// =============================================================================
// The sheet's height (the page reports it; the window region follows)
// =============================================================================

static SHEET_DIP: Mutex<f64> = Mutex::new(SPOT_FIELD_DIP);

pub fn sheet_dip() -> f64 {
    *lock(&SHEET_DIP)
}

/// Remember the sheet's height as reported by the page. A bad number counts as the field alone.
pub fn store_sheet(dip: f64) {
    *lock(&SHEET_DIP) = if dip.is_finite() { dip } else { SPOT_FIELD_DIP };
}

/// Back to the field alone (a fresh open).
pub fn reset_sheet() {
    *lock(&SHEET_DIP) = SPOT_FIELD_DIP;
}

/// The window geometry the region needs: the window's size in physical px and its scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct GlassGeometry {
    pub width_px: i32,
    pub height_px: i32,
    pub scale: f64,
}

static GEOMETRY: Mutex<Option<GlassGeometry>> = Mutex::new(None);

pub fn set_geometry(g: GlassGeometry) {
    *lock(&GEOMETRY) = Some(g);
}

pub fn geometry() -> Option<GlassGeometry> {
    *lock(&GEOMETRY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_island_stays_out_of_a_question_asked_from_the_open_glass() {
        assert_eq!(route_for(true, true), CardRoute::GlassOnly);
    }

    #[test]
    fn closing_the_sheet_before_the_answer_hands_it_to_the_island() {
        assert_eq!(route_for(true, false), CardRoute::IslandAfterGlass);
    }

    #[test]
    fn every_other_question_keeps_todays_island_card() {
        // asked from the island, the Center, the taskbar or floating bar: never noted, whatever is open
        assert_eq!(route_for(false, false), CardRoute::Everyone);
        assert_eq!(route_for(false, true), CardRoute::Everyone);
    }

    #[test]
    fn noted_questions_are_remembered_bounded_and_tied_to_their_session() {
        // one test for the shared list and session: tests run in parallel and both are process wide
        new_session();
        assert_eq!(query_standing("a"), (false, false));
        note_query("a");
        note_query("b");
        note_query("a"); // asked again: moves to the end, not twice
        assert_eq!(query_standing("a"), (true, true));
        assert_eq!(query_standing("b"), (true, true));
        assert_eq!(query_standing("c"), (false, false));
        for i in 0..40 {
            note_query(&format!("q{i}"));
        }
        assert_eq!(query_standing("a"), (false, false), "the oldest fall out");
        assert_eq!(query_standing("q39"), (true, true));
        assert_eq!(query_standing("q24"), (true, true));
        assert_eq!(query_standing("q23"), (false, false));
        assert_eq!(lock(&QUERIES).len(), MAX_QUERIES);

        // The sheet closed and a new one opened before an answer arrived: the new sheet does not know
        // that question, so the island must show its answer (the id stays noted across sessions).
        note_query("slow");
        assert_eq!(route_of("slow", true), CardRoute::GlassOnly);
        assert_eq!(route_of("slow", false), CardRoute::IslandAfterGlass, "closed meanwhile");
        new_session();
        assert_eq!(query_standing("slow"), (true, false));
        assert_eq!(route_of("slow", true), CardRoute::IslandAfterGlass, "a newer sheet is open");
        // asked again from the new sheet: it is that sheet's answer
        note_query("slow");
        assert_eq!(route_of("slow", true), CardRoute::GlassOnly);
        // a question that never came from a glass is the island's, whatever is open
        assert_eq!(route_of("from-island", true), CardRoute::Everyone);
        assert_eq!(route_of("from-island", false), CardRoute::Everyone);
    }

    #[test]
    fn the_sheet_height_defaults_to_the_field_and_ignores_garbage() {
        reset_sheet();
        assert_eq!(sheet_dip(), SPOT_FIELD_DIP);
        store_sheet(300.0);
        assert_eq!(sheet_dip(), 300.0);
        store_sheet(f64::NAN);
        assert_eq!(sheet_dip(), SPOT_FIELD_DIP);
        store_sheet(f64::INFINITY);
        assert_eq!(sheet_dip(), SPOT_FIELD_DIP);
        reset_sheet();
    }
}
