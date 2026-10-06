//! Global pointer monitor for the pill window.
//!
//! The webview only receives mouse events inside its own (transparent) window,
//! so it cannot tell when the user clicks the desktop / taskbar / another app,
//! and `mouseleave` is unreliable when the window is resized under a stationary
//! cursor. On Windows we install a `WH_MOUSE_LL` hook on a dedicated thread and
//! hit-test every event against the region the frontend reports through
//! `set_pill_hit_region`.
//!
//! Events emitted to the frontend:
//! - `"pill-pointer"`        `{ "inside": bool }` — only on transitions.
//! - `"pill-outside-press"`  `{ "x": i32, "y": i32, "button": "left"|"right"|"middle"|"x" }`
//!   — a button went down outside the region while the region is armed.
//!
//! Threads:
//! - hook thread: owns the hook + a message loop (and a 250ms thread timer that
//!   re-hit-tests from `GetCursorPos` and reinstalls the hook if Windows dropped
//!   it). It never touches Tauri, never logs, and only sends small events into
//!   an mpsc channel.
//! - worker thread: drains the channel, logs and calls `app.emit`.

use serde::Deserialize;

/// A hit rectangle in CSS pixels relative to the webview's client-area top-left.
#[derive(Debug, Clone, Copy, Deserialize)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Update the region the pointer monitor hit-tests against. Returns `true` when
/// the native hook is installed and running (the frontend falls back to DOM
/// events when `false`).
#[cfg(target_os = "windows")]
#[tauri::command]
pub fn set_pill_hit_region(window: tauri::Window, rects: Vec<HitRect>, dpr: f64, armed: bool) -> bool {
    let hwnd = window.hwnd().map(|h| h.0 as isize).unwrap_or(0);
    imp::set_region(hwnd, rects, dpr, armed)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn set_pill_hit_region(window: tauri::Window, rects: Vec<HitRect>, dpr: f64, armed: bool) -> bool {
    let _ = (window, rects, dpr, armed);
    false
}

/// Whether the native low-level mouse hook is currently installed.
#[cfg(target_os = "windows")]
#[tauri::command]
pub fn get_pointer_tracker_status() -> bool {
    imp::is_alive()
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn get_pointer_tracker_status() -> bool {
    false
}

#[cfg(target_os = "windows")]
pub use imp::start;

#[cfg(target_os = "windows")]
mod imp {
    use super::HitRect;
    use once_cell::sync::Lazy;
    use serde::Serialize;
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Mutex, MutexGuard, TryLockError};
    use std::thread;
    use std::time::{Duration, Instant};
    use tauri::Emitter;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{
        GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, IsWindowVisible, PeekMessageW,
        PostThreadMessageW, SetTimer, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        HC_ACTION, HHOOK, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_MOUSE_LL, WM_APP, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
        WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1,
    };

    const EVENT_INSIDE: &str = "pill-pointer";
    const EVENT_OUTSIDE_PRESS: &str = "pill-outside-press";

    /// Posted to the hook thread to re-hit-test from the current cursor position.
    const WM_APP_RECOMPUTE: u32 = WM_APP + 1;
    const TICK_MS: u32 = 250;
    /// Cursor moved but the hook saw nothing for this long => Windows dropped it.
    const WATCHDOG_STALE_MS: u64 = 2_000;
    /// Minimum gap between failed (re)install attempts.
    const REINSTALL_RETRY_MS: u64 = 2_000;
    /// Minimum gap between watchdog WARN lines (reinstalls are counted in between).
    const WATCHDOG_LOG_INTERVAL_MS: u64 = 60_000;

    const BTN_LEFT: u8 = 1 << 0;
    const BTN_RIGHT: u8 = 1 << 1;
    const BTN_MIDDLE: u8 = 1 << 2;
    const BTN_X1: u8 = 1 << 3;
    const BTN_X2: u8 = 1 << 4;

    // ---------------------------------------------------------------------
    // Shared state
    // ---------------------------------------------------------------------

    struct Region {
        /// HWND of the pill window as an integer (0 = unknown).
        hwnd: isize,
        /// CSS-pixel rects relative to the client-area origin.
        rects: Vec<HitRect>,
        /// window.devicePixelRatio at the time the rects were measured.
        dpr: f64,
    }

    /// Writers hold this for nanoseconds (swap a Vec). The hook `try_lock`s it
    /// on mouse moves and blocks only on button transitions.
    static REGION: Mutex<Region> = Mutex::new(Region { hwnd: 0, rects: Vec::new(), dpr: 1.0 });
    static ARMED: AtomicBool = AtomicBool::new(false);
    static HOOK_ALIVE: AtomicBool = AtomicBool::new(false);
    static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
    static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
    static STARTED: AtomicBool = AtomicBool::new(false);
    /// Monotonic ms (see `now_ms`) of the last event the hook received.
    static LAST_EVENT_MS: AtomicU64 = AtomicU64::new(0);

    static CLOCK_BASE: Lazy<Instant> = Lazy::new(Instant::now);

    fn now_ms() -> u64 {
        CLOCK_BASE.elapsed().as_millis() as u64
    }

    enum PointerEvent {
        Inside(bool),
        OutsidePress { x: i32, y: i32, button: &'static str },
        Log { level: &'static str, message: String },
    }

    #[derive(Clone, Serialize)]
    struct InsidePayload {
        inside: bool,
    }

    #[derive(Clone, Serialize)]
    struct OutsidePressPayload {
        x: i32,
        y: i32,
        button: &'static str,
    }

    // Hook-thread-only state. Cells (not RefCell borrows) so nothing can panic
    // even if a callback were ever re-entered.
    thread_local! {
        static TX: RefCell<Option<Sender<PointerEvent>>> = const { RefCell::new(None) };
        /// Bitmask of held buttons (BTN_*).
        static HELD: Cell<u8> = const { Cell::new(0) };
        /// A button went down inside the region; stay "inside" until all are released.
        static CAPTURED: Cell<bool> = const { Cell::new(false) };
        /// Effective inside state last reported to the frontend.
        static INSIDE: Cell<bool> = const { Cell::new(false) };
        /// Cursor position seen at the previous timer tick.
        static LAST_TICK_POS: Cell<(i32, i32)> = const { Cell::new((i32::MIN, i32::MIN)) };
        /// Consecutive ticks where HELD != 0 but no physical button is down.
        static STUCK_TICKS: Cell<u8> = const { Cell::new(0) };
        static LAST_INSTALL_ATTEMPT_MS: Cell<u64> = const { Cell::new(0) };
        static REINSTALLS_SINCE_LOG: Cell<u32> = const { Cell::new(0) };
        static LAST_WATCHDOG_LOG_MS: Cell<u64> = const { Cell::new(0) };
    }

    fn send(event: PointerEvent) {
        TX.with(|tx| {
            if let Some(tx) = tx.borrow().as_ref() {
                let _ = tx.send(event);
            }
        });
    }

    fn lock_region() -> MutexGuard<'static, Region> {
        REGION.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn try_lock_region() -> Option<MutexGuard<'static, Region>> {
        match REGION.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    /// Is the physical screen point `pt` inside any of the region's rects?
    fn hit_test(region: &Region, pt: POINT) -> bool {
        if region.hwnd == 0 || region.rects.is_empty() {
            return false;
        }
        let hwnd = HWND(region.hwnd as *mut core::ffi::c_void);
        let mut origin = POINT { x: 0, y: 0 };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || !ClientToScreen(hwnd, &mut origin).as_bool() {
                return false;
            }
        }
        let px = (pt.x - origin.x) as f64;
        let py = (pt.y - origin.y) as f64;
        let dpr = region.dpr;
        region.rects.iter().any(|r| {
            let left = r.x * dpr;
            let top = r.y * dpr;
            px >= left && px < left + r.w * dpr && py >= top && py < top + r.h * dpr
        })
    }

    /// Apply a new effective inside state; emits only on transitions.
    fn set_inside(inside: bool) {
        if INSIDE.with(|c| c.replace(inside)) != inside {
            send(PointerEvent::Inside(inside));
        }
    }

    fn cursor_pos() -> Option<POINT> {
        let mut pt = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut pt) }.ok().map(|_| pt)
    }

    /// Re-hit-test from the live cursor position (region changed, or timer tick).
    fn recompute_from_cursor(pt: Option<POINT>) {
        if CAPTURED.with(|c| c.get()) {
            return;
        }
        let Some(pt) = pt.or_else(cursor_pos) else { return };
        let inside = hit_test(&lock_region(), pt);
        set_inside(inside);
    }

    // ---------------------------------------------------------------------
    // Hook callback — must stay fast and non-blocking
    // ---------------------------------------------------------------------

    unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && lparam.0 != 0 {
            let info = *(lparam.0 as *const MSLLHOOKSTRUCT);
            let msg = wparam.0 as u32;
            // Never let a panic unwind across the FFI boundary.
            let _ = std::panic::catch_unwind(move || handle_mouse(msg, &info));
        }
        CallNextHookEx(HHOOK::default(), code, wparam, lparam)
    }

    fn handle_mouse(msg: u32, info: &MSLLHOOKSTRUCT) {
        LAST_EVENT_MS.store(now_ms(), Ordering::Relaxed);
        let pt = info.pt;

        if msg == WM_MOUSEMOVE {
            if CAPTURED.with(|c| c.get()) {
                return;
            }
            // Skip this move if a writer holds the region; the next one catches up.
            if let Some(region) = try_lock_region() {
                let inside = hit_test(&region, pt);
                drop(region);
                set_inside(inside);
            }
            return;
        }

        let xbutton_bit = || {
            if (info.mouseData >> 16) as u16 == XBUTTON1 {
                BTN_X1
            } else {
                BTN_X2
            }
        };
        let (bit, down, name) = match msg {
            WM_LBUTTONDOWN => (BTN_LEFT, true, "left"),
            WM_LBUTTONUP => (BTN_LEFT, false, "left"),
            WM_RBUTTONDOWN => (BTN_RIGHT, true, "right"),
            WM_RBUTTONUP => (BTN_RIGHT, false, "right"),
            WM_MBUTTONDOWN => (BTN_MIDDLE, true, "middle"),
            WM_MBUTTONUP => (BTN_MIDDLE, false, "middle"),
            WM_XBUTTONDOWN => (xbutton_bit(), true, "x"),
            WM_XBUTTONUP => (xbutton_bit(), false, "x"),
            _ => return, // wheel etc.
        };

        if down {
            HELD.with(|h| h.set(h.get() | bit));
            if CAPTURED.with(|c| c.get()) {
                // Pointer is captured by the island: every press belongs to it.
                return;
            }
            let inside = hit_test(&lock_region(), pt);
            if inside {
                CAPTURED.with(|c| c.set(true));
                set_inside(true);
            } else {
                set_inside(false);
                if ARMED.load(Ordering::Relaxed) {
                    send(PointerEvent::OutsidePress { x: pt.x, y: pt.y, button: name });
                }
            }
        } else {
            let held = HELD.with(|h| {
                let v = h.get() & !bit;
                h.set(v);
                v
            });
            if held == 0 {
                CAPTURED.with(|c| c.set(false));
                let inside = hit_test(&lock_region(), pt);
                set_inside(inside);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Hook thread
    // ---------------------------------------------------------------------

    fn install_hook() -> Result<(), String> {
        LAST_INSTALL_ATTEMPT_MS.with(|c| c.set(now_ms()));
        unsafe {
            let hmod = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {}", e))?;
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), hmod, 0)
                .map_err(|e| format!("SetWindowsHookExW: {}", e))?;
            HOOK_HANDLE.store(hook.0 as isize, Ordering::Relaxed);
        }
        LAST_EVENT_MS.store(now_ms(), Ordering::Relaxed);
        HOOK_ALIVE.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn uninstall_hook() {
        let raw = HOOK_HANDLE.swap(0, Ordering::Relaxed);
        if raw != 0 {
            unsafe {
                // Fails harmlessly if Windows already removed the hook.
                let _ = UnhookWindowsHookEx(HHOOK(raw as *mut core::ffi::c_void));
            }
        }
        HOOK_ALIVE.store(false, Ordering::Relaxed);
    }

    /// Are any mouse buttons physically down right now?
    fn any_button_down() -> bool {
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2]
            .iter()
            .any(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000 != 0)
    }

    fn on_tick() {
        let now = now_ms();
        let pt = cursor_pos();

        // (b) Watchdog: Windows silently removes LL hooks that time out.
        if let Some(p) = pt {
            let moved = LAST_TICK_POS.with(|c| c.replace((p.x, p.y))) != (p.x, p.y);
            let alive = HOOK_ALIVE.load(Ordering::Relaxed);
            let stale = now.saturating_sub(LAST_EVENT_MS.load(Ordering::Relaxed)) > WATCHDOG_STALE_MS;
            let retry_due = now.saturating_sub(LAST_INSTALL_ATTEMPT_MS.with(|c| c.get())) >= REINSTALL_RETRY_MS;
            if (alive && moved && stale) || (!alive && retry_due) {
                uninstall_hook();
                let result = install_hook();
                let count = REINSTALLS_SINCE_LOG.with(|c| {
                    let v = c.get() + 1;
                    c.set(v);
                    v
                });
                let log_due = now.saturating_sub(LAST_WATCHDOG_LOG_MS.with(|c| c.get())) >= WATCHDOG_LOG_INTERVAL_MS
                    || LAST_WATCHDOG_LOG_MS.with(|c| c.get()) == 0;
                if log_due {
                    LAST_WATCHDOG_LOG_MS.with(|c| c.set(now));
                    REINSTALLS_SINCE_LOG.with(|c| c.set(0));
                    let message = match &result {
                        Ok(()) => format!(
                            "mouse hook stopped receiving events; reinstalled ({} reinstall(s) since last report)",
                            count
                        ),
                        Err(e) => format!("mouse hook reinstall failed ({} attempt(s)): {}", count, e),
                    };
                    send(PointerEvent::Log { level: "WARN", message });
                }
                // Button UPs may have been missed while the hook was gone.
                if !any_button_down() {
                    HELD.with(|h| h.set(0));
                    CAPTURED.with(|c| c.set(false));
                }
            }
        }

        // Recover from missed button-UPs (e.g. while the hook was dropped).
        if HELD.with(|h| h.get()) != 0 {
            if any_button_down() {
                STUCK_TICKS.with(|c| c.set(0));
            } else {
                let ticks = STUCK_TICKS.with(|c| {
                    let v = c.get().saturating_add(1);
                    c.set(v);
                    v
                });
                if ticks >= 2 {
                    STUCK_TICKS.with(|c| c.set(0));
                    HELD.with(|h| h.set(0));
                    CAPTURED.with(|c| c.set(false));
                }
            }
        } else {
            STUCK_TICKS.with(|c| c.set(0));
        }

        // (a) Re-hit-test without mouse movement (window resized / region changed).
        if HELD.with(|h| h.get()) == 0 {
            recompute_from_cursor(pt);
        }
    }

    fn hook_thread_main(tx: Sender<PointerEvent>, ready: mpsc::SyncSender<Result<(), String>>) {
        TX.with(|cell| *cell.borrow_mut() = Some(tx));
        unsafe {
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            // Force creation of this thread's message queue before anyone posts to it.
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_NOREMOVE);
            HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::Relaxed);
        }

        let result = install_hook();
        let _ = ready.send(result);

        unsafe {
            // Thread timer (NULL hwnd): WM_TIMER lands in this thread's queue.
            if SetTimer(HWND::default(), 0, TICK_MS, None) == 0 {
                send(PointerEvent::Log {
                    level: "WARN",
                    message: "SetTimer failed; pointer watchdog/tick disabled".to_string(),
                });
            }

            let mut msg = MSG::default();
            loop {
                let r = GetMessageW(&mut msg, HWND::default(), 0, 0);
                if r.0 == 0 || r.0 == -1 {
                    break;
                }
                match msg.message {
                    WM_TIMER if msg.hwnd.0.is_null() => on_tick(),
                    WM_APP_RECOMPUTE => recompute_from_cursor(None),
                    _ => {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }

        uninstall_hook();
        HOOK_THREAD_ID.store(0, Ordering::Relaxed);
        send(PointerEvent::Log { level: "WARN", message: "pointer hook thread exited".to_string() });
    }

    fn worker_main(app: tauri::AppHandle, rx: Receiver<PointerEvent>) {
        for event in rx {
            match event {
                PointerEvent::Inside(inside) => {
                    dlog!("DEBUG", "pointer", "inside -> {}", inside);
                    if let Err(e) = app.emit(EVENT_INSIDE, InsidePayload { inside }) {
                        dlog!("WARN", "pointer", "emit {} failed: {}", EVENT_INSIDE, e);
                    }
                }
                PointerEvent::OutsidePress { x, y, button } => {
                    dlog!("DEBUG", "pointer", "outside press ({}) at {},{}", button, x, y);
                    if let Err(e) = app.emit(EVENT_OUTSIDE_PRESS, OutsidePressPayload { x, y, button }) {
                        dlog!("WARN", "pointer", "emit {} failed: {}", EVENT_OUTSIDE_PRESS, e);
                    }
                }
                PointerEvent::Log { level, message } => {
                    dlog!(level, "pointer", "{}", message);
                }
            }
        }
    }

    /// Start the hook thread + emit worker. Idempotent. Blocks briefly (<=1s)
    /// until the hook install result is known so the first
    /// `set_pill_hit_region` call reports an accurate status.
    pub fn start(app: tauri::AppHandle) {
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        Lazy::force(&CLOCK_BASE);
        let (tx, rx) = mpsc::channel::<PointerEvent>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        if let Err(e) = thread::Builder::new()
            .name("pillar-pointer-emit".into())
            .spawn(move || worker_main(app, rx))
        {
            dlog!("ERROR", "pointer", "failed to spawn pointer worker thread: {}", e);
            return;
        }

        if let Err(e) = thread::Builder::new()
            .name("pillar-pointer-hook".into())
            .spawn(move || hook_thread_main(tx, ready_tx))
        {
            dlog!("ERROR", "pointer", "failed to spawn pointer hook thread: {}", e);
            return;
        }

        match ready_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(())) => dlog!("INFO", "pointer", "global mouse hook installed"),
            Ok(Err(e)) => dlog!("ERROR", "pointer", "global mouse hook install failed: {}", e),
            Err(_) => dlog!("WARN", "pointer", "timed out waiting for mouse hook install"),
        }
    }

    pub fn is_alive() -> bool {
        HOOK_ALIVE.load(Ordering::Relaxed)
    }

    pub fn set_region(hwnd: isize, rects: Vec<HitRect>, dpr: f64, armed: bool) -> bool {
        let rects: Vec<HitRect> = rects
            .into_iter()
            .filter(|r| {
                r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite() && r.w > 0.0 && r.h > 0.0
            })
            .collect();
        let dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };

        let old_rects = {
            let mut region = lock_region();
            if hwnd != 0 {
                region.hwnd = hwnd;
            }
            region.dpr = dpr;
            std::mem::replace(&mut region.rects, rects)
        };
        drop(old_rects); // free outside the lock

        if ARMED.swap(armed, Ordering::Relaxed) != armed {
            dlog!("DEBUG", "pointer", "outside-press reporting {}", if armed { "armed" } else { "disarmed" });
        }

        // The region may have grown/shrunk under a stationary cursor: let the
        // hook thread (sole owner of the inside state) re-hit-test right away.
        let thread_id = HOOK_THREAD_ID.load(Ordering::Relaxed);
        if thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(thread_id, WM_APP_RECOMPUTE, WPARAM(0), LPARAM(0));
            }
        }

        HOOK_ALIVE.load(Ordering::Relaxed)
    }
}
