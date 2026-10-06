//! Process-wide, privacy-safe summary of the Outlook connection.
//! Written by the Outlook worker, read by diagnostics. Holds no event content.

use std::sync::Mutex;

#[derive(Clone, Debug, Default)]
pub struct OutlookDiag {
    /// One of: waiting, connecting, connected, newOutlookOnly, elevationMismatch, unresponsive, failed
    pub status: String,
    /// e.g. "OUTLOOK-102"
    pub error_code: Option<String>,
    /// "classic" | "new" | "none"
    pub mode: String,
    pub last_sync_unix_ms: Option<i64>,
    pub cached_count: usize,
}

static STATE: Mutex<Option<OutlookDiag>> = Mutex::new(None);

pub fn set(diag: OutlookDiag) {
    if let Ok(mut guard) = STATE.lock() {
        *guard = Some(diag);
    }
}

pub fn get() -> OutlookDiag {
    STATE
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| OutlookDiag {
            status: "waiting".into(),
            mode: "none".into(),
            ..Default::default()
        })
}
