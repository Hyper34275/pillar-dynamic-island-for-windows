//! System tray icon. Creation is non-fatal: without a tray the island still works.

use crate::{debug_log, window};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;
use windows::Win32::Globalization::GetUserDefaultUILanguage;

const TOGGLE: &str = "tray_toggle";
const ABOUT: &str = "tray_about";
const LOGS: &str = "tray_logs";
const QUIT: &str = "tray_quit";

const LANG_HEBREW: u16 = 0x0D;

struct Labels {
    toggle: &'static str,
    about: &'static str,
    logs: &'static str,
    quit: &'static str,
}

const ENGLISH: Labels = Labels { toggle: "Show / Hide island", about: "About", logs: "Open logs", quit: "Quit" };
const HEBREW: Labels = Labels { toggle: "הצג / הסתר", about: "אודות", logs: "פתח יומנים", quit: "יציאה" };

/// Menu strings for a Windows UI language id (primary language = low 10 bits); anything
/// but Hebrew gets English, like the frontend's string table.
fn labels_for(lang_id: u16) -> &'static Labels {
    if lang_id & 0x3FF == LANG_HEBREW {
        &HEBREW
    } else {
        &ENGLISH
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let labels = labels_for(unsafe { GetUserDefaultUILanguage() });
    let toggle = MenuItem::with_id(app, TOGGLE, labels.toggle, true, None::<&str>)?;
    let about = MenuItem::with_id(app, ABOUT, labels.about, true, None::<&str>)?;
    let logs = MenuItem::with_id(app, LOGS, labels.logs, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, labels.quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &about, &logs, &separator, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("CompanyIsland")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            debug_log::catch("tray", || match event.id.as_ref() {
                TOGGLE => window::toggle_visibility(app),
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
    fn hebrew_ui_gets_hebrew_menu() {
        assert_eq!(labels_for(0x040D).quit, "יציאה");
        assert_eq!(labels_for(0x040D).toggle, HEBREW.toggle);
    }

    #[test]
    fn other_languages_get_english() {
        for lang in [0x0409, 0x0809, 0x0407, 0x040C, 0x0000] {
            assert_eq!(labels_for(lang).quit, "Quit", "lang {lang:#x}");
        }
        assert_eq!(labels_for(0x0409).toggle, "Show / Hide island");
    }
}
