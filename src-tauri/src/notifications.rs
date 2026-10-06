//! Windows toast mirroring via `UserNotificationListener` (best effort).
//!
//! Non-destructive: notifications are only read, never removed from Windows.
//! Access is checked without prompting at startup; the prompt is only raised by
//! the explicit `check_notification_access` command. Notification text is never
//! logged.

use crate::{debug_log, paths, rt, settings::SettingsStore};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use windows::core::HSTRING;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{
    NotificationKinds, UserNotification, UserNotificationChangedEventArgs, UserNotificationChangedKind,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemNotification {
    pub id: u32,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub timestamp: u64, // Unix timestamp in milliseconds
    pub aumid: Option<String>, // App User Model ID for activating the source app
}

/// Cached access status so list calls don't re-query it every time.
static ACCESS_GRANTED: AtomicBool = AtomicBool::new(false);
static SUBSCRIBED: AtomicBool = AtomicBool::new(false);

fn enabled(app: &AppHandle) -> bool {
    app.state::<SettingsStore>().get().notifications_enabled
}

fn note_access(status: UserNotificationListenerAccessStatus) {
    ACCESS_GRANTED.store(status == UserNotificationListenerAccessStatus::Allowed, Ordering::Relaxed);
    match status {
        UserNotificationListenerAccessStatus::Allowed => {}
        UserNotificationListenerAccessStatus::Denied => {
            dlog!("INFO", "notifications", "NOTIF-201 notification access denied");
        }
        other => {
            dlog!("INFO", "notifications", "NOTIF-202 notification access not granted ({:?})", other);
        }
    }
}

fn poll_notifications_list(listener: &UserNotificationListener) -> Result<Vec<UserNotification>, String> {
    let op = listener
        .GetNotificationsAsync(NotificationKinds::Toast)
        .map_err(|e| format!("NOTIF-204: failed to get notifications: {e}"))?;
    let list = rt::poll_op(op, "NOTIF-204 notifications")?;
    let count = list.Size().unwrap_or(0);
    Ok((0..count).filter_map(|i| list.GetAt(i).ok()).collect())
}

/// Subscribe to NotificationChanged with retry for transient startup races.
/// Some systems return HRESULT 0x80070490 (Element not found) even when polling works.
fn subscribe_notification_changed(listener: &UserNotificationListener, app: &AppHandle) -> bool {
    const RETRIES: usize = 3;
    const RETRY_DELAY_MS: u64 = 500;
    const E_ELEMENT_NOT_FOUND: i32 = 0x80070490u32 as i32;

    if SUBSCRIBED.load(Ordering::Relaxed) {
        return true;
    }

    for attempt in 1..=RETRIES {
        let app = app.clone();
        let handler = TypedEventHandler::new(
            move |_listener: &Option<UserNotificationListener>, args: &Option<UserNotificationChangedEventArgs>| {
                // Never let a panic unwind into WinRT.
                debug_log::catch("notifications", || on_changed(&app, args.as_ref()));
                Ok(())
            },
        );

        match listener.NotificationChanged(&handler) {
            Ok(_) => {
                SUBSCRIBED.store(true, Ordering::Relaxed);
                dlog!("INFO", "notifications", "subscribed to NotificationChanged (attempt {})", attempt);
                return true;
            }
            Err(e) => {
                let not_found = e.code().0 == E_ELEMENT_NOT_FOUND;
                if not_found && attempt < RETRIES {
                    thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
                    continue;
                }
                dlog!(
                    "WARN",
                    "notifications",
                    "NOTIF-204 NotificationChanged unavailable ({:#x}); polling only",
                    e.code().0
                );
                return false;
            }
        }
    }
    false
}

fn on_changed(app: &AppHandle, args: Option<&UserNotificationChangedEventArgs>) {
    if let Some(args) = args {
        if let (Ok(UserNotificationChangedKind::Added), Ok(id)) = (args.ChangeKind(), args.UserNotificationId()) {
            if let Ok(listener) = UserNotificationListener::Current() {
                if let Ok(list) = poll_notifications_list(&listener) {
                    let found = list.iter().find(|n| n.Id().unwrap_or(0) == id);
                    if let Some(notification) = found.and_then(|n| extract_notification(n, 0)) {
                        let _ = app.emit("notification-added", &notification);
                        return;
                    }
                }
            }
        }
    }
    // Removed, or the added notification could not be read: let the UI re-list.
    let _ = app.emit("notification-changed", ());
}

/// Start the listener on a worker thread; never blocks setup.
pub fn start(app: AppHandle) {
    let spawned = thread::Builder::new().name("companyisland-notif-init".into()).spawn(move || {
        debug_log::catch("notifications", || init(&app));
    });
    if let Err(e) = spawned {
        dlog!("WARN", "notifications", "NOTIF-204 could not start listener thread: {}", e);
    }
}

fn init(app: &AppHandle) {
    rt::ensure_com_initialized();
    if !enabled(app) {
        dlog!("INFO", "notifications", "disabled in settings");
        return;
    }
    let listener = match UserNotificationListener::Current() {
        Ok(l) => l,
        Err(e) => {
            dlog!("WARN", "notifications", "NOTIF-203 listener unsupported: {}", e);
            return;
        }
    };
    // GetAccessStatus never prompts; only the explicit command does.
    match listener.GetAccessStatus() {
        Ok(status) => {
            note_access(status);
            if status == UserNotificationListenerAccessStatus::Allowed {
                subscribe_notification_changed(&listener, app);
            }
        }
        Err(e) => dlog!("WARN", "notifications", "NOTIF-204 access status failed: {}", e),
    }
}

/// Request notification access (may show the Windows consent UI) and subscribe
/// when granted. Returns whether access is allowed.
#[tauri::command]
pub async fn check_notification_access(app: AppHandle) -> Result<bool, String> {
    rt::run_blocking("check_notification_access", move || {
        rt::ensure_com_initialized();
        let listener = UserNotificationListener::Current().map_err(|e| format!("NOTIF-203: {e}"))?;
        let op = listener.RequestAccessAsync().map_err(|e| format!("NOTIF-204: {e}"))?;
        let status = rt::poll_op(op, "NOTIF-204 access request")?;
        note_access(status);
        let allowed = status == UserNotificationListenerAccessStatus::Allowed;
        if allowed && enabled(&app) {
            subscribe_notification_changed(&listener, &app);
        }
        Ok(allowed)
    })
    .await
}

/// Extract a SystemNotification from a Windows UserNotification.
/// Returns None if the notification has no meaningful content.
fn extract_notification(notif: &UserNotification, idx: usize) -> Option<SystemNotification> {
    let id = notif.Id().unwrap_or(idx as u32);

    let app_name = notif
        .AppInfo()
        .ok()
        .and_then(|app_info| app_info.DisplayInfo().ok())
        .and_then(|display_info| display_info.DisplayName().ok())
        .map(|h| h.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Windows App".to_string());

    let aumid = notif
        .AppInfo()
        .ok()
        .and_then(|app_info| app_info.AppUserModelId().ok())
        .map(|h| h.to_string())
        .filter(|s| !s.is_empty());

    let notification = notif.Notification().ok()?;
    let visual = notification.Visual().ok()?;

    let mut title = String::new();
    let mut body = String::new();

    if let Ok(bindings) = visual.Bindings() {
        if let Ok(count) = bindings.Size() {
            for i in 0..count {
                if let Ok(binding) = bindings.GetAt(i) {
                    if let Ok(elements) = binding.GetTextElements() {
                        if let Ok(elem_count) = elements.Size() {
                            for j in 0..elem_count {
                                if let Ok(elem) = elements.GetAt(j) {
                                    if let Ok(text) = elem.Text() {
                                        let text_str = text.to_string();
                                        if title.is_empty() {
                                            title = text_str;
                                        } else if body.is_empty() {
                                            body = text_str;
                                        } else {
                                            body.push('\n');
                                            body.push_str(&text_str);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    break; // Only process first binding
                }
            }
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let timestamp = notif
        .CreationTime()
        .ok()
        .map(|dt| {
            const EPOCH_OFFSET_100NS: i64 = 11644473600 * 10_000_000;
            ((dt.UniversalTime - EPOCH_OFFSET_100NS) / 10_000) as u64
        })
        .filter(|&t| t > 0 && t < now + 86400_000)
        .unwrap_or_else(|| now.saturating_sub(idx as u64 * 60000));

    if title.is_empty() && body.is_empty() {
        return None;
    }

    Some(SystemNotification { id, app_name, title, body, timestamp, aumid })
}

/// Get recent notifications (empty unless access was granted and the feature is on).
#[tauri::command]
pub async fn get_notifications(app: AppHandle) -> Result<Vec<SystemNotification>, String> {
    rt::run_blocking("get_notifications", move || {
        rt::ensure_com_initialized();
        if !ACCESS_GRANTED.load(Ordering::Relaxed) || !enabled(&app) {
            return Ok(Vec::new());
        }
        let listener = UserNotificationListener::Current().map_err(|e| format!("NOTIF-203: {e}"))?;
        let list = poll_notifications_list(&listener)?;
        Ok(list
            .iter()
            .take(10)
            .enumerate()
            .filter_map(|(idx, notif)| extract_notification(notif, idx))
            .collect())
    })
    .await
}

/// AUMIDs are package-family names, reverse-DNS ids or `{KnownFolderGuid}\path\app.exe`
/// style ids. Anything that could change how the shell parses the argument is refused.
fn is_valid_aumid(aumid: &str) -> bool {
    !aumid.is_empty()
        && aumid.chars().count() <= 256
        && !aumid.contains("..")
        && aumid
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '!' | '{' | '}' | '\\' | ' ' | '(' | ')' | '+' | ','))
}

/// Launch an app through `shell:AppsFolder\<AUMID>`, the same way Action Center does.
fn launch_aumid(aumid: &str) -> Result<(), String> {
    if !is_valid_aumid(aumid) {
        return Err("NOTIF-204: invalid application id".to_string());
    }
    let target = format!("shell:AppsFolder\\{aumid}");
    let open = HSTRING::from("open");

    let direct = unsafe { ShellExecuteW(None, &open, &HSTRING::from(target.as_str()), None, None, SW_SHOWNORMAL) };
    if direct.0 as isize > 32 {
        return Ok(());
    }

    // Some desktop apps only activate through explorer.exe (absolute path, quoted argument).
    let explorer = HSTRING::from(paths::explorer_exe().as_os_str());
    let params = HSTRING::from(format!("\"{target}\""));
    let result = unsafe { ShellExecuteW(None, &open, &explorer, &params, None, SW_SHOWNORMAL) };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("NOTIF-204: failed to activate app (ShellExecute returned {})", result.0 as isize))
    }
}

/// Activate the app that created the notification with the given ID.
#[tauri::command]
pub async fn activate_notification(id: u32) -> Result<(), String> {
    rt::run_blocking("activate_notification", move || {
        rt::ensure_com_initialized();
        if !ACCESS_GRANTED.load(Ordering::Relaxed) {
            return Err("NOTIF-201: notification access not granted".to_string());
        }
        let listener = UserNotificationListener::Current().map_err(|e| format!("NOTIF-203: {e}"))?;
        let list = poll_notifications_list(&listener)?;
        let notif = list
            .iter()
            .find(|n| n.Id().unwrap_or(0) == id)
            .ok_or_else(|| "NOTIF-204: notification no longer available".to_string())?;
        let aumid = notif
            .AppInfo()
            .and_then(|info| info.AppUserModelId())
            .map_err(|e| format!("NOTIF-204: application id unavailable: {e}"))?
            .to_string();
        launch_aumid(&aumid)
    })
    .await
}

/// Activate an app by its AUMID directly (the notification may already be gone).
#[tauri::command]
pub async fn activate_app_by_aumid(aumid: String) -> Result<(), String> {
    rt::run_blocking("activate_app_by_aumid", move || {
        rt::ensure_com_initialized();
        launch_aumid(&aumid)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_real_world_aumids() {
        assert!(is_valid_aumid("Microsoft.WindowsCalendar_8wekyb3d8bbwe!App"));
        assert!(is_valid_aumid("com.squirrel.slack.slack"));
        assert!(is_valid_aumid(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\Mozilla Firefox\firefox.exe"));
        assert!(is_valid_aumid("MSEdge"));
    }

    #[test]
    fn rejects_shell_metacharacters() {
        assert!(!is_valid_aumid(""));
        assert!(!is_valid_aumid("app\" /c calc"));
        assert!(!is_valid_aumid("a&b"));
        assert!(!is_valid_aumid("a|b"));
        assert!(!is_valid_aumid("%TEMP%\\evil"));
        assert!(!is_valid_aumid("a/b"));
        assert!(!is_valid_aumid(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\..\..\Windows\System32\calc.exe"));
        assert!(!is_valid_aumid(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}....WindowsSystem32lc.exe"));
        assert!(!is_valid_aumid("app\r\n"));
        assert!(!is_valid_aumid(&"a".repeat(300)));
    }
}
