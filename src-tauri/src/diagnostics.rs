//! Privacy-safe runtime diagnostics for the About tab's "Copy diagnostics".
//!
//! Contains system identifiers, statuses, counts and error codes only: never meeting
//! subjects, locations, organizers, notification text or e-mail addresses.
//!
//! Recent error codes come from a small ring buffer fed by [`record_error`]. The
//! logger calls it for every WARN/ERROR line that carries a known `PREFIX-123` code
//! (see [`code_in`]); modules that log a condition at INFO call it explicitly.

use crate::{calendar_diag, notifications, rt, system};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;

const MAX_RECENT: usize = 10;
/// Code families of docs/ENTERPRISE_DESIGN.md §3.
const CODE_PREFIXES: [&str; 5] = ["APP", "OUTLOOK", "NOTIF", "NET", "WIN"];

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecentError {
    pub code: String,
    pub unix_ms: i64,
}

static RECENT: Mutex<VecDeque<RecentError>> = Mutex::new(VecDeque::new());

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The first `PREFIX-123` error code in a log message, if any (known prefixes only, so
/// "SHA-256" or "UTF-8" never match).
pub fn code_in(message: &str) -> Option<&str> {
    message
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .find(|token| {
            token
                .split_once('-')
                .is_some_and(|(prefix, digits)| {
                    CODE_PREFIXES.contains(&prefix) && digits.len() == 3 && digits.bytes().all(|b| b.is_ascii_digit())
                })
        })
}

fn push(buffer: &mut VecDeque<RecentError>, code: &str, unix_ms: i64) {
    // A condition that repeats (retry loops) refreshes its entry instead of flooding the list.
    match buffer.back_mut() {
        Some(last) if last.code == code => last.unix_ms = unix_ms,
        _ => {
            if buffer.len() == MAX_RECENT {
                buffer.pop_front();
            }
            buffer.push_back(RecentError { code: code.to_string(), unix_ms });
        }
    }
}

pub fn record_error(code: &str) {
    push(&mut RECENT.lock().unwrap_or_else(|e| e.into_inner()), code, now_ms());
}

/// Newest first.
pub fn recent_errors() -> Vec<RecentError> {
    RECENT.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().cloned().collect()
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub windows_user: String,
    pub computer_name: String,
    pub local_ipv4: Option<String>,
    pub os_name: String,
    pub os_display_version: Option<String>,
    pub os_build: u32,
    pub app_version: String,
    pub webview2_version: Option<String>,
    /// waiting | connecting | connected | newOutlookOnly | elevationMismatch | unresponsive | failed
    pub outlook_status: String,
    /// Convenience for the UI: any status other than `waiting`.
    pub outlook_running: bool,
    /// classic | new | none
    pub outlook_mode: String,
    pub outlook_error_code: Option<String>,
    pub calendar_last_sync_unix_ms: Option<i64>,
    pub cached_events: usize,
    /// None until the first access check has run.
    pub notification_status: Option<String>,
    /// How toasts reach the island: events | polling | none.
    pub notification_mode: String,
    /// Newest first, at most 10.
    pub recent_error_codes: Vec<String>,
    pub recent_errors: Vec<RecentError>,
    pub log_dir: Option<String>,
}

pub fn collect() -> Diagnostics {
    let sys = system::info();
    let outlook = calendar_diag::get();
    let recent = recent_errors();
    Diagnostics {
        windows_user: sys.windows_user,
        computer_name: sys.computer_name,
        local_ipv4: sys.local_ipv4,
        os_name: sys.os_name,
        os_display_version: sys.os_display_version,
        os_build: sys.os_build,
        app_version: sys.app_version,
        webview2_version: sys.webview2_version,
        outlook_running: outlook.status != "waiting",
        outlook_status: outlook.status,
        outlook_mode: outlook.mode,
        outlook_error_code: outlook.error_code,
        calendar_last_sync_unix_ms: outlook.last_sync_unix_ms,
        cached_events: outlook.cached_count,
        notification_status: notifications::current_status(),
        notification_mode: notifications::current_mode().to_string(),
        recent_error_codes: recent.iter().map(|e| e.code.clone()).collect(),
        recent_errors: recent,
        log_dir: crate::paths::logs_dir().ok().map(|p| p.display().to_string()),
    }
}

#[tauri::command]
pub async fn get_diagnostics() -> Result<Diagnostics, String> {
    rt::run_blocking("get_diagnostics", || Ok(collect())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_known_codes() {
        assert_eq!(code_in("OUTLOOK-105 busy, retrying"), Some("OUTLOOK-105"));
        assert_eq!(code_in("calendar: OUTLOOK-108: reading items failed"), Some("OUTLOOK-108"));
        assert_eq!(code_in("(NOTIF-201)"), Some("NOTIF-201"));
        assert_eq!(code_in("APP-001 panic caught"), Some("APP-001"));
        assert_eq!(code_in("WIN-503 monitor enumeration failed"), Some("WIN-503"));
        assert_eq!(code_in("NET-301"), Some("NET-301"));
        // the first code wins
        assert_eq!(code_in("NET-301 after OUTLOOK-102"), Some("NET-301"));
    }

    #[test]
    fn ignores_lookalikes() {
        assert_eq!(code_in("SHA-256 digest"), None);
        assert_eq!(code_in("UTF-8 text"), None);
        assert_eq!(code_in("OUTLOOK-1020 too long"), None);
        assert_eq!(code_in("OUTLOOK-10 too short"), None);
        assert_eq!(code_in("XOUTLOOK-102"), None);
        assert_eq!(code_in("plain message"), None);
        assert_eq!(code_in(""), None);
    }

    #[test]
    fn ring_buffer_keeps_the_last_ten_and_collapses_repeats() {
        let mut buffer = VecDeque::new();
        for i in 0..15 {
            push(&mut buffer, &format!("WIN-{:03}", 500 + i), i as i64);
        }
        assert_eq!(buffer.len(), MAX_RECENT);
        assert_eq!(buffer.front().unwrap().code, "WIN-505");
        assert_eq!(buffer.back().unwrap().code, "WIN-514");

        push(&mut buffer, "WIN-514", 99);
        assert_eq!(buffer.len(), MAX_RECENT);
        assert_eq!(buffer.back().unwrap().unix_ms, 99);
    }

    #[test]
    fn serializes_camel_case_with_both_code_lists() {
        let json = serde_json::to_value(collect()).unwrap();
        for key in [
            "windowsUser",
            "computerName",
            "localIpv4",
            "osName",
            "osDisplayVersion",
            "osBuild",
            "appVersion",
            "webview2Version",
            "outlookStatus",
            "outlookRunning",
            "outlookMode",
            "outlookErrorCode",
            "calendarLastSyncUnixMs",
            "cachedEvents",
            "notificationStatus",
            "notificationMode",
            "recentErrorCodes",
            "recentErrors",
            "logDir",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        assert!(json["recentErrorCodes"].is_array());
    }
}
