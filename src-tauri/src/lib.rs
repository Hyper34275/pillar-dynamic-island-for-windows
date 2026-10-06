#[macro_use]
pub mod debug_log;
mod autostart;
mod notifications;
pub mod paths;
mod pointer;
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
        .invoke_handler(tauri::generate_handler![
            debug_log::write_logs,
            debug_log::log_frontend_error,
            debug_log::open_log_dir,
            settings::get_settings,
            settings::update_settings,
            window::set_click_through,
            window::position_window,
            window::set_island_geometry,
            window::is_foreground_fullscreen,
            // Global pointer monitor (native hit-testing for the island)
            pointer::set_pill_hit_region,
            pointer::get_pointer_tracker_status,
            notifications::check_notification_access,
            notifications::get_notifications,
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

            // Registry read/write and the hook install handshake stay off the UI thread.
            std::thread::spawn(move || {
                debug_log::catch("startup", || {
                    settings::sync_autostart(&handle);
                    pointer::start(handle.clone());
                });
            });
            Ok(())
        })
        .build(tauri::generate_context!());

    match built {
        Ok(app) => app.run(|_app, event| match event {
            // All windows closed without an explicit exit: stay alive in the tray.
            RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
            RunEvent::Exit => debug_log::mark_clean_exit(),
            _ => {}
        }),
        Err(e) => {
            dlog!("ERROR", "app", "APP-001 failed to start: {}", e);
            std::process::exit(1);
        }
    }
}
