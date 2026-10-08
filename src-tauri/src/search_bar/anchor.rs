//! What the shell looks like right now: the taskbar, its search control, the monitors.
//!
//! [`ShellProbe`] is the seam between Windows and the pure layout: [`Win32Probe`] asks the real
//! shell through public window APIs only (FindWindow, GetWindowRect, SHAppBarMessage, the registry);
//! tests give it other class names (a fake host window) or implement the trait themselves.
//! Nothing is read from or injected into explorer.exe's memory.

use super::layout::{Edge, Inputs, MonitorGeom, SearchMode};
use crate::monitors::{self, Bounds};
use std::sync::OnceLock;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETSTATE, ABM_GETTASKBARPOS, ABS_AUTOHIDE, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, SystemParametersInfoW, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

pub const TRAY_CLASS: &str = "Shell_TrayWnd";
/// ASSUMED (not confirmed on a real Windows 10 21H2 PC): the control that draws the search box.
/// `scripts/win10-taskbar-probe.ps1` dumps the real tree.
pub const SEARCH_CLASS: &str = "TrayDummySearchControl";
const SEARCH_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Search";
const SEARCH_VALUE: &str = "SearchboxTaskbarMode";
/// First Windows 11 build: its taskbar has no Windows 10 style search box.
const WINDOWS_11_BUILD: u32 = 22000;

/// What is in front, as far as the button cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foreground {
    Other,
    /// The Windows search flyout (Win+S) covers the box.
    SearchFlyout,
    /// Alt+Tab / Task View.
    TaskSwitcher,
}

/// Pure classification of the foreground window (class and executable name, both injectable).
pub fn classify_foreground(class: &str, image: &str, flyout_class: &str, flyout_images: &[&str]) -> Foreground {
    if class == flyout_class {
        let name = image.rsplit(['\\', '/']).next().unwrap_or(image);
        if flyout_images.iter().any(|f| f.eq_ignore_ascii_case(name)) {
            return Foreground::SearchFlyout;
        }
    }
    match class {
        "MultitaskingViewFrame" | "XamlExplorerHostIslandWindow" | "TaskSwitcherWnd" => Foreground::TaskSwitcher,
        _ => Foreground::Other,
    }
}

pub trait ShellProbe {
    fn search_mode(&self) -> SearchMode;
    /// Screen rectangle (physical px) of the search control; None if absent, hidden or empty.
    fn search_rect(&self) -> Option<Bounds>;
    fn taskbar_edge(&self) -> Option<Edge>;
    fn autohide(&self) -> bool;
    fn monitors(&self) -> Vec<MonitorGeom>;
    fn high_contrast(&self) -> bool;
    fn foreground(&self) -> Foreground;
    fn anchor_supported(&self) -> bool;
}

/// One measurement of everything the layout needs.
pub fn gather(probe: &dyn ShellProbe) -> Inputs {
    Inputs {
        mode: probe.search_mode(),
        edge: probe.taskbar_edge(),
        search_rect: probe.search_rect(),
        monitors: probe.monitors(),
        anchor_supported: probe.anchor_supported(),
        autohide: probe.autohide(),
    }
}

/// The real shell. Class names are injectable so a test can stand up a fake taskbar.
pub struct Win32Probe {
    pub tray_class: String,
    pub search_class: String,
    pub flyout_class: String,
    pub flyout_images: Vec<String>,
    /// Ask `SHAppBarMessage` for the edge (the real taskbar); else derive it from the tray rect.
    pub use_appbar: bool,
    /// Skip the registry (tests).
    pub mode_override: Option<SearchMode>,
    pub support_override: Option<bool>,
}

impl Default for Win32Probe {
    fn default() -> Self {
        Win32Probe {
            tray_class: TRAY_CLASS.into(),
            search_class: SEARCH_CLASS.into(),
            flyout_class: "Windows.UI.Core.CoreWindow".into(),
            flyout_images: vec!["SearchApp.exe".into(), "SearchUI.exe".into()],
            use_appbar: true,
            mode_override: None,
            support_override: None,
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn bounds(r: RECT) -> Bounds {
    Bounds::from(r)
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..len])
}

fn image_of(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        if pid == 0 {
            return String::new();
        }
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        if ok {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        }
    }
}

/// Windows build number (RtlGetVersion is not subject to manifest shimming).
fn os_build() -> u32 {
    static BUILD: OnceLock<u32> = OnceLock::new();
    *BUILD.get_or_init(|| unsafe {
        let mut info = OSVERSIONINFOW { dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
        if windows::Wdk::System::SystemServices::RtlGetVersion(&mut info).is_ok() {
            info.dwBuildNumber
        } else {
            0
        }
    })
}

impl Win32Probe {
    pub fn tray_hwnd(&self) -> Option<HWND> {
        let class = wide(&self.tray_class);
        let hwnd = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.ok()?;
        (!hwnd.0.is_null()).then_some(hwnd)
    }

    pub fn search_hwnd(&self) -> Option<HWND> {
        let tray = self.tray_hwnd()?;
        let class = wide(&self.search_class);
        let hwnd = unsafe { FindWindowExW(tray, HWND::default(), PCWSTR(class.as_ptr()), PCWSTR::null()) }.ok()?;
        (!hwnd.0.is_null()).then_some(hwnd)
    }

    fn tray_rect(&self) -> Option<Bounds> {
        let tray = self.tray_hwnd()?;
        let mut r = RECT::default();
        unsafe { GetWindowRect(tray, &mut r) }.ok()?;
        Some(bounds(r))
    }
}

fn edge_from_rect(tray: Bounds, monitors: &[MonitorGeom]) -> Option<Edge> {
    let monitor = monitors.iter().find(|m| {
        let cx = (tray.left + tray.right) / 2;
        let cy = (tray.top + tray.bottom) / 2;
        cx >= m.bounds.left && cx < m.bounds.right && cy >= m.bounds.top && cy < m.bounds.bottom
    })?;
    let m = monitor.bounds;
    if tray.width() >= tray.height() {
        // a horizontal bar: nearer the top or the bottom of its monitor
        if (tray.top - m.top).abs() < (m.bottom - tray.bottom).abs() {
            Some(Edge::Top)
        } else {
            Some(Edge::Bottom)
        }
    } else if (tray.left - m.left).abs() < (m.right - tray.right).abs() {
        Some(Edge::Left)
    } else {
        Some(Edge::Right)
    }
}

impl ShellProbe for Win32Probe {
    fn search_mode(&self) -> SearchMode {
        if let Some(m) = self.mode_override {
            return m;
        }
        let key = wide(SEARCH_KEY);
        let value = wide(SEARCH_VALUE);
        let mut data = 0u32;
        let mut size = 4u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        SearchMode::from_registry(status.is_ok().then_some(data))
    }

    fn search_rect(&self) -> Option<Bounds> {
        let hwnd = self.search_hwnd()?;
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return None;
            }
            let mut r = RECT::default();
            GetWindowRect(hwnd, &mut r).ok()?;
            let b = bounds(r);
            (b.width() > 0 && b.height() > 0).then_some(b)
        }
    }

    fn taskbar_edge(&self) -> Option<Edge> {
        if self.use_appbar {
            let mut data = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
            // The result is the taskbar's HWND-like value; a zero means the call failed.
            if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) } != 0 {
                match data.uEdge {
                    ABE_BOTTOM => return Some(Edge::Bottom),
                    ABE_TOP => return Some(Edge::Top),
                    ABE_LEFT => return Some(Edge::Left),
                    ABE_RIGHT => return Some(Edge::Right),
                    _ => {}
                }
            }
        }
        edge_from_rect(self.tray_rect()?, &self.monitors())
    }

    fn autohide(&self) -> bool {
        if !self.use_appbar {
            return false;
        }
        let mut data = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
        let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) } as u32;
        state & ABS_AUTOHIDE != 0
    }

    fn monitors(&self) -> Vec<MonitorGeom> {
        monitors::list()
            .into_iter()
            .map(|m| MonitorGeom { bounds: m.bounds, work: m.work, dpi: m.dpi, primary: m.primary })
            .collect()
    }

    fn high_contrast(&self) -> bool {
        let mut hc = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                hc.cbSize,
                Some(&mut hc as *mut HIGHCONTRASTW as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        ok.is_ok() && hc.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
    }

    fn foreground(&self) -> Foreground {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return Foreground::Other;
        }
        let class = class_of(hwnd);
        // The executable name is only read for the one class that can be the search flyout.
        let image = if class == self.flyout_class { image_of(hwnd) } else { String::new() };
        let images: Vec<&str> = self.flyout_images.iter().map(String::as_str).collect();
        classify_foreground(&class, &image, &self.flyout_class, &images)
    }

    fn anchor_supported(&self) -> bool {
        self.support_override.unwrap_or_else(|| os_build() < WINDOWS_11_BUILD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search_bar::layout::compute_layout;
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, SetWindowPos, SWP_NOACTIVATE,
        SWP_NOSIZE, SWP_NOZORDER, WNDCLASSEXW, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
    };

    #[test]
    fn foreground_classification() {
        let images = ["SearchApp.exe", "SearchUI.exe"];
        let fly = "Windows.UI.Core.CoreWindow";
        assert_eq!(
            classify_foreground(fly, r"C:\Windows\SystemApps\Microsoft.Windows.Search_cw5n1h2txyewy\SearchApp.exe", fly, &images),
            Foreground::SearchFlyout
        );
        assert_eq!(classify_foreground(fly, r"c:\x\searchui.EXE", fly, &images), Foreground::SearchFlyout);
        // the same class from another host (Start, notifications) is not the search flyout
        assert_eq!(classify_foreground(fly, r"C:\Windows\SystemApps\ShellExperienceHost.exe", fly, &images), Foreground::Other);
        assert_eq!(classify_foreground(fly, "", fly, &images), Foreground::Other);
        assert_eq!(classify_foreground("Chrome_WidgetWin_1", "SearchApp.exe", fly, &images), Foreground::Other);
        assert_eq!(classify_foreground("MultitaskingViewFrame", "", fly, &images), Foreground::TaskSwitcher);
    }

    #[test]
    fn edge_is_derived_from_the_tray_rect() {
        let m = MonitorGeom {
            bounds: Bounds { left: 0, top: 0, right: 1920, bottom: 1080 },
            work: Bounds { left: 0, top: 0, right: 1920, bottom: 1040 },
            dpi: 96,
            primary: true,
        };
        let r = |l, t, rr, b| Bounds { left: l, top: t, right: rr, bottom: b };
        assert_eq!(edge_from_rect(r(0, 1040, 1920, 1080), &[m]), Some(Edge::Bottom));
        assert_eq!(edge_from_rect(r(0, 0, 1920, 40), &[m]), Some(Edge::Top));
        assert_eq!(edge_from_rect(r(0, 0, 60, 1080), &[m]), Some(Edge::Left));
        assert_eq!(edge_from_rect(r(1860, 0, 1920, 1080), &[m]), Some(Edge::Right));
        assert_eq!(edge_from_rect(r(5000, 0, 6000, 40), &[m]), None);
    }

    unsafe extern "system" fn fake_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        DefWindowProcW(hwnd, msg, w, l)
    }

    fn register(name: &[u16]) {
        unsafe {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(fake_proc),
                hInstance: GetModuleHandleW(None).unwrap().into(),
                lpszClassName: PCWSTR(name.as_ptr()),
                ..Default::default()
            };
            assert!(RegisterClassExW(&class) != 0, "class registration");
        }
    }

    /// A fake taskbar (top-level window) with a fake search control (child) of unique classes: the
    /// real Win32Probe finds the rectangle and the layout follows a move of the child.
    #[test]
    fn probe_finds_a_fake_host_and_follows_it() {
        let monitors = monitors::list();
        let Some(primary) = monitors.iter().find(|m| m.primary) else { return };
        let pid = std::process::id();
        let tray_name = wide(&format!("CIFakeTray{pid}"));
        let search_name = wide(&format!("CIFakeSearch{pid}"));
        register(&tray_name);
        register(&search_name);

        let mb = primary.bounds;
        let bar_h = 60;
        let (tray_top, tray_left) = (mb.bottom - bar_h, mb.left);
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let tray = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(tray_name.as_ptr()),
                w!(""),
                WS_POPUP | WS_VISIBLE,
                tray_left,
                tray_top,
                mb.width(),
                bar_h,
                None,
                None,
                hinst,
                None,
            )
            .expect("fake tray");
            let child = CreateWindowExW(
                Default::default(),
                PCWSTR(search_name.as_ptr()),
                w!(""),
                WS_CHILD | WS_VISIBLE,
                100,
                8,
                400,
                44,
                tray,
                None,
                hinst,
                None,
            )
            .expect("fake search");

            let probe = Win32Probe {
                tray_class: format!("CIFakeTray{pid}"),
                search_class: format!("CIFakeSearch{pid}"),
                use_appbar: false,
                mode_override: Some(SearchMode::Box),
                support_override: Some(true),
                ..Default::default()
            };
            let rect = probe.search_rect().expect("the fake search control is found");
            assert_eq!((rect.width(), rect.height()), (400, 44));
            assert_eq!(rect.left, tray_left + 100);
            assert_eq!(probe.taskbar_edge(), Some(Edge::Bottom));

            let first = compute_layout(&gather(&probe));
            assert!(first.anchored, "the fake box is on screen at mode 2");
            let first_button = first.button.unwrap();
            assert!(rect.contains(&first_button));

            // move the child 120 px to the right (a user resizing the taskbar items)
            SetWindowPos(child, None, 220, 8, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
            let second = compute_layout(&gather(&probe));
            assert_eq!(second.button.unwrap().left - first_button.left, 120);
            assert_eq!(second.window.left - first.window.left, 120);

            // hidden control (the user switched the box to an icon): no rect, no anchor
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(child, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
            assert!(probe.search_rect().is_none());
            assert!(!compute_layout(&gather(&probe)).anchored);

            // a control that is missing altogether
            let none = Win32Probe { search_class: "CINoSuchClass".into(), ..probe };
            assert!(none.search_rect().is_none());

            let _ = DestroyWindow(tray);
        }
    }

    #[test]
    fn live_probe_calls_do_not_panic() {
        let p = Win32Probe::default();
        let _ = gather(&p);
        let _ = p.high_contrast();
        let _ = p.foreground();
        let _ = p.autohide();
        assert!(!p.monitors().is_empty() || cfg!(not(windows)));
    }
}
