//! Windows toast mirroring via `UserNotificationListener` (best effort).
//!
//! Non-destructive: notifications are only read, never removed from Windows
//! (`RemoveNotification` is never called). Access is checked without prompting; the
//! consent prompt is only raised by the explicit `notifications_request_access`
//! command. Every outcome is a status string, never a panic, and the app carries on
//! without the feature when access is denied, blocked by policy or unsupported.
//! Notification text is never logged.
//!
//! Events: `notification-received` (one toast) and `notification-status` (string).
//! Delivery runs only while access is allowed *and* `settings.notificationsEnabled` is on,
//! and follows both changes live.
//!
//! Delivery mode (`events` | `polling` | `none`, reported by diagnostics as
//! `notificationMode`): the `NotificationChanged` event is preferred. An unpackaged exe
//! usually cannot subscribe to it (`NOTIF-204 ... (0x80070490)`, "element not found"); the
//! access status is still `allowed` then, so the module falls back to ONE poller thread that
//! reads `GetNotificationsAsync(Toast)` and forwards only ids it has not seen. `NOTIF-204`
//! in general means "listener error" (docs/ENTERPRISE_DESIGN.md section 3): a failed
//! subscription, read or access query. It never disables the app.

use crate::{debug_log, diagnostics, paths, rt, settings::SettingsStore, system};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use windows::core::HSTRING;
use windows::Foundation::{AsyncOperationCompletedHandler, EventRegistrationToken, TypedEventHandler};
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{
    NotificationKinds, UserNotification, UserNotificationChangedEventArgs, UserNotificationChangedKind,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const POLICY_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy";
const POLICY_VALUE: &str = "LetAppsAccessNotifications";
/// `LetAppsAccessNotifications`: 0 = user decides, 1 = force allow, 2 = force deny.
const POLICY_FORCE_DENY: u32 = 2;
const E_ELEMENT_NOT_FOUND: i32 = 0x80070490u32 as i32;
const SUBSCRIBE_RETRIES: usize = 3;
const SUBSCRIBE_RETRY_MS: u64 = 500;
/// Fallback poller: cheap read of the Action Center every 5 s, 30 s after repeated failures.
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const POLL_BACKOFF: Duration = Duration::from_secs(30);
const POLL_BACKOFF_AFTER_FAILURES: u32 = 3;
/// Notification ids remembered by the poller (the newest ones, ids only ever grow).
const SEEN_MAX: usize = 256;

/// The notification as sent to the frontend (`normalizeNotification` in `src/lib/ipc.ts`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemNotification {
    pub id: u32,
    pub app_name: String,
    pub title: String,
    pub body: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// App User Model ID for activating the source app.
    pub aumid: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Allowed,
    Denied,
    Unspecified,
    Unsupported,
    Policy,
    Error,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Allowed => "allowed",
            Status::Denied => "denied",
            Status::Unspecified => "unspecified",
            Status::Unsupported => "unsupported",
            Status::Policy => "policy",
            Status::Error => "error",
        }
    }

    /// Diagnostic code of docs/ENTERPRISE_DESIGN.md §3 for a status that disables the feature.
    fn code(self) -> Option<&'static str> {
        match self {
            Status::Allowed => None,
            Status::Denied => Some("NOTIF-201"),
            Status::Unspecified | Status::Policy => Some("NOTIF-202"),
            Status::Unsupported => Some("NOTIF-203"),
            Status::Error => Some("NOTIF-204"),
        }
    }
}

/// `Denied` while group policy forces notification access off is reported as `Policy`.
fn classify(access: UserNotificationListenerAccessStatus, policy_denies: bool) -> Status {
    match access {
        UserNotificationListenerAccessStatus::Allowed => Status::Allowed,
        UserNotificationListenerAccessStatus::Denied if policy_denies => Status::Policy,
        UserNotificationListenerAccessStatus::Denied => Status::Denied,
        UserNotificationListenerAccessStatus::Unspecified => Status::Unspecified,
        _ => Status::Error,
    }
}

fn policy_denies() -> bool {
    system::hklm_dword(POLICY_KEY, POLICY_VALUE) == Some(POLICY_FORCE_DENY)
}

/// How many distinct notification senders are remembered for `activate_app_by_aumid`.
const RECENT_AUMIDS_MAX: usize = 64;

/// AUMIDs of the toasts forwarded to the island, oldest first. `activate_app_by_aumid` only
/// launches one of these: an AUMID can also spell `{KnownFolder}\any.exe`, so without this
/// the webview could name any shell target.
#[derive(Default)]
struct RecentAumids(VecDeque<String>);

impl RecentAumids {
    fn remember(&mut self, aumid: &str) {
        if self.contains(aumid) {
            return;
        }
        if self.0.len() == RECENT_AUMIDS_MAX {
            self.0.pop_front();
        }
        self.0.push_back(aumid.to_string());
    }

    fn contains(&self, aumid: &str) -> bool {
        self.0.iter().any(|known| known == aumid)
    }
}

static RECENT_AUMIDS: Mutex<RecentAumids> = Mutex::new(RecentAumids(VecDeque::new()));

/// Last published status (`None` before the first check).
static STATUS: Mutex<Option<Status>> = Mutex::new(None);
/// How toasts reach the island right now. Kept apart from [`DELIVERY`] so diagnostics never
/// wait for a subscription attempt.
static MODE: Mutex<Mode> = Mutex::new(Mode::None);
/// The live delivery. Also serializes `sync` so concurrent callers cannot double-start.
static DELIVERY: Mutex<Delivery> = Mutex::new(Delivery { token: None, events_unavailable: false, poller: None });

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    None,
    Events,
    Polling,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::None => "none",
            Mode::Events => "events",
            Mode::Polling => "polling",
        }
    }
}

struct Delivery {
    /// Registration token of the live `NotificationChanged` subscription.
    token: Option<i64>,
    /// Subscribing failed for good: do not retry (each try costs ~1 s) until access has
    /// been lost and regained.
    events_unavailable: bool,
    /// Dropping the sender stops the poller thread.
    poller: Option<mpsc::Sender<()>>,
}

impl Delivery {
    fn mode(&self) -> Mode {
        if self.token.is_some() {
            Mode::Events
        } else if self.poller.is_some() {
            Mode::Polling
        } else {
            Mode::None
        }
    }

    fn start(&mut self, app: &AppHandle, listener: &UserNotificationListener) {
        if self.mode() != Mode::None {
            return;
        }
        if !self.events_unavailable {
            self.token = subscribe(app, listener);
            self.events_unavailable = self.token.is_none();
        }
        if self.token.is_none() {
            self.poller = spawn_poller(app);
        }
    }

    fn stop(&mut self, listener: Option<&UserNotificationListener>) {
        if let (Some(token), Some(listener)) = (self.token, listener) {
            unsubscribe(listener, token);
            self.token = None;
        }
        self.poller = None;
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn enabled(app: &AppHandle) -> bool {
    app.state::<SettingsStore>().get().notifications_enabled
}

pub fn current_status() -> Option<String> {
    lock(&STATUS).map(|s| s.as_str().to_string())
}

/// `events` | `polling` | `none`.
pub fn current_mode() -> &'static str {
    lock(&MODE).as_str()
}

/// Record the delivery mode, logged once per change.
fn set_mode(mode: Mode) {
    {
        let mut current = lock(&MODE);
        if *current == mode {
            return;
        }
        *current = mode;
    }
    dlog!("INFO", "notifications", "notification delivery mode: {}", mode.as_str());
}

/// Record and announce a status, once per change.
fn publish(app: &AppHandle, status: Status) {
    {
        let mut current = lock(&STATUS);
        if *current == Some(status) {
            return;
        }
        *current = Some(status);
    }
    match status.code() {
        Some(code) => {
            diagnostics::record_error(code);
            dlog!("INFO", "notifications", "{} notification access is {}", code, status.as_str());
        }
        None => dlog!("INFO", "notifications", "notification access allowed"),
    }
    if let Err(e) = app.emit("notification-status", status.as_str()) {
        dlog!("WARN", "notifications", "emit notification-status failed: {}", e);
    }
}

/// Ask Windows for access. The poll is bounded; if the consent UI outlives it, the
/// completion handler re-syncs so a late grant takes effect without a restart.
fn request_access(app: &AppHandle, listener: &UserNotificationListener) {
    let op = match listener.RequestAccessAsync() {
        Ok(op) => op,
        Err(e) => {
            dlog!("WARN", "notifications", "NOTIF-204 access request failed: {}", e);
            return;
        }
    };
    if let Err(e) = rt::poll_op(op.clone(), "NOTIF-204 access request") {
        dlog!("INFO", "notifications", "{}", e);
        if e.ends_with("timed out") {
            let app = app.clone();
            let handler = AsyncOperationCompletedHandler::new(move |_, _| {
                let app = app.clone();
                thread::spawn(move || {
                    debug_log::catch("notifications", || sync(&app, false));
                });
                Ok(())
            });
            if let Err(e) = op.SetCompleted(&handler) {
                dlog!("WARN", "notifications", "NOTIF-204 could not observe the access request: {}", e);
            }
        }
    }
}

fn query(app: &AppHandle, request: bool) -> (Status, Option<UserNotificationListener>) {
    rt::ensure_com_initialized();
    let listener = match UserNotificationListener::Current() {
        Ok(l) => l,
        Err(e) => {
            dlog!("INFO", "notifications", "listener class unavailable: {}", e);
            return (Status::Unsupported, None);
        }
    };
    if request {
        request_access(app, &listener);
    }
    // GetAccessStatus never prompts.
    match listener.GetAccessStatus() {
        Ok(access) => (classify(access, policy_denies()), Some(listener)),
        Err(e) => {
            dlog!("WARN", "notifications", "NOTIF-204 access status failed: {}", e);
            (Status::Error, Some(listener))
        }
    }
}

/// Subscribe to NotificationChanged with retry for transient startup races: some
/// systems return HRESULT 0x80070490 (Element not found) before the listener is ready.
fn subscribe(app: &AppHandle, listener: &UserNotificationListener) -> Option<i64> {
    for attempt in 1..=SUBSCRIBE_RETRIES {
        let app = app.clone();
        let handler = TypedEventHandler::new(
            move |_listener: &Option<UserNotificationListener>, args: &Option<UserNotificationChangedEventArgs>| {
                // Never let a panic unwind into WinRT.
                debug_log::catch("notifications", || on_changed(&app, args.as_ref()));
                Ok(())
            },
        );
        match listener.NotificationChanged(&handler) {
            Ok(token) => {
                dlog!("INFO", "notifications", "subscribed to NotificationChanged (attempt {})", attempt);
                return Some(token.Value);
            }
            Err(e) if e.code().0 == E_ELEMENT_NOT_FOUND && attempt < SUBSCRIBE_RETRIES => {
                thread::sleep(Duration::from_millis(SUBSCRIBE_RETRY_MS));
            }
            Err(e) => {
                dlog!("WARN", "notifications", "NOTIF-204 NotificationChanged unavailable ({:#x})", e.code().0);
                return None;
            }
        }
    }
    None
}

fn unsubscribe(listener: &UserNotificationListener, token: i64) {
    match listener.RemoveNotificationChanged(EventRegistrationToken { Value: token }) {
        Ok(()) => dlog!("INFO", "notifications", "unsubscribed from NotificationChanged"),
        Err(e) => dlog!("WARN", "notifications", "NOTIF-204 unsubscribe failed: {}", e),
    }
}

/// Bring status and delivery in line with Windows' access state and the setting.
fn sync(app: &AppHandle, request: bool) -> Status {
    let mut delivery = lock(&DELIVERY);
    let (status, listener) = query(app, request);
    publish(app, status);
    if status != Status::Allowed {
        delivery.events_unavailable = false;
    }
    match (&listener, status == Status::Allowed && enabled(app)) {
        (Some(listener), true) => delivery.start(app, listener),
        _ => delivery.stop(listener.as_ref()),
    }
    set_mode(delivery.mode());
    status
}

/// Remember the sender (so the toast can be activated later) and hand the toast to the island.
fn forward(app: &AppHandle, notification: &SystemNotification) {
    if let Some(aumid) = &notification.aumid {
        lock(&RECENT_AUMIDS).remember(aumid);
    }
    if let Err(e) = app.emit("notification-received", notification) {
        dlog!("WARN", "notifications", "emit notification-received failed: {}", e);
    }
}

fn on_changed(app: &AppHandle, args: Option<&UserNotificationChangedEventArgs>) {
    if !enabled(app) {
        return;
    }
    let Some(args) = args else { return };
    let (Ok(UserNotificationChangedKind::Added), Ok(id)) = (args.ChangeKind(), args.UserNotificationId()) else {
        return;
    };
    let Ok(listener) = UserNotificationListener::Current() else { return };
    let Some(notification) = listener.GetNotification(id).ok().and_then(|n| extract_notification(&n)) else {
        return;
    };
    forward(app, &notification);
}

// ---------------------------------------------------------------------------
// Polling fallback
// ---------------------------------------------------------------------------

/// Ids already seen by the poller. The first poll only records (the Action Center's old
/// items must not flood the island); afterwards only ids that were not in the previous
/// poll are new. An id that left the list and comes back counts as new again. Only the
/// newest [`SEEN_MAX`] ids are kept, so the set stays bounded.
#[derive(Default)]
struct Seen {
    ids: HashSet<u32>,
    primed: bool,
}

/// Ids in `current` that `seen` has not seen, oldest first; `seen` then becomes `current`.
fn new_since(seen: &mut Seen, current: &[u32]) -> Vec<u32> {
    let mut ids = current.to_vec();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids.dedup();
    ids.truncate(SEEN_MAX);
    ids.reverse();
    let fresh = if seen.primed { ids.iter().copied().filter(|id| !seen.ids.contains(id)).collect() } else { Vec::new() };
    seen.ids = ids.into_iter().collect();
    seen.primed = true;
    fresh
}

fn poll_interval(consecutive_failures: u32) -> Duration {
    if consecutive_failures >= POLL_BACKOFF_AFTER_FAILURES {
        POLL_BACKOFF
    } else {
        POLL_INTERVAL
    }
}

enum PollFailure {
    /// Access is no longer `Allowed`: let `sync` publish it and stop the poller.
    AccessLost,
    Failed(String),
}

fn spawn_poller(app: &AppHandle) -> Option<mpsc::Sender<()>> {
    let (stop, stopped) = mpsc::channel();
    let app = app.clone();
    let spawned = thread::Builder::new().name("companyisland-notif-poll".into()).spawn(move || poll_loop(&app, &stopped));
    match spawned {
        Ok(_) => Some(stop),
        Err(e) => {
            dlog!("WARN", "notifications", "NOTIF-204 could not start the poller thread: {}", e);
            None
        }
    }
}

/// Runs until the sender is dropped. Waits on the channel, so a stop is immediate and
/// there is no tight loop.
fn poll_loop(app: &AppHandle, stopped: &mpsc::Receiver<()>) {
    rt::ensure_com_initialized();
    let mut seen = Seen::default();
    let mut failures = 0u32;
    loop {
        let outcome = debug_log::catch("notifications", || poll_once(app, stopped, &mut seen))
            .unwrap_or_else(|| Err(PollFailure::Failed("NOTIF-204: poll panicked".to_string())));
        match outcome {
            Ok(()) => failures = 0,
            Err(PollFailure::AccessLost) => {
                failures = 0;
                sync(app, false);
            }
            Err(PollFailure::Failed(message)) => {
                failures += 1;
                if failures == 1 || failures == POLL_BACKOFF_AFTER_FAILURES {
                    dlog!("WARN", "notifications", "{} (failure {})", message, failures);
                }
            }
        }
        match stopped.recv_timeout(poll_interval(failures)) {
            Err(RecvTimeoutError::Timeout) => {}
            _ => return,
        }
    }
}

/// One read-only pass over the Action Center's toasts.
fn poll_once(app: &AppHandle, stopped: &mpsc::Receiver<()>, seen: &mut Seen) -> Result<(), PollFailure> {
    let listener = UserNotificationListener::Current()
        .map_err(|e| PollFailure::Failed(format!("NOTIF-204: listener unavailable: {e}")))?;
    match listener.GetAccessStatus() {
        Ok(UserNotificationListenerAccessStatus::Allowed) => {}
        Ok(_) => return Err(PollFailure::AccessLost),
        Err(e) => return Err(PollFailure::Failed(format!("NOTIF-204: access status failed: {e}"))),
    }
    let list = poll_notifications_list(&listener).map_err(PollFailure::Failed)?;
    let ids: Vec<u32> = list.iter().filter_map(|n| n.Id().ok()).collect();
    let fresh = new_since(seen, &ids);
    // Stopped or disabled while reading: record, emit nothing.
    if matches!(stopped.try_recv(), Err(TryRecvError::Disconnected)) || !enabled(app) {
        return Ok(());
    }
    if !fresh.is_empty() {
        dlog!("DEBUG", "notifications", "poll: {} new of {} toasts", fresh.len(), ids.len());
    }
    let new = list.iter().filter(|n| n.Id().is_ok_and(|id| fresh.contains(&id)));
    for notification in new.filter_map(extract_notification) {
        forward(app, &notification);
    }
    Ok(())
}

/// Start the listener on a worker thread; never blocks setup.
pub fn start(app: AppHandle) {
    spawn_sync(app, "companyisland-notif-init");
}

/// `settings.notificationsEnabled` changed: subscribe or unsubscribe.
pub fn on_enabled_changed(app: &AppHandle) {
    spawn_sync(app.clone(), "companyisland-notif-sync");
}

fn spawn_sync(app: AppHandle, name: &str) {
    let spawned = thread::Builder::new().name(name.into()).spawn(move || {
        debug_log::catch("notifications", || sync(&app, false));
    });
    if let Err(e) = spawned {
        dlog!("WARN", "notifications", "NOTIF-204 could not start listener thread: {}", e);
    }
}

/// Last known access status, returned at once. A background `sync` refreshes it (picking
/// up a grant made in Windows Settings since the last check) and announces a change through
/// `notification-status`; waiting for it would queue behind a subscription attempt.
#[tauri::command]
pub async fn notifications_get_status(app: AppHandle) -> Result<String, String> {
    rt::run_blocking("notifications_get_status", move || {
        let known = *lock(&STATUS);
        let status = known.unwrap_or_else(|| {
            // Before the boot check has published anything: ask Windows (never prompts).
            let (status, _) = query(&app, false);
            publish(&app, status);
            status
        });
        spawn_sync(app, "companyisland-notif-sync");
        Ok(status.as_str().to_string())
    })
    .await
}

/// Raise the Windows consent prompt. Only ever called from an explicit user click.
#[tauri::command]
pub async fn notifications_request_access(app: AppHandle) -> Result<String, String> {
    rt::run_blocking("notifications_request_access", move || Ok(sync(&app, true).as_str().to_string())).await
}

/// Extract a SystemNotification from a Windows UserNotification.
/// Returns None if the notification has no meaningful content.
fn extract_notification(notif: &UserNotification) -> Option<SystemNotification> {
    let id = notif.Id().unwrap_or(0);

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

    if title.is_empty() && body.is_empty() {
        return None;
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
        .unwrap_or(now);

    Some(SystemNotification { id, app_name, title, body, timestamp, aumid })
}

fn poll_notifications_list(listener: &UserNotificationListener) -> Result<Vec<UserNotification>, String> {
    let op = listener
        .GetNotificationsAsync(NotificationKinds::Toast)
        .map_err(|e| format!("NOTIF-204: failed to get notifications: {e}"))?;
    let list = rt::poll_op(op, "NOTIF-204 notifications")?;
    let count = list.Size().unwrap_or(0);
    Ok((0..count).filter_map(|i| list.GetAt(i).ok()).collect())
}

/// AUMIDs are package-family names, reverse-DNS ids or `{KnownFolderGuid}\path\app.exe`
/// style ids. Anything that could change how the shell parses the argument is refused.
pub(crate) fn is_valid_aumid(aumid: &str) -> bool {
    !aumid.is_empty()
        && aumid.chars().count() <= 256
        && !aumid.contains("..")
        && aumid
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '!' | '{' | '}' | '\\' | ' ' | '(' | ')' | '+' | ','))
}

/// Launch an app through `shell:AppsFolder\<AUMID>`, the same way Action Center does.
pub(crate) fn launch_aumid(aumid: &str) -> Result<(), String> {
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
        let listener = UserNotificationListener::Current().map_err(|e| format!("NOTIF-203: {e}"))?;
        if listener.GetAccessStatus().ok() != Some(UserNotificationListenerAccessStatus::Allowed) {
            return Err("NOTIF-201: notification access not granted".to_string());
        }
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

/// Activate an app by its AUMID directly (the notification may already be gone). Only the
/// sender of a toast the island has shown can be activated.
#[tauri::command]
pub async fn activate_app_by_aumid(aumid: String) -> Result<(), String> {
    rt::run_blocking("activate_app_by_aumid", move || {
        if !lock(&RECENT_AUMIDS).contains(&aumid) {
            return Err("NOTIF-204: unknown application id".to_string());
        }
        rt::ensure_com_initialized();
        launch_aumid(&aumid)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_status_mapping() {
        use UserNotificationListenerAccessStatus as A;
        assert_eq!(classify(A::Allowed, false), Status::Allowed);
        assert_eq!(classify(A::Allowed, true), Status::Allowed);
        assert_eq!(classify(A::Denied, false), Status::Denied);
        assert_eq!(classify(A::Denied, true), Status::Policy);
        assert_eq!(classify(A::Unspecified, false), Status::Unspecified);
        assert_eq!(classify(A::Unspecified, true), Status::Unspecified);
        assert_eq!(classify(A(42), false), Status::Error);
    }

    #[test]
    fn status_strings_match_the_contract() {
        let all = [
            (Status::Allowed, "allowed", None),
            (Status::Denied, "denied", Some("NOTIF-201")),
            (Status::Unspecified, "unspecified", Some("NOTIF-202")),
            (Status::Policy, "policy", Some("NOTIF-202")),
            (Status::Unsupported, "unsupported", Some("NOTIF-203")),
            (Status::Error, "error", Some("NOTIF-204")),
        ];
        for (status, text, code) in all {
            assert_eq!(status.as_str(), text);
            assert_eq!(status.code(), code);
        }
    }

    #[test]
    fn delivery_mode_strings() {
        assert_eq!(Mode::None.as_str(), "none");
        assert_eq!(Mode::Events.as_str(), "events");
        assert_eq!(Mode::Polling.as_str(), "polling");
        assert_eq!(Delivery { token: None, events_unavailable: false, poller: None }.mode(), Mode::None);
        assert_eq!(Delivery { token: Some(1), events_unavailable: false, poller: None }.mode(), Mode::Events);
        let (stop, _stopped) = mpsc::channel();
        assert_eq!(Delivery { token: None, events_unavailable: true, poller: Some(stop) }.mode(), Mode::Polling);
    }

    #[test]
    fn first_poll_only_records_existing_items() {
        let mut seen = Seen::default();
        assert!(new_since(&mut seen, &[3, 1, 2]).is_empty());
        assert!(new_since(&mut seen, &[3, 1, 2]).is_empty());
    }

    #[test]
    fn only_unseen_ids_are_new_oldest_first_and_never_twice() {
        let mut seen = Seen::default();
        new_since(&mut seen, &[1, 2]);
        assert_eq!(new_since(&mut seen, &[5, 2, 1, 4, 4]), vec![4, 5]);
        assert!(new_since(&mut seen, &[5, 4, 2, 1]).is_empty());
        // An empty Action Center is a valid poll, not a reset.
        assert!(new_since(&mut seen, &[]).is_empty());
    }

    #[test]
    fn an_id_that_left_and_returned_is_new_again() {
        let mut seen = Seen::default();
        new_since(&mut seen, &[7]);
        assert!(new_since(&mut seen, &[]).is_empty());
        assert_eq!(new_since(&mut seen, &[7]), vec![7]);
    }

    #[test]
    fn seen_ids_are_bounded_and_old_ones_do_not_resurface() {
        let mut seen = Seen::default();
        let newest = SEEN_MAX as u32 + 50;
        let many: Vec<u32> = (1..=newest).collect();
        new_since(&mut seen, &many);
        assert_eq!(seen.ids.len(), SEEN_MAX);
        assert!(!seen.ids.contains(&1) && seen.ids.contains(&newest));
        // The same oversized list again reports nothing: evicted ids are not re-emitted.
        assert!(new_since(&mut seen, &many).is_empty());
        assert_eq!(new_since(&mut seen, &[newest + 1]), vec![newest + 1]);
    }

    #[test]
    fn poll_interval_backs_off_after_repeated_failures() {
        assert_eq!(poll_interval(0), POLL_INTERVAL);
        assert_eq!(poll_interval(POLL_BACKOFF_AFTER_FAILURES - 1), POLL_INTERVAL);
        assert_eq!(poll_interval(POLL_BACKOFF_AFTER_FAILURES), POLL_BACKOFF);
        assert_eq!(poll_interval(100), POLL_BACKOFF);
    }

    #[test]
    fn payload_is_camel_case() {
        let json = serde_json::to_value(SystemNotification {
            id: 7,
            app_name: "App".into(),
            title: "T".into(),
            body: "B".into(),
            timestamp: 1,
            aumid: None,
        })
        .unwrap();
        for key in ["id", "appName", "title", "body", "timestamp", "aumid"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn only_remembered_senders_can_be_activated_and_the_list_is_bounded() {
        let mut recent = RecentAumids::default();
        assert!(!recent.contains("MSEdge"));
        recent.remember("MSEdge");
        recent.remember("MSEdge");
        assert!(recent.contains("MSEdge"));
        assert_eq!(recent.0.len(), 1);
        for i in 0..RECENT_AUMIDS_MAX + 10 {
            recent.remember(&format!("app.{i}"));
        }
        assert_eq!(recent.0.len(), RECENT_AUMIDS_MAX);
        assert!(!recent.contains("MSEdge"), "oldest entries are evicted");
        assert!(recent.contains(&format!("app.{}", RECENT_AUMIDS_MAX + 9)));
        assert!(!recent.contains(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\cmd.exe"));
    }

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
        assert!(!is_valid_aumid(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}....WindowsSystem32lc.exe"));
        assert!(!is_valid_aumid("app\r\n"));
        assert!(!is_valid_aumid(&"a".repeat(300)));
    }
}
