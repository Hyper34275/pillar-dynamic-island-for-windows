//! Helpers for running blocking Win32/WinRT work off the UI thread.

use std::thread;
use std::time::Duration;
use windows::core::RuntimeType;
use windows::Foundation::{AsyncStatus, IAsyncOperation};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

const POLL_MAX_ITERS: usize = 400; // x POLL_SLEEP_MS = 2s budget
const POLL_SLEEP_MS: u64 = 5;

/// Run a blocking backend call on Tokio's blocking pool. A plain sync command
/// would run on the main thread, and a plain async one on one of the few async
/// workers; slow WinRT polls there would queue every other command behind them.
pub async fn run_blocking<T, F>(name: &'static str, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let started = std::time::Instant::now();
    let result = tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("APP-001: {} task failed: {}", name, e))?;
    let ms = started.elapsed().as_millis();
    if ms > 400 {
        dlog!("WARN", "cmd", "slow backend command {} took {}ms", name, ms);
    }
    result
}

/// Initialize COM (multithreaded apartment) on the current thread. Safe to call
/// repeatedly; an existing different apartment (RPC_E_CHANGED_MODE) is ignored.
pub fn ensure_com_initialized() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// Poll a WinRT async operation to completion with a 2 s budget.
pub fn poll_op<T: RuntimeType + 'static>(op: IAsyncOperation<T>, what: &str) -> Result<T, String> {
    for _ in 0..POLL_MAX_ITERS {
        let status = op.Status().map_err(|e| format!("{what}: status failed: {e}"))?;
        match status {
            AsyncStatus::Completed => return op.GetResults().map_err(|e| format!("{what}: results failed: {e}")),
            AsyncStatus::Error => return Err(format!("{what}: operation failed")),
            AsyncStatus::Canceled => return Err(format!("{what}: operation canceled")),
            _ => thread::sleep(Duration::from_millis(POLL_SLEEP_MS)),
        }
    }
    Err(format!("{what}: timed out"))
}
