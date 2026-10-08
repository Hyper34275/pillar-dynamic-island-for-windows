//! Local (non-Outlook) sources for smart search: files (Windows Search index, bounded walk as
//! fallback), Start menu apps, the user's notes. Nothing leaves the machine; paths, AUMIDs and note
//! text are never logged. Results carry opaque keys; open/launch take only those keys.
//!
//! CONTRACT (used by `assistant`): every pub item below.

mod apps;
mod files;
mod note_search;

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use windows::core::HSTRING;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Programs, SHGetKnownFolderPath,
    ShellExecuteW, KF_FLAG_DEFAULT,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[derive(Clone, Debug, PartialEq)]
pub struct FileHit {
    /// Opaque, resolvable by [`open_file`] for a while (memory only).
    pub key: String,
    /// File name with extension.
    pub name: String,
    /// The known folder it is under ("Desktop", "Documents", "Downloads", ...), for display.
    pub place: String,
    pub modified: Option<DateTime<Utc>>,
    pub size: Option<u64>,
    pub is_dir: bool,
    /// Executable/script type: [`open_file`] only reveals it in Explorer.
    pub risky: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileSearch {
    pub hits: Vec<FileHit>,
    /// The fallback walk ran out of time: not every folder was looked at.
    pub partial: bool,
    /// The Windows Search index answered (false = fallback walk).
    pub index_used: bool,
}

/// Find files under the user's own folders by name (and indexed content, ranked lower).
/// `terms`: AND of OR-groups (see `intent::Slots::terms`). Blocking.
pub fn search_files(terms: &[Vec<String>], ext: Option<&str>, limit: usize, budget_ms: u64) -> Result<FileSearch, String> {
    files::search(terms, ext, limit, budget_ms)
}

/// Open a found file (explicit click only); executables are revealed, not run. Blocking.
pub fn open_file(key: &str) -> Result<(), String> {
    files::open(key)
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppHit {
    pub key: String,
    pub name: String,
}

/// Find installed apps (Start menu + Apps folder, cached). `names`: alternatives OR-ed. Blocking.
pub fn search_apps(names: &[String], limit: usize) -> Result<Vec<AppHit>, String> {
    apps::search(names, limit)
}

/// Launch a found app (explicit click only). Blocking.
pub fn launch_app(key: &str) -> Result<(), String> {
    apps::launch(key)
}

/// Build the app cache in the background (called once at startup, low priority).
pub fn warm_up() {
    apps::warm_up();
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteHit {
    /// The note id (valid for `center::open(app, "note:<id>")`).
    pub id: String,
    /// First non-empty line, clipped.
    pub title: String,
    /// The matching line (or the second line), clipped.
    pub snippet: String,
    pub updated_at: i64,
    pub pinned: bool,
}

/// Search the user's notes. Empty `terms` + `latest` = the newest notes. Blocking (reads the file).
pub fn search_notes(terms: &[Vec<String>], latest: bool, limit: usize) -> Result<Vec<NoteHit>, String> {
    note_search::search(terms, latest, limit)
}

// ---------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------

/// A shell known folder as an absolute path (from the shell, not from environment variables).
fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = raw.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(raw.as_ptr() as *const _));
        path.filter(|p| p.is_absolute())
    }
}

/// The folders a file search may look at: label + path. Fixed, never derived from user text.
/// Desktop, Documents and Downloads first, then OneDrive when it is set up on this PC.
fn search_roots() -> Vec<(String, PathBuf)> {
    let mut roots: Vec<(String, PathBuf)> = Vec::new();
    for (label, id) in [("Desktop", &FOLDERID_Desktop), ("Documents", &FOLDERID_Documents), ("Downloads", &FOLDERID_Downloads)] {
        if let Some(p) = known_folder(id).filter(|p| p.is_dir()) {
            roots.push((label.to_string(), p));
        }
    }
    // OneDrive (personal or business): the environment variable is set by the OneDrive client.
    for var in ["OneDriveCommercial", "OneDrive"] {
        if let Some(p) = std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute() && p.is_dir()) {
            let dup = roots.iter().any(|(_, r)| same_path(r, &p));
            if !dup {
                roots.push(("OneDrive".to_string(), p));
            }
            break;
        }
    }
    roots
}

fn start_menu_roots() -> Vec<PathBuf> {
    [&FOLDERID_Programs, &FOLDERID_CommonPrograms]
        .into_iter()
        .filter_map(|id| known_folder(id))
        .filter(|p| p.is_dir())
        .collect()
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().to_string_lossy().trim_end_matches('\\').eq_ignore_ascii_case(b.as_os_str().to_string_lossy().trim_end_matches('\\'))
}

/// `ShellExecuteW("open", target)` with no parameters (never a command line).
fn shell_open(target: &Path) -> Result<(), String> {
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(target.as_os_str()), None, None, SW_SHOWNORMAL) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecute returned {}", r.0 as isize))
    }
}

/// `explorer.exe /select,"path"`: shows the item in its folder without running it.
fn reveal_in_explorer(target: &Path) -> Result<(), String> {
    let explorer = HSTRING::from(crate::paths::explorer_exe().as_os_str());
    let params = HSTRING::from(format!("/select,\"{}\"", target.display()));
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &explorer, &params, None, SW_SHOWNORMAL) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecute returned {}", r.0 as isize))
    }
}

/// Opaque key: a short hash of a counter and a payload (memory only, not stable across restarts).
fn mint_key(prefix: char, counter: u64, payload: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(counter.to_le_bytes());
    h.update(payload.as_bytes());
    let d = h.finalize();
    let hex: String = d.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("{prefix}{hex}")
}

/// Clip to `max` characters with an ellipsis.
fn clip(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let mut s: String = t.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_counts_chars_not_bytes() {
        assert_eq!(clip("שלום עולם", 20), "שלום עולם");
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("  hi  ", 10), "hi");
    }

    #[test]
    fn keys_are_opaque_and_distinct() {
        let a = mint_key('f', 1, "C:\\a");
        let b = mint_key('f', 2, "C:\\a");
        assert_ne!(a, b);
        assert!(a.starts_with('f') && a.len() == 17);
        assert!(!a.contains('C'));
    }

    #[test]
    fn same_path_ignores_case_and_trailing_slash() {
        assert!(same_path(Path::new(r"C:\Users\X\"), Path::new(r"c:\users\x")));
        assert!(!same_path(Path::new(r"C:\Users\X"), Path::new(r"C:\Users\Y")));
    }
}
