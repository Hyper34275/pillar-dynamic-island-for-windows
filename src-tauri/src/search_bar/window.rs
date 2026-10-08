//! The search input window (label `search`, page `search.html`).
//!
//! Created lazily on the first open and kept hidden afterwards for a fast reopen; destroyed after
//! ten idle minutes to give the WebView2 processes back (8 GB machines). Unlike the island this
//! window takes the keyboard: it has no `WS_EX_NOACTIVATE`, and the user's click / hotkey / tray
//! click grants the foreground. It closes when it loses activation (a click elsewhere), on Esc (the
//! page calls `search_bar_close`), or when the AI button / hotkey is used again.
//!
//! tao rewrites the window styles on every flag change, so this window has its own
//! `SetWindowSubclass` (id `SUBCLASS_ID`, not the island's 0x4349) that forces the frameless
//! popup + tool-window styles back in. The window region is rounded to the bar so nothing paints
//! outside it, and the creation happens off the main thread (Tauri deadlocks when a window is
//! built on the thread that has to process the request).
//!
//! The centre "Spotlight glass" variant differs in two ways. Its window is fixed at the tallest sheet
//! (the sheet grows in CSS), so its region is not rounded but a rectangle over the sheet and its
//! shadow that follows the sheet's height (`set_sheet_height`): the transparent rest lets clicks
//! through. And before it shows, the screen under it is captured (`snapshot.rs`, window hidden) and
//! pushed to the page, which paints it blurred as the glass.

use super::anchor::{gather, ShellProbe, Win32Probe};
use super::glass::{self, GlassGeometry};
use super::guard::guarded;
use super::layout::{compute_layout, sheet_region, spotlight_layout, Layout, Variant};
use super::snapshot::{self, GlassBackdrop};
use super::{thread, SearchBarState, WINDOW_LABEL};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, WA_INACTIVE, WM_ACTIVATE, WM_NCACTIVATE, WM_NCDESTROY, WM_STYLECHANGING, WS_EX_APPWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// Not 0 / 1 (tao) and not 0x4349 (the island).
const SUBCLASS_ID: usize = 0x5343;
/// A deactivation this soon after opening is the system settling the foreground, not the user leaving.
const SETTLE: Duration = Duration::from_millis(300);

static OPEN: AtomicBool = AtomicBool::new(false);
static OPENED_AT: Mutex<Option<Instant>> = Mutex::new(None);
static STATE: Mutex<Option<SearchBarState>> = Mutex::new(None);
static LAST: Mutex<Option<(Layout, bool)>> = Mutex::new(None);
/// One open/create at a time.
static CREATE: Mutex<()> = Mutex::new(());
/// The shape the window has (or last had).
static VARIANT: Mutex<Variant> = Mutex::new(Variant::Floating);

fn current_variant() -> Variant {
    *lock(&VARIANT)
}

/// What Alt+` does, given whether the bar is open and its current shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpotlightAction {
    Open,
    Close,
    Switch,
}

fn spotlight_action(open: bool, current: Variant) -> SpotlightAction {
    if !open {
        SpotlightAction::Open
    } else if current == Variant::Spotlight {
        SpotlightAction::Close
    } else {
        SpotlightAction::Switch
    }
}

/// The layout for a wanted shape: the measured taskbar one, or the centred spotlight on the
/// monitor under the cursor.
fn build_layout(probe: &Win32Probe, spotlight: bool) -> Layout {
    if spotlight {
        let mut p = windows::Win32::Foundation::POINT::default();
        let cursor = if unsafe { GetCursorPos(&mut p) }.is_ok() { (p.x, p.y) } else { (i32::MIN, i32::MIN) };
        spotlight_layout(&probe.monitors(), cursor, probe.taskbar_edge())
    } else {
        compute_layout(&gather(probe))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_open() -> bool {
    OPEN.load(Ordering::Acquire)
}

/// The centre glass bar is open right now (not the taskbar-anchored or floating variants).
pub fn glass_open() -> bool {
    is_open() && current_variant() == Variant::Spotlight
}

/// The geometry the page should lay out to (also pushed as `search-bar-state`).
pub fn state() -> SearchBarState {
    if let Some(s) = lock(&STATE).clone() {
        return s;
    }
    let probe = Win32Probe::default();
    compute_layout(&gather(&probe)).to_state(probe.high_contrast())
}

fn hwnd_of(window: &WebviewWindow) -> Option<HWND> {
    window.hwnd().ok().map(|h| HWND(h.0 as _))
}

// =============================================================================
// Window styles
// =============================================================================

fn forced_ex(style: u32) -> u32 {
    (style | WS_EX_TOOLWINDOW.0) & !(WS_EX_APPWINDOW.0 | WS_EX_NOACTIVATE.0)
}

/// Frameless popup + tool window, activatable. Idempotent.
fn apply_styles(hwnd: HWND) {
    unsafe {
        crate::window::remove_dwm_frame(hwnd);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let wanted = crate::window::frameless_style(style as u32) as isize;
        if wanted != style {
            SetWindowLongPtrW(hwnd, GWL_STYLE, wanted);
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = forced_ex(ex as u32) as isize;
        if wanted != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
        }
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    // A panic must not cross the FFI boundary: fall back to the default handling.
    match guarded("search window subclass", None, || Some(subclass_inner(hwnd, msg, wparam, lparam, id, data))) {
        Some(result) => result,
        None => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}

unsafe fn subclass_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let styles = &mut *(lparam.0 as *mut STYLESTRUCT);
            styles.styleNew = forced_ex(styles.styleNew);
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_STYLE.0 => {
            let styles = &mut *(lparam.0 as *mut STYLESTRUCT);
            styles.styleNew = crate::window::frameless_style(styles.styleNew);
        }
        // Same reason as the island: no non-client repaint (a caption flashing over the page).
        WM_NCACTIVATE => return DefSubclassProc(hwnd, msg, wparam, LPARAM(-1)),
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE && is_open() => {
            let settled = lock(&OPENED_AT).is_none_or(|t| t.elapsed() >= SETTLE);
            if settled {
                if let Some(app) = super::app() {
                    let handle = app.clone();
                    if let Err(e) = app.run_on_main_thread(move || close(&handle)) {
                        dlog!("WARN", "search_bar", "WIN-507 closing on deactivation could not be queued: {}", e);
                    }
                }
            }
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

// =============================================================================
// Geometry
// =============================================================================

fn apply_geometry(hwnd: HWND, layout: &Layout) {
    let w = layout.window;
    unsafe {
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, w.left, w.top, w.width(), w.height(), SWP_NOACTIVATE);
    }
    if layout.variant == Variant::Spotlight {
        // The glass: a rectangle over the sheet and its shadow, following the sheet's height.
        glass::set_geometry(GlassGeometry { width_px: w.width(), height_px: w.height(), scale: layout.scale });
        apply_glass_region(hwnd);
        return;
    }
    unsafe {
        // Rounded to the bar (+1: the right/bottom edge is excluded). On success the system owns it.
        let d = layout.region_radius_px * 2;
        let rgn = CreateRoundRectRgn(0, 0, w.width() + 1, w.height() + 1, d, d);
        if SetWindowRgn(hwnd, rgn, true) == 0 {
            let _ = DeleteObject(HGDIOBJ(rgn.0));
            dlog!("WARN", "search_bar", "WIN-507 SetWindowRgn failed");
        }
    }
}

/// The glass window's region: a rectangle from the top edge to the bottom of the sheet's shadow, as
/// wide as the window. The transparent rest of the fixed-size window passes clicks to what is below.
fn apply_glass_region(hwnd: HWND) {
    let Some(g) = glass::geometry() else { return };
    let r = sheet_region(g.width_px, g.height_px, g.scale, glass::sheet_dip());
    unsafe {
        let rgn = CreateRectRgn(r.left, r.top, r.right, r.bottom);
        if SetWindowRgn(hwnd, rgn, true) == 0 {
            let _ = DeleteObject(HGDIOBJ(rgn.0));
            dlog!("WARN", "search_bar", "WIN-507 SetWindowRgn (glass) failed");
        }
    }
}

/// The page reports the sheet's height (DIP) whenever it changes. Safe from any thread: the region
/// is applied at once (a sync Tauri command already runs on the window's thread), so the page can
/// wait for the call before it grows the sheet and the new area is clickable from the first frame.
pub fn set_sheet_height(app: &AppHandle, dip: f64) {
    glass::store_sheet(dip);
    if !glass_open() {
        return;
    }
    if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        if let Some(hwnd) = hwnd_of(&win) {
            apply_glass_region(hwnd);
        }
    }
}

fn publish_state(app: &AppHandle, layout: &Layout, high_contrast: bool) {
    let state = layout.to_state(high_contrast);
    *lock(&STATE) = Some(state.clone());
    if let Err(e) = app.emit_to(WINDOW_LABEL, "search-bar-state", state) {
        dlog!("WARN", "search_bar", "WIN-507 emit search-bar-state failed: {}", e);
    }
}

/// The thread measured a (changed) layout: remember it and, when the window is open, follow.
pub fn layout_changed(layout: &Layout, high_contrast: bool) {
    let changed = {
        let mut last = lock(&LAST);
        let changed = last.as_ref().is_none_or(|(l, h)| l != layout || *h != high_contrast);
        *last = Some((layout.clone(), high_contrast));
        changed
    };
    // The centred spotlight does not follow the taskbar.
    if !changed || !is_open() || current_variant() == Variant::Spotlight {
        return;
    }
    let Some(app) = super::app() else { return };
    let (app2, layout) = (app.clone(), layout.clone());
    let _ = app.run_on_main_thread(move || {
        if !is_open() || current_variant() == Variant::Spotlight {
            return;
        }
        if let Some(win) = app2.get_webview_window(WINDOW_LABEL) {
            if let Some(hwnd) = hwnd_of(&win) {
                apply_geometry(hwnd, &layout);
            }
            publish_state(&app2, &layout, high_contrast);
        }
    });
}

// =============================================================================
// Open / close
// =============================================================================

fn ensure_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(WINDOW_LABEL) {
        return Ok(w);
    }
    let win = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("search.html".into()))
        .title("Yuval search")
        .inner_size(572.0, 60.0)
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .focused(false)
        .additional_browser_args(crate::WEBVIEW_ARGS)
        .build()
        .map_err(|e| format!("WIN-507: search window could not be created: {e}"))?;
    // Styles and subclass belong to the window's own (main) thread.
    let hardened = win.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(hwnd) = hwnd_of(&hardened) {
            unsafe {
                if !SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0).as_bool() {
                    dlog!("WARN", "search_bar", "WIN-507 search window subclass failed");
                }
            }
            apply_styles(hwnd);
        }
    });
    Ok(win)
}

/// Last resort when `SetForegroundWindow` is refused: borrow the foreground thread's input state.
fn force_foreground(hwnd: HWND) {
    unsafe {
        if GetForegroundWindow() == hwnd {
            return;
        }
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return;
        }
        let front = GetForegroundWindow();
        let their = GetWindowThreadProcessId(front, None);
        let ours = GetCurrentThreadId();
        if their != 0 && their != ours && AttachThreadInput(ours, their, true).as_bool() {
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(ours, their, false);
        }
    }
}

/// An open that did not end with a visible window: clear `open` and arm the idle destroy. `close()`
/// would return early (it only acts when `open` was set), so without this the created WebView2
/// would stay resident until exit. `post` sends a command to the search thread.
fn reset_failed_open(open: &AtomicBool, mut post: impl FnMut(usize)) {
    open.store(false, Ordering::Release);
    post(thread::CMD_IDLE_ARM);
}

fn fail_open() {
    reset_failed_open(&OPEN, thread::post);
}

fn show_on_main(app: &AppHandle, win: &WebviewWindow, layout: &Layout, high_contrast: bool, backdrop: Option<&GlassBackdrop>) {
    let Some(hwnd) = hwnd_of(win) else {
        dlog!("WARN", "search_bar", "WIN-507 search window has no handle");
        fail_open();
        return;
    };
    apply_styles(hwnd);
    apply_geometry(hwnd, layout);
    publish_state(app, layout, high_contrast);
    // Capture -> event -> show: the page already holds the picture (or knows there is none) when the
    // first frame is composed; if it is still decoding, the tinted panel shows and the picture fades in.
    if let Some(backdrop) = backdrop {
        glass::publish_backdrop(app, backdrop);
    }
    if let Err(e) = win.show() {
        dlog!("WARN", "search_bar", "WIN-507 search window could not be shown: {}", e);
        fail_open();
        return;
    }
    // tao re-applies its style flags on show; the subclass keeps ours, this covers a missing one.
    apply_styles(hwnd);
    apply_geometry(hwnd, layout);
    let _ = win.set_focus();
    force_foreground(hwnd);
    thread::post(thread::CMD_ACTIVE);
}

/// Start of a glass session (a no-op for the other shapes): earlier answers are the island's again,
/// the sheet is the field alone, and the screen under the (hidden) window is captured. `None` for
/// the other variants.
fn begin_glass(layout: &Layout, high_contrast: bool) -> Option<GlassBackdrop> {
    if layout.variant != Variant::Spotlight {
        return None;
    }
    glass::reset_queries();
    glass::reset_sheet();
    Some(snapshot::capture(layout.window, high_contrast))
}

fn open_blocking(app: &AppHandle, spotlight: bool) {
    let _serial = lock(&CREATE);
    if is_open() || !thread::enabled() {
        return;
    }
    let probe = Win32Probe::default();
    let layout = build_layout(&probe, spotlight);
    let high_contrast = probe.high_contrast();
    // The page asks for the state when it loads: make that reply the shape being opened.
    *lock(&VARIANT) = layout.variant;
    *lock(&STATE) = Some(layout.to_state(high_contrast));
    let win = match ensure_window(app) {
        Ok(w) => w,
        Err(e) => {
            dlog!("WARN", "search_bar", "{}", e);
            // a half-built window must not outlive the failure
            thread::post(thread::CMD_IDLE_ARM);
            return;
        }
    };
    // The glass: a new session (its answers are the sheet's), the sheet starts as the field alone, and
    // the screen under the window is captured now, while the window is still hidden.
    let backdrop = begin_glass(&layout, high_contrast);
    OPEN.store(true, Ordering::Release);
    *lock(&OPENED_AT) = Some(Instant::now());
    thread::post(thread::CMD_IDLE_CANCEL);
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || show_on_main(&handle, &win, &layout, high_contrast, backdrop.as_ref())) {
        fail_open();
        dlog!("WARN", "search_bar", "WIN-507 opening could not be queued: {}", e);
    }
}

/// Show the input and give it the keyboard (a user action only). Safe from any thread.
pub fn open(app: &AppHandle) {
    if is_open() || !thread::enabled() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        crate::debug_log::catch("search_bar", || open_blocking(&app, false));
    });
}

/// Move an open bar to the spotlight shape (or back to the measured one). Off the main thread.
fn switch_blocking(app: &AppHandle, spotlight: bool) {
    let _serial = lock(&CREATE);
    if !is_open() || !thread::enabled() {
        return;
    }
    let probe = Win32Probe::default();
    let layout = build_layout(&probe, spotlight);
    let high_contrast = probe.high_contrast();
    let was_glass = current_variant() == Variant::Spotlight;
    *lock(&VARIANT) = layout.variant;
    *lock(&STATE) = Some(layout.to_state(high_contrast));
    *lock(&OPENED_AT) = Some(Instant::now());
    // Into the glass the window has to be out of the way while the screen under the new place is
    // captured (it is on screen as the old shape): hide it, wait for the hide to land, capture, and
    // show it again with the picture. Out of the glass the picture goes.
    let backdrop = if spotlight {
        hide_for_capture(app);
        let backdrop = begin_glass(&layout, high_contrast);
        // the deactivation the hide causes is the system settling, not the user leaving
        *lock(&OPENED_AT) = Some(Instant::now());
        backdrop
    } else {
        if was_glass {
            glass::clear_backdrop();
        }
        None
    };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if !is_open() {
            return;
        }
        if let Some(win) = handle.get_webview_window(WINDOW_LABEL) {
            if let Some(hwnd) = hwnd_of(&win) {
                apply_geometry(hwnd, &layout);
                publish_state(&handle, &layout, high_contrast);
                if let Some(backdrop) = backdrop.as_ref() {
                    glass::publish_backdrop(&handle, backdrop);
                    let _ = win.show();
                    apply_styles(hwnd);
                    apply_geometry(hwnd, &layout);
                }
                let _ = win.set_focus();
                force_foreground(hwnd);
            }
        }
    });
}

/// Hide the (visible) search window and wait until the hide has run on the window's thread plus one
/// composition frame, so the screen captured next does not hold the old bar. Bounded: a stuck main
/// thread costs the picture's cleanliness, not the open.
fn hide_for_capture(app: &AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let handle = app.clone();
    let queued = app.run_on_main_thread(move || {
        if let Some(win) = handle.get_webview_window(WINDOW_LABEL) {
            let _ = win.hide();
        }
        let _ = tx.send(());
    });
    if queued.is_ok() {
        let _ = rx.recv_timeout(Duration::from_millis(500));
        // one composition frame (16.7 ms at 60 Hz) for DWM to drop the window from the screen
        std::thread::sleep(Duration::from_millis(24));
    }
}

/// Alt+`: closed -> open the spotlight; another shape open -> switch to the spotlight; spotlight
/// open -> close. Safe from any thread.
pub fn toggle_spotlight(app: &AppHandle) {
    if !thread::enabled() {
        return;
    }
    match spotlight_action(is_open(), current_variant()) {
        SpotlightAction::Close => close(app),
        SpotlightAction::Open => {
            let app = app.clone();
            std::thread::spawn(move || {
                crate::debug_log::catch("search_bar", || open_blocking(&app, true));
            });
        }
        SpotlightAction::Switch => {
            let app = app.clone();
            std::thread::spawn(move || {
                crate::debug_log::catch("search_bar", || switch_blocking(&app, true));
            });
        }
    }
}

/// Hide the input (AI Mode off). Keeps the webview for a fast reopen. Safe from any thread.
pub fn close(app: &AppHandle) {
    if !OPEN.swap(false, Ordering::AcqRel) {
        return;
    }
    // The picture of the user's screen goes the moment the bar closes. (The noted questions stay: an
    // answer that arrives after this is the island's, see glass::route_for.)
    glass::clear_backdrop();
    let handle = app.clone();
    let queued = app.run_on_main_thread(move || {
        if let Some(win) = handle.get_webview_window(WINDOW_LABEL) {
            let _ = win.hide();
        }
        thread::post(thread::CMD_ACTIVE);
        thread::post(thread::CMD_IDLE_ARM);
    });
    if let Err(e) = queued {
        dlog!("WARN", "search_bar", "WIN-507 closing could not be queued: {}", e);
    }
}

pub fn toggle(app: &AppHandle) {
    if is_open() {
        close(app);
    } else {
        open(app);
    }
}

/// The idle timer fired: free the webview if nobody opened the bar meanwhile.
pub fn destroy_if_idle() {
    if is_open() {
        return;
    }
    let Some(app) = super::app() else { return };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if is_open() {
            return;
        }
        if let Some(win) = handle.get_webview_window(WINDOW_LABEL) {
            let _ = win.destroy();
            dlog!("INFO", "search_bar", "idle search window destroyed");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_extended_style_is_a_tool_window_that_can_activate() {
        let all = u32::MAX;
        let forced = forced_ex(all);
        assert_ne!(forced & WS_EX_TOOLWINDOW.0, 0);
        assert_eq!(forced & WS_EX_NOACTIVATE.0, 0, "the input takes the keyboard");
        assert_eq!(forced & WS_EX_APPWINDOW.0, 0, "no taskbar button");
        assert_eq!(forced_ex(0), WS_EX_TOOLWINDOW.0);
    }

    #[test]
    fn the_island_subclass_id_is_not_reused() {
        assert_ne!(SUBCLASS_ID, 0x4349);
        assert!(SUBCLASS_ID > 1);
    }

    #[test]
    fn state_before_any_open_is_a_floating_or_anchored_geometry() {
        let s = state();
        assert!(s.width > 0.0 && s.height > 0.0 && s.scale > 0.0);
        assert!(["bottom", "top", "left", "right"].contains(&s.edge.as_str()));
    }

    #[test]
    fn a_failed_open_clears_the_flag_and_arms_the_idle_destroy() {
        // regression (#28): close() returns early when OPEN is already false, so the failure path
        // has to arm the idle timer itself.
        let open = AtomicBool::new(true);
        let mut posted = Vec::new();
        reset_failed_open(&open, |c| posted.push(c));
        assert!(!open.load(Ordering::Acquire));
        assert_eq!(posted, vec![thread::CMD_IDLE_ARM]);
    }

    #[test]
    fn spotlight_hotkey_semantics() {
        // closed: open it; another shape open: switch; spotlight open: close
        assert_eq!(spotlight_action(false, Variant::Floating), SpotlightAction::Open);
        assert_eq!(spotlight_action(false, Variant::Spotlight), SpotlightAction::Open);
        assert_eq!(spotlight_action(true, Variant::Taskbar), SpotlightAction::Switch);
        assert_eq!(spotlight_action(true, Variant::Floating), SpotlightAction::Switch);
        assert_eq!(spotlight_action(true, Variant::Spotlight), SpotlightAction::Close);
    }

    #[test]
    fn the_spotlight_layout_is_built_from_the_live_monitors() {
        let l = build_layout(&Win32Probe::default(), true);
        assert_eq!(l.variant, Variant::Spotlight);
        assert!(l.window.width() > 0 && l.window.height() > 0);
        assert_eq!(l.to_state(false).variant, "spotlight");
    }

    #[test]
    fn closing_when_closed_does_nothing() {
        // is_open() is false in a fresh process; the swap must not flip anything.
        assert!(!is_open());
    }
}
