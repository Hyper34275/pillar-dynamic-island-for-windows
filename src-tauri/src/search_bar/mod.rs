//! The smart search entry point on the Windows taskbar (AI Mode).
//!
//! - A small native AI button anchored at the right edge of the Windows 10 search box (when the box
//!   is shown), found through public window APIs and out-of-context WinEvent hooks only: no DLL
//!   injection, nothing inside explorer.exe, Windows' own search untouched.
//! - Ctrl+Alt+Space (RegisterHotKey) opens the same input anywhere (fallback, Windows 11, icon-only
//!   or hidden search box, taskbar on the side).
//! - Alt+` (the key left of 1) opens a centred "spotlight glass" bar on the monitor under the cursor;
//!   pressing it again closes it. Both hotkeys follow `aiSearchHotkey`. The glass captures the
//!   screen under it before it shows (`snapshot.rs`) and shows the answer in its own sheet: a question
//!   asked from it is not also shown as a card in the island (`glass.rs`).
//! - The input + glow is a lazily created Tauri window, label `search`, page `search.html`.
//!
//! CONTRACT: `start`, `apply_settings`, `open`, `close` and the Tauri commands below.

mod anchor;
mod button;
mod glass;
mod guard;
mod hotkey;
mod layout;
mod raise;
mod snapshot;
mod thread;
mod window;

pub(crate) use glass::CardRoute;
use snapshot::GlassBackdrop;

use crate::settings::{Settings, SettingsStore};
use serde::Serialize;
use std::sync::OnceLock;
use tauri::{AppHandle, Listener, Manager};

pub const WINDOW_LABEL: &str = "search";

static APP: OnceLock<AppHandle> = OnceLock::new();

pub(crate) fn app() -> Option<&'static AppHandle> {
    APP.get()
}

/// Where the search window is and what it covers (sent to the search page).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBarState {
    /// "taskbar" (anchored on the Windows 10 box), "floating" (above the taskbar) or "spotlight"
    /// (centre of the screen).
    pub variant: String,
    /// The input sits over the real Windows search box (else: a floating bar above the taskbar).
    pub anchored: bool,
    /// Size of the search window in DIPs (the page lays out to it).
    pub width: f64,
    pub height: f64,
    /// Corner radius of the bar in DIPs (0 = square Windows 10 box).
    pub radius: f64,
    pub scale: f64,
    pub high_contrast: bool,
    /// The taskbar is at the bottom / top / left / right.
    pub edge: String,
}

/// Install the anchor thread, hotkey and button per the settings. Called once from `lib.rs` setup.
pub fn start(app: AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    let settings = app.state::<SettingsStore>().get();
    thread::set_flags(settings.ai_search_enabled, settings.ai_search_button, settings.ai_search_hotkey);
    thread::set_fullscreen(crate::fullscreen::get_fullscreen_state());

    // Fullscreen content hides the button and closes the input (the island's own detection).
    let handle = app.clone();
    app.listen("fullscreen-changed", move |event| {
        let on = event.payload().trim() == "true";
        thread::set_fullscreen(on);
        if on {
            window::close(&handle);
        }
        thread::post(thread::CMD_MEASURE);
    });
    thread::start(app);
}

/// Settings changed (`aiSearchEnabled`, `aiSearchButton`, `aiSearchHotkey`).
pub fn apply_settings(app: &AppHandle, settings: &Settings) {
    thread::set_flags(settings.ai_search_enabled, settings.ai_search_button, settings.ai_search_hotkey);
    crate::tray::set_search_enabled(settings.ai_search_enabled);
    if !settings.ai_search_enabled {
        window::close(app);
    }
    thread::post(thread::CMD_RECONCILE);
}

/// Show the search input (AI Mode on) and give it the keyboard. Only after a user action (button
/// click, hotkey, tray item).
pub fn open(app: &AppHandle) {
    window::open(app);
}

/// Back to the normal Windows search (AI Mode off).
pub fn close(app: &AppHandle) {
    window::close(app);
}

/// Open when closed, close when open (button, hotkey).
pub fn toggle(app: &AppHandle) {
    window::toggle(app);
}

/// Ctrl+Alt+Space, delivered on the search-bar thread.
pub(crate) fn on_hotkey() {
    if let Some(app) = app() {
        toggle(app);
    }
}

/// Alt+` (the key left of 1), delivered on the search-bar thread: the centred spotlight bar.
/// Closed -> opens it; another variant open -> switches to it; spotlight open -> closes it.
pub(crate) fn on_spotlight_hotkey() {
    if let Some(app) = app() {
        window::toggle_spotlight(app);
    }
}

/// The AI button was clicked (search-bar thread).
pub(crate) fn on_button_clicked() {
    if let Some(app) = app() {
        toggle(app);
    }
}

#[tauri::command]
pub fn search_bar_state() -> SearchBarState {
    window::state()
}

/// The page asks to leave AI Mode (Esc, AI button in the page).
#[tauri::command]
pub fn search_bar_close(app: AppHandle) {
    close(&app);
}

/// The page reports the height of the glass sheet (DIP): the click-through window region follows it,
/// so the transparent part of the fixed-size window never swallows a click. Sync, so it runs on the
/// window's thread and the page can wait for it before it grows the sheet.
#[tauri::command]
pub fn search_bar_region(app: AppHandle, height: f64) {
    window::set_sheet_height(&app, height);
}

/// The picture of the screen under the glass bar (memory only), for a page that was not loaded when
/// the bar opened. `None` outside the glass, after it closed, or when the screen could not be read.
#[tauri::command]
pub fn search_bar_backdrop() -> Option<GlassBackdrop> {
    window::glass_open().then(glass::current_backdrop).flatten()
}

/// The centre glass bar is open (not the taskbar-anchored or floating bar).
pub(crate) fn glass_open() -> bool {
    window::glass_open()
}

/// A question was asked from the open glass bar: its answer is shown in the sheet.
pub(crate) fn note_glass_query(query_id: &str) {
    glass::note_query(query_id);
}

/// Where the `assistant-update` card of this query goes (see `glass::route_for`).
pub(crate) fn card_route(query_id: &str) -> CardRoute {
    glass::route_for(glass::is_glass_query(query_id), window::glass_open())
}
