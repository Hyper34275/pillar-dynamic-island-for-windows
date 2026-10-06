#[macro_use]
pub mod debug_log;
mod calendar;
mod calendar_diag;
mod com;
mod autostart;
mod notifications;
mod outlook;
pub mod paths;
mod reminder_state;
mod clipboard;
mod diagnostics;
mod fullscreen;
mod monitors;
mod system;
mod rt;
mod settings;
mod tray;
mod window;

use tauri::{Manager, RunEvent};

pub fn run() {
    debug_log::init(env!("CARGO_PKG_VERSION"));

    let built = tauri::Builder::default()
        // Single instance per Windows session: tauri-plugin-single-instance creates its
        // mutex without a Global\ prefix (session-local namespace) and finds the first
        // instance with FindWindowW on the current desktop, so other sessions are independent.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            dlog!("INFO", "app", "second launch signalled the running instance");
            window::show(app);
            window::emit_island_toggle(app, None);
        }))
        .manage(settings::SettingsStore::load())
        .manage(calendar::CalendarState::default())
        .invoke_handler(tauri::generate_handler![
            debug_log::write_logs,
            debug_log::log_frontend_error,
            debug_log::open_log_dir,
            settings::get_settings,
            settings::update_settings,
            calendar::calendar_get_snapshot,
            calendar::calendar_refresh,
            reminder_state::reminder_state_load,
            reminder_state::reminder_state_save,
            window::set_click_through,
            window::set_island_geometry,
            window::get_monitors,
            system::get_system_info,
            diagnostics::get_diagnostics,
            clipboard::copy_text_to_clipboard,
            notifications::notifications_get_status,
            notifications::notifications_request_access,
            notifications::activate_notification,
            notifications::activate_app_by_aumid,
        ])
        .on_window_event(|window, event| window::on_window_event(window, event))
        .setup(|app| {
            let handle = app.handle().clone();
            debug_log::set_debug(handle.state::<settings::SettingsStore>().get().debug_logging);
            debug_log::report_previous_session();

            window::init(&handle);
            tray::init(&handle);
            notifications::start(handle.clone());
            calendar::start(handle.clone());

            // Registry read/write and the hook install handshake stay off the UI thread.
            std::thread::spawn(move || {
                debug_log::catch("startup", || {
                    settings::sync_autostart(&handle);
                    fullscreen::start(handle.clone());
                });
            });
            Ok(())
        })
        .build(tauri::generate_context!());

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
