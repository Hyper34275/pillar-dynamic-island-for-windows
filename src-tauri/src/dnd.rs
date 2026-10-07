//! Windows "Do not disturb" (the bell in the island's header).
//!
//! Windows has no public API that *sets* Do not disturb. The Settings app and the quick
//! settings toggle go through the `QuietHoursSettings` COM class, which lives in the user's
//! notification service (WpnUserService, out of process). Its `UserSelectedProfile` is the
//! user's own choice: `Unrestricted` = off, `PriorityOnly` = Do not disturb (Windows 11 shows
//! only priority notifications, alarms and reminders). The interface is undocumented, so every
//! failure is a plain `Err` and the island simply hides the bell.
//!
//! Automatic rules (full screen, gaming, duplicating the display) change the *active* profile
//! but not the user's choice; the bell and the island follow the user's choice only.
//!
//! Event: `dnd-changed` (bool), when the choice changes outside the island (quick settings),
//! as far as Windows reports it (`NotificationModeChanged`); the island re-reads on open too.

use crate::{debug_log, rt};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};
use windows::core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, HSTRING, PCWSTR, PWSTR};
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{ToastNotificationManager, ToastNotificationManagerForUser};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_LOCAL_SERVER};

const CLSID_QUIET_HOURS_SETTINGS: GUID = GUID::from_u128(0xf53321fa_34f8_4b7f_b9a3_361877cb94cf);
const PROFILE_OFF: &str = "Microsoft.QuietHoursProfile.Unrestricted";
const PROFILE_DND: &str = "Microsoft.QuietHoursProfile.PriorityOnly";

/// The first two slots of the shell's `IQuietHoursSettings` (the rest are not used).
#[interface("6bff4732-81ec-4ffb-ae67-b6c1bc29631f")]
unsafe trait IQuietHoursSettings: IUnknown {
    fn get_user_selected_profile(&self, profile: *mut PWSTR) -> HRESULT;
    fn put_user_selected_profile(&self, profile: PCWSTR) -> HRESULT;
}

/// After a mode change event: when to read the user's choice again (ms, cumulative waits).
const RECHECK_MS: [u64; 4] = [0, 250, 750, 2000];

/// The last state reported to the frontend (None until first read), so events only fire on change.
static LAST: Mutex<Option<bool>> = Mutex::new(None);

fn settings() -> Result<IQuietHoursSettings, String> {
    rt::ensure_com_initialized();
    unsafe { CoCreateInstance(&CLSID_QUIET_HOURS_SETTINGS, None, CLSCTX_LOCAL_SERVER) }
        .map_err(|e| format!("DND-001: QuietHoursSettings unavailable (0x{:08X})", e.code().0))
}

/// Whether the user has Do not disturb on (any profile other than "Unrestricted").
pub fn is_on() -> Result<bool, String> {
    let qh = settings()?;
    let mut raw = PWSTR::null();
    unsafe { qh.get_user_selected_profile(&mut raw) }
        .ok()
        .map_err(|e| format!("DND-002: read failed (0x{:08X})", e.code().0))?;
    if raw.is_null() {
        return Err("DND-002: read returned no profile".into());
    }
    let profile = unsafe { raw.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(raw.0 as *const _)) };
    Ok(profile != PROFILE_OFF)
}

fn set(on: bool) -> Result<bool, String> {
    let qh = settings()?;
    let profile = HSTRING::from(if on { PROFILE_DND } else { PROFILE_OFF });
    unsafe { qh.put_user_selected_profile(PCWSTR(profile.as_ptr())) }
        .ok()
        .map_err(|e| format!("DND-003: write failed (0x{:08X})", e.code().0))?;
    let now = is_on()?;
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = Some(now);
    dlog!("INFO", "dnd", "do not disturb set to {} (now {})", on, now);
    Ok(now)
}

#[tauri::command]
pub async fn dnd_get() -> Result<bool, String> {
    rt::run_blocking("dnd_get", || {
        let now = is_on()?;
        *LAST.lock().unwrap_or_else(|p| p.into_inner()) = Some(now);
        Ok(now)
    })
    .await
}

/// Only ever from an explicit click on the bell. Returns the state after the change.
#[tauri::command]
pub async fn dnd_set(on: bool) -> Result<bool, String> {
    rt::run_blocking("dnd_set", move || set(on)).await
}

fn publish_if_changed(app: &AppHandle) {
    let Ok(now) = is_on() else { return };
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if *last == Some(now) {
        return;
    }
    *last = Some(now);
    drop(last);
    let _ = app.emit("dnd-changed", now);
}

/// Follow changes made outside the island. Best effort: an unpackaged exe may not get the
/// event, and then the island only re-reads when it opens.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        debug_log::catch("dnd watch", || {
            rt::ensure_com_initialized();
            let manager = match ToastNotificationManager::GetDefault() {
                Ok(m) => m,
                Err(e) => {
                    dlog!("WARN", "dnd", "no toast manager (0x{:08X}): changes are read on open only", e.code().0);
                    return;
                }
            };
            let handler = TypedEventHandler::<ToastNotificationManagerForUser, windows::core::IInspectable>::new(move |_, _| {
                // The mode can change a moment before the user's choice reads back changed (seen
                // when turning it off from outside), so look again a few times after the event.
                let app = app.clone();
                std::thread::spawn(move || {
                    debug_log::catch("dnd changed", || {
                        rt::ensure_com_initialized();
                        for delay in RECHECK_MS {
                            std::thread::sleep(std::time::Duration::from_millis(delay));
                            publish_if_changed(&app);
                        }
                    });
                });
                Ok(())
            });
            match manager.NotificationModeChanged(&handler) {
                // The subscription lives as long as the manager: keep both for the app's lifetime.
                Ok(_) => std::mem::forget(manager),
                Err(e) => dlog!("WARN", "dnd", "mode events unavailable (0x{:08X}): changes are read on open only", e.code().0),
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only: the shell's quiet-hours service answers on this machine (it never writes).
    #[test]
    fn live_state_is_readable() {
        assert!(is_on().is_ok(), "{:?}", is_on());
    }
}
