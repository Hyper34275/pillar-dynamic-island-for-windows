//! The smart search entry point on the Windows taskbar (AI Mode).
//!
//! - A small native AI button anchored at the right edge of the Windows 10 search box (when the box
//!   is shown), found through public window APIs and out-of-context WinEvent hooks only: no DLL
//!   injection, nothing inside explorer.exe, Windows' own search untouched.
//! - Ctrl+Alt+Space (RegisterHotKey) opens the same input anywhere (fallback, Windows 11, icon-only
//!   or hidden search box, taskbar on the side).
//! - The input + glow is a lazily created Tauri window, label `search`, page `search.html`.
//!
//! CONTRACT: `start`, `apply_settings`, `open`, `close` and the Tauri commands below.

mod anchor;
mod button;
mod hotkey;
mod layout;
mod thread;
mod window;

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
