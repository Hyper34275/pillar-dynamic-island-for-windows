//! Ctrl+Alt+Space and Alt+` (RegisterHotKey on the search-bar thread's window; no keyboard hook).

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_OEM_3, VK_SPACE,
};

pub const HOTKEY_ID: i32 = 0x5342;
/// Alt + backtick (the key left of 1, VK_OEM_3; types ';' on Hebrew layouts): the centred spotlight.
pub const SPOTLIGHT_ID: i32 = 0x5343;

/// Register the spotlight hotkey; false (and a WIN-505 log) when another program owns it.
pub fn register_spotlight(hwnd: HWND) -> bool {
    match unsafe { RegisterHotKey(hwnd, SPOTLIGHT_ID, MOD_ALT | MOD_NOREPEAT, VK_OEM_3.0 as u32) } {
        Ok(()) => true,
        Err(e) => {
            dlog!("WARN", "search_bar", "WIN-505 hotkey Alt+` unavailable: {}", e.code().0);
            false
        }
    }
}

pub fn unregister_spotlight(hwnd: HWND) {
    unsafe {
        let _ = UnregisterHotKey(hwnd, SPOTLIGHT_ID);
    }
}

/// Register the hotkey; false (and a WIN-505 log) when another program owns the combination.
pub fn register(hwnd: HWND) -> bool {
    match unsafe { RegisterHotKey(hwnd, HOTKEY_ID, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_SPACE.0 as u32) } {
        Ok(()) => true,
        Err(e) => {
            dlog!("WARN", "search_bar", "WIN-505 hotkey Ctrl+Alt+Space unavailable: {}", e.code().0);
            false
        }
    }
}

pub fn unregister(hwnd: HWND) {
    unsafe {
        let _ = UnregisterHotKey(hwnd, HOTKEY_ID);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE, WS_EX_TOOLWINDOW, WS_POPUP};

    #[test]
    fn the_spotlight_hotkey_registers_once_and_frees_on_unregister() {
        assert_ne!(HOTKEY_ID, SPOTLIGHT_ID);
        unsafe {
            let make = || {
                CreateWindowExW(WS_EX_TOOLWINDOW, w!("Static"), w!(""), WS_POPUP, 0, 0, 0, 0, HWND_MESSAGE, None, None, None)
                    .expect("window")
            };
            let (a, b) = (make(), make());
            if register_spotlight(a) {
                assert!(!register_spotlight(b), "the same combination cannot be registered twice");
                unregister_spotlight(a);
                assert!(register_spotlight(b));
                unregister_spotlight(b);
            }
            let _ = DestroyWindow(a);
            let _ = DestroyWindow(b);
        }
    }

    #[test]
    fn registering_twice_reports_a_conflict_and_unregister_frees_it() {
        unsafe {
            let make = || {
                CreateWindowExW(WS_EX_TOOLWINDOW, w!("Static"), w!(""), WS_POPUP, 0, 0, 0, 0, HWND_MESSAGE, None, None, None)
                    .expect("window")
            };
            let (a, b) = (make(), make());
            if !register(a) {
                // The combination is owned by another program on this machine.
                let _ = DestroyWindow(a);
                let _ = DestroyWindow(b);
                return;
            }
            assert!(!register(b), "the same combination cannot be registered twice");
            unregister(a);
            assert!(register(b));
            unregister(b);
            let _ = DestroyWindow(a);
            let _ = DestroyWindow(b);
        }
    }
}
