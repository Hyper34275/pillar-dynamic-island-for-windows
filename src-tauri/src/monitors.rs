//! Monitor enumeration and the island's placement math (pure, unit-tested).
//!
//! All coordinates are physical pixels in virtual-screen space. The island's size
//! arrives in logical px (fractional DIPs, never rounded by the frontend) and is converted
//! with the *target monitor's* effective DPI. Pixels are snapped exactly once, here, and
//! always by EDGES, never by sizes: a span is `left = round(centre - extent/2)`,
//! `right = round(centre + extent/2)` and its width is `right - left`. Rounding a width and an
//! x independently makes the centre drift by up to a pixel (and an "even DIP" width does not
//! help: 2 DIP is 2.5 px at 125%). Snapping the edges keeps the centre within half a physical
//! pixel of the float centre at every scale, and never mixes the primary monitor's scale with
//! another monitor's geometry.
//!
//! The stage window starts at the monitor's top edge. The island is drawn `ISLAND_TOP_INSET`
//! below it (the visual gap), and the window region adds a "bridge" over that gap so a pointer
//! thrown against the screen's top edge still lands on the island.

use serde::Serialize;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_TOP, ABM_GETAUTOHIDEBAREX, APPBARDATA};

const BASE_DPI: f64 = 96.0;
const MONITORINFOF_PRIMARY: u32 = 1;
/// The window region is cut this many physical px *outside* the CSS corner curve so
/// the antialiased edge pixels of the pill are never clipped (a hard-edged region
/// exactly on the curve would make the rim look jagged).
const REGION_INSET_PX: i32 = 2;
/// Gap between the screen's top edge and the island (logical px, = design token
/// `island top inset` in docs/DESIGN_SYSTEM.md): the island reads as an object suspended at the top
/// centre rather than a shape clipped by the edge. Scaled with the monitor's DPI like every size.
pub const ISLAND_TOP_INSET: f64 = 8.0;
/// Room kept between the panel and the monitor's left/right edges (logical px). Mirrors
/// `tokens.panel.inset` (12), the panel's own inner margin, so a panel squeezed by a small
/// screen keeps the same breathing room around it as inside it.
pub const PANEL_SIDE_MARGIN: f64 = 12.0;
/// Room kept between the panel and the work area's bottom edge (logical px); same value and
/// reason as `PANEL_SIDE_MARGIN` (`tokens.panel.inset`).
pub const PANEL_BOTTOM_MARGIN: f64 = 12.0;

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
    /// `rcWork`: the monitor minus the taskbar and other app bars.
    pub work: Bounds,
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

    /// The largest the island's shapes can be on this monitor (see `island_limits`).
    pub fn limits(&self) -> IslandLimits {
        island_limits(self.bounds, self.work, self.dpi)
    }

    /// True when the work area starts at the monitor's top edge (no top-docked taskbar).
    pub fn top_is_free(&self) -> bool {
        self.work.top <= self.bounds.top
    }
}

/// How large the island may be on one monitor, in logical px, plus the monitor's scale. The
/// frontend's preferred panel (400x440) is shrunk to fit; the scrolling body absorbs the change.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IslandLimits {
    pub max_width: f64,
    pub max_height: f64,
    pub scale: f64,
}

/// Limits for an island on a monitor: the width is the monitor's minus a side margin on each
/// side; the height runs from the island's top (monitor top + inset) to the work area's bottom
/// (so the taskbar is never covered) minus a bottom margin. Never negative.
pub fn island_limits(monitor: Bounds, work: Bounds, dpi: u32) -> IslandLimits {
    let scale = dpi as f64 / BASE_DPI;
    let island_top = monitor.top as f64 + ISLAND_TOP_INSET * scale;
    IslandLimits {
        max_width: (monitor.width() as f64 / scale - 2.0 * PANEL_SIDE_MARGIN).max(0.0),
        max_height: ((work.bottom as f64 - island_top) / scale - PANEL_BOTTOM_MARGIN).max(0.0),
        scale,
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
            work: info.monitorInfo.rcWork.into(),
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

/// Snap a span given by its float centre and extent to whole pixels by its EDGES (round both),
/// so the centre moves by at most half a pixel whatever the extent. At least 1 px wide.
fn snap_span(centre: f64, extent: f64) -> (i32, i32) {
    let left = (centre - extent / 2.0).round() as i32;
    let right = (centre + extent / 2.0).round() as i32;
    (left, right.max(left + 1))
}

/// The stage window's rectangle for a stage of `width` x `height` logical px: horizontally
/// centred on the monitor (edges snapped from the float centre, see `snap_span`), flush with
/// the monitor's top edge (the island's visual gap is drawn inside the page, and the frontend
/// includes it in `height`), clamped inside the monitor.
///
/// The top is the monitor's, not the work area's: like Apple's island it sits at the top of the
/// screen. With a top-docked taskbar the island overlays the taskbar's middle; the taskbar can
/// raise itself above it when activated, and the next display event or geometry call
/// re-asserts topmost.
pub fn island_bounds(monitor: Bounds, dpi: u32, width: f64, height: f64) -> Bounds {
    let scale = dpi as f64 / BASE_DPI;
    let w_f = (width * scale).clamp(1.0, monitor.width().max(1) as f64);
    let h_f = (height * scale).clamp(1.0, monitor.height().max(1) as f64);
    let centre = monitor.left as f64 + monitor.width() as f64 / 2.0;
    let (mut left, mut right) = snap_span(centre, w_f);
    // Inside the monitor, keeping the snapped width.
    if left < monitor.left {
        right += monitor.left - left;
        left = monitor.left;
    }
    if right > monitor.right {
        left -= right - monitor.right;
        right = monitor.right;
    }
    left = left.max(monitor.left);
    right = right.min(monitor.right).max(left + 1);
    let bottom = ((monitor.top as f64 + h_f).round() as i32).clamp(monitor.top + 1, monitor.bottom.max(monitor.top + 1));
    Bounds { left, top: monitor.top, right, bottom }
}

/// The island's rectangle inside its stage window (window-relative physical px), exactly where
/// the page lays it out: centred on `window_width / 2` (a float: the page's centre is the
/// window's, not a whole pixel) and ISLAND_TOP_INSET below the top. Its edges are snapped
/// OUTWARD (left and top floor, right and bottom ceil) so the region always covers the float
/// extent of the island, then widened by one more pixel on each side and below so the
/// antialiased edge is never cut. Never leaves the window.
pub fn island_region(window_width: i32, window_height: i32, dpi: u32, width: f64, height: f64) -> Bounds {
    let scale = dpi as f64 / BASE_DPI;
    let w_f = (width * scale).clamp(1.0, window_width.max(1) as f64);
    let centre = window_width as f64 / 2.0;
    let top_f = ISLAND_TOP_INSET * scale;
    let h_f = (height * scale).max(1.0);
    Bounds {
        left: ((centre - w_f / 2.0).floor() as i32 - 1).max(0),
        top: (top_f.floor() as i32 - 1).clamp(0, window_height.max(0)),
        right: ((centre + w_f / 2.0).ceil() as i32 + 1).min(window_width),
        bottom: ((top_f + h_f).ceil() as i32 + 1).min(window_height),
    }
}

/// The two parts of the window region. The island's rounded shape is drawn inside the page
/// `ISLAND_TOP_INSET` below the monitor's top edge; `bridge` fills that gap (from the top of the
/// window to the top of the island, only as wide as the island) so a pointer pushed against the
/// screen's top edge lands on the island (an edge is an infinitely deep target).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionShape {
    pub island: Bounds,
    pub bridge: Option<Bounds>,
}

/// `reaches_edge`: the monitor's work area starts at the monitor's top (no top-docked taskbar).
/// With a top taskbar the bridge would sit on the taskbar and swallow clicks meant for it, so it
/// is left out; the island then just floats below the edge.
pub fn region_shape(window_width: i32, window_height: i32, dpi: u32, width: f64, height: f64, reaches_edge: bool) -> RegionShape {
    let island = island_region(window_width, window_height, dpi, width, height);
    let bridge = (reaches_edge && island.top > 0).then_some(Bounds { left: island.left, top: 0, right: island.right, bottom: island.top });
    RegionShape { island, bridge }
}

/// Whether the invisible top bridge (the region strip over the 8 DIP gap) may exist on a monitor.
/// It must not when anything of the shell lives on that top edge:
///   - `work_reaches_top` false: a top-docked taskbar (or other app bar) owns the strip; the bridge
///     would sit on it and swallow its clicks;
///   - `top_autohide_bar` true: a taskbar docked at the TOP with auto-hide. Its work area still
///     reaches the monitor's top (the bar is hidden), so `work_reaches_top` alone says "free", but
///     the hidden bar is revealed by the pointer touching the screen's top edge and a topmost window
///     covering that edge would steal the touch. The island starts ISLAND_TOP_INSET below the edge,
///     so without the bridge the strip above it is not part of the window at all.
pub fn bridge_wanted(work_reaches_top: bool, top_autohide_bar: bool) -> bool {
    work_reaches_top && !top_autohide_bar
}

/// Whether an auto-hide app bar (the Windows taskbar) is registered on the TOP edge of the monitor
/// with these bounds: `ABM_GETAUTOHIDEBAREX` with the monitor's rectangle answers per monitor, so a
/// taskbar auto-hiding on the top of another monitor does not count. A cross-process call into the
/// shell: the caller caches the answer and asks again only on a display or settings change.
pub fn top_autohide_bar(monitor: Bounds) -> bool {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        uEdge: ABE_TOP,
        rc: RECT { left: monitor.left, top: monitor.top, right: monitor.right, bottom: monitor.bottom },
        ..Default::default()
    };
    // The answer is the bar's window handle, 0 when there is none.
    unsafe { SHAppBarMessage(ABM_GETAUTOHIDEBAREX, &mut data) != 0 }
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
    /// Every scale Windows offers between 100% and 250% (240 DPI = 250%).
    const DPIS: [u32; 6] = [96, 120, 144, 168, 192, 240];

    fn monitor(left: i32, top: i32, primary: bool) -> Monitor {
        let bounds = Bounds { left, top, right: left + 1920, bottom: top + 1080 };
        Monitor { device: format!(r"\\.\DISPLAY{}", left), bounds, work: bounds, primary, dpi: 96 }
    }

    #[test]
    fn snaps_edges_from_the_float_centre_at_every_common_scale() {
        // (dpi, logical width, logical height) -> exact physical rect. The stage is flush with the
        // monitor's top: the island's 8 DIP gap lives inside the page.
        let cases = [
            (96, 200.0, 44.0, Bounds { left: 860, top: 0, right: 1060, bottom: 44 }),
            (120, 200.0, 44.0, Bounds { left: 835, top: 0, right: 1085, bottom: 55 }),
            (144, 200.0, 44.0, Bounds { left: 810, top: 0, right: 1110, bottom: 66 }),
            (168, 200.0, 44.0, Bounds { left: 785, top: 0, right: 1135, bottom: 77 }),
            (192, 200.0, 44.0, Bounds { left: 760, top: 0, right: 1160, bottom: 88 }),
            // 133 * 1.75 = 232.75 wide: both edges are rounded (843.625 -> 844, 1076.375 -> 1076),
            // not the width and the x separately.
            (168, 133.0, 37.0, Bounds { left: 844, top: 0, right: 1076, bottom: 65 }),
        ];
        for (dpi, w, h, expected) in cases {
            assert_eq!(island_bounds(FHD, dpi, w, h), expected, "dpi {dpi} size {w}x{h}");
        }
    }

    #[test]
    fn stage_centre_never_drifts_more_than_half_a_pixel() {
        // Fractional logical sizes (a measured text width is never a whole DIP), odd and even
        // monitors, every scale.
        let odd = Bounds { left: -1367, top: 0, right: 0, bottom: 768 };
        for monitor in [FHD, odd] {
            let centre = (monitor.left + monitor.right) as f64 / 2.0;
            for dpi in DPIS {
                let scale = dpi as f64 / 96.0;
                for width in [141.37, 142.0, 233.9, 368.0, 400.0, 177.11] {
                    let b = island_bounds(monitor, dpi, width, 36.0);
                    let drift = (b.left + b.right) as f64 / 2.0 - centre;
                    assert!(drift.abs() <= 0.5, "dpi {dpi} width {width}: centre off by {drift}");
                    let extent = width * scale;
                    assert_eq!(b.left, (centre - extent / 2.0).round() as i32, "dpi {dpi} width {width}: left edge");
                    assert_eq!(b.right, (centre + extent / 2.0).round() as i32, "dpi {dpi} width {width}: right edge");
                }
            }
        }
    }

    #[test]
    fn follows_monitor_origin() {
        let left = Bounds { left: -1920, top: 0, right: 0, bottom: 1080 };
        assert_eq!(island_bounds(left, 96, 200.0, 44.0).left, -1060);
        // The stage sits on the monitor's own top edge, wherever that is in the virtual screen.
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
        for dpi in DPIS {
            let r = island_bounds(FHD, dpi, 400.0, 448.0);
            assert!(FHD.contains(&r));
        }
    }

    #[test]
    fn island_region_covers_the_float_extent_and_keeps_the_centre() {
        for dpi in DPIS {
            let scale = dpi as f64 / 96.0;
            let stage = island_bounds(FHD, dpi, 400.0, 448.0);
            let window_width = stage.width();
            let window_height = stage.height();
            let centre = window_width as f64 / 2.0;
            for (w, h) in [(141.37, 36.0), (400.0, 440.0), (368.0, 173.5), (176.25, 36.0), (233.9, 36.0)] {
                let r = island_region(window_width, window_height, dpi, w, h);
                let (w_f, h_f, top_f) = (w * scale, h * scale, ISLAND_TOP_INSET * scale);
                let clamped = r.left == 0 || r.right == window_width;
                // covers the float extent on every side...
                assert!(r.left == 0 || (r.left as f64) <= centre - w_f / 2.0, "dpi {dpi} {w}x{h}: left {r:?}");
                assert!(r.right == window_width || (r.right as f64) >= centre + w_f / 2.0, "dpi {dpi} {w}x{h}: right {r:?}");
                assert!((r.top as f64) <= top_f, "dpi {dpi} {w}x{h}: top {r:?}");
                assert!(r.bottom == window_height || (r.bottom as f64) >= top_f + h_f, "dpi {dpi} {w}x{h}: bottom {r:?}");
                if !clamped {
                    // with at most ~2 px of slack per side (1 for the antialiased edge, < 1 for snapping)...
                    assert!((r.width() as f64) < w_f + 4.0, "dpi {dpi} {w}x{h}: {r:?}");
                    // ...and centred on the window's float centre.
                    let drift = (r.left + r.right) as f64 / 2.0 - centre;
                    assert!(drift.abs() <= 0.5, "dpi {dpi} {w}x{h}: centre off by {drift}");
                }
                assert!(r.bottom <= window_height && r.right <= window_width);
            }
        }
    }

    #[test]
    fn island_region_never_leaves_the_window() {
        let r = island_region(200, 44, 96, 500.0, 500.0);
        assert_eq!(r, Bounds { left: 0, top: 7, right: 200, bottom: 44 });
    }

    #[test]
    fn island_sits_the_top_inset_below_the_window_top_at_every_scale() {
        // The page draws the island ISLAND_TOP_INSET (scaled) down; the region starts there
        // (minus the 1 px antialias slack), never at 0.
        for dpi in DPIS {
            let r = island_region(500, 500, dpi, 142.0, 36.0);
            let top_f = ISLAND_TOP_INSET * dpi as f64 / 96.0;
            assert_eq!(r.top, top_f.floor() as i32 - 1, "dpi {dpi}");
        }
    }

    #[test]
    fn bridge_spans_the_gap_and_only_the_islands_width() {
        for dpi in DPIS {
            let stage = island_bounds(FHD, dpi, 400.0, 448.0);
            for w in [141.37, 233.9, 400.0] {
                let shape = region_shape(stage.width(), stage.height(), dpi, w, 36.0, true);
                let bridge = shape.bridge.expect("bridge at a free top edge");
                // From the very top of the window to the island's top, no gap and no overlap...
                assert_eq!((bridge.top, bridge.bottom), (0, shape.island.top), "dpi {dpi} width {w}");
                // ...exactly as wide as the island, so it never blocks anything beside it.
                assert_eq!((bridge.left, bridge.right), (shape.island.left, shape.island.right), "dpi {dpi} width {w}");
                assert!(bridge.height() >= 1);
            }
        }
    }

    #[test]
    fn no_bridge_under_a_top_taskbar() {
        let shape = region_shape(400, 448, 96, 200.0, 36.0, false);
        assert_eq!(shape.bridge, None);
        assert_eq!(shape.island, island_region(400, 448, 96, 200.0, 36.0));
        // The flag comes from the monitor's work area.
        let mut taskbar_on_top = monitor(0, 0, true);
        assert!(taskbar_on_top.top_is_free());
        taskbar_on_top.work.top = 48;
        assert!(!taskbar_on_top.top_is_free());
        // A taskbar at the bottom leaves the top free.
        let mut bottom = monitor(0, 0, true);
        bottom.work.bottom = 1040;
        assert!(bottom.top_is_free());
    }

    #[test]
    fn bridge_decision_covers_every_taskbar_arrangement() {
        // (work area reaches the top, a top auto-hide bar is registered, bridge)
        let cases = [
            (true, false, true),   // no taskbar on top (bottom/left/right, or none): bridge
            (false, false, false), // taskbar docked at the top, always visible: it owns the strip
            (true, true, false),   // taskbar docked at the top with auto-hide: it reveals on the edge
            (false, true, false),  // another app bar on top plus an auto-hide bar: still none
        ];
        for (reaches_top, autohide, expected) in cases {
            assert_eq!(bridge_wanted(reaches_top, autohide), expected, "reaches {reaches_top} autohide {autohide}");
        }
    }

    #[test]
    fn auto_hide_taskbar_on_top_drops_the_bridge_from_the_region() {
        let wanted = bridge_wanted(monitor(0, 0, true).top_is_free(), true);
        let shape = region_shape(400, 448, 96, 200.0, 36.0, wanted);
        assert_eq!(shape.bridge, None);
        // The island itself is untouched and still 8 DIP below the edge, so the strip is free.
        assert_eq!(shape.island, island_region(400, 448, 96, 200.0, 36.0));
        assert!(shape.island.top > 0);
    }

    fn limits_of(width: i32, height: i32, taskbar: i32, dpi: u32) -> IslandLimits {
        let bounds = Bounds { left: 0, top: 0, right: width, bottom: height };
        island_limits(bounds, Bounds { bottom: height - taskbar, ..bounds }, dpi)
    }

    fn assert_close(actual: f64, expected: f64, what: &str) {
        assert!((actual - expected).abs() < 0.01, "{what}: {actual} vs {expected}");
    }

    #[test]
    fn full_hd_at_100_percent_fits_the_preferred_panel() {
        let l = limits_of(1920, 1080, 40, 96);
        assert_close(l.max_width, 1920.0 - 24.0, "width");
        assert_close(l.max_height, 1040.0 - 8.0 - 12.0, "height");
        assert!(l.max_width >= 400.0 && l.max_height >= 440.0);
        assert_eq!(l.scale, 1.0);
    }

    #[test]
    fn a_720p_screen_at_150_percent_limits_the_height() {
        // 1280x720 physical = 853x480 logical; the 48 px taskbar leaves 672 physical.
        let l = limits_of(1280, 720, 48, 144);
        assert_close(l.max_width, 1280.0 / 1.5 - 24.0, "width");
        // (672 - 12 px inset) / 1.5 - 12 = 428
        assert_close(l.max_height, 428.0, "height");
        assert!(l.max_width >= 400.0 && l.max_height < 440.0);
        assert_eq!(l.scale, 1.5);
    }

    #[test]
    fn a_4k_screen_at_200_and_250_percent_fits() {
        let l200 = limits_of(3840, 2160, 96, 192);
        assert_close(l200.max_width, 1920.0 - 24.0, "200% width");
        assert_close(l200.max_height, (2064.0 - 16.0) / 2.0 - 12.0, "200% height");
        assert!(l200.max_width >= 400.0 && l200.max_height >= 440.0);
        let l250 = limits_of(3840, 2160, 120, 240);
        assert_close(l250.max_width, 1536.0 - 24.0, "250% width");
        assert_close(l250.max_height, (2040.0 - 20.0) / 2.5 - 12.0, "250% height");
        assert!(l250.max_width >= 400.0 && l250.max_height >= 440.0);
        assert_eq!(l250.scale, 2.5);
    }

    #[test]
    fn a_tiny_1024x600_screen_at_125_percent_limits_the_height_only() {
        let l = limits_of(1024, 600, 40, 120);
        assert_close(l.max_width, 1024.0 / 1.25 - 24.0, "width");
        assert_close(l.max_height, (560.0 - 10.0) / 1.25 - 12.0, "height");
        assert!(l.max_width >= 400.0 && l.max_height < 440.0);
    }

    #[test]
    fn limits_are_never_negative_and_serialize_in_camel_case() {
        let l = limits_of(10, 10, 20, 96);
        assert_eq!((l.max_width, l.max_height), (0.0, 0.0));
        let json = serde_json::to_value(limits_of(1920, 1080, 40, 96)).unwrap();
        assert!(json.get("maxWidth").is_some() && json.get("maxHeight").is_some() && json.get("scale").is_some());
        // A monitor reports the same numbers as the pure function.
        let m = monitor(0, 0, true);
        assert_eq!(m.limits(), island_limits(m.bounds, m.work, m.dpi));
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
        assert!(target.bounds.contains(&island_bounds(target.bounds, target.dpi, 400.0, 440.0)));
        assert!(monitors.windows(2).all(|w| (w[0].bounds.left, w[0].bounds.top) <= (w[1].bounds.left, w[1].bounds.top)));
    }
}
