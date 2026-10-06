//! Copy text to the Windows clipboard without needing webview focus.
//!
//! The island window is non-activating, so `navigator.clipboard` (which needs a
//! focused document) is unreliable; this uses the Win32 clipboard directly.

use crate::{rt, window};
use tauri::AppHandle;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Largest text accepted (UTF-8 bytes); diagnostics are a few hundred bytes.
const MAX_BYTES: usize = 64 * 1024;
/// The clipboard is a shared resource; another process may hold it for a moment.
const OPEN_ATTEMPTS: u32 = 8;
const OPEN_RETRY_MS: u64 = 25;

/// Keeps the clipboard open for exactly the lifetime of the guard.
struct OpenClipboardGuard;

impl OpenClipboardGuard {
    fn open(owner: HWND) -> Result<Self, String> {
        let mut last = String::new();
        for attempt in 0..OPEN_ATTEMPTS {
            match unsafe { OpenClipboard(owner) } {
                Ok(()) => return Ok(OpenClipboardGuard),
                Err(e) => last = e.to_string(),
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                std::thread::sleep(std::time::Duration::from_millis(OPEN_RETRY_MS));
            }
        }
        Err(format!("WIN-504: clipboard is busy ({last})"))
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn validate(text: &str) -> Result<(), String> {
    if text.len() > MAX_BYTES {
        Err(format!("WIN-504: text exceeds {} KB", MAX_BYTES / 1024))
    } else {
        Ok(())
    }
}

/// UTF-16 with the terminating NUL that CF_UNICODETEXT requires.
fn utf16_z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_text(owner: HWND, text: &str) -> Result<(), String> {
    validate(text)?;
    let wide = utf16_z(text);
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let _guard = OpenClipboardGuard::open(owner)?;
    unsafe {
        // A NULL owner would make SetClipboardData fail after EmptyClipboard, hence the island window.
        EmptyClipboard().map_err(|e| format!("WIN-504: clipboard could not be cleared: {e}"))?;
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| format!("WIN-504: out of memory: {e}"))?;
        let target = GlobalLock(memory) as *mut u16;
        if target.is_null() {
            let _ = GlobalFree(memory);
            return Err("WIN-504: clipboard memory could not be locked".to_string());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
        let _ = GlobalUnlock(memory);
        // On success the clipboard owns the memory; on failure it is still ours to free.
        if let Err(e) = SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(memory.0)) {
            let _ = GlobalFree(memory);
            return Err(format!("WIN-504: clipboard write failed: {e}"));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn copy_text_to_clipboard(app: AppHandle, text: String) -> Result<(), String> {
    validate(&text)?;
    let owner = window::main_hwnd(&app)?.0 as isize;
    rt::run_blocking("copy_text_to_clipboard", move || set_text(HWND(owner as _), &text)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_cap_is_enforced_on_utf8_bytes() {
        assert!(validate("").is_ok());
        assert!(validate(&"a".repeat(MAX_BYTES)).is_ok());
        assert!(validate(&"a".repeat(MAX_BYTES + 1)).is_err());
        // 3-byte characters count as 3 bytes each
        assert!(validate(&"€".repeat(MAX_BYTES / 3 + 1)).is_err());
    }

    #[test]
    fn utf16_is_nul_terminated_and_keeps_surrogates() {
        assert_eq!(utf16_z(""), vec![0]);
        assert_eq!(utf16_z("hi"), vec![b'h' as u16, b'i' as u16, 0]);
        assert_eq!(utf16_z("שלום").len(), 5);
        assert_eq!(utf16_z("😀"), vec![0xD83D, 0xDE00, 0]);
    }
}
