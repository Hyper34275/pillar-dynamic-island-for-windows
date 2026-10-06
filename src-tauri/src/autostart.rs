//! Per-user autostart opt-out.
//!
//! Autostart itself is a machine-wide HKLM `Run` value written by the installer.
//! A standard user can switch it off for their own account the same way Task
//! Manager does: by writing the `StartupApproved\Run` value under HKCU. Byte 0 is
//! even (`02`) when enabled and odd (`03`) when disabled; the rest is a timestamp
//! we leave zero. The app never writes HKLM.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_OPTION_NON_VOLATILE, REG_VALUE_TYPE,
};

/// Name of the installer's HKLM `Run` value; the NSIS hook must use the same name.
const RUN_VALUE_NAME: PCWSTR = w!("CompanyIsland");
const APPROVED_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");

fn encode(enabled: bool) -> [u8; 12] {
    let mut bytes = [0u8; 12];
    bytes[0] = if enabled { 0x02 } else { 0x03 };
    bytes
}

fn decode(bytes: &[u8]) -> Option<bool> {
    bytes.first().map(|b| b & 1 == 0)
}

struct Key(HKEY);

impl Key {
    fn open() -> Result<Key, String> {
        let mut hkey = HKEY::default();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                APPROVED_KEY,
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                None,
                &mut hkey,
                None,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(Key(hkey))
        } else {
            Err(format!("registry open failed ({})", status.0))
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Current per-user state: `Some(true/false)` when a StartupApproved value
/// exists, `None` when it does not (Windows then treats the entry as enabled).
pub fn read_state() -> Option<bool> {
    let key = Key::open().ok()?;
    let mut data = [0u8; 12];
    let mut len = data.len() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let status = unsafe {
        RegQueryValueExW(key.0, RUN_VALUE_NAME, None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut len))
    };
    if status != ERROR_SUCCESS || kind != REG_BINARY {
        return None;
    }
    decode(&data[..(len as usize).min(data.len())])
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let key = Key::open()?;
    let status = unsafe { RegSetValueExW(key.0, RUN_VALUE_NAME, 0, REG_BINARY, Some(&encode(enabled))) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("registry write failed ({})", status.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_matches_task_manager() {
        assert_eq!(encode(true), [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(encode(false), [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn decoding_follows_low_bit() {
        assert_eq!(decode(&[0x02, 0, 0]), Some(true));
        assert_eq!(decode(&[0x06, 0, 0]), Some(true));
        assert_eq!(decode(&[0x03, 0, 0]), Some(false));
        assert_eq!(decode(&[0x07, 0, 0]), Some(false));
        assert_eq!(decode(&[]), None);
    }
}
