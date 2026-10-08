//! Per-user autostart opt-out.
//!
//! Autostart itself is a machine-wide HKLM `Run` value written by the installer.
//! A standard user can switch it off for their own account the same way Task
//! Manager does: by writing the `StartupApproved\Run` value under HKCU. Byte 0 is
//! even (`02`) when enabled and odd (`03`) when disabled; the rest is a timestamp
//! we leave zero. The app never writes HKLM.
//!
//! The product was called CompanyIsland before it was called Yuval, and its `Run` value had that
//! name. A user who switched autostart off has a disabled `CompanyIsland` entry under
//! `StartupApproved\Run`; [`migrate_legacy_opt_out`] copies it to `Yuval` once so the choice survives
//! the rename.

use std::sync::Once;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_OPTION_NON_VOLATILE, REG_VALUE_TYPE,
};

/// Name of the installer's HKLM `Run` value; the NSIS hook must use the same name.
const RUN_VALUE_NAME: PCWSTR = w!("Yuval");
/// The `Run` value name before the rename.
const LEGACY_RUN_VALUE_NAME: PCWSTR = w!("CompanyIsland");
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
        Key::open_at(APPROVED_KEY)
    }

    fn open_at(path: PCWSTR) -> Result<Key, String> {
        let mut hkey = HKEY::default();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path,
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

/// The bytes of a `REG_BINARY` value (Task Manager writes 12; room for more), or `None` when the value
/// is missing, of another type or larger than expected.
fn read_value(key: &Key, name: PCWSTR) -> Option<Vec<u8>> {
    let mut data = [0u8; 64];
    let mut len = data.len() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let status = unsafe { RegQueryValueExW(key.0, name, None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut len)) };
    if status != ERROR_SUCCESS || kind != REG_BINARY {
        return None;
    }
    Some(data[..(len as usize).min(data.len())].to_vec())
}

fn write_value(key: &Key, name: PCWSTR, bytes: &[u8]) -> Result<(), String> {
    let status = unsafe { RegSetValueExW(key.0, name, 0, REG_BINARY, Some(bytes)) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("registry write failed ({})", status.0))
    }
}

/// Carries a per-user opt-out over from the old value name: a DISABLED `from` entry is copied (same
/// bytes) to `to`, once. Nothing happens when `to` already exists (the user has chosen since), when
/// `from` is missing, or when `from` is enabled (Windows treats a missing entry as enabled anyway).
/// The old value is left in place. Returns whether it copied.
fn migrate_opt_out(key: &Key, from: PCWSTR, to: PCWSTR) -> Result<bool, String> {
    if read_value(key, to).is_some() {
        return Ok(false);
    }
    match read_value(key, from) {
        Some(bytes) if decode(&bytes) == Some(false) => {
            write_value(key, to, &bytes)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Keeps a user's "do not start with Windows" choice across the CompanyIsland -> Yuval rename (see
/// the module comment). Runs once per process, before the first state read.
pub fn migrate_legacy_opt_out() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| match Key::open().and_then(|key| migrate_opt_out(&key, LEGACY_RUN_VALUE_NAME, RUN_VALUE_NAME)) {
        Ok(true) => crate::dlog!("INFO", "autostart", "carried the autostart opt-out over from the CompanyIsland entry"),
        Ok(false) => {}
        Err(e) => crate::dlog!("WARN", "autostart", "autostart opt-out migration failed: {}", e),
    });
}

/// Current per-user state: `Some(true/false)` when a StartupApproved value
/// exists, `None` when it does not (Windows then treats the entry as enabled).
pub fn read_state() -> Option<bool> {
    migrate_legacy_opt_out();
    let key = Key::open().ok()?;
    decode(&read_value(&key, RUN_VALUE_NAME)?)
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let key = Key::open()?;
    write_value(&key, RUN_VALUE_NAME, &encode(enabled))
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

    #[test]
    fn run_value_is_yuval_and_the_old_name_is_kept_for_the_migration() {
        assert_eq!(unsafe { RUN_VALUE_NAME.to_string().unwrap() }, "Yuval");
        assert_eq!(unsafe { LEGACY_RUN_VALUE_NAME.to_string().unwrap() }, "CompanyIsland");
    }

    /// A scratch key under HKCU that is deleted again when the test ends.
    struct Scratch {
        path: HSTRING,
        key: Key,
    }

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let path = HSTRING::from(format!(r"Software\YuvalTest-autostart-{}-{}", tag, std::process::id()));
            let key = Key::open_at(PCWSTR(path.as_ptr())).unwrap();
            Scratch { path, key }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            use windows::Win32::System::Registry::RegDeleteKeyW;
            let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(self.path.as_ptr())) };
        }
    }

    #[test]
    fn a_disabled_old_entry_disables_the_new_one_once() {
        let scratch = Scratch::new("disabled");
        let disabled = [3, 0, 0, 0, 0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4];
        write_value(&scratch.key, LEGACY_RUN_VALUE_NAME, &disabled).unwrap();

        assert_eq!(migrate_opt_out(&scratch.key, LEGACY_RUN_VALUE_NAME, RUN_VALUE_NAME), Ok(true));
        assert_eq!(read_value(&scratch.key, RUN_VALUE_NAME).unwrap(), disabled, "the same bytes, timestamp included");
        assert!(read_value(&scratch.key, LEGACY_RUN_VALUE_NAME).is_some(), "the old value is left alone");

        // Never twice: the user turns it back on, a later start must not undo that.
        write_value(&scratch.key, RUN_VALUE_NAME, &encode(true)).unwrap();
        assert_eq!(migrate_opt_out(&scratch.key, LEGACY_RUN_VALUE_NAME, RUN_VALUE_NAME), Ok(false));
        assert_eq!(decode(&read_value(&scratch.key, RUN_VALUE_NAME).unwrap()), Some(true));
    }

    #[test]
    fn an_enabled_or_missing_old_entry_leaves_the_new_one_unset() {
        let scratch = Scratch::new("enabled");
        assert_eq!(migrate_opt_out(&scratch.key, LEGACY_RUN_VALUE_NAME, RUN_VALUE_NAME), Ok(false));
        assert!(read_value(&scratch.key, RUN_VALUE_NAME).is_none());

        write_value(&scratch.key, LEGACY_RUN_VALUE_NAME, &encode(true)).unwrap();
        assert_eq!(migrate_opt_out(&scratch.key, LEGACY_RUN_VALUE_NAME, RUN_VALUE_NAME), Ok(false));
        assert!(read_value(&scratch.key, RUN_VALUE_NAME).is_none(), "no entry = enabled, as before");
    }

    #[test]
    fn an_existing_new_entry_wins_over_a_disabled_old_one() {
        let scratch = Scratch::new("wins");
        write_value(&scratch.key, LEGACY_RUN_VALUE_NAME, &encode(false)).unwrap();
        write_value(&scratch.key, RUN_VALUE_NAME, &encode(true)).unwrap();
        assert_eq!(migrate_opt_out(&scratch.key, LEGACY_RUN_VALUE_NAME, RUN_VALUE_NAME), Ok(false));
        assert_eq!(decode(&read_value(&scratch.key, RUN_VALUE_NAME).unwrap()), Some(true));
    }
}
