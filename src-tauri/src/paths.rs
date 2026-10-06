//! Single source of per-user paths.
//!
//! Everything the app persists lives under `%LOCALAPPDATA%\CompanyIsland\`:
//! `logs\`, `state\` and `settings.json`. Nothing is ever written next to the
//! executable, to `%APPDATA%` or to the current directory. When the known folder
//! cannot be resolved the functions return an error and callers run in degraded
//! in-memory mode.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Per-user data folder name. Keep in sync with `tauri.conf.json` when renaming.
pub const APP_DIR: &str = "CompanyIsland";

const SETTINGS_FILE: &str = "settings.json";
const LOGS_DIR: &str = "logs";
const STATE_DIR: &str = "state";

static BASE: OnceLock<Result<PathBuf, String>> = OnceLock::new();

fn known_folder_local_app_data() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    unsafe {
        let raw = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None).ok()?;
        let path = raw.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(raw.as_ptr() as *const _));
        path
    }
}

/// A usable base must be an absolute path; a relative one would silently resolve
/// against the current directory.
fn validate_base(base: PathBuf) -> Option<PathBuf> {
    base.is_absolute().then_some(base)
}

fn local_app_data() -> Result<PathBuf, String> {
    known_folder_local_app_data()
        .and_then(validate_base)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from).and_then(validate_base))
        .ok_or_else(|| "LocalAppData folder is unavailable".to_string())
}

fn root_under(base: &Path) -> PathBuf {
    base.join(APP_DIR)
}

/// `%LOCALAPPDATA%\CompanyIsland` (not created).
pub fn root() -> Result<PathBuf, String> {
    BASE.get_or_init(|| local_app_data().map(|b| root_under(&b))).clone()
}

fn ensure_dir(dir: PathBuf) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    Ok(dir)
}

pub fn logs_dir() -> Result<PathBuf, String> {
    ensure_dir(root()?.join(LOGS_DIR))
}

pub fn state_dir() -> Result<PathBuf, String> {
    ensure_dir(root()?.join(STATE_DIR))
}

/// Path of `settings.json`; its parent directory is created.
pub fn settings_file() -> Result<PathBuf, String> {
    ensure_dir(root()?)?;
    Ok(root()?.join(SETTINGS_FILE))
}

/// Absolute `explorer.exe`, so a planted `explorer.exe` on PATH or in the working
/// directory is never launched.
pub fn explorer_exe() -> PathBuf {
    let windir = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("windir"))
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    windir.join("explorer.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_company_island_under_base() {
        let root = root_under(Path::new(r"C:\Users\Test\AppData\Local"));
        assert_eq!(root, PathBuf::from(r"C:\Users\Test\AppData\Local\CompanyIsland"));
        assert_eq!(root.join(LOGS_DIR), PathBuf::from(r"C:\Users\Test\AppData\Local\CompanyIsland\logs"));
        assert_eq!(root.join(SETTINGS_FILE).file_name().unwrap(), "settings.json");
    }

    #[test]
    fn relative_base_is_rejected() {
        assert!(validate_base(PathBuf::from(".")).is_none());
        assert!(validate_base(PathBuf::from("AppData")).is_none());
        assert!(validate_base(PathBuf::from(r"C:\Users\Test")).is_some());
    }

    #[test]
    fn resolves_on_this_machine() {
        let root = root().expect("LocalAppData resolves on a dev machine");
        assert!(root.is_absolute());
        assert!(root.ends_with(APP_DIR));
    }

    #[test]
    fn explorer_is_absolute() {
        let p = explorer_exe();
        assert!(p.is_absolute());
        assert!(p.ends_with("explorer.exe"));
    }
}
