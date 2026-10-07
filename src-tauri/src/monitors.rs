//! Monitor enumeration and the island's placement math (pure, unit-tested).
//!
//! All coordinates are physical pixels in virtual-screen space. The island's size
//! arrives in logical px and is converted with the *target monitor's* effective DPI,
//! rounded exactly once, so there is no fractional jitter at 100-200% scaling and no
//! mixing of the primary monitor's scale with another monitor's geometry.

use serde::Serialize;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

const BASE_DPI: f64 = 96.0;
const MONITORINFOF_PRIMARY: u32 = 1;
/// The window region is cut this many physical px *outside* the CSS corner curve so
/// the antialiased edge pixels of the pill are never clipped (a hard-edged region
/// exactly on the curve would make the rim look jagged).
const REGION_INSET_PX: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Bounds {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn contains(&self, inner: &Bounds) -> bool {
        self.left <= inner.left && self.top <= inner.top && self.right >= inner.right && self.bottom >= inner.bottom
    }
}

impl From<RECT> for Bounds {
    fn from(r: RECT) -> Self {
        Bounds { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

#[derive(Clone, Debug)]
pub struct Monitor {
    /// Device name such as `\\.\DISPLAY1`: no serial number, no user data.
    pub device: String,
    pub bounds: Bounds,
    pub primary: bool,
    /// Effective DPI (96 = 100%).
    pub dpi: u32,
}

/// What the settings UI needs to offer a display choice.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    /// Index in `list()` order, as a string (the `settings.monitor` value).
    pub id: String,
    pub name: String,
    pub primary: bool,
    /// Same as `primary`; the frontend normalizer reads this name.
    pub is_primary: bool,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

impl Monitor {
    pub fn scale(&self) -> f64 {
        self.dpi as f64 / BASE_DPI
    }
}

pub fn infos(monitors: &[Monitor]) -> Vec<MonitorInfo> {
    monitors
        .iter()
        .enumerate()
        .map(|(index, m)| MonitorInfo {
            id: index.to_string(),
            name: format!(
                "{} ({}x{})",
                m.device.trim_start_matches(r"\\.\"),
                m.bounds.width(),
                m.bounds.height()
            ),
            primary: m.primary,
            is_primary: m.primary,
            width: m.bounds.width().max(0) as u32,
            height: m.bounds.height().max(0) as u32,
            scale: m.scale(),
        })
        .collect()
}

unsafe extern "system" fn collect(handle: HMONITOR, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
    let out = &mut *(data.0 as *mut Vec<Monitor>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(handle, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
        let dpi = match GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) {
            Ok(()) if dpi_x > 0 => dpi_x,
            _ => BASE_DPI as u32,
        };
        let name_len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        out.push(Monitor {
            device: String::from_utf16_lossy(&info.szDevice[..name_len]),
            bounds: info.monitorInfo.rcMonitor.into(),
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            dpi,
        });
    }
    BOOL(1)
}

/// Connected monitors sorted by (left, top), which gives the index stable meaning
/// for `settings.monitor`. Empty only if enumeration fails (WIN-503).
pub fn list() -> Vec<Monitor> {
    let mut monitors: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(collect),
            LPARAM(&mut monitors as *mut Vec<Monitor> as isize),
        );
    }
    monitors.sort_by_key(|m| (m.bounds.left, m.bounds.top));
    monitors
}

/// Resolve `settings.monitor` (`"primary"` or an index). A missing index (monitor
/// unplugged) falls back to the primary monitor so the island is never stranded.
pub fn pick<'a>(setting: &str, monitors: &'a [Monitor]) -> Option<&'a Monitor> {
    let primary = || monitors.iter().find(|m| m.primary).or_else(|| monitors.first());
    match setting.trim().parse::<usize>() {
        Ok(index) => monitors.get(index).or_else(primary),
        Err(_) => primary(),
    }
}

fn to_physical(logical: f64, dpi: u32) -> i32 {
    (logical * dpi as f64 / BASE_DPI).round() as i32
}

/// Window rectangle for an island of `width` x `height` logical px: horizontally
/// centered on the monitor, flush with the monitor's top edge, clamped inside it.
///
/// The top is `rcMonitor.top`, not the work area: like Apple's island it sits at the
/// very top of the screen. With a top-docked taskbar the island overlays the taskbar's
/// middle; the taskbar can raise itself above it when activated, and the next display
/// event or geometry call re-asserts topmost.
pub fn island_bounds(monitor: Bounds, dpi: u32, width: f64, height: f64) -> Bounds {
    let w = to_physical(width, dpi).clamp(1, monitor.width().max(1));
    let h = to_physical(height, dpi).clamp(1, monitor.height().max(1));
    let x = (monitor.left + (monitor.width() - w) / 2).clamp(monitor.left, (monitor.right - w).max(monitor.left));
    let y = monitor.top;
    Bounds { left: x, top: y, right: x + w, bottom: y + h }
}

/// The island's rectangle inside its stage window (window-relative physical px): horizontally
/// centred and flush with the top, exactly where the page lays the island out. It is widened by
/// one pixel on each side (and below) so the island's antialiased edge is never cut by the
/// rounding of the centre, and it never leaves the window.
pub fn island_region(window_width: i32, window_height: i32, dpi: u32, width: f64, height: f64) -> Bounds {
    let w = to_physical(width, dpi).clamp(1, window_width.max(1));
    let h = to_physical(height, dpi).clamp(1, window_height.max(1));
    let left = (window_width - w) / 2;
    Bounds {
        left: (left - 1).max(0),
        top: 0,
        right: (left + w + 1).min(window_width),
        bottom: (h + 1).min(window_height),
    }
}

/// Corner radius in physical px for the window region, or 0 for "no region".
pub fn region_radius(radius: f64, dpi: u32, width_px: i32, height_px: i32) -> i32 {
    let max = (width_px.min(height_px) / 2).max(0);
    (to_physical(radius, dpi).clamp(0, max) - REGION_INSET_PX).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FHD: Bounds = Bounds { left: 0, top: 0, right: 1920, bottom: 1080 };

    fn monitor(left: i32, top: i32, primary: bool) -> Monitor {
        Monitor {
            device: format!(r"\\.\DISPLAY{}", left),
            bounds: Bounds { left, top, right: left + 1920, bottom: top + 1080 },
            primary,
            dpi: 96,
        }
    }

    #[test]
    fn centers_at_every_common_scale_without_jitter() {
        // (dpi, logical width, logical height) -> exact physical rect
        let cases = [
            (96, 200.0, 44.0, Bounds { left: 860, top: 0, right: 1060, bottom: 44 }),
            (120, 200.0, 44.0, Bounds { left: 835, top: 0, right: 1085, bottom: 55 }),
            (144, 200.0, 44.0, Bounds { left: 810, top: 0, right: 1110, bottom: 66 }),
            (168, 200.0, 44.0, Bounds { left: 785, top: 0, right: 1135, bottom: 77 }),
            (192, 200.0, 44.0, Bounds { left: 760, top: 0, right: 1160, bottom: 88 }),
            // fractional physical sizes are rounded once: 133 * 1.75 = 232.75 -> 233
            (168, 133.0, 37.0, Bounds { left: 843, top: 0, right: 1076, bottom: 65 }),
        ];
        for (dpi, w, h, expected) in cases {
            assert_eq!(island_bounds(FHD, dpi, w, h), expected, "dpi {dpi} size {w}x{h}");
        }
    }

    #[test]
    fn follows_monitor_origin() {
        let left = Bounds { left: -1920, top: 0, right: 0, bottom: 1080 };
        assert_eq!(island_bounds(left, 96, 200.0, 44.0).left, -1060);
        let above = Bounds { left: 0, top: -1080, right: 1920, bottom: 0 };
        assert_eq!(island_bounds(above, 96, 200.0, 44.0).top, -1080);
    }

    #[test]
    fn clamps_inside_the_monitor() {
        let b = island_bounds(FHD, 96, 5000.0, 5000.0);
        assert_eq!(b, FHD);
        let tiny = island_bounds(FHD, 96, 0.2, 0.2);
        assert_eq!((tiny.width(), tiny.height()), (1, 1));
        // result always lies within the monitor
        for dpi in [96, 120, 144, 168, 192] {
            let r = island_bounds(FHD, dpi, 404.0, 420.0);
            assert!(FHD.contains(&r));
        }
    }

    #[test]
    fn island_region_is_centred_in_the_stage_at_every_scale() {
        for dpi in [96, 120, 144, 168, 192] {
            let stage = island_bounds(FHD, dpi, 404.0, 420.0);
            for (w, h) in [(142.0, 34.0), (404.0, 420.0), (380.0, 131.0), (200.0, 48.0)] {
                let r = island_region(stage.width(), stage.height(), dpi, w, h);
                let island_px = to_physical(w, dpi);
                // covers the island, with at most a pixel of slack on each side
                assert!(r.width() >= island_px && r.width() <= island_px + 2, "dpi {dpi} {w}x{h}: {r:?}");
                // centred: both margins differ by at most one pixel
                let (left, right) = (r.left, stage.width() - r.right);
                assert!((left - right).abs() <= 1, "dpi {dpi} {w}x{h}: {left} vs {right}");
                assert_eq!(r.top, 0);
                assert!(r.bottom <= stage.height());
            }
        }
    }

    #[test]
    fn island_region_never_leaves_the_window() {
        let r = island_region(200, 44, 96, 500.0, 500.0);
        assert_eq!(r, Bounds { left: 0, top: 0, right: 200, bottom: 44 });
    }

    #[test]
    fn region_radius_is_scaled_clamped_and_inset() {
        assert_eq!(region_radius(22.0, 96, 200, 44), 20);
        assert_eq!(region_radius(22.0, 192, 400, 88), 42);
        // a radius larger than half the height is clamped to a pill
        assert_eq!(region_radius(500.0, 96, 200, 44), 20);
        // tiny radii disable the region
        assert_eq!(region_radius(1.0, 96, 200, 44), 0);
        assert_eq!(region_radius(0.0, 96, 200, 44), 0);
    }

    #[test]
    fn picks_primary_index_or_falls_back() {
        let list = [monitor(-1920, 0, false), monitor(0, 0, true), monitor(1920, 0, false)];
        assert_eq!(pick("primary", &list).unwrap().bounds.left, 0);
        assert_eq!(pick("2", &list).unwrap().bounds.left, 1920);
        assert_eq!(pick("0", &list).unwrap().bounds.left, -1920);
        // unplugged monitor, junk, empty -> primary
        assert_eq!(pick("7", &list).unwrap().bounds.left, 0);
        assert_eq!(pick("", &list).unwrap().bounds.left, 0);
        assert_eq!(pick("left", &list).unwrap().bounds.left, 0);
        assert!(pick("0", &[]).is_none());
        // no flagged primary: first monitor
        let no_primary = [monitor(5, 0, false)];
        assert_eq!(pick("primary", &no_primary).unwrap().bounds.left, 5);
    }

    #[test]
    fn infos_expose_index_ids_and_both_primary_names() {
        let list = [monitor(0, 0, true), monitor(1920, 0, false)];
        let infos = infos(&list);
        assert_eq!(infos[0].id, "0");
        assert_eq!(infos[1].id, "1");
        assert!(infos[0].primary && infos[0].is_primary && !infos[1].primary);
        assert_eq!((infos[0].width, infos[0].height), (1920, 1080));
        assert_eq!(infos[0].scale, 1.0);
        let json = serde_json::to_value(&infos[0]).unwrap();
        assert!(json.get("isPrimary").is_some() && json.get("primary").is_some());
    }

    #[test]
    fn live_enumeration_finds_a_primary_monitor() {
        let monitors = list();
        assert!(!monitors.is_empty(), "no display on this machine");
        assert!(monitors.iter().any(|m| m.primary));
        assert!(monitors.iter().all(|m| m.dpi >= 96 && m.bounds.width() > 0 && m.bounds.height() > 0));
        let target = pick("primary", &monitors).unwrap();
        assert!(target.bounds.contains(&island_bounds(target.bounds, target.dpi, 404.0, 420.0)));
        assert!(monitors.windows(2).all(|w| (w[0].bounds.left, w[0].bounds.top) <= (w[1].bounds.left, w[1].bounds.top)));
    }
}
