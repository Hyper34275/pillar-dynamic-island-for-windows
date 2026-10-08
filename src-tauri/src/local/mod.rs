//! Local (non-Outlook) sources for smart search: files (Windows Search index, bounded walk as
//! fallback), Start menu apps, the user's notes. Nothing leaves the machine; paths, AUMIDs and note
//! text are never logged. Results carry opaque keys; open/launch take only those keys.
//!
//! CONTRACT (used by `assistant`): every pub item below.

use chrono::{DateTime, Utc};

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
    let _ = (terms, ext, limit, budget_ms);
    Ok(FileSearch::default())
}

/// Open a found file (explicit click only); executables are revealed, not run. Blocking.
pub fn open_file(key: &str) -> Result<(), String> {
    let _ = key;
    Err("FILES-104: file no longer available".into())
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppHit {
    pub key: String,
    pub name: String,
}

/// Find installed apps (Start menu + Apps folder, cached). `names`: alternatives OR-ed. Blocking.
pub fn search_apps(names: &[String], limit: usize) -> Result<Vec<AppHit>, String> {
    let _ = (names, limit);
    Ok(Vec::new())
}

/// Launch a found app (explicit click only). Blocking.
pub fn launch_app(key: &str) -> Result<(), String> {
    let _ = key;
    Err("APPS-104: app no longer available".into())
}

/// Build the app cache in the background (called once at startup, low priority).
pub fn warm_up() {}

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
    let _ = (terms, latest, limit);
    Ok(Vec::new())
}
