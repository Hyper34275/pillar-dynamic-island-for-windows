//! Event-driven fullscreen/presentation detection (no polling).
//!
//! A dedicated thread owns a message loop and three WinEvent hooks (`WINEVENT_OUTOFCONTEXT`,
//! so nothing is injected into other processes and no admin rights are needed):
//! foreground change, minimize start/end, and location changes of the foreground
//! window. Bursts are debounced with a thread timer. Each evaluation combines the
//! shell's `SHQueryUserNotificationState` with a geometry heuristic ([`decide`]).
//!
//! On a transition `fullscreen-changed` (bool) is emitted and, when
//! `settings.hideInFullscreen` is on, the native window is hidden and later restored.
//! The window is only restored if this module hid it.

use crate::{debug_log, monitors, settings::SettingsStore, window};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsZoomed, KillTimer, PeekMessageW, PostThreadMessageW, SetTimer, TranslateMessage,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART,
    GWL_STYLE, MSG, OBJID_WINDOW, PM_NOREMOVE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_USER,
    WS_CAPTION, WS_POPUP,
};

const DEBOUNCE_MS: u32 = 250;
const WM_REEVALUATE: u32 = WM_APP + 1;

/// Shell surfaces are full-screen WS_POPUP windows but never "content": the desktop
/// (Progman/WorkerW, i.e. clicking the desktop or Win+D), the taskbar, Alt-Tab, Task
/// View and Start/Search (CoreWindow hosts such as ShellExperienceHost). Treating them
/// as fullscreen slid the island away while its window still swallowed clicks.
const SHELL_CLASSES: [&str; 9] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "XamlExplorerHostIslandWindow",
    "MultitaskingViewFrame",
    "ForegroundStaging",
    "Windows.UI.Core.CoreWindow",
    "TopLevelWindowForOverflowXamlIsland",
];

static APP: OnceLock<AppHandle> = OnceLock::new();
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
/// Last published state; `None` until the first evaluation.
static STATE: Mutex<Option<bool>> = Mutex::new(None);
static HIDDEN_BY_US: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TIMER: Cell<usize> = const { Cell::new(0) };
    static LOCATION_HOOK: Cell<isize> = const { Cell::new(0) };
}

// =============================================================================
// Pure decision logic
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    /// Nothing noteworthy (also: lock screen, screensaver, quiet hours, ...).
    Other,
    /// A full-screen application or Presentation Settings (`QUNS_BUSY`).
    Busy,
    /// Exclusive-mode Direct3D application (`QUNS_RUNNING_D3D_FULL_SCREEN`).
    D3dFullScreen,
    /// Presentation mode (`QUNS_PRESENTATION_MODE`).
    PresentationMode,
}

impl From<QUERY_USER_NOTIFICATION_STATE> for Shell {
    fn from(state: QUERY_USER_NOTIFICATION_STATE) -> Self {
        match state {
            QUNS_BUSY => Shell::Busy,
            QUNS_RUNNING_D3D_FULL_SCREEN => Shell::D3dFullScreen,
            QUNS_PRESENTATION_MODE => Shell::PresentationMode,
            _ => Shell::Other,
        }
    }
}

/// Facts about the foreground window, gathered by [`foreground_info`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowInfo {
    pub own_process: bool,
    pub shell_surface: bool,
    /// On the monitor the island lives on.
    pub on_target_monitor: bool,
    /// Its rectangle covers its whole monitor.
    pub covers_monitor: bool,
    pub maximized: bool,
    pub popup: bool,
    pub has_caption: bool,
}

/// Fullscreen content = the shell says so, or the foreground window fills the island's
/// monitor like a video/game/F11 window does.
///
/// Exclusive D3D and presentation mode are taken at the shell's word. For `Busy` the
/// shell has already confirmed a fullscreen app, so only the geometry must agree (the
/// maximized/style checks are skipped). Without a shell signal, maximized windows
/// (custom-chrome apps like Spotify) and ordinary captioned windows do not count.
pub fn decide(shell: Shell, window: Option<&WindowInfo>) -> bool {
    match shell {
        Shell::D3dFullScreen | Shell::PresentationMode => true,
        Shell::Busy => window.is_some_and(candidate),
        Shell::Other => window.is_some_and(|w| candidate(w) && !w.maximized && (w.popup || !w.has_caption)),
    }
}

fn candidate(w: &WindowInfo) -> bool {
    !w.own_process && !w.shell_surface && w.on_target_monitor && w.covers_monitor
}

// =============================================================================
// Win32 gathering
// =============================================================================

fn foreground_info(hwnd: HWND, target: Option<&monitors::Monitor>) -> Option<WindowInfo> {
    unsafe {
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        let mut class = [0u16; 128];
        let len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..len]);

        let mut rect = windows::Win32::Foundation::RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if monitor.is_invalid() || !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let monitor_bounds = monitors::Bounds::from(info.rcMonitor);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;

        Some(WindowInfo {
            own_process: pid == std::process::id(),
            shell_surface: SHELL_CLASSES.contains(&class.as_str()),
            on_target_monitor: target.is_some_and(|t| t.bounds == monitor_bounds),
            covers_monitor: monitors::Bounds::from(rect).contains(&monitor_bounds),
            maximized: IsZoomed(hwnd).as_bool(),
            popup: style & WS_POPUP.0 != 0,
            has_caption: style & WS_CAPTION.0 == WS_CAPTION.0,
        })
    }
}

fn evaluate_now(app: &AppHandle) -> bool {
    let shell = unsafe { SHQueryUserNotificationState() }.map(Shell::from).unwrap_or(Shell::Other);
    let setting = app.state::<SettingsStore>().get().monitor;
    let monitors = monitors::list();
    let target = monitors::pick(&setting, &monitors);
    let info = foreground_info(unsafe { GetForegroundWindow() }, target);
    decide(shell, info.as_ref())
}

/// The last published state, for a page that attached its listener after the event fired
/// (a fullscreen app already in front when CompanyIsland starts at logon).
#[tauri::command]
pub fn get_fullscreen_state() -> bool {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or(false)
}

/// Re-evaluate, publish a transition and apply the hide/restore policy (idempotent).
fn evaluate(app: &AppHandle) {
    let fullscreen = evaluate_now(app);

    let changed = {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let changed = *state != Some(fullscreen);
        *state = Some(fullscreen);
        changed
    };
    if changed {
        dlog!("INFO", "fullscreen", "fullscreen state is now {}", fullscreen);
        if let Err(e) = app.emit("fullscreen-changed", fullscreen) {
            dlog!("WARN", "fullscreen", "emit fullscreen-changed failed: {}", e);
        }
    }

    let hide = fullscreen && app.state::<SettingsStore>().get().hide_in_fullscreen;
    if hide {
        if window::is_visible(app) && !HIDDEN_BY_US.swap(true, Ordering::Relaxed) {
            window::hide(app);
        }
    } else if HIDDEN_BY_US.swap(false, Ordering::Relaxed) {
        window::show(app);
    }
}

// =============================================================================
// Hook thread
// =============================================================================

fn schedule() {
    TIMER.with(|timer| {
        // With a NULL hwnd, passing the previous id restarts that timer: this is the debounce.
        let id = unsafe { SetTimer(None, timer.get(), DEBOUNCE_MS, Some(on_timer)) };
        timer.set(id);
    });
}

unsafe extern "system" fn on_timer(_hwnd: HWND, _msg: u32, id: usize, _time: u32) {
    let _ = KillTimer(None, id);
    TIMER.with(|timer| timer.set(0));
    debug_log::catch("fullscreen", || {
        if let Some(app) = APP.get() {
            evaluate(app);
        }
    });
}

/// Location changes are only subscribed for the foreground window's own thread, so
/// the system-wide stream (every window move, cursor updates) is filtered in the kernel.
fn retarget_location_hook(foreground: HWND) {
    LOCATION_HOOK.with(|slot| unsafe {
        let old = HWINEVENTHOOK(slot.get() as _);
        if !old.0.is_null() {
            let _ = UnhookWinEvent(old);
            slot.set(0);
        }
        if foreground.0.is_null() {
            return;
        }
        let mut pid = 0u32;
        let tid = GetWindowThreadProcessId(foreground, Some(&mut pid as *mut u32));
        if tid == 0 || pid == std::process::id() {
            return;
        }
        let hook = SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(win_event_proc),
            pid,
            tid,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        slot.set(hook.0 as isize);
    });
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    debug_log::catch("fullscreen", || match event {
        EVENT_SYSTEM_FOREGROUND => {
            retarget_location_hook(hwnd);
            schedule();
        }
        EVENT_SYSTEM_MINIMIZESTART | EVENT_SYSTEM_MINIMIZEEND => schedule(),
        EVENT_OBJECT_LOCATIONCHANGE if id_object == OBJID_WINDOW.0 && hwnd == GetForegroundWindow() => schedule(),
        _ => {}
    });
}

fn run(app: &AppHandle) {
    unsafe {
        let mut msg = MSG::default();
        // Create this thread's message queue before publishing its id for PostThreadMessage.
        let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
        THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);

        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let foreground = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event_proc),
            0,
            0,
            flags,
        );
        let minimize = SetWinEventHook(
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            None,
            Some(win_event_proc),
            0,
            0,
            flags,
        );
        if foreground.0.is_null() || minimize.0.is_null() {
            dlog!("WARN", "fullscreen", "WIN-501 WinEvent hooks unavailable; fullscreen detection disabled");
            return;
        }

        retarget_location_hook(GetForegroundWindow());
        evaluate(app);

        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.hwnd.0.is_null() && msg.message == WM_REEVALUATE {
                evaluate(app);
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Start detection; emits the initial state once the hooks are installed.
pub fn start(app: AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("companyisland-fullscreen".into()).spawn(move || {
        debug_log::catch("fullscreen", || run(&app));
    });
    if let Err(e) = spawned {
        dlog!("WARN", "fullscreen", "WIN-501 could not start fullscreen thread: {}", e);
    }
}

/// Ask the hook thread to re-evaluate now (setting or target monitor changed).
pub fn reevaluate() {
    let thread = THREAD_ID.load(Ordering::Acquire);
    if thread != 0 {
        unsafe {
            let _ = PostThreadMessageW(thread, WM_REEVALUATE, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A video player / game window: borderless popup covering the island's monitor.
    fn fullscreen_window() -> WindowInfo {
        WindowInfo { on_target_monitor: true, covers_monitor: true, popup: true, ..Default::default() }
    }

    #[test]
    fn plain_borderless_window_is_fullscreen() {
        assert!(decide(Shell::Other, Some(&fullscreen_window())));
    }

    #[test]
    fn nothing_in_the_foreground_is_not_fullscreen() {
        assert!(!decide(Shell::Other, None));
        assert!(!decide(Shell::Busy, None));
    }

    #[test]
    fn exclusions_apply_without_a_shell_signal() {
        let base = fullscreen_window();
        let cases = [
            ("own process", WindowInfo { own_process: true, ..base }),
            ("shell surface", WindowInfo { shell_surface: true, ..base }),
            ("other monitor", WindowInfo { on_target_monitor: false, ..base }),
            ("does not cover monitor", WindowInfo { covers_monitor: false, ..base }),
            ("maximized", WindowInfo { maximized: true, ..base }),
            ("captioned, not popup", WindowInfo { popup: false, has_caption: true, ..base }),
        ];
        for (name, info) in cases {
            assert!(!decide(Shell::Other, Some(&info)), "{name}");
        }
        // F11 browser: no popup bit but also no caption
        assert!(decide(Shell::Other, Some(&WindowInfo { popup: false, has_caption: false, ..base })));
    }

    #[test]
    fn busy_confirms_but_still_needs_geometry_and_exclusions() {
        let maximized_chrome = WindowInfo { maximized: true, popup: false, has_caption: true, ..fullscreen_window() };
        assert!(decide(Shell::Busy, Some(&maximized_chrome)));
        assert!(!decide(Shell::Busy, Some(&WindowInfo { own_process: true, ..maximized_chrome })));
        assert!(!decide(Shell::Busy, Some(&WindowInfo { shell_surface: true, ..maximized_chrome })));
        assert!(!decide(Shell::Busy, Some(&WindowInfo { on_target_monitor: false, ..maximized_chrome })));
        assert!(!decide(Shell::Busy, Some(&WindowInfo { covers_monitor: false, ..maximized_chrome })));
    }

    #[test]
    fn d3d_and_presentation_mode_are_trusted() {
        assert!(decide(Shell::D3dFullScreen, None));
        assert!(decide(Shell::PresentationMode, None));
        assert!(decide(Shell::D3dFullScreen, Some(&WindowInfo { own_process: true, ..Default::default() })));
    }

    #[test]
    fn shell_state_mapping() {
        assert_eq!(Shell::from(QUNS_BUSY), Shell::Busy);
        assert_eq!(Shell::from(QUNS_RUNNING_D3D_FULL_SCREEN), Shell::D3dFullScreen);
        assert_eq!(Shell::from(QUNS_PRESENTATION_MODE), Shell::PresentationMode);
        assert_eq!(Shell::from(windows::Win32::UI::Shell::QUNS_ACCEPTS_NOTIFICATIONS), Shell::Other);
        assert_eq!(Shell::from(windows::Win32::UI::Shell::QUNS_NOT_PRESENT), Shell::Other);
    }

    #[test]
    fn live_probes_do_not_panic() {
        let monitors = monitors::list();
        let target = monitors::pick("primary", &monitors);
        let _ = foreground_info(unsafe { GetForegroundWindow() }, target);
        assert!(unsafe { SHQueryUserNotificationState() }.is_ok());
        assert!(foreground_info(HWND::default(), target).is_none());
    }
}
