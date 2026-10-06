//! Debug log file for diagnosing issues on real machines.
//!
//! Everything (backend + frontend, via the `write_logs` command) goes to
//! `%LOCALAPPDATA%\PILLAR\logs\pillar.log`. The file rotates to `pillar.1.log`
//! once it passes MAX_BYTES so it never grows unbounded.

use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_BYTES: u64 = 5 * 1024 * 1024;
const FILE_NAME: &str = "pillar.log";
const ROTATED_NAME: &str = "pillar.1.log";

struct Logger {
    file: File,
    written: u64,
}

static LOGGER: OnceCell<Mutex<Logger>> = OnceCell::new();

pub fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("PILLAR")
        .join("logs")
}

fn open_log_file() -> Option<(File, u64)> {
    let dir = log_dir();
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(FILE_NAME);
    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > MAX_BYTES {
        let _ = fs::rename(&path, dir.join(ROTATED_NAME));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    let written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Some((file, written))
}

/// Open the log file, write a session header and route panics into the log.
pub fn init(version: &str) {
    if let Some((file, written)) = open_log_file() {
        let _ = LOGGER.set(Mutex::new(Logger { file, written }));
    }
    write(
        "INFO",
        "app",
        &format!(
            "==================== PILLAR {} started (pid {}, {} {}) ====================",
            version,
            std::process::id(),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write("ERROR", "panic", &info.to_string());
        default_hook(info);
    }));
}

pub fn write(level: &str, scope: &str, message: &str) {
    write_at(None, level, scope, message);
}

fn write_at(timestamp_ms: Option<i64>, level: &str, scope: &str, message: &str) {
    let Some(lock) = LOGGER.get() else { return };
    let Ok(mut logger) = lock.lock() else { return };

    let ts = timestamp_ms
        .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms))
        .map(|dt| dt.with_timezone(&chrono::Local))
        .unwrap_or_else(chrono::Local::now);
    // Keep each entry on one line so the log greps cleanly.
    let message = message.replace('\r', "").replace('\n', " ⏎ ");
    let line = format!(
        "{} {:<5} [{}] {}\n",
        ts.format("%Y-%m-%d %H:%M:%S%.3f"),
        level,
        scope,
        message
    );

    if logger.file.write_all(line.as_bytes()).is_ok() {
        logger.written += line.len() as u64;
    }
    let _ = logger.file.flush();

    if logger.written > MAX_BYTES {
        drop(logger);
        if let Some((file, written)) = open_log_file() {
            if let Ok(mut l) = lock.lock() {
                l.file = file;
                l.written = written;
            }
        }
    }
}

#[macro_export]
macro_rules! dlog {
    ($level:expr, $scope:expr, $($arg:tt)*) => {
        $crate::debug_log::write($level, $scope, &format!($($arg)*))
    };
}

// =============================================================================
// Commands
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct FrontendLogEntry {
    pub ts: i64,
    pub level: String,
    pub scope: String,
    pub message: String,
}

/// Batched log lines from the webview.
#[tauri::command]
pub async fn write_logs(entries: Vec<FrontendLogEntry>) -> Result<(), String> {
    for e in entries.iter().take(500) {
        write_at(Some(e.ts), &e.level.to_uppercase(), &format!("ui:{}", e.scope), &e.message);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct FrontendErrorPayload {
    pub scope: String,
    pub message: String,
    pub detail: Option<String>,
    pub timestamp: i64,
}

/// Target of `src/lib/logger.ts` error forwarding.
#[tauri::command]
pub async fn log_frontend_error(payload: FrontendErrorPayload) -> Result<(), String> {
    let msg = match payload.detail {
        Some(d) if !d.is_empty() => format!("{} | {}", payload.message, d),
        _ => payload.message,
    };
    write_at(Some(payload.timestamp), "ERROR", &format!("ui:{}", payload.scope), &msg);
    Ok(())
}

#[tauri::command]
pub fn get_log_dir() -> String {
    log_dir().to_string_lossy().to_string()
}

#[tauri::command]
pub fn open_log_dir() -> Result<(), String> {
    let dir = log_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
