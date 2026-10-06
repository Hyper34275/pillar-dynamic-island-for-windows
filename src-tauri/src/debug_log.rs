//! Diagnostic log file.
//!
//! Backend and frontend (via the `write_logs` command) log to
//! `%LOCALAPPDATA%\CompanyIsland\logs\companyisland.log`. The file rotates at 1 MB
//! and the newest 5 files are kept. Logging never fails the app: if the log
//! directory cannot be used (APP-010) every call is a no-op.
//!
//! Privacy: callers must never pass meeting subjects, locations, organizers or
//! notification text. Use [`hash_id`] for identifiers. The file header carries
//! only app version, OS and a random session id.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 1024 * 1024;
const KEEP_FILES: usize = 5;
const MAX_MESSAGE_BYTES: usize = 2_000;
const MAX_PANIC_BYTES: usize = 8_000;
const MAX_SCOPE_CHARS: usize = 40;
const BASE_NAME: &str = "companyisland";
const CLEAN_EXIT_MARKER: &str = "---- clean exit ----";
const TRUNCATED: &str = "...[truncated]";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
enum Level {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl Level {
    fn parse(s: &str) -> Level {
        match s.trim().to_ascii_lowercase().as_str() {
            "debug" | "trace" => Level::Debug,
            "warn" | "warning" => Level::Warn,
            "error" | "fatal" => Level::Error,
            _ => Level::Info,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

struct Logger {
    dir: PathBuf,
    file: Option<File>,
    written: u64,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();
static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);
static ENV_DEBUG: AtomicBool = AtomicBool::new(false);
static PROFILE_DIR: OnceLock<Option<String>> = OnceLock::new();
static PREVIOUS_CLEAN: AtomicBool = AtomicBool::new(true);

/// First 8 hex chars of SHA-256, for logging identifiers without exposing them.
pub fn hash_id(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .take(4)
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn file_name(index: usize) -> String {
    if index == 0 {
        format!("{BASE_NAME}.log")
    } else {
        format!("{BASE_NAME}.{index}.log")
    }
}

/// Shift `companyisland.log` -> `.1.log` -> ... keeping `keep` files in total.
fn rotate(dir: &Path, keep: usize) -> std::io::Result<()> {
    for i in (0..keep.saturating_sub(1)).rev() {
        let from = dir.join(file_name(i));
        if from.exists() {
            let to = dir.join(file_name(i + 1));
            let _ = fs::remove_file(&to);
            fs::rename(&from, &to)?;
        }
    }
    Ok(())
}

/// True when the previous session's last line is the clean-exit marker (or there
/// was no previous log).
fn ended_cleanly(tail: &str) -> bool {
    tail.trim_end().lines().last().map_or(true, |l| l.contains(CLEAN_EXIT_MARKER))
}

fn read_tail(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(512)));
    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn truncate_utf8(s: &str, max_bytes: usize) -> Cow<'_, str> {
    if s.len() <= max_bytes {
        return Cow::Borrowed(s);
    }
    let mut end = max_bytes.saturating_sub(TRUNCATED.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    Cow::Owned(format!("{}{}", &s[..end], TRUNCATED))
}

/// Replace the user's profile path so user names never reach the log.
fn scrub_profile(message: &str) -> Cow<'_, str> {
    let profile = PROFILE_DIR.get_or_init(|| {
        std::env::var("USERPROFILE").ok().filter(|p| p.len() > 3)
    });
    match profile {
        Some(p) if message.contains(p.as_str()) => Cow::Owned(message.replace(p.as_str(), "%USERPROFILE%")),
        _ => Cow::Borrowed(message),
    }
}

fn open_current(dir: &Path) -> Option<(File, u64)> {
    let path = dir.join(file_name(0));
    let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    let written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Some((file, written))
}

fn session_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    hash_id(&format!("{}-{}", nanos, std::process::id()))
}

/// Open the log, write the session header and route panics into the log.
pub fn init(version: &str) {
    let env_debug = std::env::var("COMPANYISLAND_LOG")
        .map(|v| Level::parse(&v) == Level::Debug)
        .unwrap_or(false);
    ENV_DEBUG.store(env_debug, Ordering::Relaxed);
    set_debug(false);

    let mut previous_clean = true;
    if let Ok(dir) = crate::paths::logs_dir() {
        let current = dir.join(file_name(0));
        previous_clean = ended_cleanly(&read_tail(&current));
        if fs::metadata(&current).map(|m| m.len()).unwrap_or(0) >= MAX_BYTES {
            let _ = rotate(&dir, KEEP_FILES);
        }
        if let Some((file, written)) = open_current(&dir) {
            let _ = LOGGER.set(Mutex::new(Logger { dir, file: Some(file), written }));
        }
    }

    write(
        "INFO",
        "app",
        &format!(
            "==== CompanyIsland {} session {} ({} {}) ====",
            version,
            session_id(),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );
    PREVIOUS_CLEAN.store(previous_clean, Ordering::Relaxed);

    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".into());
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".into());
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture();
        write_capped(
            None,
            Level::Error,
            "panic",
            &format!(
                "APP-001 panic in thread '{}' at {}: {} | {}",
                thread.name().unwrap_or("unnamed"),
                location,
                message,
                backtrace
            ),
            MAX_PANIC_BYTES,
        );
    }));
}

/// Warn when the previous run ended without the clean-exit marker. Called from setup,
/// not `init`: a second launch also runs `init` before the single-instance plugin ends
/// it, and it would report the still-running first instance as a crash.
pub fn report_previous_session() {
    if !PREVIOUS_CLEAN.load(Ordering::Relaxed) {
        write("WARN", "app", "previous session did not exit cleanly");
    }
}

/// Whether a log file is open (false means APP-010).
pub fn is_active() -> bool {
    LOGGER.get().is_some()
}

/// Enable or disable debug-level logging (`COMPANYISLAND_LOG=debug` always wins).
pub fn set_debug(enabled: bool) {
    let level = if enabled || ENV_DEBUG.load(Ordering::Relaxed) { Level::Debug } else { Level::Info };
    LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Record a clean shutdown so the next start can tell it from a crash.
pub fn mark_clean_exit() {
    write_capped(None, Level::Error, "app", CLEAN_EXIT_MARKER, MAX_MESSAGE_BYTES);
}

pub fn write(level: &str, scope: &str, message: &str) {
    write_at(None, level, scope, message);
}

fn write_at(timestamp_ms: Option<i64>, level: &str, scope: &str, message: &str) {
    write_capped(timestamp_ms, Level::parse(level), scope, message, MAX_MESSAGE_BYTES);
}

fn write_capped(timestamp_ms: Option<i64>, level: Level, scope: &str, message: &str, cap: usize) {
    if (level as u8) < LEVEL.load(Ordering::Relaxed) {
        return;
    }
    let Some(lock) = LOGGER.get() else { return };
    let mut logger = lock.lock().unwrap_or_else(|e| e.into_inner());

    let ts = timestamp_ms
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|dt| dt.with_timezone(&chrono::Local))
        .unwrap_or_else(chrono::Local::now);
    // Keep each entry on one line so the log greps cleanly.
    let message = message.replace('\r', "").replace('\n', " | ");
    let message = truncate_utf8(scrub_profile(&message).as_ref(), cap).into_owned();
    let scope: String = scope.chars().take(MAX_SCOPE_CHARS).collect();
    let line = format!(
        "{} {:<5} [{}] {}\n",
        ts.format("%Y-%m-%d %H:%M:%S%.3f"),
        level.label(),
        scope,
        message
    );

    if logger.written + line.len() as u64 > MAX_BYTES {
        // Windows cannot rename an open file: close, rotate, reopen.
        logger.file = None;
        let _ = rotate(&logger.dir, KEEP_FILES);
        match open_current(&logger.dir) {
            Some((file, written)) => {
                logger.file = Some(file);
                logger.written = written;
            }
            None => logger.written = 0,
        }
    }
    let Some(file) = logger.file.as_mut() else { return };
    if file.write_all(line.as_bytes()).is_ok() {
        let _ = file.flush();
        logger.written += line.len() as u64;
    }
}

#[macro_export]
macro_rules! dlog {
    ($level:expr, $scope:expr, $($arg:tt)*) => {
        $crate::debug_log::write($level, $scope, &format!($($arg)*))
    };
}

/// Run `f`, logging (APP-001) instead of unwinding if it panics. For thread
/// entries and callbacks invoked from native code.
pub fn catch<R>(scope: &str, f: impl FnOnce() -> R) -> Option<R> {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(_) => {
            write("ERROR", scope, "APP-001 panic caught");
            None
        }
    }
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
        write_at(Some(e.ts), &e.level, &format!("ui:{}", e.scope), &e.message);
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

/// Open the log folder in Explorer (absolute explorer.exe path).
pub fn open_dir() -> Result<(), String> {
    let dir = crate::paths::logs_dir().map_err(|e| format!("APP-010: {e}"))?;
    std::process::Command::new(crate::paths::explorer_exe())
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("APP-010: cannot open log folder: {e}"))
}

#[tauri::command]
pub async fn open_log_dir() -> Result<(), String> {
    crate::rt::run_blocking("open_log_dir", open_dir).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("companyisland-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn hash_id_is_stable_8_hex() {
        let a = hash_id("meeting-entry-id");
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, hash_id("meeting-entry-id"));
        assert_ne!(a, hash_id("other"));
    }

    #[test]
    fn level_parsing_defaults_to_info() {
        assert_eq!(Level::parse("DEBUG"), Level::Debug);
        assert_eq!(Level::parse("warning"), Level::Warn);
        assert_eq!(Level::parse("error"), Level::Error);
        assert_eq!(Level::parse("whatever"), Level::Info);
        assert!(Level::Debug < Level::Info && Level::Info < Level::Warn && Level::Warn < Level::Error);
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        assert_eq!(truncate_utf8("short", 100), "short");
        let long = "é".repeat(100);
        let cut = truncate_utf8(&long, 40);
        assert!(cut.len() <= 40);
        assert!(cut.ends_with(TRUNCATED));
    }

    #[test]
    fn clean_exit_detection() {
        assert!(ended_cleanly(""));
        assert!(ended_cleanly(&format!("a line\n2026 INFO [app] {CLEAN_EXIT_MARKER}\n")));
        assert!(!ended_cleanly("2026 INFO [app] something\n"));
    }

    #[test]
    fn rotation_keeps_five_files() {
        let dir = scratch_dir("rotate");
        for round in 0..8 {
            fs::write(dir.join(file_name(0)), format!("round {round}")).unwrap();
            rotate(&dir, KEEP_FILES).unwrap();
        }
        // After the last rotation the current file is gone and 4 history files remain
        // (the 5th slot is the fresh current file the logger would open next).
        assert!(!dir.join(file_name(0)).exists());
        for i in 1..KEEP_FILES {
            assert!(dir.join(file_name(i)).exists(), "missing {}", file_name(i));
        }
        assert!(!dir.join(file_name(KEEP_FILES)).exists());
        assert_eq!(fs::read_to_string(dir.join(file_name(1))).unwrap(), "round 7");
        assert_eq!(fs::read_to_string(dir.join(file_name(4))).unwrap(), "round 4");
        let _ = fs::remove_dir_all(&dir);
    }
}
