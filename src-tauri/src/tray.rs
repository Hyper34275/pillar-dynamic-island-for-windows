//! System tray icon. Creation is non-fatal: without a tray the island still works.

use crate::{center, debug_log, window};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;

const TOGGLE: &str = "tray_toggle";
const CENTER: &str = "tray_center";
const ABOUT: &str = "tray_about";
const LOGS: &str = "tray_logs";
const QUIT: &str = "tray_quit";

struct Labels {
    toggle: &'static str,
    center: &'static str,
    about: &'static str,
    logs: &'static str,
    quit: &'static str,
}

/// The app is Hebrew whatever language Windows runs in, like the island itself (main.tsx).
const LABELS: Labels =
    Labels { toggle: "הצג / הסתר", center: "מרכז האי", about: "אודות", logs: "פתח יומנים", quit: "יציאה" };

fn build(app: &AppHandle) -> tauri::Result<()> {
    let labels = &LABELS;
    let toggle = MenuItem::with_id(app, TOGGLE, labels.toggle, true, None::<&str>)?;
    let center = MenuItem::with_id(app, CENTER, labels.center, true, None::<&str>)?;
    let about = MenuItem::with_id(app, ABOUT, labels.about, true, None::<&str>)?;
    let logs = MenuItem::with_id(app, LOGS, labels.logs, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, labels.quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &center, &about, &logs, &separator, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("CompanyIsland")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            debug_log::catch("tray", || match event.id.as_ref() {
                TOGGLE => window::toggle_visibility(app),
                CENTER => {
                    // Starting a process or talking to the pipe stays off the UI thread.
                    let app = app.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if let Err(e) = center::open(&app, "welcome") {
                            dlog!("WARN", "tray", "{}", e);
                        }
                    });
                }
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
        assert_eq!(LABELS.center, "מרכז האי");
        assert_eq!(LABELS.about, "אודות");
    }
}
