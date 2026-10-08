//! Opening the Island Center (the WinUI 3 app installed in `<install dir>\center\`).
//!
//! `open` is only ever called after a user click (an island button, the tray menu) or on the
//! very first run. When a Center is already connected on the pipe it is sent a `navigate`
//! event; otherwise the exe is started with `--page <page>`. Either way the island hands the
//! foreground right to that one Center process (`AllowSetForegroundWindow` with its pid, never
//! `ASFW_ANY`), so the window can come forward without the island ever taking focus itself.

use crate::{center_ipc, notes, rt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tauri::AppHandle;
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

const CENTER_DIR: &str = "center";
const CENTER_EXE: &str = "CompanyIsland.Center.exe";
/// Debug builds only: absolute path of a Center exe built elsewhere.
const EXE_OVERRIDE_ENV: &str = "COMPANYISLAND_CENTER_EXE";

/// `welcome`, `tour`, `settings`, `notes`, `notes-new`, `search`, `note:<id>` (a valid note id)
/// or `search:<id>` (a valid query id, the same rule as note ids).
pub fn valid_page(page: &str) -> bool {
    match page {
        "welcome" | "tour" | "settings" | "notes" | "notes-new" | "search" => true,
        _ => page
            .strip_prefix("note:")
            .or_else(|| page.strip_prefix("search:"))
            .is_some_and(notes::valid_id),
    }
}

/// For logs: the page without a note or query id.
fn page_kind(page: &str) -> &str {
    if page.starts_with("note:") {
        "note"
    } else if page.starts_with("search:") {
        "search"
    } else {
        page
    }
}

/// `<dir of the island exe>\center\CompanyIsland.Center.exe`, or the override when one is
/// allowed (debug builds) and absolute.
fn exe_path(island_exe: &Path, override_path: Option<PathBuf>, allow_override: bool) -> Option<PathBuf> {
    if allow_override {
        if let Some(path) = override_path.filter(|p| p.is_absolute()) {
            return Some(path);
        }
    }
    island_exe.parent().map(|dir| dir.join(CENTER_DIR).join(CENTER_EXE))
}

fn center_exe() -> Option<PathBuf> {
    let island = std::env::current_exe().ok()?;
    let override_path = std::env::var_os(EXE_OVERRIDE_ENV).map(PathBuf::from);
    exe_path(&island, override_path, cfg!(debug_assertions))
}

fn allow_foreground(pid: u32) {
    // Best effort: without it the Center still opens, it just may start behind other windows.
    if let Err(e) = unsafe { AllowSetForegroundWindow(pid) } {
        dlog!("DEBUG", "center", "AllowSetForegroundWindow failed: {}", e);
    }
}

fn unavailable() -> String {
    "APP-030: island center unavailable".to_string()
}

/// How long a started Center gets to connect to the island before the next click is told it failed.
const CONNECT_GRACE_MS: u64 = 8_000;

/// What a click may do when no Center is connected.
#[derive(Debug, PartialEq, Eq)]
enum SpawnDecision {
    /// Nothing was started recently: start the Center.
    Go,
    /// A Center was started a moment ago and has not connected yet: do not start another one.
    Pending,
    /// The Center started `pid` over the grace ago and never connected. Reported once; the next click starts again.
    Failed(u32),
}

/// Spawn-storm guard (pure: the caller passes the clock, in milliseconds since any fixed point).
#[derive(Debug, Default)]
struct SpawnGuard {
    started: Option<(u32, u64)>,
}

impl SpawnGuard {
    fn check(&mut self, now_ms: u64) -> SpawnDecision {
        match self.started {
            None => SpawnDecision::Go,
            Some((_, at)) if now_ms.saturating_sub(at) < CONNECT_GRACE_MS => SpawnDecision::Pending,
            Some((pid, _)) => {
                self.started = None;
                SpawnDecision::Failed(pid)
            }
        }
    }

    fn spawned(&mut self, pid: u32, now_ms: u64) {
        self.started = Some((pid, now_ms));
    }

    fn connected(&mut self) {
        self.started = None;
    }
}

static GUARD: Mutex<SpawnGuard> = Mutex::new(SpawnGuard { started: None });
static CLOCK: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn guard() -> std::sync::MutexGuard<'static, SpawnGuard> {
    GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A Center connected to the island's pipe (called by `center_ipc`): the start worked.
pub fn note_connected() {
    guard().connected();
}

const NOT_OPENED: &str = "APP-033: island center did not open";

/// The island's page only sees `false` from a failed open, so the failure is also shown as a plain message box
/// (own thread: it blocks until dismissed).
fn tell_user_not_opened() {
    use windows::core::w;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONWARNING, MB_OK, MB_RIGHT, MB_RTLREADING, MB_SETFOREGROUND, MB_TOPMOST,
    };
    let _ = std::thread::Builder::new().name("companyisland-center-failed".into()).spawn(|| unsafe {
        MessageBoxW(
            HWND::default(),
            w!("מרכז האי לא נפתח. פרטים בקובץ center.log"),
            w!("CompanyIsland"),
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST | MB_RTLREADING | MB_RIGHT,
        );
    });
}

fn spawn(page: &str) -> Result<u32, String> {
    let Some(exe) = center_exe().filter(|p| p.is_file()) else {
        dlog!("WARN", "center", "APP-030 center app not found next to the island");
        return Err(unavailable());
    };
    let mut command = std::process::Command::new(&exe);
    command.arg("--page").arg(page);
    if let Some(dir) = exe.parent() {
        command.current_dir(dir);
    }
    // Not waited on: the Center lives its own life, and its handle is closed right away.
    let child = command.spawn().map_err(|e| {
        dlog!("WARN", "center", "APP-030 center app could not start: {}", e);
        unavailable()
    })?;
    Ok(child.id())
}

/// Show the Center on `page`.
pub fn open(_app: &AppHandle, page: &str) -> Result<(), String> {
    if !valid_page(page) {
        return Err("APP-032: unknown center page".to_string());
    }
    if let Some(pid) = center_ipc::send_navigate(page, allow_foreground) {
        guard().connected();
        dlog!("INFO", "center", "navigate {} sent to center pid {}", page_kind(page), pid);
        return Ok(());
    }
    match guard().check(now_ms()) {
        SpawnDecision::Go => {}
        SpawnDecision::Pending => {
            dlog!("DEBUG", "center", "a center was just started and has not connected yet, not starting another");
            return Ok(());
        }
        SpawnDecision::Failed(pid) => {
            dlog!("WARN", "center", "APP-030 center started but did not connect (pid {})", pid);
            tell_user_not_opened();
            return Err(NOT_OPENED.to_string());
        }
    }
    let pid = spawn(page)?;
    guard().spawned(pid, now_ms());
    allow_foreground(pid);
    dlog!("INFO", "center", "started center pid {} on {}", pid, page_kind(page));
    Ok(())
}

#[tauri::command]
pub async fn open_center(app: AppHandle, page: String) -> Result<(), String> {
    rt::run_blocking("open_center", move || open(&app, &page)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_starts_once_then_waits_for_the_connection() {
        let mut g = SpawnGuard::default();
        assert_eq!(g.check(0), SpawnDecision::Go);
        g.spawned(41, 1_000);
        for t in [1_000, 1_200, 5_000, 8_999] {
            assert_eq!(g.check(t), SpawnDecision::Pending, "{t}");
        }
    }

    #[test]
    fn a_center_that_never_connected_is_reported_once_and_the_next_click_retries() {
        let mut g = SpawnGuard::default();
        g.spawned(41, 1_000);
        assert_eq!(g.check(9_000), SpawnDecision::Failed(41));
        assert_eq!(g.check(9_001), SpawnDecision::Go, "reported once, then a new try is allowed");
        g.spawned(42, 9_500);
        assert_eq!(g.check(10_000), SpawnDecision::Pending);
        assert_eq!(g.check(17_500), SpawnDecision::Failed(42));
    }

    #[test]
    fn a_connection_clears_the_guard() {
        let mut g = SpawnGuard::default();
        g.spawned(41, 1_000);
        g.connected();
        assert_eq!(g.check(20_000), SpawnDecision::Go, "a Center that connected and later closed is not a failure");
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_panic() {
        let mut g = SpawnGuard::default();
        g.spawned(1, 5_000);
        assert_eq!(g.check(100), SpawnDecision::Pending);
    }

    #[test]
    fn page_names() {
        for page in [
            "welcome",
            "tour",
            "settings",
            "notes",
            "notes-new",
            "search",
            "note:0123456789abcdef",
            "note:A_b-9",
            "search:q1-2_X",
        ] {
            assert!(valid_page(page), "{page}");
        }
        for page in [
            "",
            "Welcome",
            "about",
            "note",
            "note:",
            "note:has space",
            "note:a/b",
            "note:a:b",
            "notes-new ",
            "../tour",
            "welcome --x",
            "search:",
            "search:has space",
            "search:a/b",
            "search:a:b",
            "search ",
            "Search",
        ] {
            assert!(!valid_page(page), "{page}");
        }
        assert!(!valid_page(&format!("note:{}", "a".repeat(65))));
        assert!(valid_page(&format!("note:{}", "a".repeat(64))));
        assert!(!valid_page(&format!("search:{}", "a".repeat(65))));
        assert!(valid_page(&format!("search:{}", "a".repeat(64))));
    }

    #[test]
    fn logs_never_carry_a_note_id() {
        assert_eq!(page_kind("note:0123456789abcdef"), "note");
        assert_eq!(page_kind("search:0123456789abcdef"), "search");
        assert_eq!(page_kind("search"), "search");
        assert_eq!(page_kind("settings"), "settings");
    }

    #[test]
    fn exe_is_next_to_the_island_in_center() {
        let island = Path::new(r"C:\Program Files\CompanyIsland\CompanyIsland.exe");
        assert_eq!(
            exe_path(island, None, true),
            Some(PathBuf::from(r"C:\Program Files\CompanyIsland\center\CompanyIsland.Center.exe"))
        );
    }

    #[test]
    fn the_override_only_works_when_allowed_and_absolute() {
        let island = Path::new(r"C:\Apps\CompanyIsland.exe");
        let normal = Some(PathBuf::from(r"C:\Apps\center\CompanyIsland.Center.exe"));
        let custom = PathBuf::from(r"D:\build\Center.exe");
        assert_eq!(exe_path(island, Some(custom.clone()), true), Some(custom.clone()));
        assert_eq!(exe_path(island, Some(custom), false), normal, "release builds ignore it");
        assert_eq!(exe_path(island, Some(PathBuf::from(r"relative\Center.exe")), true), normal);
        assert_eq!(exe_path(island, Some(PathBuf::new()), true), normal);
    }

    #[test]
    fn an_exe_without_a_folder_has_no_center_path() {
        assert_eq!(exe_path(Path::new(""), None, false), None);
    }
}
