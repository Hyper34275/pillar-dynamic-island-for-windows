//! System tray icon. Creation is non-fatal: without a tray the island still works.

use crate::{center, debug_log, search_bar, settings::SettingsStore, window};
use std::sync::OnceLock;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

const TOGGLE: &str = "tray_toggle";
const CENTER: &str = "tray_center";
const NOTES: &str = "tray_notes";
const SEARCH: &str = "tray_search";

/// The smart search item, so a settings change can enable/disable it at runtime.
static SEARCH_ITEM: OnceLock<MenuItem<Wry>> = OnceLock::new();

/// Smart search switched on/off in the settings: the tray item follows (a disabled item is greyed).
pub fn set_search_enabled(enabled: bool) {
    if let Some(item) = SEARCH_ITEM.get() {
        let _ = item.set_enabled(enabled);
    }
}
const ABOUT: &str = "tray_about";
const LOGS: &str = "tray_logs";
const QUIT: &str = "tray_quit";

/// The tray icon's tooltip: the product name, as Windows shows it everywhere else.
const TOOLTIP: &str = "Yuval";

struct Labels {
    toggle: &'static str,
    center: &'static str,
    notes: &'static str,
    search: &'static str,
    about: &'static str,
    logs: &'static str,
    quit: &'static str,
}

/// The app is Hebrew whatever language Windows runs in, like the island itself (main.tsx).
const LABELS: Labels = Labels {
    toggle: "הצג / הסתר",
    center: "מרכז יובל",
    notes: "פתקים",
    search: "חיפוש חכם",
    about: "אודות",
    logs: "פתח יומנים",
    quit: "יציאה",
};

/// Opens the Yuval Center on `page`. Starting a process or talking to the pipe stays off the UI thread.
fn open_center(app: &AppHandle, page: &'static str) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = center::open(&app, page) {
            dlog!("WARN", "tray", "{}", e);
        }
    });
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let labels = &LABELS;
    let toggle = MenuItem::with_id(app, TOGGLE, labels.toggle, true, None::<&str>)?;
    let center = MenuItem::with_id(app, CENTER, labels.center, true, None::<&str>)?;
    let notes = MenuItem::with_id(app, NOTES, labels.notes, true, None::<&str>)?;
    let search_on = app.state::<SettingsStore>().get().ai_search_enabled;
    let search = MenuItem::with_id(app, SEARCH, labels.search, search_on, None::<&str>)?;
    let _ = SEARCH_ITEM.set(search.clone());
    let about = MenuItem::with_id(app, ABOUT, labels.about, true, None::<&str>)?;
    let logs = MenuItem::with_id(app, LOGS, labels.logs, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, labels.quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &center, &notes, &search, &about, &logs, &separator, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .tooltip(TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            debug_log::catch("tray", || match event.id.as_ref() {
                TOGGLE => window::toggle_visibility(app),
                CENTER => open_center(app, "welcome"),
                NOTES => open_center(app, "notes"),
                SEARCH => search_bar::open(app),
                ABOUT => {
                    window::show(app);
                    window::emit_island_toggle(app, Some("about"));
                }
                LOGS => {
                    tauri::async_runtime::spawn_blocking(|| {
                        if let Err(e) = debug_log::open_dir() {
                            dlog!("WARN", "tray", "{}", e);
                        }
                    });
                }
                QUIT => app.exit(0),
                _ => {}
            });
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn init(app: &AppHandle) {
    if let Err(e) = build(app) {
        dlog!("WARN", "tray", "WIN-502 tray unavailable: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_is_hebrew() {
        assert_eq!(LABELS.quit, "יציאה");
        assert_eq!(LABELS.toggle, "הצג / הסתר");
        assert_eq!(LABELS.center, "מרכז יובל");
        assert_eq!(LABELS.notes, "פתקים");
        assert_eq!(LABELS.search, "חיפוש חכם");
        assert_eq!(LABELS.about, "אודות");
    }

    #[test]
    fn nothing_in_the_tray_carries_the_old_product_name() {
        assert_eq!(TOOLTIP, "Yuval");
        for label in [
            LABELS.toggle, LABELS.center, LABELS.notes, LABELS.search, LABELS.about, LABELS.logs, LABELS.quit, TOOLTIP,
        ] {
            assert!(!label.contains("CompanyIsland") && !label.contains("האי"), "{label}");
        }
    }
}
