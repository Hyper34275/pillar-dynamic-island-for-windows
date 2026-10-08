#[macro_use]
pub mod debug_log;
mod calendar;
mod calendar_diag;
mod center;
mod center_ipc;
mod com;
mod autostart;
mod notifications;
mod outlook;
mod outlook_nav;
pub mod paths;
mod reminder_state;
mod clipboard;
mod diagnostics;
mod dnd;
mod fullscreen;
mod monitors;
mod notes;
mod system;
mod rt;
mod settings;
mod tray;
mod window;
mod backdrop;
mod assistant;
mod intent;
mod local;
mod outlook_mail;
mod search_bar;

use tauri::utils::config::AppDirectoriesOverride;
use tauri::{Manager, RunEvent};

/// WebView2 switches for the island window. Passing any switch replaces wry's defaults, so
/// those are repeated first: no "mini menu", no PDF toolbar, no SmartScreen reputation
/// lookups. `--disable-background-networking` stops the runtime's own background traffic
/// (component updates, variations, safe-browsing lists): the page is local and offline.
pub(crate) const WEBVIEW_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-networking";

/// Config that is only known at runtime: the WebView2 profile (cache, cookies, crash dumps)
/// lives under the same per-user root as everything else instead of a second
/// `%LOCALAPPDATA%\<identifier>` folder, and the webview gets the switches above.
fn prepare(context: &mut tauri::Context) {
    let config = context.config_mut();
    if let Ok(root) = paths::root() {
        config.app.app_directories_override = Some(AppDirectoriesOverride::Root(root));
    }
    for window in config.app.windows.iter_mut() {
        window.additional_browser_args = Some(WEBVIEW_ARGS.to_string());
    }
}

/// Logging and the managed state, as a plugin so it initializes AFTER the single-instance
/// plugin. `tauri-plugin-single-instance` runs its check in its plugin setup (during
/// `Builder::build`, plugins in registration order), and a duplicate launch signals the
/// first instance and exits right there. Doing this earlier (before `build`) made the
/// duplicate write a session banner, rotate the log and quarantine settings under the
/// running instance; here it never touches a file.
fn core_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("companyisland-core")
        .setup(|app, _api| {
            debug_log::init(env!("CARGO_PKG_VERSION"));
            app.manage(settings::SettingsStore::load());
            app.manage(calendar::CalendarState::default());
            Ok(())
        })
        .build()
}

/// First start (or the first one after an upgrade from a version without onboarding): show the
/// Center's Welcome page once. The flag is only set when the Center really opened, so a missing
/// or failing Center is retried on the next start. A short wait lets the island settle first.
fn first_run_welcome(app: &tauri::AppHandle) {
    if app.state::<settings::SettingsStore>().get().onboarding_done {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("companyisland-onboarding".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        debug_log::catch("onboarding", || match center::open(&app, "welcome") {
            Ok(()) => settings::mark_onboarding_done(&app),
            Err(e) => dlog!("WARN", "center", "welcome page not shown, will retry next start: {}", e),
        });
    });
    if let Err(e) = spawned {
        dlog!("WARN", "center", "APP-030 could not start the welcome thread: {}", e);
    }
}

pub fn run() {
    let mut context = tauri::generate_context!();
    prepare(&mut context);

    let built = tauri::Builder::default()
        // The page only needs ordinary pointer messages. Without this, tao registers the
        // process for raw keyboard and mouse input (the pattern endpoint security tools flag
        // as a key logger) and receives it for nothing; `Always` removes the registration.
        .device_event_filter(tauri::DeviceEventFilter::Always)
        // Single instance per Windows session: tauri-plugin-single-instance creates its
        // mutex without a Global\ prefix (session-local namespace) and finds the first
        // instance with FindWindowW on the current desktop, so other sessions are independent.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            dlog!("INFO", "app", "second launch signalled the running instance");
            window::show(app);
            window::emit_island_toggle(app, None);
        }))
        // Must stay after the single-instance plugin (see `core_plugin`).
        .plugin(core_plugin())
        .invoke_handler(tauri::generate_handler![
            debug_log::write_logs,
            debug_log::log_frontend_error,
            debug_log::open_log_dir,
            settings::get_settings,
            settings::update_settings,
            calendar::calendar_get_snapshot,
            calendar::calendar_refresh,
            calendar::calendar_get_range,
            calendar::calendar_prefetch_status,
            reminder_state::reminder_state_load,
            reminder_state::reminder_state_save,
            notes::notes_load,
            notes::notes_save,
            center::open_center,
            window::set_island_geometry,
            window::island_keyboard,
            window::get_monitors,
            window::get_island_limits,
            fullscreen::get_fullscreen_state,
            system::get_system_info,
            system::get_format_locale,
            diagnostics::get_diagnostics,
            clipboard::copy_text_to_clipboard,
            notifications::notifications_get_status,
            notifications::notifications_request_access,
            notifications::activate_notification,
            notifications::activate_app_by_aumid,
            dnd::dnd_get,
            dnd::dnd_set,
            outlook::outlook_open_calendar,
            outlook::outlook_respond_invite,
            outlook::outlook_set_calendar_selected,
            outlook::open_meeting_url,
            backdrop::get_island_backdrop,
            backdrop::refresh_island_backdrop,
            assistant::assistant_submit,
            assistant::assistant_choose,
            assistant::assistant_extend,
            assistant::assistant_open_item,
            assistant::assistant_open_center,
            assistant::assistant_dismiss,
            search_bar::search_bar_state,
            search_bar::search_bar_close,
        ])
        .on_window_event(|window, event| window::on_window_event(window, event))
        .setup(|app| {
            let handle = app.handle().clone();
            debug_log::set_debug(handle.state::<settings::SettingsStore>().get().debug_logging);
            debug_log::report_previous_session();

            window::init(&handle);
            tray::init(&handle);
            notifications::start(handle.clone());
            dnd::start(handle.clone());
            calendar::start(handle.clone());
            center_ipc::start(handle.clone());
            backdrop::start(handle.clone());
            search_bar::start(handle.clone());
            first_run_welcome(&handle);

            // Registry read/write and the hook install handshake stay off the UI thread. Each step
            // is guarded on its own: a panic in one must not skip the other.
            std::thread::spawn(move || {
                debug_log::catch("startup autostart", || settings::sync_autostart(&handle));
                debug_log::catch("startup fullscreen", || fullscreen::start(handle.clone()));
                debug_log::catch("startup apps cache", local::warm_up);
            });
            Ok(())
        })
        .build(context);

    match built {
        Ok(app) => app.run(|_app, event| match event {
            // All windows closed without an explicit exit: stay alive in the tray.
            RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
            RunEvent::Exit => {
                calendar::stop();
                debug_log::mark_clean_exit();
            }
            _ => {}
        }),
        Err(e) => {
            dlog!("ERROR", "app", "APP-001 failed to start: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WEBVIEW_ARGS;

    #[test]
    fn webview_args_keep_wrys_defaults_and_stay_offline() {
        // Setting any argument replaces wry's own `--disable-features=...` default.
        for feature in ["msWebOOUI", "msPdfOOUI", "msSmartScreenProtection"] {
            assert!(WEBVIEW_ARGS.contains(feature), "{feature}");
        }
        assert!(WEBVIEW_ARGS.contains("--disable-background-networking"));
        assert!(!WEBVIEW_ARGS.contains("--disable-gpu"));
    }
}
