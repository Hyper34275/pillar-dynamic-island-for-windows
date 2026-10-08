//! The AI button: a small native layered window drawn by us.
//!
//! A webview would cost tens of MB and a slow start for a 24 DIP icon, so the pill is rendered here
//! with an analytic signed-distance function into a premultiplied 32-bit bitmap
//! (`UpdateLayeredWindow`), anti-aliased, per DPI. The window never activates (`WS_EX_NOACTIVATE`),
//! so clicking it leaves the Windows search box's focus alone; a click toggles AI Mode.
//!
//! Everything here runs on the search-bar thread (it owns the message loop).

use crate::monitors::Bounds;
use std::cell::{Cell, RefCell};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetSysColor, ReleaseDC, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, COLOR_WINDOW, COLOR_WINDOWTEXT,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, RegisterClassExW, SendMessageW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, HWND_TOPMOST, IDC_HAND, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, ULW_ALPHA, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_SETCURSOR, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const TOOLTIP: &str = "חיפוש חכם (Ctrl+Alt+Space)";
const CLASS: PCWSTR = w!("CompanyIslandAiButton");
/// `WM_MOUSELEAVE` (not exported by the windows crate).
const WM_MOUSELEAVE: u32 = 0x02A3;

// Tooltip control (tooltips_class32). Declared here instead of enabling the windows crate's
// Win32_UI_Controls feature: that feature makes the unit-test binary import TaskDialogIndirect, which
// exists only in comctl32 v6 (the test binary has no manifest) and stops it from loading.
const TTM_ADDTOOLW: u32 = 0x0432;
const TTS_ALWAYSTIP: u32 = 0x01;
const TTS_NOPREFIX: u32 = 0x02;
const TTF_IDISHWND: u32 = 0x0001;
const TTF_RTLREADING: u32 = 0x0004;
const TTF_SUBCLASS: u32 = 0x0010;

#[repr(C)]
struct TTTOOLINFOW {
    cb_size: u32,
    flags: u32,
    hwnd: HWND,
    id: usize,
    rect: RECT,
    instance: HINSTANCE,
    text: PWSTR,
    param: LPARAM,
    reserved: *mut core::ffi::c_void,
}

// =============================================================================
// Rendering (pure)
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Normal,
    Hover,
    Pressed,
    /// AI Mode is open.
    Active,
}

/// Colours of a rendering, 0..255 per channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Fill gradient stops, top-left to bottom-right (equal stops = a flat colour).
    pub stops: [[f32; 3]; 3],
    pub sparkle: [f32; 3],
    /// Ring around the pill (width in px; 0 = none).
    pub ring: [f32; 3],
    pub ring_px: f32,
}

pub const CYAN: [f32; 3] = [64.0, 200.0, 224.0];
pub const VIOLET: [f32; 3] = [191.0, 90.0, 242.0];
pub const MAGENTA: [f32; 3] = [224.0, 64.0, 200.0];

impl Palette {
    pub fn colour() -> Palette {
        Palette { stops: [CYAN, VIOLET, MAGENTA], sparkle: [255.0; 3], ring: [255.0; 3], ring_px: 0.0 }
    }

    /// High contrast: system colours, no gradient, a 2 px ring.
    pub fn high_contrast(window: [f32; 3], text: [f32; 3]) -> Palette {
        Palette { stops: [window; 3], sparkle: text, ring: text, ring_px: 2.0 }
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn gradient(stops: &[[f32; 3]; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        lerp(stops[0], stops[1], t * 2.0)
    } else {
        lerp(stops[1], stops[2], (t - 0.5) * 2.0)
    }
}

/// Signed distance from `(x, y)` (relative to the centre) to a rounded rectangle with half size
/// `(hx, hy)` and corner radius `r`: negative inside.
fn sd_round_rect(x: f32, y: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = x.abs() - (hx - r);
    let qy = y.abs() - (hy - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// Approximate signed distance to a four-point star (an astroid: `sqrt|x| + sqrt|y| = sqrt(R)`),
/// first-order normalised by the gradient so the edge anti-aliases over about one pixel.
fn sd_sparkle(x: f32, y: f32, radius: f32) -> f32 {
    let ax = x.abs().max(1e-3);
    let ay = y.abs().max(1e-3);
    let f = ax.sqrt() + ay.sqrt() - radius.sqrt();
    let gx = 0.5 / ax.sqrt();
    let gy = 0.5 / ay.sqrt();
    f / (gx * gx + gy * gy).sqrt()
}

fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

/// Render the button as `width * height` premultiplied BGRA pixels, top-down.
pub fn render(width: u32, height: u32, variant: Variant, palette: &Palette) -> Vec<u8> {
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    let mut out = vec![0u8; w * h * 4];
    let (fw, fh) = (w as f32, h as f32);
    let (cx, cy) = (fw / 2.0, fh / 2.0);
    // one pixel of margin so the anti-aliased edge is never cut; a pressed button is a pixel smaller
    let shrink = if variant == Variant::Pressed { 1.5 } else { 1.0 };
    let (hx, hy) = (fw / 2.0 - shrink, fh / 2.0 - shrink);
    let radius = hx.min(hy);
    let sparkle_r = hx.min(hy) * 0.78;
    let sparkle_cx = cx - fw * 0.03;
    let sparkle_cy = cy + fh * 0.03;
    let small_r = hx.min(hy) * 0.30;
    let (small_cx, small_cy) = (cx + hx * 0.50, cy - hy * 0.52);
    let (lift, dim) = match variant {
        Variant::Hover => (0.22, 1.0),
        Variant::Active => (0.12, 1.0),
        Variant::Pressed => (0.0, 0.84),
        Variant::Normal => (0.0, 1.0),
    };
    let ring_px = if variant == Variant::Active && palette.ring_px == 0.0 { 1.5 } else { palette.ring_px };

    for py in 0..h {
        for px in 0..w {
            let (x, y) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
            let body = coverage(sd_round_rect(x, y, hx, hy, radius));
            if body <= 0.0 {
                continue;
            }
            // diagonal gradient, top-left to bottom-right
            let t = ((px as f32 + 0.5) / fw + (py as f32 + 0.5) / fh) / 2.0;
            let mut c = gradient(&palette.stops, t);
            if lift > 0.0 {
                c = lerp(c, [255.0; 3], lift);
            }
            for v in &mut c {
                *v *= dim;
            }
            if ring_px > 0.0 {
                // a band of ring_px just inside the outer edge (soft inner edge)
                let depth = -sd_round_rect(x, y, hx, hy, radius);
                let band = 1.0 - (depth - ring_px + 0.5).clamp(0.0, 1.0);
                c = lerp(c, palette.ring, band);
            }
            // sparkles over the fill
            let s1 = coverage(sd_sparkle(x + (cx - sparkle_cx), y + (cy - sparkle_cy), sparkle_r));
            let s2 = coverage(sd_sparkle(x + (cx - small_cx), y + (cy - small_cy), small_r)) * 0.9;
            let s = s1.max(s2);
            let c = lerp(c, palette.sparkle, s);
            let i = (py * w + px) * 4;
            // premultiply: the pill coverage is the alpha (the sparkle sits inside it)
            out[i] = (c[2] * body).round().clamp(0.0, 255.0) as u8;
            out[i + 1] = (c[1] * body).round().clamp(0.0, 255.0) as u8;
            out[i + 2] = (c[0] * body).round().clamp(0.0, 255.0) as u8;
            out[i + 3] = (body * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

// =============================================================================
// Window
// =============================================================================

#[derive(Clone, Copy)]
struct Look {
    width: i32,
    height: i32,
    hover: bool,
    pressed: bool,
    active: bool,
    high_contrast: bool,
}

thread_local! {
    static LOOK: Cell<Look> = const { Cell::new(Look { width: 24, height: 24, hover: false, pressed: false, active: false, high_contrast: false }) };
    static TOOLTIP_TEXT: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
    /// What a click does; injected so this module (and its tests) does not reach the app layer.
    static ON_CLICK: Cell<Option<fn()>> = const { Cell::new(None) };
}

pub struct Button {
    pub hwnd: HWND,
    tooltip: HWND,
    visible: bool,
    position: Option<(i32, i32)>,
}

fn variant_of(look: &Look) -> Variant {
    if look.pressed {
        Variant::Pressed
    } else if look.hover {
        Variant::Hover
    } else if look.active {
        Variant::Active
    } else {
        Variant::Normal
    }
}

fn colour(c: u32) -> [f32; 3] {
    [(c & 0xFF) as f32, ((c >> 8) & 0xFF) as f32, ((c >> 16) & 0xFF) as f32]
}

fn palette_for(high_contrast: bool) -> Palette {
    if high_contrast {
        unsafe { Palette::high_contrast(colour(GetSysColor(COLOR_WINDOW)), colour(GetSysColor(COLOR_WINDOWTEXT))) }
    } else {
        Palette::colour()
    }
}

/// Push the current look into the layered window; `at` moves it in the same call.
unsafe fn present(hwnd: HWND, look: &Look, at: Option<(i32, i32)>) {
    // a DIB is never 0x0 (or negative): CreateDIBSection and the pixel copy would fail
    let look = &Look { width: look.width.clamp(1, 4096), height: look.height.clamp(1, 4096), ..*look };
    let pixels = render(look.width as u32, look.height as u32, variant_of(look), &palette_for(look.high_contrast));
    let screen = GetDC(None);
    let memory = CreateCompatibleDC(screen);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: look.width,
            biHeight: -look.height, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    if let Ok(dib) = CreateDIBSection(memory, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
            let old = SelectObject(memory, dib);
            let size = SIZE { cx: look.width, cy: look.height };
            let source = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let destination = at.map(|(x, y)| POINT { x, y });
            let result = UpdateLayeredWindow(
                hwnd,
                screen,
                destination.as_ref().map(|p| p as *const POINT),
                Some(&size),
                memory,
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            if result.is_err() {
                dlog!("WARN", "search_bar", "WIN-506 button bitmap could not be applied");
            }
            SelectObject(memory, old);
        }
        let _ = DeleteObject(HGDIOBJ(dib.0));
    }
    let _ = DeleteDC(memory);
    ReleaseDC(None, screen);
}

fn redraw(hwnd: HWND) {
    LOOK.with(|l| unsafe { present(hwnd, &l.get(), None) });
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A panic must not cross the FFI boundary: fall back to the default handling.
    match super::guard::guarded("button window proc", None, || Some(proc_inner(hwnd, msg, wparam, lparam))) {
        Some(result) => result,
        None => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn proc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
                windows::Win32::UI::WindowsAndMessaging::SetCursor(cursor);
                return LRESULT(1);
            }
        }
        WM_MOUSEMOVE => {
            let changed = LOOK.with(|l| {
                let mut look = l.get();
                let changed = !look.hover;
                look.hover = true;
                l.set(look);
                changed
            });
            if changed {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut track);
                redraw(hwnd);
            }
        }
        WM_MOUSELEAVE => {
            LOOK.with(|l| {
                let mut look = l.get();
                look.hover = false;
                look.pressed = false;
                l.set(look);
            });
            redraw(hwnd);
        }
        WM_LBUTTONDOWN => {
            LOOK.with(|l| {
                let mut look = l.get();
                look.pressed = true;
                l.set(look);
            });
            SetCapture(hwnd);
            redraw(hwnd);
        }
        WM_LBUTTONUP => {
            let was_pressed = LOOK.with(|l| {
                let mut look = l.get();
                let was = look.pressed;
                look.pressed = false;
                l.set(look);
                was
            });
            let _ = ReleaseCapture();
            redraw(hwnd);
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let inside = LOOK.with(|l| {
                let look = l.get();
                x >= 0 && y >= 0 && x < look.width && y < look.height
            });
            if was_pressed && inside {
                if let Some(click) = ON_CLICK.with(|c| c.get()) {
                    click();
                }
            }
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn register_class() {
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        // Registering twice (the button is rebuilt when the setting is toggled) fails harmlessly.
        let _ = RegisterClassExW(&class);
    }
}

impl Button {
    pub fn create(on_click: fn()) -> Option<Button> {
        ON_CLICK.with(|c| c.set(Some(on_click)));
        register_class();
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                CLASS,
                w!(""),
                WS_POPUP,
                0,
                0,
                24,
                24,
                None,
                None,
                instance,
                None,
            )
            .ok()?;
            let tooltip = CreateWindowExW(
                WS_EX_TOPMOST,
                w!("tooltips_class32"),
                PCWSTR::null(),
                WS_POPUP | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                0,
                0,
                0,
                0,
                hwnd,
                None,
                instance,
                None,
            )
            .unwrap_or_default();
            if !tooltip.0.is_null() {
                TOOLTIP_TEXT.with(|t| {
                    if let Ok(mut text) = t.try_borrow_mut() {
                        *text = TOOLTIP.encode_utf16().chain(std::iter::once(0)).collect();
                    }
                });
                let info = TTTOOLINFOW {
                    cb_size: std::mem::size_of::<TTTOOLINFOW>() as u32,
                    flags: TTF_IDISHWND | TTF_SUBCLASS | TTF_RTLREADING,
                    hwnd,
                    id: hwnd.0 as usize,
                    rect: RECT::default(),
                    instance: HINSTANCE::default(),
                    text: PWSTR(TOOLTIP_TEXT.with(|t| t.try_borrow_mut().map_or(std::ptr::null_mut(), |mut v| v.as_mut_ptr()))),
                    param: LPARAM(0),
                    reserved: std::ptr::null_mut(),
                };
                SendMessageW(tooltip, TTM_ADDTOOLW, WPARAM(0), LPARAM(&info as *const TTTOOLINFOW as isize));
            }
            Some(Button { hwnd, tooltip, visible: false, position: None })
        }
    }

    /// Move/size to `rect` (physical px), render for the size, and show without activating. Keeps
    /// the window above the taskbar (HWND_TOPMOST) only when `raise`.
    pub fn place(&mut self, rect: Bounds, high_contrast: bool, active: bool, raise: bool) {
        let (width, height) = (rect.width().max(1), rect.height().max(1));
        let prev = LOOK.with(|l| l.get());
        let look = Look { width, height, high_contrast, active, ..prev };
        let needs_render = prev.width != width || prev.height != height || prev.high_contrast != high_contrast
            || prev.active != active || !self.visible;
        let moved = self.position != Some((rect.left, rect.top));
        LOOK.with(|l| l.set(look));
        unsafe {
            if needs_render || moved {
                present(self.hwnd, &look, Some((rect.left, rect.top)));
                self.position = Some((rect.left, rect.top));
            }
            if !self.visible || raise {
                let _ = SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
                );
            }
        }
        self.visible = true;
    }

    pub fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }

    pub fn set_active(&mut self, active: bool) {
        let changed = LOOK.with(|l| {
            let mut look = l.get();
            let changed = look.active != active;
            look.active = active;
            l.set(look);
            changed
        });
        if changed && self.visible {
            redraw(self.hwnd);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn destroy(self) {
        unsafe {
            if !self.tooltip.0.is_null() {
                let _ = DestroyWindow(self.tooltip);
            }
            let _ = DestroyWindow(self.hwnd);
        }
        LOOK.with(|l| {
            let mut look = l.get();
            look.hover = false;
            look.pressed = false;
            l.set(look);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn premultiplied_invariant_holds_everywhere() {
        for size in [24u32, 30, 36, 42, 48] {
            for variant in [Variant::Normal, Variant::Hover, Variant::Pressed, Variant::Active] {
                for palette in [Palette::colour(), Palette::high_contrast([0.0, 0.0, 0.0], [255.0, 255.0, 0.0])] {
                    let buf = render(size, size, variant, &palette);
                    assert_eq!(buf.len(), (size * size * 4) as usize);
                    for p in buf.chunks(4) {
                        // premultiplied: no channel may exceed alpha
                        assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3], "size {size} {variant:?}: {p:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn centre_is_opaque_and_corners_are_transparent() {
        for size in [24usize, 30, 36, 48] {
            let buf = render(size as u32, size as u32, Variant::Normal, &Palette::colour());
            // the centre is covered by the white sparkle
            let c = px(&buf, size, size / 2, size / 2);
            assert_eq!(c[3], 255, "size {size}");
            for (x, y) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
                assert_eq!(px(&buf, size, x, y), [0, 0, 0, 0], "size {size} corner {x},{y}");
            }
        }
    }

    #[test]
    fn edges_are_anti_aliased() {
        let size = 48usize;
        let buf = render(size as u32, size as u32, Variant::Normal, &Palette::colour());
        // somewhere along the rim there are partial alphas
        let partial = buf.chunks(4).filter(|p| p[3] > 0 && p[3] < 255).count();
        assert!(partial >= size, "{partial} partial pixels");
        // and the body is mostly opaque
        let opaque = buf.chunks(4).filter(|p| p[3] == 255).count();
        assert!(opaque > size * size / 2);
    }

    #[test]
    fn the_gradient_runs_from_cyan_to_magenta() {
        let size = 48usize;
        let buf = render(size as u32, size as u32, Variant::Normal, &Palette::colour());
        // away from the sparkles: upper-left is cyan-ish (blue channel high, red low),
        // lower-right is magenta-ish (red high)
        let tl = px(&buf, size, 9, 14); // BGRA
        let br = px(&buf, size, 38, 36);
        assert!(tl[2] < br[2], "red grows towards the bottom right: {tl:?} {br:?}");
        assert!(tl[1] > br[1] || tl[0] > 0, "green is high at the cyan end: {tl:?} {br:?}");
    }

    #[test]
    fn variants_differ() {
        let n = render(36, 36, Variant::Normal, &Palette::colour());
        let h = render(36, 36, Variant::Hover, &Palette::colour());
        let p = render(36, 36, Variant::Pressed, &Palette::colour());
        let a = render(36, 36, Variant::Active, &Palette::colour());
        assert_ne!(n, h);
        assert_ne!(n, p);
        assert_ne!(n, a);
        // hover is brighter, pressed darker (sum of the colour channels at a body pixel)
        let sum = |b: &[u8]| -> u32 { b.chunks(4).map(|q| q[0] as u32 + q[1] as u32 + q[2] as u32).sum() };
        assert!(sum(&h) > sum(&n));
        assert!(sum(&p) < sum(&n));
    }

    #[test]
    fn high_contrast_uses_only_the_given_colours() {
        let palette = Palette::high_contrast([0.0, 0.0, 0.0], [255.0, 255.0, 255.0]);
        let buf = render(48, 48, Variant::Normal, &palette);
        // no hue: every opaque pixel is grey (blue == green == red)
        for p in buf.chunks(4).filter(|p| p[3] == 255) {
            assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
        }
        // the ring is at the rim and the fill is black: the pixel just inside the left edge is lit
        let rim = px(&buf, 48, 2, 24);
        assert!(rim[0] > 100, "{rim:?}");
        let inside = px(&buf, 48, 9, 30);
        assert_eq!(inside[3], 255);
        assert_eq!(inside[0], 0);
    }

    #[test]
    fn degenerate_sizes_do_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (2, 40), (40, 2)] {
            let buf = render(w, h, Variant::Hover, &Palette::colour());
            assert_eq!(buf.len(), (w.max(1) * h.max(1) * 4) as usize);
        }
    }

    #[test]
    fn a_wide_pill_keeps_round_ends() {
        let buf = render(60, 24, Variant::Normal, &Palette::colour());
        assert_eq!(px(&buf, 60, 0, 0)[3], 0);
        assert_eq!(px(&buf, 60, 59, 23)[3], 0);
        assert_eq!(px(&buf, 60, 30, 12)[3], 255);
        // the flat middle of the top edge is covered, the rounded end corner is not
        assert!(px(&buf, 60, 30, 2)[3] > 0);
    }

    #[test]
    fn the_button_window_is_created_and_painted() {
        // Real layered window on this thread: creation, layered update, hide, destroy.
        let Some(mut b) = Button::create(|| {}) else { return };
        b.place(Bounds { left: 20, top: 20, right: 44, bottom: 44 }, false, false, true);
        assert!(b.is_visible());
        b.set_active(true);
        b.place(Bounds { left: 30, top: 20, right: 66, bottom: 56 }, false, true, false);
        b.hide();
        assert!(!b.is_visible());
        b.destroy();
    }
}
