//! Pure geometry of the search bar: where the AI button and the input window go.
//!
//! No Win32 here. `anchor.rs` gathers [`Inputs`] from the shell (or a fake in tests), and
//! [`compute_layout`] turns them into physical-pixel rectangles.
//!
//! Anchored (Windows 10 search box shown at the bottom or top of the primary screen): the button
//! sits at the right edge inside the box and the input window covers the box plus a glow margin.
//! Otherwise the input is a floating bar above the primary taskbar (hotkey / tray / Windows 11).

use super::SearchBarState;
use crate::monitors::Bounds;

/// The AI button, its inset from the box's right edge, and the glow margin around the box (DIP).
pub const BUTTON_DIP: f64 = 24.0;
pub const BUTTON_INSET_DIP: f64 = 6.0;
pub const GLOW_MARGIN_DIP: f64 = 6.0;
/// Floating bar: size, distance from the taskbar, corner radius (DIP).
pub const FLOAT_WIDTH_DIP: f64 = 560.0;
pub const FLOAT_HEIGHT_DIP: f64 = 48.0;
pub const FLOAT_GAP_DIP: f64 = 16.0;
pub const FLOAT_RADIUS_DIP: f64 = 24.0;
/// A rectangle smaller than this is not a search box (a collapsed or half-animated control).
const MIN_BOX_WIDTH_DIP: f64 = 80.0;
const MIN_BOX_HEIGHT_DIP: f64 = 20.0;
/// Room kept above and below the button inside the box (DIP, each side).
const BUTTON_VERTICAL_PAD_DIP: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

impl Edge {
    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Bottom => "bottom",
            Edge::Top => "top",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }
}

/// `HKCU\...\Search\SearchboxTaskbarMode`: 0 hidden, 1 icon, 2 box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    Hidden,
    Icon,
    Box,
    /// Value missing or something else: fail closed (no anchor, the hotkey still works).
    Unknown,
}

impl SearchMode {
    pub fn from_registry(value: Option<u32>) -> SearchMode {
        match value {
            Some(0) => SearchMode::Hidden,
            Some(1) => SearchMode::Icon,
            Some(2) => SearchMode::Box,
            _ => SearchMode::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorGeom {
    pub bounds: Bounds,
    pub work: Bounds,
    pub dpi: u32,
    pub primary: bool,
}

/// Everything `compute_layout` needs, measured by a `ShellProbe`.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    pub mode: SearchMode,
    /// Edge of the primary taskbar, when known.
    pub edge: Option<Edge>,
    /// Screen rectangle (physical px) of the taskbar's search control, when found and visible.
    pub search_rect: Option<Bounds>,
    pub monitors: Vec<MonitorGeom>,
    /// The OS draws the Windows 10 style search box (false on Windows 11).
    pub anchor_supported: bool,
    /// The primary taskbar auto-hides (informational; a hidden bar has its rect off screen).
    pub autohide: bool,
}

/// Result: physical px everywhere.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub anchored: bool,
    /// The AI button (only when anchored).
    pub button: Option<Bounds>,
    /// The input window.
    pub window: Bounds,
    pub scale: f64,
    pub dpi: u32,
    /// Bar corner radius in DIP as the page sees it (0 = square Windows 10 box).
    pub radius_dip: f64,
    /// Corner radius of the window region in physical px.
    pub region_radius_px: i32,
    pub edge: Edge,
    pub monitor: Bounds,
}

impl Layout {
    pub fn to_state(&self, high_contrast: bool) -> SearchBarState {
        SearchBarState {
            anchored: self.anchored,
            width: self.window.width() as f64 / self.scale,
            height: self.window.height() as f64 / self.scale,
            radius: self.radius_dip,
            scale: self.scale,
            high_contrast,
            edge: self.edge.as_str().to_string(),
        }
    }
}

fn px(dip: f64, scale: f64) -> i32 {
    (dip * scale).round() as i32
}

pub fn scale_of(dpi: u32) -> f64 {
    if dpi == 0 {
        1.0
    } else {
        dpi as f64 / 96.0
    }
}

fn inflate(r: Bounds, by: i32) -> Bounds {
    Bounds { left: r.left - by, top: r.top - by, right: r.right + by, bottom: r.bottom + by }
}

fn anchored(i: &Inputs) -> Option<Layout> {
    if !i.anchor_supported || i.mode != SearchMode::Box {
        return None;
    }
    let edge = match i.edge {
        Some(e @ (Edge::Bottom | Edge::Top)) => e,
        _ => return None,
    };
    let rect = i.search_rect?;
    if rect.width() <= 0 || rect.height() <= 0 {
        return None;
    }
    // On screen: wholly inside one monitor. An auto-hidden bar that slid away (or is sliding) is not.
    let monitor = i.monitors.iter().find(|m| m.bounds.contains(&rect))?;
    let scale = scale_of(monitor.dpi);
    if (rect.width() as f64) < MIN_BOX_WIDTH_DIP * scale || (rect.height() as f64) < MIN_BOX_HEIGHT_DIP * scale {
        return None;
    }

    let height = rect.height();
    let side = px(BUTTON_DIP, scale).min(height - 2 * px(BUTTON_VERTICAL_PAD_DIP, scale)).max(1);
    let right = rect.right - px(BUTTON_INSET_DIP, scale);
    let top = rect.top + (height - side) / 2;
    let button = Bounds { left: right - side, top, right, bottom: top + side };
    let margin = px(GLOW_MARGIN_DIP, scale);

    Some(Layout {
        anchored: true,
        button: Some(button),
        window: inflate(rect, margin),
        scale,
        dpi: monitor.dpi,
        radius_dip: 0.0,
        region_radius_px: margin,
        edge,
        monitor: monitor.bounds,
    })
}

fn floating(i: &Inputs) -> Layout {
    let fallback = MonitorGeom {
        bounds: Bounds { left: 0, top: 0, right: 1920, bottom: 1080 },
        work: Bounds { left: 0, top: 0, right: 1920, bottom: 1040 },
        dpi: 96,
        primary: true,
    };
    let monitor = i.monitors.iter().find(|m| m.primary).or_else(|| i.monitors.first()).copied().unwrap_or(fallback);
    let scale = scale_of(monitor.dpi);
    let work = monitor.work;
    let gap = px(FLOAT_GAP_DIP, scale);
    let width = px(FLOAT_WIDTH_DIP, scale).min((work.width() - 2 * gap).max(px(160.0, scale)));
    let height = px(FLOAT_HEIGHT_DIP, scale);
    let left = work.left + (work.width() - width) / 2;
    let on_top = i.edge == Some(Edge::Top);
    let top = if on_top { work.top + gap } else { work.bottom - gap - height };
    Layout {
        anchored: false,
        button: None,
        window: Bounds { left, top, right: left + width, bottom: top + height },
        scale,
        dpi: monitor.dpi,
        radius_dip: FLOAT_RADIUS_DIP,
        region_radius_px: px(FLOAT_RADIUS_DIP, scale),
        edge: i.edge.unwrap_or(Edge::Bottom),
        monitor: monitor.bounds,
    }
}

pub fn compute_layout(i: &Inputs) -> Layout {
    anchored(i).unwrap_or_else(|| floating(i))
}

/// Whether the AI button is shown at all. `flyout`: the Windows search flyout / task switcher is
/// in front (the box is covered by it).
pub fn button_visible(layout: &Layout, enabled: bool, button_setting: bool, fullscreen: bool, flyout: bool) -> bool {
    enabled && button_setting && layout.anchored && layout.button.is_some() && !fullscreen && !flyout
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(l: i32, t: i32, r: i32, bt: i32) -> Bounds {
        Bounds { left: l, top: t, right: r, bottom: bt }
    }

    /// 1920x1080-DIP-scaled monitor at origin with a 48 DIP taskbar at the bottom.
    fn monitor(left: i32, top: i32, dpi: u32, primary: bool) -> MonitorGeom {
        let s = scale_of(dpi);
        let (w, h) = ((1920.0 * s) as i32, (1080.0 * s) as i32);
        let bar = (48.0 * s) as i32;
        MonitorGeom {
            bounds: b(left, top, left + w, top + h),
            work: b(left, top, left + w, top + h - bar),
            dpi,
            primary,
        }
    }

    /// A Windows 10 search box on the bottom taskbar: 330 DIP wide, 40 DIP tall, after the Start button.
    fn box_rect(m: &MonitorGeom) -> Bounds {
        let s = scale_of(m.dpi);
        let top = m.bounds.bottom - (44.0 * s) as i32;
        let left = m.bounds.left + (48.0 * s) as i32;
        b(left, top, left + (330.0 * s) as i32, top + (40.0 * s) as i32)
    }

    fn inputs(m: MonitorGeom) -> Inputs {
        Inputs {
            mode: SearchMode::Box,
            edge: Some(Edge::Bottom),
            search_rect: Some(box_rect(&m)),
            monitors: vec![m],
            anchor_supported: true,
            autohide: false,
        }
    }

    #[test]
    fn anchored_at_every_dpi() {
        for dpi in [96, 120, 144, 168, 192] {
            let m = monitor(0, 0, dpi, true);
            let i = inputs(m);
            let l = compute_layout(&i);
            let s = scale_of(dpi);
            assert!(l.anchored, "dpi {dpi}");
            let rect = i.search_rect.unwrap();
            let btn = l.button.unwrap();
            // 24 DIP square, 6 DIP from the right edge, vertically centred, wholly inside the box
            assert_eq!(btn.width(), (24.0 * s).round() as i32, "dpi {dpi}");
            assert_eq!(btn.width(), btn.height());
            assert_eq!(rect.right - btn.right, (6.0 * s).round() as i32);
            assert!(rect.contains(&btn));
            assert!(((btn.top - rect.top) - (rect.bottom - btn.bottom)).abs() <= 1, "dpi {dpi}");
            // the window is the box plus the glow margin on every side
            let g = (6.0 * s).round() as i32;
            assert_eq!(l.window, b(rect.left - g, rect.top - g, rect.right + g, rect.bottom + g));
            assert_eq!(l.region_radius_px, g);
            assert_eq!(l.radius_dip, 0.0);
            assert_eq!(l.dpi, dpi);
            let st = l.to_state(false);
            assert!(st.anchored && st.edge == "bottom");
            assert!((st.width - (330.0 + 12.0)).abs() < 1.0, "dpi {dpi}: {}", st.width);
        }
    }

    #[test]
    fn top_edge_is_anchored_too() {
        let m = monitor(0, 0, 96, true);
        let mut i = inputs(m);
        i.edge = Some(Edge::Top);
        i.search_rect = Some(b(48, 2, 378, 42));
        let l = compute_layout(&i);
        assert!(l.anchored);
        assert_eq!(l.edge, Edge::Top);
    }

    #[test]
    fn side_edges_fall_back_to_the_floating_bar() {
        for edge in [Edge::Left, Edge::Right] {
            let mut i = inputs(monitor(0, 0, 96, true));
            i.edge = Some(edge);
            let l = compute_layout(&i);
            assert!(!l.anchored && l.button.is_none(), "{edge:?}");
        }
    }

    #[test]
    fn mode_other_than_box_is_not_anchored() {
        for mode in [SearchMode::Hidden, SearchMode::Icon, SearchMode::Unknown] {
            let mut i = inputs(monitor(0, 0, 96, true));
            i.mode = mode;
            assert!(!compute_layout(&i).anchored, "{mode:?}");
        }
        assert_eq!(SearchMode::from_registry(Some(2)), SearchMode::Box);
        assert_eq!(SearchMode::from_registry(Some(1)), SearchMode::Icon);
        assert_eq!(SearchMode::from_registry(Some(0)), SearchMode::Hidden);
        assert_eq!(SearchMode::from_registry(Some(7)), SearchMode::Unknown);
        assert_eq!(SearchMode::from_registry(None), SearchMode::Unknown);
    }

    #[test]
    fn windows_11_is_never_anchored() {
        let mut i = inputs(monitor(0, 0, 96, true));
        i.anchor_supported = false;
        assert!(!compute_layout(&i).anchored);
    }

    #[test]
    fn missing_zero_or_tiny_rect_is_not_anchored() {
        let m = monitor(0, 0, 96, true);
        let mut i = inputs(m);
        i.search_rect = None;
        assert!(!compute_layout(&i).anchored);
        i.search_rect = Some(b(100, 1000, 100, 1000));
        assert!(!compute_layout(&i).anchored);
        i.search_rect = Some(b(100, 1000, 40, 1040)); // inverted
        assert!(!compute_layout(&i).anchored);
        i.search_rect = Some(b(100, 1030, 140, 1050)); // 40x20: below the minimum width
        assert!(!compute_layout(&i).anchored);
    }

    #[test]
    fn off_screen_rect_is_not_anchored() {
        let m = monitor(0, 0, 96, true);
        let mut i = inputs(m);
        // an auto-hidden taskbar parked below the screen
        i.autohide = true;
        i.search_rect = Some(b(48, 1078, 378, 1118));
        let l = compute_layout(&i);
        assert!(!l.anchored);
        assert!(l.window.bottom <= m.work.bottom, "the floating bar stays on screen");
        // half-way through the slide: partly off screen is still "hidden"
        i.search_rect = Some(b(48, 1060, 378, 1100));
        assert!(!compute_layout(&i).anchored);
        // fully revealed again
        i.search_rect = Some(box_rect(&m));
        assert!(compute_layout(&i).anchored);
        // left of the whole desktop
        i.search_rect = Some(b(-500, 1000, -170, 1040));
        assert!(!compute_layout(&i).anchored);
    }

    #[test]
    fn floating_bar_is_centred_above_the_taskbar() {
        for dpi in [96, 120, 144, 168, 192] {
            let m = monitor(0, 0, dpi, true);
            let mut i = inputs(m);
            i.mode = SearchMode::Icon;
            let l = compute_layout(&i);
            let s = scale_of(dpi);
            assert!(!l.anchored);
            assert_eq!(l.window.width(), (560.0 * s).round() as i32);
            assert_eq!(l.window.height(), (48.0 * s).round() as i32);
            assert_eq!(m.work.bottom - l.window.bottom, (16.0 * s).round() as i32, "dpi {dpi}");
            let centre = (l.window.left + l.window.right) / 2;
            assert!((centre - (m.work.left + m.work.right) / 2).abs() <= 1);
            assert_eq!(l.radius_dip, 24.0);
            assert_eq!(l.to_state(true).height, 48.0);
            assert!(l.to_state(true).high_contrast);
        }
    }

    #[test]
    fn floating_bar_goes_below_a_top_taskbar() {
        let mut m = monitor(0, 0, 96, true);
        m.work = b(0, 48, 1920, 1080);
        let mut i = inputs(m);
        i.mode = SearchMode::Icon;
        i.edge = Some(Edge::Top);
        let l = compute_layout(&i);
        assert_eq!(l.window.top, 48 + 16);
        assert_eq!(l.edge, Edge::Top);
    }

    #[test]
    fn floating_bar_shrinks_on_a_narrow_work_area() {
        let mut m = monitor(0, 0, 96, true);
        m.bounds = b(0, 0, 400, 800);
        m.work = b(0, 0, 400, 760);
        let mut i = inputs(m);
        i.mode = SearchMode::Hidden;
        let l = compute_layout(&i);
        assert_eq!(l.window.width(), 400 - 32);
        assert!(m.work.contains(&l.window));
    }

    #[test]
    fn primary_monitor_with_a_secondary_on_its_left() {
        // The taskbar with the search box is on the primary monitor, which sits right of another
        // monitor with a different scale: coordinates are positive but the desktop starts at -2560.
        let left = monitor(-2560, 0, 96, false);
        let primary = monitor(0, 0, 144, true);
        let mut i = inputs(primary);
        i.monitors = vec![left, primary];
        let l = compute_layout(&i);
        assert!(l.anchored);
        assert_eq!(l.dpi, 144);
        assert_eq!(l.monitor, primary.bounds);

        // negative desktop coordinates: the primary monitor itself is at x < 0 relative to a right one
        let primary = monitor(-1920, 0, 96, true);
        let right = monitor(0, 0, 192, false);
        let mut i = inputs(primary);
        i.monitors = vec![right, primary];
        let l = compute_layout(&i);
        assert!(l.anchored);
        assert!(l.window.left < 0);
        assert_eq!(l.dpi, 96);

        // not anchored: the floating bar still lands on the PRIMARY monitor
        i.mode = SearchMode::Icon;
        let f = compute_layout(&i);
        assert!(!f.anchored);
        assert!(primary.work.contains(&f.window));
    }

    #[test]
    fn a_rect_on_a_secondary_monitor_uses_that_monitors_dpi() {
        let primary = monitor(0, 0, 96, true);
        let second = monitor(1920, 0, 192, false);
        let mut i = inputs(primary);
        i.monitors = vec![primary, second];
        i.search_rect = Some(box_rect(&second));
        let l = compute_layout(&i);
        assert!(l.anchored);
        assert_eq!(l.dpi, 192);
        assert_eq!(l.button.unwrap().width(), 48);
    }

    #[test]
    fn no_monitors_still_gives_a_layout() {
        let i = Inputs {
            mode: SearchMode::Unknown,
            edge: None,
            search_rect: None,
            monitors: vec![],
            anchor_supported: true,
            autohide: false,
        };
        let l = compute_layout(&i);
        assert!(!l.anchored);
        assert_eq!(l.window.width(), 560);
    }

    #[test]
    fn button_visibility_rules() {
        let l = compute_layout(&inputs(monitor(0, 0, 96, true)));
        assert!(button_visible(&l, true, true, false, false));
        assert!(!button_visible(&l, false, true, false, false));
        assert!(!button_visible(&l, true, false, false, false));
        assert!(!button_visible(&l, true, true, true, false));
        assert!(!button_visible(&l, true, true, false, true));
        let mut i = inputs(monitor(0, 0, 96, true));
        i.mode = SearchMode::Icon;
        assert!(!button_visible(&compute_layout(&i), true, true, false, false));
    }

    #[test]
    fn layout_follows_the_search_rect() {
        let m = monitor(0, 0, 96, true);
        let mut i = inputs(m);
        let a = compute_layout(&i);
        let mut r = i.search_rect.unwrap();
        r.left += 40;
        r.right += 40;
        i.search_rect = Some(r);
        let c = compute_layout(&i);
        assert_eq!(c.button.unwrap().left - a.button.unwrap().left, 40);
        assert_eq!(c.window.left - a.window.left, 40);
    }
}
