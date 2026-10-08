//! State that belongs to the centre "Spotlight glass" bar and nothing else: the backdrop picture the
//! page asked for, the height of the sheet (for the click-through region), and which questions were
//! asked from the open glass (their answers live in the sheet, not in the island).
//!
//! Everything here is memory only. The picture and the question ids are dropped when the bar closes
//! or the next one opens; nothing is logged or written to disk.

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

static QUERIES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A question was just asked from the open glass bar (or answered there).
pub fn note_query(query_id: &str) {
    let mut ids = lock(&QUERIES);
    ids.retain(|id| id != query_id);
    ids.push(query_id.to_string());
    let extra = ids.len().saturating_sub(MAX_QUERIES);
    ids.drain(..extra);
}

pub fn is_glass_query(query_id: &str) -> bool {
    lock(&QUERIES).iter().any(|id| id == query_id)
}

/// A new glass session: answers of earlier sessions are the island's again.
pub fn reset_queries() {
    lock(&QUERIES).clear();
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
    fn noted_questions_are_remembered_bounded_and_reset() {
        // one test for the shared list: tests run in parallel and the list is process wide
        reset_queries();
        assert!(!is_glass_query("a"));
        note_query("a");
        note_query("b");
        note_query("a"); // asked again: moves to the end, not twice
        assert!(is_glass_query("a") && is_glass_query("b") && !is_glass_query("c"));
        for i in 0..40 {
            note_query(&format!("q{i}"));
        }
        assert!(!is_glass_query("a"), "the oldest fall out");
        assert!(is_glass_query("q39") && is_glass_query("q24"));
        assert!(!is_glass_query("q23"));
        assert_eq!(lock(&QUERIES).len(), MAX_QUERIES);
        reset_queries();
        assert!(!is_glass_query("q39"), "a new glass session starts clean");
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
