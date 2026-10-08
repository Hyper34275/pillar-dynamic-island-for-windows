//! The search-bar thread: one message loop that owns the AI button, the hotkey and the hooks.
//!
//! - A hidden top-level window (NOT message-only: message-only windows do not receive the
//!   `TaskbarCreated` and settings broadcasts) gets `WM_HOTKEY`, `TaskbarCreated`, display, DPI and
//!   settings messages.
//! - Out-of-context WinEvent hooks (`WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS`: nothing is
//!   injected into explorer.exe) report the taskbar thread's show/hide/reorder/location events and
//!   foreground changes. Each only arms a ~50 ms throttle timer; the re-measure happens once.
//! - After an Explorer restart (`TaskbarCreated`) the hooks are rebuilt with a backoff.
//! - `HWND_TOPMOST` is re-asserted only when the taskbar really sits above the button, and at most
//!   every 250 ms, so two topmost windows never fight in a loop.

use super::anchor::{
    class_of, classify_shell_surface, foreground_covers_button, gather, image_of, Foreground, ShellProbe, ShellSurface,
    Win32Probe,
};
use super::button::Button;
use super::guard::{guarded, Decision, FileMarker, StartupGuard, CONFIRM_AFTER, START_DELAY};
use super::layout::{button_visible, compute_layout};
use super::raise::RaiseSchedule;
use super::{hotkey, window};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetAncestor, GetMessageW, GetWindow, GetWindowThreadProcessId,
    KillTimer, PostMessageW, RegisterClassExW, RegisterWindowMessageW, SetTimer, TranslateMessage, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_OBJECT_REORDER, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND, GA_ROOT, GW_HWNDPREV, MSG, OBJID_WINDOW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_POWERBROADCAST,
    WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};

const CLASS: PCWSTR = w!("CompanyIslandSearchBarHost");
pub const WM_CMD: u32 = WM_APP + 10;
pub const CMD_RECONCILE: usize = 1;
pub const CMD_ACTIVE: usize = 2;
pub const CMD_IDLE_ARM: usize = 3;
pub const CMD_IDLE_CANCEL: usize = 4;
pub const CMD_MEASURE: usize = 5;

const T_MEASURE: usize = 1;
const T_REHOOK: usize = 2;
const T_IDLE: usize = 3;
/// Throttle between a shell event and the re-measure.
const MEASURE_MS: u32 = 50;
/// Minimum time between two topmost re-assertions.
const RAISE_EVERY: Duration = Duration::from_millis(250);
/// The hidden search window is destroyed after this long (saves the WebView2 processes).
pub const IDLE_DESTROY_MS: u32 = 10 * 60 * 1000;
const REHOOK_FIRST_MS: u32 = 1500;
const REHOOK_MAX_ATTEMPTS: u32 = 6;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

static HOST: AtomicIsize = AtomicIsize::new(0);
static TRAY: AtomicIsize = AtomicIsize::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);
static BUTTON: AtomicBool = AtomicBool::new(false);
static HOTKEY: AtomicBool = AtomicBool::new(false);
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static MEASURE_PENDING: AtomicBool = AtomicBool::new(false);
static NEED_RAISE: AtomicBool = AtomicBool::new(false);
/// A shell surface (taskbar, Start, search, shell host) came to the front: run a raise burst.
static BURST_REQUEST: AtomicBool = AtomicBool::new(false);
/// The button and the hooks start only a few seconds after the app (see `guard`), and not at all in
/// a session that follows a start that died while they were starting.
static BUTTON_ALLOWED: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct Ctx {
    probe: Win32Probe,
    button: Option<Button>,
    hooks: Vec<isize>,
    hotkey_on: bool,
    spotlight_on: bool,
    rehook_attempt: u32,
    last_raise: Instant,
    sched: RaiseSchedule,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

pub fn set_flags(enabled: bool, button: bool, hotkey: bool) {
    ENABLED.store(enabled, Ordering::Release);
    BUTTON.store(button, Ordering::Release);
    HOTKEY.store(hotkey, Ordering::Release);
}

pub fn set_fullscreen(on: bool) {
    FULLSCREEN.store(on, Ordering::Release);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

fn host() -> HWND {
    HWND(HOST.load(Ordering::Acquire) as _)
}

/// Ask the thread to do something (any thread). Ignored before the thread is up; the thread
/// reconciles with the current flags when it starts.
pub fn post(cmd: usize) {
    let hwnd = host();
    if !hwnd.0.is_null() {
        unsafe {
            let _ = PostMessageW(hwnd, WM_CMD, WPARAM(cmd), LPARAM(0));
        }
    }
}

/// Arm the throttle timer: the first event of a burst starts it, the rest are absorbed.
fn schedule() {
    if !MEASURE_PENDING.swap(true, Ordering::AcqRel) {
        unsafe {
            SetTimer(host(), T_MEASURE, MEASURE_MS, None);
        }
    }
}

/// Ask for a raise burst and measure almost at once (the burst itself runs in `measure`).
fn request_burst() {
    BURST_REQUEST.store(true, Ordering::Release);
    MEASURE_PENDING.store(true, Ordering::Release);
    unsafe {
        SetTimer(host(), T_MEASURE, 10, None);
    }
}

fn belongs_to_tray(hwnd: HWND) -> bool {
    let tray = HWND(TRAY.load(Ordering::Acquire) as _);
    !tray.0.is_null() && (hwnd == tray || unsafe { GetAncestor(hwnd, GA_ROOT) } == tray)
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    guarded("winevent hook", (), || match event {
        EVENT_SYSTEM_FOREGROUND | EVENT_OBJECT_REORDER => {
            NEED_RAISE.store(true, Ordering::Release);
            schedule();
            let shell_surface = if event == EVENT_SYSTEM_FOREGROUND {
                let class = class_of(hwnd);
                let image = if class == "Windows.UI.Core.CoreWindow" { image_of(hwnd) } else { String::new() };
                classify_shell_surface(&class, &image) != ShellSurface::Other
            } else {
                belongs_to_tray(hwnd)
            };
            if shell_surface {
                request_burst();
            }
        }
        EVENT_OBJECT_LOCATIONCHANGE if id_object == OBJID_WINDOW.0 && belongs_to_tray(hwnd) => schedule(),
        // SHOW and HIDE
        _ if id_object == OBJID_WINDOW.0 && belongs_to_tray(hwnd) => {
            NEED_RAISE.store(true, Ordering::Release);
            schedule();
        }
        _ => {}
    });
}

fn uninstall_hooks(ctx: &mut Ctx) {
    for h in ctx.hooks.drain(..) {
        unsafe {
            let _ = UnhookWinEvent(HWINEVENTHOOK(h as _));
        }
    }
}

/// (Re)build the hooks. False when the taskbar window does not exist (Explorer not up yet).
fn install_hooks(ctx: &mut Ctx) -> bool {
    uninstall_hooks(ctx);
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    unsafe {
        let foreground = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_event), 0, 0, flags);
        if foreground.0.is_null() {
            dlog!("WARN", "search_bar", "WIN-506 WinEvent hooks unavailable; the AI button is disabled");
            return true; // nothing to retry: the hotkey still works
        }
        ctx.hooks.push(foreground.0 as isize);

        let Some(tray) = ctx.probe.tray_hwnd() else { return false };
        TRAY.store(tray.0 as isize, Ordering::Release);
        let mut pid = 0u32;
        let tray_thread = GetWindowThreadProcessId(tray, Some(&mut pid as *mut u32));
        let mut threads = vec![tray_thread];
        if let Some(search) = ctx.probe.search_hwnd() {
            let t = GetWindowThreadProcessId(search, None);
            if t != 0 && t != tray_thread {
                threads.push(t);
            }
        }
        if tray_thread == 0 || pid == 0 {
            return false;
        }
        for tid in threads {
            for (min, max) in [(EVENT_OBJECT_SHOW, EVENT_OBJECT_REORDER), (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE)] {
                let h = SetWinEventHook(min, max, None, Some(on_event), pid, tid, flags);
                if !h.0.is_null() {
                    ctx.hooks.push(h.0 as isize);
                }
            }
        }
    }
    true
}

fn arm_rehook(ctx: &Ctx) {
    let delay = (REHOOK_FIRST_MS << ctx.rehook_attempt.min(4)).min(30_000);
    unsafe {
        SetTimer(host(), T_REHOOK, delay, None);
    }
}

fn is_below_tray(button: HWND) -> bool {
    let tray = HWND(TRAY.load(Ordering::Acquire) as _);
    if tray.0.is_null() {
        return false;
    }
    let mut cursor = button;
    // Walk up the z-order from the button: if the taskbar is above it, the button is covered.
    for _ in 0..512 {
        match unsafe { GetWindow(cursor, GW_HWNDPREV) } {
            Ok(prev) if !prev.0.is_null() => {
                if prev == tray {
                    return true;
                }
                cursor = prev;
            }
            _ => return false,
        }
    }
    false
}

/// Bring the button, the hooks and the hotkey in line with the flags.
fn reconcile(ctx: &mut Ctx) {
    let hwnd = host();
    let enabled = ENABLED.load(Ordering::Acquire);
    let want_hotkey = enabled && HOTKEY.load(Ordering::Acquire);
    if want_hotkey && !ctx.hotkey_on {
        ctx.hotkey_on = hotkey::register(hwnd);
    } else if !want_hotkey && ctx.hotkey_on {
        hotkey::unregister(hwnd);
        ctx.hotkey_on = false;
    }
    if want_hotkey && !ctx.spotlight_on {
        ctx.spotlight_on = hotkey::register_spotlight(hwnd);
    } else if !want_hotkey && ctx.spotlight_on {
        hotkey::unregister_spotlight(hwnd);
        ctx.spotlight_on = false;
    }

    let want_button = enabled && BUTTON.load(Ordering::Acquire) && BUTTON_ALLOWED.load(Ordering::Acquire);
    if want_button {
        if ctx.button.is_none() {
            ctx.button = Button::create(super::on_button_clicked);
            if ctx.button.is_none() {
                dlog!("WARN", "search_bar", "WIN-506 the AI button window could not be created");
            }
        }
        if ctx.hooks.is_empty() {
            ctx.rehook_attempt = 0;
            if !install_hooks(ctx) {
                arm_rehook(ctx);
            }
        }
    } else {
        uninstall_hooks(ctx);
        if let Some(button) = ctx.button.take() {
            button.destroy();
        }
    }
    measure(ctx);
}

/// Measure the shell, publish the layout to the search window and place/hide the button.
fn measure(ctx: &mut Ctx) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    let inputs = gather(&ctx.probe);
    let layout = compute_layout(&inputs);
    let high_contrast = ctx.probe.high_contrast();
    window::layout_changed(&layout, high_contrast);

    let now = Instant::now();
    if BURST_REQUEST.swap(false, Ordering::AcqRel) {
        ctx.sched.trigger(now);
    }
    let burst_due = ctx.sched.take_due(now);

    if let Some(button) = ctx.button.as_mut() {
        let foreground = ctx.probe.foreground();
        // Start and the taskbar never hide the button; the search flyout only where it overlaps it
        // (on Windows 10 it opens above the box); the task switcher always does.
        let covered = match layout.button {
            Some(rect) => {
                let fg_rect = if foreground == Foreground::SearchFlyout { ctx.probe.foreground_rect() } else { None };
                foreground_covers_button(foreground, fg_rect, rect)
            }
            None => false,
        };
        let visible = button_visible(&layout, true, BUTTON.load(Ordering::Acquire), FULLSCREEN.load(Ordering::Acquire), covered);
        match (visible, layout.button) {
            (true, Some(rect)) => {
                let mut raise = false;
                if burst_due && (!button.is_visible() || is_below_tray(button.hwnd)) {
                    // the shell raised the taskbar over us: take the top back (never activates)
                    raise = true;
                    ctx.last_raise = now;
                    NEED_RAISE.store(false, Ordering::Release);
                } else if NEED_RAISE.load(Ordering::Acquire) {
                    if !button.is_visible() || is_below_tray(button.hwnd) {
                        let since = ctx.last_raise.elapsed();
                        if since >= RAISE_EVERY {
                            raise = true;
                            ctx.last_raise = Instant::now();
                            NEED_RAISE.store(false, Ordering::Release);
                        } else {
                            // too soon: measure again when the window is over
                            MEASURE_PENDING.store(true, Ordering::Release);
                            unsafe {
                                SetTimer(host(), T_MEASURE, (RAISE_EVERY - since).as_millis() as u32 + 1, None);
                            }
                        }
                    } else {
                        NEED_RAISE.store(false, Ordering::Release);
                    }
                }
                button.place(rect, high_contrast, window::is_open(), raise);
            }
            _ => button.hide(),
        }
    }

    // the rest of a raise burst: measure again when its next check is due
    if let Some(wait) = ctx.sched.next_in(Instant::now()) {
        MEASURE_PENDING.store(true, Ordering::Release);
        unsafe {
            SetTimer(host(), T_MEASURE, (wait.as_millis() as u32).max(10) + 1, None);
        }
    }
}

fn with_ctx(f: impl FnOnce(&mut Ctx)) {
    CTX.with(|cell| {
        // try_borrow_mut: a message sent while the context is in use is skipped, never re-entered
        if let Ok(mut slot) = cell.try_borrow_mut() {
            if let Some(ctx) = slot.as_mut() {
                f(ctx);
            }
        }
    });
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let handled = guarded("host window proc", false, || -> bool {
        match msg {
            WM_HOTKEY if wparam.0 as i32 == hotkey::HOTKEY_ID => {
                super::on_hotkey();
                true
            }
            WM_HOTKEY if wparam.0 as i32 == hotkey::SPOTLIGHT_ID => {
                super::on_spotlight_hotkey();
                true
            }
            WM_CMD => {
                match wparam.0 {
                    CMD_RECONCILE => with_ctx(reconcile),
                    CMD_MEASURE => with_ctx(measure),
                    CMD_ACTIVE => with_ctx(|ctx| {
                        if let Some(b) = ctx.button.as_mut() {
                            b.set_active(window::is_open());
                        }
                    }),
                    CMD_IDLE_ARM => {
                        SetTimer(hwnd, T_IDLE, IDLE_DESTROY_MS, None);
                    }
                    CMD_IDLE_CANCEL => {
                        let _ = KillTimer(hwnd, T_IDLE);
                    }
                    _ => {}
                }
                true
            }
            WM_TIMER => {
                match wparam.0 {
                    T_MEASURE => {
                        let _ = KillTimer(hwnd, T_MEASURE);
                        MEASURE_PENDING.store(false, Ordering::Release);
                        with_ctx(measure);
                    }
                    T_REHOOK => {
                        let _ = KillTimer(hwnd, T_REHOOK);
                        with_ctx(|ctx| {
                            if !(ENABLED.load(Ordering::Acquire)
                                && BUTTON.load(Ordering::Acquire)
                                && BUTTON_ALLOWED.load(Ordering::Acquire))
                            {
                                return;
                            }
                            if install_hooks(ctx) {
                                measure(ctx);
                            } else {
                                ctx.rehook_attempt += 1;
                                if ctx.rehook_attempt < REHOOK_MAX_ATTEMPTS {
                                    arm_rehook(ctx);
                                } else {
                                    dlog!("WARN", "search_bar", "WIN-506 the taskbar was not found; waiting for TaskbarCreated");
                                }
                            }
                        });
                    }
                    T_IDLE => {
                        let _ = KillTimer(hwnd, T_IDLE);
                        window::destroy_if_idle();
                    }
                    _ => {}
                }
                true
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => {
                schedule();
                false
            }
            WM_POWERBROADCAST if wparam.0 == PBT_APMRESUMEAUTOMATIC => {
                schedule();
                false
            }
            m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
                // Explorer restarted: the old taskbar handles and the hooks that reference its
                // thread are dead. Rebuild after it has settled.
                dlog!("INFO", "search_bar", "TaskbarCreated: rebuilding the taskbar hooks");
                TRAY.store(0, Ordering::Release);
                with_ctx(|ctx| {
                    uninstall_hooks(ctx);
                    ctx.rehook_attempt = 0;
                    if ENABLED.load(Ordering::Acquire) && BUTTON.load(Ordering::Acquire) && BUTTON_ALLOWED.load(Ordering::Acquire) {
                        arm_rehook(ctx);
                    }
                    if let Some(b) = ctx.button.as_mut() {
                        b.hide();
                    }
                });
                true
            }
            _ => false,
        }
    });
    if handled {
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn run(app: &AppHandle) {
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            dlog!("WARN", "search_bar", "WIN-506 the search-bar host window class could not be registered");
            return;
        }
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        let hwnd = match CreateWindowExW(WS_EX_TOOLWINDOW, CLASS, w!("CompanyIsland search bar"), WS_POPUP, 0, 0, 0, 0, None, None, instance, None) {
            Ok(h) => h,
            Err(e) => {
                dlog!("WARN", "search_bar", "WIN-506 the search-bar host window could not be created: {}", e);
                return;
            }
        };
        HOST.store(hwnd.0 as isize, Ordering::Release);
        let _ = app;

        let now = Instant::now();
        CTX.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut() {
                *slot = Some(Ctx {
                    probe: Win32Probe::default(),
                    button: None,
                    hooks: Vec::new(),
                    hotkey_on: false,
                    spotlight_on: false,
                    rehook_attempt: 0,
                    last_raise: now.checked_sub(RAISE_EVERY).unwrap_or(now),
                    sched: RaiseSchedule::default(),
                });
            }
        });
        // Hotkeys only for now; the button and the hooks follow from `spawn_gate`.
        with_ctx(reconcile);
        spawn_gate();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// A few seconds after the app start (the island is up by then), start the AI button and the
/// hooks, behind the startup marker: if the previous run died while they were starting, this
/// session stays hotkey-only and the next one tries again.
fn spawn_gate() {
    let spawned = std::thread::Builder::new().name("companyisland-searchbar-gate".into()).spawn(|| {
        guarded("startup gate", (), || {
            std::thread::sleep(START_DELAY);
            let marker = FileMarker::default_path();
            let guard = marker.as_ref().map(|m| StartupGuard::new(m));
            match guard.as_ref().map_or(Decision::Run, |g| g.begin()) {
                Decision::Run => {
                    BUTTON_ALLOWED.store(true, Ordering::Release);
                    post(CMD_RECONCILE);
                    std::thread::sleep(CONFIRM_AFTER);
                    if let Some(g) = guard.as_ref() {
                        g.confirm();
                    }
                }
                Decision::SkipThisSession => {
                    dlog!(
                        "WARN",
                        "search_bar",
                        "WIN-508 the previous start ended while the AI button started; the button and the taskbar hooks stay off for this session (the hotkeys work), they are tried again at the next start"
                    );
                }
            }
        })
    });
    if let Err(e) = spawned {
        dlog!("WARN", "search_bar", "WIN-506 could not start the search-bar gate thread: {}", e);
    }
}

pub fn start(app: AppHandle) {
    let spawned = std::thread::Builder::new().name("companyisland-searchbar".into()).spawn(move || {
        guarded("search-bar thread", (), || run(&app));
    });
    if let Err(e) = spawned {
        dlog!("WARN", "search_bar", "WIN-506 could not start the search-bar thread: {}", e);
    }
}
