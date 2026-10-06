//! The island window: non-activating styles, placement, click-through, show/hide
//! and fullscreen detection.
//!
//! The window never takes focus: `WS_EX_NOACTIVATE` keeps clicks from activating
//! it and `WS_EX_TOOLWINDOW` removes it from Alt+Tab. Nothing in this app calls
//! `SetForegroundWindow`, `set_focus` or `AllowSetForegroundWindow`.

use crate::rt;
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
    IsZoomed, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

pub const MAIN: &str = "main";

fn main_window(app: &AppHandle) -> Option<Window> {
    app.get_webview_window(MAIN).map(|w| w.as_ref().window())
}

fn hwnd_of(window: &Window) -> Result<HWND, String> {
    window
        .hwnd()
        .map(|h| HWND(h.0 as _))
        .map_err(|e| format!("WIN-501: no window handle: {e}"))
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

/// Show or hide through tauri/tao rather than `ShowWindow`: tao keeps its own VISIBLE
/// flag and re-applies it on every later flag change (e.g. click-through), so a raw
/// `ShowWindow` would be undone by the next `set_click_through`. Activation is
/// prevented by WS_EX_NOACTIVATE (`focusable: false`).
fn set_shown(window: &Window, shown: bool) {
    let result = if shown { window.show() } else { window.hide() };
    if let Err(e) = result {
        dlog!("WARN", "window", "WIN-501: show/hide failed: {}", e);
    }
}

fn is_shown(window: &Window) -> bool {
    window.is_visible().unwrap_or(false)
}

fn top_center(monitor_x: i32, monitor_y: i32, monitor_width: i32, window_width: i32) -> (i32, i32) {
    (monitor_x + (monitor_width - window_width) / 2, monitor_y)
}

fn center_top(window: &Window) -> Result<(), String> {
    let monitor = window
        .primary_monitor()
        .map_err(|e| format!("WIN-503: monitor lookup failed: {e}"))?
        .ok_or_else(|| "WIN-503: no primary monitor".to_string())?;
    let size = window.outer_size().map_err(|e| format!("WIN-501: window size unavailable: {e}"))?;
    let (x, y) = top_center(
        monitor.position().x,
        monitor.position().y,
        monitor.size().width as i32,
        size.width as i32,
    );
    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
        .map_err(|e| format!("WIN-501: positioning failed: {e}"))
}

/// First-run setup: non-activating styles, position, then show (never focused).
pub fn init(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        dlog!("ERROR", "window", "WIN-501 main window is missing");
        return;
    };
    if let Err(e) = hwnd_of(&window).and_then(apply_non_activating) {
        dlog!("WARN", "window", "{}", e);
    }
    if let Err(e) = center_top(&window) {
        dlog!("WARN", "window", "{}", e);
    }
    set_shown(&window, true);
}

pub fn show(app: &AppHandle) {
    if let Some(window) = main_window(app) {
        set_shown(&window, true);
    }
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

/// Set click-through mode: when enabled, mouse events pass to the apps behind.
#[tauri::command]
pub fn set_click_through(window: Window, ignore: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| format!("WIN-501: failed to set click-through: {e}"))?;
    // tao rewrites the whole extended style whenever a window flag changes, which drops
    // WS_EX_TOOLWINDOW (and would drop NOACTIVATE without `focusable: false` in the
    // config). Messages are handled in order, so this runs after that rewrite.
    let hwnd = hwnd_of(&window)?.0 as isize;
    window
        .run_on_main_thread(move || {
            if let Err(e) = apply_non_activating(HWND(hwnd as _)) {
                dlog!("WARN", "window", "{}", e);
            }
        })
        .map_err(|e| format!("WIN-501: restyling failed: {e}"))
}

/// Center the window at the top of the primary monitor.
#[tauri::command]
pub fn position_window(window: Window) -> Result<(), String> {
    center_top(&window)
}

/// Resize the island (logical px) and keep it top-centered on the primary monitor
/// in one SetWindowPos, so no frame shows the new size at the old position.
/// `radius`/`animate` from the frontend are CSS concerns and ignored here.
#[tauri::command]
pub fn set_island_geometry(window: Window, width: f64, height: f64) -> Result<(), String> {
    dlog!("DEBUG", "window", "set_island_geometry {}x{}", width, height);
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        return Err("WIN-501: invalid island dimensions".to_string());
    }
    let monitor = window
        .primary_monitor()
        .map_err(|e| format!("WIN-503: monitor lookup failed: {e}"))?
        .ok_or_else(|| "WIN-503: no primary monitor".to_string())?;
    let hwnd = hwnd_of(&window)?;
    let scale = monitor.scale_factor();
    let w = (width * scale).round() as i32;
    let h = (height * scale).round() as i32;
    let (x, y) = top_center(monitor.position().x, monitor.position().y, monitor.size().width as i32, w);
    unsafe { SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE) }
        .map_err(|e| format!("WIN-501: resize failed: {e}"))
}

/// Check if the foreground window is "content" fullscreen (video/game), not just
/// window fullscreen: browser F11 and maximized apps do not count.
#[tauri::command]
pub async fn is_foreground_fullscreen() -> Result<bool, String> {
    rt::run_blocking("is_foreground_fullscreen", is_foreground_fullscreen_blocking).await
}

fn is_foreground_fullscreen_blocking() -> Result<bool, String> {
    // Shell surfaces are full-screen WS_POPUP windows but never "content": the
    // desktop (Progman/WorkerW, i.e. clicking the desktop or Win+D), the taskbar,
    // Alt-Tab, Task View and Start. Treating them as fullscreen slid the island
    // off-screen while its window still swallowed clicks.
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

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(false);
        }

        // Our own window is never "fullscreen content".
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        if pid == std::process::id() {
            return Ok(false);
        }

        let mut class_buf = [0u16; 128];
        let len = GetClassNameW(hwnd, &mut class_buf).max(0) as usize;
        let class_name = String::from_utf16_lossy(&class_buf[..len]);
        if SHELL_CLASSES.contains(&class_name.as_str()) {
            return Ok(false);
        }

        // Maximized windows (custom-chrome apps like Spotify/Steam) are not fullscreen.
        if IsZoomed(hwnd).as_bool() {
            return Ok(false);
        }

        // Only fullscreen on the primary monitor (where the island lives) matters.
        let primary = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let window_monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        if primary.is_invalid() || window_monitor != primary {
            return Ok(false);
        }

        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(primary, &mut info).as_bool() {
            return Ok(false);
        }

        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return Ok(false);
        }

        // Must cover the entire monitor (not just 90% of it).
        let mon = info.rcMonitor;
        if rect.left > mon.left || rect.top > mon.top || rect.right < mon.right || rect.bottom < mon.bottom {
            return Ok(false);
        }

        // Content fullscreen (video/game/F11): popup style or no title bar.
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if style == 0 {
            return Ok(false);
        }
        let style = style as u32;
        let fullscreen = (style & WS_POPUP.0) != 0 || (style & WS_CAPTION.0) == 0;
        if fullscreen {
            dlog!("DEBUG", "fullscreen", "foreground window (class '{}') detected as fullscreen", class_name);
        }
        Ok(fullscreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_on_monitor_with_offset() {
        assert_eq!(top_center(0, 0, 1920, 200), (860, 0));
        assert_eq!(top_center(-1920, 0, 1920, 200), (-1060, 0));
        assert_eq!(top_center(0, 0, 1920, 1920), (0, 0));
    }
}
