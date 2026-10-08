//! The island window: non-activating styles, placement, display-change handling and show/hide.
//!
//! The window has no frame: a WS_POPUP with no caption or system menu (`frameless_style`), and
//! activation changes never repaint a non-client area (`WM_NCACTIVATE`), so Windows never draws a
//! title bar over the island.
//!
//! The window never takes focus: `WS_EX_NOACTIVATE` keeps clicks from activating
//! it and `WS_EX_TOOLWINDOW` removes it from Alt+Tab. This app's own code never calls
//! `SetForegroundWindow` or `set_focus`. (The single-instance plugin makes the second process
//! call `AllowSetForegroundWindow` for the first one; the first instance never uses that right.)
//! This app's own `AllowSetForegroundWindow` calls are two: `outlook::open_calendar`, after a
//! click on a meeting invitation, lets the user's own Outlook come forward for that one click;
//! `center::open`, after a click on an island button or the tray item (or on the first run),
//! lets the Island Center process come forward, naming only that process's pid (never
//! `ASFW_ANY`).
//!
//! The one exception to "never takes focus" is typing a note in the island's Notes tab: a
//! click in its text box calls `island_keyboard(true)`, which drops `WS_EX_NOACTIVATE` and
//! activates the window (the user's click is the last input, so Windows allows it). It ends
//! when the text box loses focus (`island_keyboard(false)`) or another window becomes active
//! (`WM_ACTIVATE`, then `island-keyboard-ended` to the frontend); the style comes back at once.
//!
//! There is no global mouse hook. The native window is a fixed *stage*, as large as the
//! largest island (the frontend sends logical px), placed once and never resized or moved
//! while the island animates: resizing a WebView2 window shows its previous frame at the new
//! size for a frame or two (measured: the closing island flashed as a ~12 px sliver) and
//! stalls its frame pipeline (~110 ms). Only the window *region* follows the island: a rounded
//! rectangle around the island's resting shape, so the transparent rest of the stage is not
//! part of the window at all and every click there reaches the windows below.
//!
//! The stage starts at the monitor's top edge; the island is drawn `ISLAND_TOP_INSET` below it. The
//! region is the union of the island's rounded shape and a "bridge" rectangle over that gap, as wide
//! as the island and nothing more, so a pointer thrown against the screen's top edge still hits the
//! island (no bridge when a top-docked taskbar owns that edge). The stage is sized by the frontend
//! from `get_island_limits` (the panel shrinks on a small monitor), never beyond the monitor.

use crate::{fullscreen, monitors, settings::SettingsStore};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ, RGN_OR};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, KillTimer, RegisterWindowMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos,
    GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, SPI_SETDESKWALLPAPER, SPI_SETWORKAREA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, STYLESTRUCT, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_NCDESTROY, WM_POWERBROADCAST,
    WA_INACTIVE, WM_ACTIVATE, WM_NCACTIVATE, WM_SETTINGCHANGE, WM_STYLECHANGING, WM_TIMER, WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
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
/// (left, top, width, height, region radius, bridge) in physical px of the region currently applied.
type AppliedRegion = (i32, i32, i32, i32, i32, Option<monitors::Bounds>);
static APPLIED_REGION: Mutex<Option<AppliedRegion>> = Mutex::new(None);
/// Window rectangle (physical px) last applied, so an unchanged stage is never moved again.
static APPLIED_RECT: Mutex<Option<monitors::Bounds>> = Mutex::new(None);
/// Whether the target monitor has a taskbar auto-hiding on its TOP edge, with the monitor bounds it
/// was asked for. The question is a cross-process call into the shell, so it is asked once per
/// monitor and again only after a display or settings change (`reflow` clears it).
static TOP_AUTOHIDE: Mutex<Option<(monitors::Bounds, bool)>> = Mutex::new(None);
/// The island's resting shape in screen px (what the window region currently is, margin included)
/// and the monitor it is on: backdrop.rs samples the pixels just outside it.
static ISLAND_SCREEN: Mutex<Option<(monitors::Bounds, monitors::Bounds)>> = Mutex::new(None);
static APP: OnceLock<AppHandle> = OnceLock::new();

/// The island's region on screen (physical px) and its monitor's bounds, or None before the first
/// placement.
pub fn island_screen() -> Option<(monitors::Bounds, monitors::Bounds)> {
    *lock(&ISLAND_SCREEN)
}

/// Whether a taskbar auto-hides on the top edge of this monitor (cached, see `TOP_AUTOHIDE`).
fn top_autohide(monitor: monitors::Bounds) -> bool {
    let mut cache = lock(&TOP_AUTOHIDE);
    if let Some((bounds, answer)) = *cache {
        if bounds == monitor {
            return answer;
        }
    }
    let answer = monitors::top_autohide_bar(monitor);
    if answer {
        dlog!("INFO", "window", "a taskbar auto-hides on the top edge: no top bridge");
    }
    *cache = Some((monitor, answer));
    answer
}
/// Registered "TaskbarCreated" message (explorer restarted); 0 until `init`.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// A note is being typed in the island: the window may be active and hold the keyboard.
static KEYBOARD: AtomicBool = AtomicBool::new(false);

/// The extended-style bits the island always forces: no taskbar button, and no activation
/// unless a note is being typed.
fn forced_ex_style() -> u32 {
    if KEYBOARD.load(Ordering::Acquire) {
        WS_EX_TOOLWINDOW.0
    } else {
        (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0
    }
}

/// The island has no frame of any kind: tao builds the style of an "undecorated" window as
/// WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX (measured on the live window: 0x14CB0000)
/// and only hides the caption by making the non-client area zero-sized in WM_NCCALCSIZE. The caption
/// is still there for user32, which paints it itself (the window region turns DWM's frame off)
/// straight over the client area: when a note's text box took or gave back the keyboard (the window
/// activating or deactivating, the island collapsing), a white tool-window title bar
/// ("CompanyIsland" and an X) flashed at the top of the stage. A WS_POPUP without those bits has no
/// caption, system menu or sizing frame; the activation repaint is refused in `subclass_proc`
/// (WM_NCACTIVATE). tao rewrites GWL_STYLE from its flags on every flag change (show/hide,
/// focusable, topmost) and never reads the bits back, so this is forced into every write.
fn frameless_style(style: u32) -> u32 {
    (style & !(WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0) | WS_POPUP.0
}

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

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, attribute: u32, value: *const core::ffi::c_void, size: u32) -> i32;
}
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_BORDER_COLOR: u32 = 34;
const DWMWCP_DONOTROUND: u32 = 1;
/// `DWMWA_COLOR_NONE`: no border at all.
const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;

/// The island is a shape of its own: ask DWM for no frame treatment at all. On Windows 11 DWM
/// applies its frame look to a top-level window (and tao's undecorated window had WS_CAPTION until
/// `frameless_style`): a 1 px border in the system border colour and rounded-corner clipping. Neither belongs to a black silhouette (the border reads as a bright outline on a
/// dark wallpaper, the rounding as a second, different, corner under the island's own). Border
/// colour NONE and corner preference DO-NOT-ROUND remove both; on Windows 10 the attributes do
/// not exist and the call just fails, which is the right answer there too.
fn remove_dwm_frame(hwnd: HWND) {
    unsafe {
        let corner = DWMWCP_DONOTROUND;
        let border = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const u32 as *const _, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const u32 as *const _, 4);
    }
}

fn apply_non_activating(hwnd: HWND) -> Result<(), String> {
    remove_dwm_frame(hwnd);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let wanted = frameless_style(style as u32) as isize;
        if wanted != style {
            SetWindowLongPtrW(hwnd, GWL_STYLE, wanted);
        }
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = style | forced_ex_style() as isize;
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

/// Apply the window region (window-relative physical px): the rounded island, united with the
/// bridge rectangle above it when there is one, so the transparent rest of the stage does not
/// catch clicks but the strip between the island and the screen's top edge does.
fn apply_region(hwnd: HWND, shape: monitors::RegionShape, radius: i32) {
    let region = shape.island;
    let key = Some((region.left, region.top, region.width(), region.height(), radius, shape.bridge));
    let mut applied = lock(&APPLIED_REGION);
    if *applied == key {
        return;
    }
    unsafe {
        // CreateRoundRectRgn excludes the right/bottom edge, hence +1. A zero radius is a
        // plain rectangle (still a region: the stage around it must stay click-through).
        let diameter = radius * 2;
        let mut rgn = CreateRoundRectRgn(region.left, region.top, region.right + 1, region.bottom + 1, diameter, diameter);
        if let Some(bridge) = shape.bridge {
            // Same +1 on the right so the bridge is exactly as wide as the island's region.
            let strip = CreateRectRgn(bridge.left, bridge.top, bridge.right + 1, bridge.bottom);
            let united = CreateRectRgn(0, 0, 0, 0);
            if CombineRgn(united, rgn, strip, RGN_OR).0 != 0 {
                let _ = DeleteObject(HGDIOBJ(rgn.0));
                rgn = united;
            } else {
                // Without the union the island itself still works (the bridge is a convenience).
                let _ = DeleteObject(HGDIOBJ(united.0));
                dlog!("WARN", "window", "WIN-501: CombineRgn failed, no top bridge");
            }
            let _ = DeleteObject(HGDIOBJ(strip.0));
        }
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

    // An island larger than the stage (a size the frontend did not announce) still fits. The stage
    // starts at the monitor's top edge, so it also holds the island's gap below it (the frontend
    // adds it to `stage_height`; this keeps an older or smaller announcement safe).
    let stage_width = geometry.stage_width.max(geometry.width);
    let stage_height = geometry.stage_height.max(geometry.height + monitors::ISLAND_TOP_INSET);
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

    // The bridge over the top gap exists only where nothing of the shell is on the top edge: not a
    // top-docked taskbar (the bridge would sit on it and swallow its clicks) and not an AUTO-HIDE
    // one either (its work area reaches the edge, but it is revealed by touching the edge, which a
    // window covering it would steal). See `monitors::bridge_wanted`.
    let bridge = monitors::bridge_wanted(target.top_is_free(), top_autohide(target.bounds));
    let shape = monitors::region_shape(rect.width(), rect.height(), target.dpi, geometry.width, geometry.height, bridge);
    *lock(&ISLAND_SCREEN) = Some((
        monitors::Bounds {
            left: rect.left + shape.island.left,
            top: rect.top + shape.island.top,
            right: rect.left + shape.island.right,
            bottom: rect.top + shape.island.bottom,
        },
        target.bounds,
    ));
    apply_region(
        hwnd,
        shape,
        monitors::region_radius(geometry.radius, target.dpi, shape.island.width(), shape.island.height()),
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
    // The taskbar may have been docked, undocked or switched to auto-hide: ask the shell again.
    *lock(&TOP_AUTOHIDE) = None;
    if let Err(e) = place(app) {
        dlog!("WARN", "window", "{}", e);
    }
    // The payload is the new target monitor's limits (`IslandLimits`); a listener that ignores
    // it keeps working.
    if let Err(e) = app.emit("display-changed", current_limits(app)) {
        dlog!("WARN", "window", "emit display-changed failed: {}", e);
    }
    fullscreen::reevaluate();
    // Another monitor, scale or wallpaper: what is behind the island may have changed.
    crate::backdrop::request_sample();
}

/// Re-place the island from any thread (e.g. after the monitor setting changed).
pub fn reflow_on_main(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || reflow(&handle)) {
        dlog!("WARN", "window", "WIN-501: reflow could not be queued: {}", e);
    }
}

/// `WM_SETTINGCHANGE` with `wParam` 0 and the string "TraySettings" in `lParam`: Explorer's taskbar
/// settings (auto-hide on/off, which edge) changed.
unsafe fn is_tray_settings(msg: u32, wparam: usize, lparam: LPARAM) -> bool {
    if msg != WM_SETTINGCHANGE || wparam != 0 || lparam.0 == 0 {
        return false;
    }
    windows::core::PCWSTR(lparam.0 as *const u16).to_string().is_ok_and(|name| name == "TraySettings")
}

/// How long to wait before re-flowing for a window message, if it is a display event.
fn reflow_delay_ms(msg: u32, wparam: usize, taskbar_created: u32, tray_settings: bool) -> Option<(usize, u32)> {
    match msg {
        WM_DPICHANGED => Some((TIMER_FAST, DELAY_DPI_MS)),
        WM_DISPLAYCHANGE => Some((TIMER_FAST, DELAY_DISPLAY_MS)),
        WM_SETTINGCHANGE if wparam == SPI_SETWORKAREA.0 as usize => Some((TIMER_FAST, DELAY_DISPLAY_MS)),
        // The taskbar's own settings (auto-hide, position) changed: its work area may not move at all
        // (auto-hide), but whether the top bridge may exist does.
        WM_SETTINGCHANGE if tray_settings => Some((TIMER_FAST, DELAY_DISPLAY_MS)),
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
        // would give it a taskbar button) or WS_EX_NOACTIVATE (unless a note is being typed).
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let styles = &mut *(lparam.0 as *mut STYLESTRUCT);
            styles.styleNew |= forced_ex_style();
        }
        // The same rewrite puts tao's caption bits back into the style: keep them out, so neither
        // a title change, a frame recalculation nor the system menu has a title bar to draw.
        WM_STYLECHANGING if wparam.0 as i32 == GWL_STYLE.0 => {
            let styles = &mut *(lparam.0 as *mut STYLESTRUCT);
            styles.styleNew = frameless_style(styles.styleNew);
        }
        // The island has no non-client area, so an activation change has nothing to repaint there.
        // With the window region DWM does not draw this window's frame (DWMWA_NCRENDERING_ENABLED
        // is false), user32 does, and a window *created* with a caption (tao always creates one)
        // still gets one painted over its client area on WM_NCACTIVATE after the style lost
        // WS_CAPTION (measured in a replica of this window: the white "CompanyIsland" bar on every
        // activation change; none with lParam -1). lParam -1 is DefWindowProc's documented "do not
        // repaint the non-client area"; tao still sees the message and tracks focus as before.
        WM_NCACTIVATE => return DefSubclassProc(hwnd, msg, wparam, LPARAM(-1)),
        // Another window became active while a note was being typed: the island gives the
        // keyboard back. Queued, so the style is not rewritten inside the activation itself.
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE && KEYBOARD.load(Ordering::Acquire) => {
            if let Some(app) = APP.get() {
                let handle = app.clone();
                if let Err(e) = app.run_on_main_thread(move || end_keyboard(&handle, true)) {
                    dlog!("WARN", "window", "WIN-501: ending keyboard input could not be queued: {}", e);
                }
            }
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
    // The wallpaper changed: what is behind the island changed with it.
    if msg == WM_SETTINGCHANGE && wparam.0 == SPI_SETDESKWALLPAPER.0 as usize {
        crate::backdrop::request_sample();
    }
    let tray_settings = is_tray_settings(msg, wparam.0, lparam);
    if let Some((timer, delay)) = reflow_delay_ms(msg, wparam.0, TASKBAR_CREATED.load(Ordering::Relaxed), tray_settings) {
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

/// Typing in the island is over: back to non-activating. `notify` tells the frontend (its
/// text box still has DOM focus when another window took the keyboard away). Main thread.
fn end_keyboard(app: &AppHandle, notify: bool) {
    if !KEYBOARD.swap(false, Ordering::AcqRel) {
        return;
    }
    if let Some(window) = main_window(app) {
        if let Err(e) = window.set_focusable(false) {
            dlog!("WARN", "window", "WIN-501: restoring non-activating style failed: {}", e);
        }
        restyle_later(&window);
    }
    dlog!("INFO", "window", "keyboard input ended");
    if notify {
        if let Err(e) = app.emit("island-keyboard-ended", ()) {
            dlog!("WARN", "window", "emit island-keyboard-ended failed: {}", e);
        }
    }
}

/// The Notes tab's text box wants the keyboard (`on`, after a click in it) or is done with it.
/// While on, the island window is activatable and active, so key presses reach the page. Runs
/// on the main thread (sync command), like every other window mutation.
#[tauri::command]
pub fn island_keyboard(app: AppHandle, on: bool) -> Result<(), String> {
    if !on {
        end_keyboard(&app, false);
        return Ok(());
    }
    let window = app.get_webview_window(MAIN).ok_or_else(|| "WIN-501: main window is missing".to_string())?;
    if !KEYBOARD.swap(true, Ordering::AcqRel) {
        dlog!("INFO", "window", "keyboard input for a note");
    }
    let focused = window.set_focusable(true).and_then(|()| window.set_focus());
    if let Err(e) = focused {
        end_keyboard(&app, false);
        return Err(format!("WIN-501: the island could not take the keyboard: {e}"));
    }
    Ok(())
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

/// How large the island may be on the monitor it is on (logical px) and that monitor's scale.
/// None only when no monitor can be enumerated (the frontend then keeps its preferred sizes).
fn current_limits(app: &AppHandle) -> Option<monitors::IslandLimits> {
    let setting = app.state::<SettingsStore>().get().monitor;
    let monitors = monitors::list();
    monitors::pick(&setting, &monitors).map(|m| m.limits())
}

/// The target monitor's limits for the panel, so the frontend can fit it into a small screen.
#[tauri::command(async)]
pub fn get_island_limits(app: AppHandle) -> Result<monitors::IslandLimits, String> {
    current_limits(&app).ok_or_else(|| "WIN-503: no monitors found".to_string())
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
        assert_eq!(reflow_delay_ms(WM_DISPLAYCHANGE, 0, TASKBAR, false), Some((TIMER_FAST, DELAY_DISPLAY_MS)));
        assert_eq!(reflow_delay_ms(WM_DPICHANGED, 0, TASKBAR, false), Some((TIMER_FAST, DELAY_DPI_MS)));
        assert_eq!(
            reflow_delay_ms(WM_SETTINGCHANGE, SPI_SETWORKAREA.0 as usize, TASKBAR, false),
            Some((TIMER_FAST, DELAY_DISPLAY_MS))
        );
        assert_eq!(
            reflow_delay_ms(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, TASKBAR, false),
            Some((TIMER_SLOW, DELAY_SLOW_MS))
        );
        assert_eq!(reflow_delay_ms(TASKBAR, 0, TASKBAR, false), Some((TIMER_SLOW, DELAY_SLOW_MS)));
    }

    #[test]
    fn taskbar_settings_changes_schedule_a_reflow() {
        // Auto-hide toggled: the work area may not change, but the top bridge decision does.
        assert_eq!(reflow_delay_ms(WM_SETTINGCHANGE, 0, TASKBAR, true), Some((TIMER_FAST, DELAY_DISPLAY_MS)));
        assert_eq!(reflow_delay_ms(WM_SETTINGCHANGE, 0, TASKBAR, false), None);
    }

    #[test]
    fn tray_settings_string_is_recognised_only_on_a_setting_change() {
        let name: Vec<u16> = "TraySettings\0".encode_utf16().collect();
        let other: Vec<u16> = "Environment\0".encode_utf16().collect();
        let lp = |v: &Vec<u16>| LPARAM(v.as_ptr() as isize);
        unsafe {
            assert!(is_tray_settings(WM_SETTINGCHANGE, 0, lp(&name)));
            assert!(!is_tray_settings(WM_SETTINGCHANGE, 0, lp(&other)));
            assert!(!is_tray_settings(WM_SETTINGCHANGE, 0, LPARAM(0)));
            assert!(!is_tray_settings(WM_SETTINGCHANGE, 0x13, lp(&name)));
            assert!(!is_tray_settings(WM_DISPLAYCHANGE, 0, lp(&name)));
        }
    }

    #[test]
    fn the_island_style_has_no_caption_to_paint() {
        // tao's undecorated, non-resizable, visible window as measured on the live island.
        let tao = 0x14CB_0000;
        let style = frameless_style(tao);
        assert_eq!(style & WS_CAPTION.0, 0);
        assert_eq!(style & (WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0, 0);
        // WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS
        assert_eq!(style, 0x9400_0000);
        // Stable: forcing it again (every later tao rewrite) changes nothing.
        assert_eq!(frameless_style(style), style);
        // A hidden window stays hidden.
        assert_eq!(frameless_style(tao & !0x1000_0000), 0x8400_0000);
    }

    #[test]
    fn unrelated_messages_are_ignored() {
        assert_eq!(reflow_delay_ms(WM_SETTINGCHANGE, 0x13, TASKBAR, false), None);
        assert_eq!(reflow_delay_ms(WM_POWERBROADCAST, 0x4, TASKBAR, false), None);
        assert_eq!(reflow_delay_ms(WM_TIMER, 1, TASKBAR, false), None);
        // before RegisterWindowMessage ran the id is 0, which must not match message 0
        assert_eq!(reflow_delay_ms(0, 0, 0, false), None);
    }
}
