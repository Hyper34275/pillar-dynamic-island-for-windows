//! The island window: non-activating styles, placement, display-change handling and show/hide.
//!
//! The window never takes focus: `WS_EX_NOACTIVATE` keeps clicks from activating
//! it and `WS_EX_TOOLWINDOW` removes it from Alt+Tab. This app's own code never calls
//! `SetForegroundWindow` or `set_focus`. (The single-instance plugin makes the second process
//! call `AllowSetForegroundWindow` for the first one; the first instance never uses that right.)
//! This app's own `AllowSetForegroundWindow` calls are two: `outlook::open_calendar`, after a
//! click on a meeting invitation, lets the user's own Outlook come forward for that one click;
//! `center::open`, after a click on an island button or the tray item (or on the first run),
//! lets the Island Center process come forward, naming only that process's pid (never
//! `ASFW_ANY`). The island window itself still never takes focus.
//!
//! There is no global mouse hook. The native window is a fixed *stage*, as large as the
//! largest island (the frontend sends logical px), placed once and never resized or moved
//! while the island animates: resizing a WebView2 window shows its previous frame at the new
//! size for a frame or two (measured: the closing island flashed as a ~12 px sliver) and
//! stalls its frame pipeline (~110 ms). Only the window *region* follows the island: a rounded
//! rectangle around the island's resting shape, so the transparent rest of the stage is not
//! part of the window at all and every click there reaches the windows below.

use crate::{fullscreen, monitors, settings::SettingsStore};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, KillTimer, RegisterWindowMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos,
    GWL_EXSTYLE, HWND_TOPMOST, SPI_SETWORKAREA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
    SWP_NOSIZE, SWP_NOZORDER, STYLESTRUCT, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_NCDESTROY, WM_POWERBROADCAST,
    WM_SETTINGCHANGE, WM_STYLECHANGING, WM_TIMER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

pub const MAIN: &str = "main";

/// Size the window starts with (mirrors `tauri.conf.json`); the frontend replaces it
/// with its own geometry as soon as it boots.
const INITIAL: Geometry = Geometry { width: 200.0, height: 44.0, radius: 22.0, stage_width: 200.0, stage_height: 44.0 };

/// `wParam` of `WM_POWERBROADCAST` when the system resumed without user input.
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
/// Arbitrary subclass id; tao uses 0 and 1 on its own windows.
const SUBCLASS_ID: usize = 0x4349;
/// Debounce timers: displays settle quickly, resume/explorer restarts need longer.
const TIMER_FAST: usize = 1;
const TIMER_SLOW: usize = 2;
const DELAY_DPI_MS: u32 = 30;
const DELAY_DISPLAY_MS: u32 = 250;
const DELAY_SLOW_MS: u32 = 1500;

/// The island's last requested resting shape (the region) and the stage window around it,
/// in logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    width: f64,
    height: f64,
    radius: f64,
    stage_width: f64,
    stage_height: f64,
}

static GEOMETRY: Mutex<Geometry> = Mutex::new(INITIAL);
/// (left, width, height, region radius) in physical px of the region currently applied.
static APPLIED_REGION: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
/// Window rectangle (physical px) last applied, so an unchanged stage is never moved again.
static APPLIED_RECT: Mutex<Option<monitors::Bounds>> = Mutex::new(None);
static APP: OnceLock<AppHandle> = OnceLock::new();
/// Registered "TaskbarCreated" message (explorer restarted); 0 until `init`.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn main_window(app: &AppHandle) -> Option<Window> {
    app.get_webview_window(MAIN).map(|w| w.as_ref().window())
}

fn hwnd_of(window: &Window) -> Result<HWND, String> {
    window
        .hwnd()
        .map(|h| HWND(h.0 as _))
        .map_err(|e| format!("WIN-501: no window handle: {e}"))
}

/// Native handle of the island window (the clipboard's owner window).
pub fn main_hwnd(app: &AppHandle) -> Result<HWND, String> {
    main_window(app)
        .ok_or_else(|| "WIN-501: main window is missing".to_string())
        .and_then(|w| hwnd_of(&w))
}

fn apply_non_activating(hwnd: HWND) -> Result<(), String> {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = style | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize;
        if wanted != style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
        }
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
        .map_err(|e| format!("WIN-501: applying window styles failed: {e}"))
    }
}

/// tao rewrites the whole extended style whenever a window flag changes (show/hide,
/// click-through), which drops WS_EX_TOOLWINDOW. The subclass re-adds the bits as the
/// rewrite happens; this restyle, queued after the change, covers the case where the
/// subclass could not be installed.
fn restyle_later(window: &Window) {
    let Ok(hwnd) = hwnd_of(window).map(|h| h.0 as isize) else { return };
    let queued = window.run_on_main_thread(move || {
        if let Err(e) = apply_non_activating(HWND(hwnd as _)) {
            dlog!("WARN", "window", "{}", e);
        }
    });
    if let Err(e) = queued {
        dlog!("WARN", "window", "WIN-501: restyling failed: {}", e);
    }
}

/// Show or hide through tauri/tao rather than `ShowWindow`: tao keeps its own VISIBLE
/// flag and re-applies it on every later flag change, so a raw `ShowWindow` could be
/// undone behind its back. Activation is prevented by WS_EX_NOACTIVATE (`focusable: false` makes tao use SW_SHOWNOACTIVATE).
fn set_shown(window: &Window, shown: bool) {
    let result = if shown { window.show() } else { window.hide() };
    match result {
        Ok(()) if shown => restyle_later(window),
        Ok(()) => {}
        Err(e) => dlog!("WARN", "window", "WIN-501: show/hide failed: {}", e),
    }
}

fn is_shown(window: &Window) -> bool {
    window.is_visible().unwrap_or(false)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Apply the rounded window region around the island (window-relative physical px), so the
/// transparent rest of the stage does not catch clicks.
fn apply_region(hwnd: HWND, region: monitors::Bounds, radius: i32) {
    let key = Some((region.left, region.width(), region.height(), radius));
    let mut applied = lock(&APPLIED_REGION);
    if *applied == key {
        return;
    }
    unsafe {
        // CreateRoundRectRgn excludes the right/bottom edge, hence +1. A zero radius is a
        // plain rectangle (still a region: the stage around it must stay click-through).
        let diameter = radius * 2;
        let rgn = CreateRoundRectRgn(region.left, region.top, region.right + 1, region.bottom + 1, diameter, diameter);
        // On success the system owns the region; on failure it is ours to free.
        if SetWindowRgn(hwnd, rgn, true) == 0 {
            let _ = DeleteObject(HGDIOBJ(rgn.0));
            dlog!("WARN", "window", "WIN-501: SetWindowRgn failed");
            return;
        }
    }
    *applied = key;
}

/// Place the stage on the target monitor and cut its region to the island. The stage is
/// moved or resized only when it actually changes (boot, display or DPI change, monitor
/// setting): never during an island transition. When it does, it is one atomic SetWindowPos
/// (position, size and topmost together). Otherwise only topmost is re-asserted.
fn place(app: &AppHandle) -> Result<(), String> {
    let window = main_window(app).ok_or_else(|| "WIN-501: main window is missing".to_string())?;
    let hwnd = hwnd_of(&window)?;
    let geometry = *lock(&GEOMETRY);
    let setting = app.state::<SettingsStore>().get().monitor;
    let monitors = monitors::list();
    let target = monitors::pick(&setting, &monitors).ok_or_else(|| "WIN-503: no monitors found".to_string())?;

    // An island larger than the stage (a size the frontend did not announce) still fits.
    let stage_width = geometry.stage_width.max(geometry.width);
    let stage_height = geometry.stage_height.max(geometry.height);
    let rect = monitors::island_bounds(target.bounds, target.dpi, stage_width, stage_height);
    let mut applied_rect = lock(&APPLIED_RECT);
    let moved = *applied_rect != Some(rect);
    let flags = if moved {
        SWP_NOACTIVATE | SWP_NOOWNERZORDER
    } else {
        SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOMOVE | SWP_NOSIZE
    };
    unsafe { SetWindowPos(hwnd, HWND_TOPMOST, rect.left, rect.top, rect.width(), rect.height(), flags) }
        .map_err(|e| format!("WIN-501: placing the island failed: {e}"))?;
    if moved {
        dlog!("INFO", "window", "stage {}x{} at {},{}", rect.width(), rect.height(), rect.left, rect.top);
        *applied_rect = Some(rect);
    }
    drop(applied_rect);

    let region = monitors::island_region(rect.width(), rect.height(), target.dpi, geometry.width, geometry.height);
    apply_region(
        hwnd,
        region,
        monitors::region_radius(geometry.radius, target.dpi, region.width(), region.height()),
    );
    Ok(())
}

/// The display configuration changed: restyle, re-place on the (possibly new) target
/// monitor, re-assert topmost, tell the frontend and re-check fullscreen. Runs on the
/// main thread.
fn reflow(app: &AppHandle) {
    if let Some(Ok(hwnd)) = main_window(app).map(|w| hwnd_of(&w)) {
        if let Err(e) = apply_non_activating(hwnd) {
            dlog!("WARN", "window", "{}", e);
        }
    }
    *lock(&APPLIED_REGION) = None;
    *lock(&APPLIED_RECT) = None;
    if let Err(e) = place(app) {
        dlog!("WARN", "window", "{}", e);
    }
    if let Err(e) = app.emit("display-changed", ()) {
        dlog!("WARN", "window", "emit display-changed failed: {}", e);
    }
    fullscreen::reevaluate();
}

/// Re-place the island from any thread (e.g. after the monitor setting changed).
pub fn reflow_on_main(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || reflow(&handle)) {
        dlog!("WARN", "window", "WIN-501: reflow could not be queued: {}", e);
    }
}

/// How long to wait before re-flowing for a window message, if it is a display event.
fn reflow_delay_ms(msg: u32, wparam: usize, taskbar_created: u32) -> Option<(usize, u32)> {
    match msg {
        WM_DPICHANGED => Some((TIMER_FAST, DELAY_DPI_MS)),
        WM_DISPLAYCHANGE => Some((TIMER_FAST, DELAY_DISPLAY_MS)),
        WM_SETTINGCHANGE if wparam == SPI_SETWORKAREA.0 as usize => Some((TIMER_FAST, DELAY_DISPLAY_MS)),
        WM_POWERBROADCAST if wparam == PBT_APMRESUMEAUTOMATIC => Some((TIMER_SLOW, DELAY_SLOW_MS)),
        m if taskbar_created != 0 && m == taskbar_created => Some((TIMER_SLOW, DELAY_SLOW_MS)),
        _ => None,
    }
}

/// Subclass of the island window (comctl32 v6 `SetWindowSubclass`, the same mechanism tao
/// uses, with a distinct id). It lets tao handle every message first and then schedules
/// a debounced reflow; no polling.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        // tao rewrites the whole extended style on every window flag change (show/hide,
        // click-through) and then shows the window; forcing our bits into the style being
        // applied means the window never becomes visible without WS_EX_TOOLWINDOW (which
        // would give it a taskbar button) or WS_EX_NOACTIVATE.
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let styles = &mut *(lparam.0 as *mut STYLESTRUCT);
            styles.styleNew |= (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0;
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
        }
        WM_TIMER if wparam.0 == TIMER_FAST || wparam.0 == TIMER_SLOW => {
            let _ = KillTimer(hwnd, wparam.0);
            crate::debug_log::catch("window", || {
                if let Some(app) = APP.get() {
                    reflow(app);
                }
            });
            return LRESULT(0);
        }
        _ => {}
    }
    let result = DefSubclassProc(hwnd, msg, wparam, lparam);
    if let Some((timer, delay)) = reflow_delay_ms(msg, wparam.0, TASKBAR_CREATED.load(Ordering::Relaxed)) {
        SetTimer(hwnd, timer, delay, None);
    }
    result
}

/// First-run setup: non-activating styles, display-change subclass, placement, then
/// show (never focused). Must run on the main thread.
pub fn init(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        dlog!("ERROR", "window", "WIN-501 main window is missing");
        return;
    };
    match hwnd_of(&window) {
        Ok(hwnd) => {
            if let Err(e) = apply_non_activating(hwnd) {
                dlog!("WARN", "window", "{}", e);
            }
            let _ = APP.set(app.clone());
            TASKBAR_CREATED.store(unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }, Ordering::Relaxed);
            if !unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) }.as_bool() {
                dlog!("WARN", "window", "WIN-501: display-change handling unavailable");
            }
        }
        Err(e) => dlog!("WARN", "window", "{}", e),
    }
    if let Err(e) = place(app) {
        dlog!("WARN", "window", "{}", e);
    }
    set_shown(&window, true);
}

pub fn show(app: &AppHandle) {
    if let Some(window) = main_window(app) {
        set_shown(&window, true);
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = main_window(app) {
        set_shown(&window, false);
    }
}

pub fn is_visible(app: &AppHandle) -> bool {
    main_window(app).is_some_and(|w| is_shown(&w))
}

pub fn toggle_visibility(app: &AppHandle) {
    if let Some(window) = main_window(app) {
        set_shown(&window, !is_shown(&window));
    }
}

/// Ask the frontend to expand/collapse the island, optionally on a given tab.
pub fn emit_island_toggle(app: &AppHandle, tab: Option<&str>) {
    if let Err(e) = app.emit("island-toggle", serde_json::json!({ "tab": tab })) {
        dlog!("WARN", "window", "emit island-toggle failed: {}", e);
    }
}

/// The window is only ever hidden, never destroyed; Quit lives in the tray menu.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        set_shown(window, false);
    }
}

/// The island's resting shape (logical px): `width` x `height` with corner `radius`
/// (default a full pill) becomes the window region, centred at the top of the stage.
/// `stage_width` x `stage_height` is the fixed window around every island shape; it only
/// moves the window when it changes (older frontends without it get a stage exactly the
/// island's size, the previous behaviour). Runs on the main thread (sync command), like
/// every other window mutation.
#[tauri::command]
pub fn set_island_geometry(
    app: AppHandle,
    width: f64,
    height: f64,
    radius: Option<f64>,
    stage_width: Option<f64>,
    stage_height: Option<f64>,
) -> Result<(), String> {
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        return Err("WIN-501: invalid island dimensions".to_string());
    }
    let radius = radius.filter(|r| r.is_finite() && *r >= 0.0).unwrap_or(height / 2.0);
    let stage = |v: Option<f64>, min: f64| v.filter(|s| s.is_finite() && *s > 0.0).map_or(min, |s| s.max(min));
    *lock(&GEOMETRY) = Geometry {
        width,
        height,
        radius,
        stage_width: stage(stage_width, width),
        stage_height: stage(stage_height, height),
    };
    place(&app)
}

/// Connected displays for the settings UI (physical size, no identifying data).
#[tauri::command(async)]
pub fn get_monitors() -> Result<Vec<monitors::MonitorInfo>, String> {
    let list = monitors::list();
    if list.is_empty() {
        return Err("WIN-503: monitor enumeration failed".to_string());
    }
    Ok(monitors::infos(&list))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASKBAR: u32 = 0xC123;

    #[test]
    fn display_messages_schedule_a_reflow() {
        assert_eq!(reflow_delay_ms(WM_DISPLAYCHANGE, 0, TASKBAR), Some((TIMER_FAST, DELAY_DISPLAY_MS)));
        assert_eq!(reflow_delay_ms(WM_DPICHANGED, 0, TASKBAR), Some((TIMER_FAST, DELAY_DPI_MS)));
        assert_eq!(
            reflow_delay_ms(WM_SETTINGCHANGE, SPI_SETWORKAREA.0 as usize, TASKBAR),
            Some((TIMER_FAST, DELAY_DISPLAY_MS))
        );
        assert_eq!(
            reflow_delay_ms(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, TASKBAR),
            Some((TIMER_SLOW, DELAY_SLOW_MS))
        );
        assert_eq!(reflow_delay_ms(TASKBAR, 0, TASKBAR), Some((TIMER_SLOW, DELAY_SLOW_MS)));
    }

    #[test]
    fn unrelated_messages_are_ignored() {
        assert_eq!(reflow_delay_ms(WM_SETTINGCHANGE, 0x13, TASKBAR), None);
        assert_eq!(reflow_delay_ms(WM_POWERBROADCAST, 0x4, TASKBAR), None);
        assert_eq!(reflow_delay_ms(WM_TIMER, 1, TASKBAR), None);
        // before RegisterWindowMessage ran the id is 0, which must not match message 0
        assert_eq!(reflow_delay_ms(0, 0, 0), None);
    }
}
