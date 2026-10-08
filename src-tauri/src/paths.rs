//! Single source of per-user paths.
//!
//! Everything the app persists lives under `%LOCALAPPDATA%\Yuval\`:
//! `logs\`, `state\` and `settings.json`. Nothing is ever written next to the
//! executable, to `%APPDATA%` or to the current directory. When the known folder
//! cannot be resolved the functions return an error and callers run in degraded
//! in-memory mode.
//!
//! The product used to be called CompanyIsland (and, before that, PILLAR). [`migrate_data_dir`]
//! carries an older folder over once, at startup, before anything opens a file.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows::core::GUID;
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_Windows};

/// Per-user data folder name.
pub const APP_DIR: &str = "Yuval";

/// Names the data folder had in earlier versions, newest first. Only the first one that exists is
/// used, and only while `APP_DIR` does not exist yet (see [`migrate_data_dir`]).
pub const LEGACY_APP_DIRS: &[&str] = &["CompanyIsland", "PILLAR"];

/// Written into the new folder after a migration; says where the data came from.
const MIGRATION_MARKER: &str = "migrated-from.txt";

const SETTINGS_FILE: &str = "settings.json";
const LOGS_DIR: &str = "logs";
const STATE_DIR: &str = "state";

static BASE: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// A shell known folder, from the shell itself rather than from environment variables a
/// user can override for their own process.
fn known_folder(id: &GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
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
    known_folder(&FOLDERID_LocalAppData)
        .and_then(validate_base)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from).and_then(validate_base))
        .ok_or_else(|| "LocalAppData folder is unavailable".to_string())
}

fn root_under(base: &Path) -> PathBuf {
    base.join(APP_DIR)
}

/// `%LOCALAPPDATA%\Yuval` (not created).
pub fn root() -> Result<PathBuf, String> {
    BASE.get_or_init(|| local_app_data().map(|b| root_under(&b))).clone()
}

/// What [`migrate_data_dir`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Migration {
    /// The new folder already exists (it wins; an older folder is left alone), or another process
    /// migrated while this one tried.
    NotNeeded,
    /// Neither the new nor an older folder exists: a fresh install.
    NothingToMigrate,
    /// The older folder was renamed to the new name (same volume: atomic, nothing was copied).
    Moved { from: &'static str },
    /// The rename failed (a file is locked, e.g. the old app still runs for this user), so the user's
    /// data files were copied and the older folder was left untouched. `skipped` counts files that
    /// could not be copied.
    Copied { from: &'static str, files: usize, skipped: usize, rename_error: String },
    /// Neither the rename nor the copy worked; the app starts with defaults and the older folder is
    /// untouched.
    Failed { from: &'static str, error: String },
}

impl Migration {
    /// One line for the log (no paths: they carry the profile name).
    pub fn describe(&self) -> String {
        match self {
            Migration::NotNeeded => "data folder: already current".to_string(),
            Migration::NothingToMigrate => "data folder: fresh install".to_string(),
            Migration::Moved { from } => format!("data folder: moved from {from}"),
            Migration::Copied { from, files, skipped, rename_error } => format!(
                "data folder: copied {files} files from {from} ({skipped} skipped), the old folder is untouched (rename: {rename_error})"
            ),
            Migration::Failed { from, error } => format!("data folder: could not migrate from {from}: {error}"),
        }
    }

    /// True when the outcome deserves a WARN line instead of INFO.
    pub fn is_warning(&self) -> bool {
        matches!(self, Migration::Copied { .. } | Migration::Failed { .. })
    }
}

/// Top-level folders of the data folder that are not user data and are not copied in the fallback:
/// the old app's logs (still being written while it runs) and WebView2 profiles (large, locked while
/// a webview runs, rebuilt on demand). A rename moves them like everything else.
fn is_cache_dir(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "logs" || n.starts_with("ebwebview")
}

/// True for a directory with no entries at all.
fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).map_or(false, |mut entries| entries.next().is_none())
}

/// Copies `from` into `to` (created), counting copied and skipped files. A file that cannot be
/// copied is skipped, never fatal. Temp files of the atomic writers (`*.tmp`) are not user data.
fn copy_tree(from: &Path, to: &Path, top_level: bool, files: &mut usize, skipped: &mut usize) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let Ok(entry) = entry else {
            *skipped += 1;
            continue;
        };
        let name = entry.file_name();
        let lossy = name.to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            *skipped += 1;
            continue;
        };
        if kind.is_dir() {
            if top_level && is_cache_dir(&lossy) {
                continue;
            }
            if copy_tree(&entry.path(), &to.join(&name), false, files, skipped).is_err() {
                *skipped += 1;
            }
        } else if kind.is_file() {
            if lossy.to_ascii_lowercase().ends_with(".tmp") {
                continue;
            }
            match std::fs::copy(entry.path(), to.join(&name)) {
                Ok(_) => *files += 1,
                Err(_) => *skipped += 1,
            }
        }
    }
    Ok(())
}

fn write_marker(dir: &Path, how: &str, from: &str) {
    let line = format!("{how} from {from} at {}\r\n", chrono::Local::now().to_rfc3339());
    let _ = std::fs::write(dir.join(MIGRATION_MARKER), line);
}

/// One-time migration of the per-user data folder, `base` being `%LOCALAPPDATA%`.
///
/// * `Yuval` exists: nothing happens (an older folder is left alone, even if it holds newer data).
///   An existing but EMPTY `Yuval` folder counts as absent: something may have created the shell
///   before the first start.
/// * Otherwise the first existing folder of [`LEGACY_APP_DIRS`] is renamed to `Yuval`: atomic on one
///   volume, keeps settings, reminder state, notes, logs and WebView2 profiles together.
/// * If the rename fails (a file is locked) the user's data files are copied instead, into a temp
///   sibling that is renamed into place when complete (so `Yuval` never exists half-filled), and the
///   older folder stays untouched.
/// * Nothing is ever deleted except the temp sibling this call created itself.
///
/// Afterwards `Yuval` exists (or nothing existed), so a second call never migrates again.
pub fn migrate_data_dir(base: &Path) -> Migration {
    migrate_with(base, &|from, to| std::fs::rename(from, to))
}

/// [`migrate_data_dir`] with the first folder rename (old -> new) injectable, so the copy fallback can
/// be tested without depending on how the OS reports a locked file.
fn migrate_with(base: &Path, rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>) -> Migration {
    let new = root_under(base);
    if new.exists() {
        if new.is_dir() && is_empty_dir(&new) {
            // Only succeeds on an empty directory.
            let _ = std::fs::remove_dir(&new);
        }
        if new.exists() {
            return Migration::NotNeeded;
        }
    }
    let Some(from) = LEGACY_APP_DIRS.iter().copied().find(|name| base.join(name).is_dir()) else {
        return Migration::NothingToMigrate;
    };
    let old = base.join(from);

    let rename_error = match rename(&old, &new) {
        Ok(()) => {
            write_marker(&new, "moved", from);
            return Migration::Moved { from };
        }
        Err(e) => e.to_string(),
    };
    // Lost a race against another instance that migrated first: its result stands.
    if new.exists() {
        return Migration::NotNeeded;
    }

    let staging = base.join(format!("{APP_DIR}.migrating-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging); // our own leftover from an earlier attempt of this pid
    let (mut files, mut skipped) = (0usize, 0usize);
    if let Err(e) = copy_tree(&old, &staging, true, &mut files, &mut skipped) {
        let _ = std::fs::remove_dir_all(&staging);
        return Migration::Failed { from, error: format!("copy: {e}; rename: {rename_error}") };
    }
    write_marker(&staging, "copied", from);
    match std::fs::rename(&staging, &new) {
        Ok(()) => Migration::Copied { from, files, skipped, rename_error },
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            if new.exists() {
                Migration::NotNeeded
            } else {
                Migration::Failed { from, error: format!("finish: {e}; rename: {rename_error}") }
            }
        }
    }
}

/// [`migrate_data_dir`] on the real `%LOCALAPPDATA%`. Call it first thing at startup, before any file
/// of the data folder is opened, and after the single-instance check so that a duplicate launch
/// never moves a folder under a running instance.
pub fn migrate_data_dir_in_place() -> Option<Migration> {
    local_app_data().ok().map(|base| migrate_data_dir(&base))
}

/// Errors name the folder, never its full path: they reach the webview (and diagnostics),
/// and the path carries the user's profile name.
fn ensure_dir(dir: PathBuf) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&dir).map_err(|e| {
        format!("cannot create the {} folder: {}", dir.file_name().and_then(|n| n.to_str()).unwrap_or("data"), e)
    })?;
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

/// Absolute `explorer.exe` in the Windows folder, so a planted `explorer.exe` on PATH, in the
/// working directory or behind a user-level `SystemRoot` variable is never launched.
pub fn explorer_exe() -> PathBuf {
    let windir = known_folder(&FOLDERID_Windows)
        .and_then(validate_base)
        .or_else(|| std::env::var_os("SystemRoot").map(PathBuf::from).and_then(validate_base))
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    windir.join("explorer.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_yuval_under_base() {
        let root = root_under(Path::new(r"C:\Users\Test\AppData\Local"));
        assert_eq!(root, PathBuf::from(r"C:\Users\Test\AppData\Local\Yuval"));
        assert_eq!(root.join(LOGS_DIR), PathBuf::from(r"C:\Users\Test\AppData\Local\Yuval\logs"));
        assert_eq!(root.join(SETTINGS_FILE).file_name().unwrap(), "settings.json");
        assert_eq!(LEGACY_APP_DIRS, &["CompanyIsland", "PILLAR"]);
    }

    // ---- migration -------------------------------------------------------------------------

    /// A fresh `%LOCALAPPDATA%` stand-in, removed when dropped.
    struct Base(PathBuf);

    impl Base {
        fn new(tag: &str) -> Base {
            let dir = std::env::temp_dir().join(format!("yuval-migrate-{}-{}", tag, std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Base(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, rel: &str, text: &str) {
            let file = self.0.join(rel);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }

        fn read(&self, rel: &str) -> Option<String> {
            std::fs::read_to_string(self.0.join(rel)).ok()
        }

        fn exists(&self, rel: &str) -> bool {
            self.0.join(rel).exists()
        }

        /// What an installed CompanyIsland leaves behind.
        fn seed_old(&self, dir: &str) {
            self.write(&format!("{dir}/settings.json"), r#"{"hideInFullscreen":false}"#);
            self.write(&format!("{dir}/state/notes.json"), "[notes]");
            self.write(&format!("{dir}/state/reminders.json"), "[reminders]");
            self.write(&format!("{dir}/logs/companyisland.log"), "old log");
            self.write(&format!("{dir}/EBWebView/Default/Cache/data_0"), "cache");
            self.write(&format!("{dir}/EBWebView-Center/Default/Cookies"), "cookies");
        }
    }

    impl Drop for Base {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn locked(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked by the old app"))
    }

    #[test]
    fn migration_moves_the_old_folder_whole() {
        let base = Base::new("move");
        base.seed_old("CompanyIsland");
        assert_eq!(migrate_data_dir(base.path()), Migration::Moved { from: "CompanyIsland" });
        assert!(!base.exists("CompanyIsland"), "a rename leaves nothing behind");
        assert_eq!(base.read("Yuval/settings.json").unwrap(), r#"{"hideInFullscreen":false}"#);
        assert_eq!(base.read("Yuval/state/notes.json").unwrap(), "[notes]");
        assert_eq!(base.read("Yuval/state/reminders.json").unwrap(), "[reminders]");
        assert!(base.exists("Yuval/logs/companyisland.log"));
        assert!(base.exists("Yuval/EBWebView/Default/Cache/data_0"), "the WebView2 profile moves with the folder");
        assert!(base.exists("Yuval/EBWebView-Center/Default/Cookies"));
        assert!(base.read("Yuval/migrated-from.txt").unwrap().starts_with("moved from CompanyIsland at "));
    }

    #[test]
    fn migration_copies_user_data_and_leaves_the_old_folder_when_the_rename_fails() {
        let base = Base::new("copy");
        base.seed_old("CompanyIsland");
        base.write("CompanyIsland/state/notes.json.tmp", "half written");
        let outcome = migrate_with(base.path(), &locked);
        match &outcome {
            Migration::Copied { from, files, skipped, rename_error } => {
                assert_eq!(*from, "CompanyIsland");
                assert_eq!(*files, 3, "settings.json, notes.json and reminders.json");
                assert_eq!(*skipped, 0);
                assert!(rename_error.contains("locked"), "{rename_error}");
            }
            other => panic!("expected a copy, got {other:?}"),
        }
        assert!(outcome.is_warning());
        assert_eq!(base.read("Yuval/settings.json").unwrap(), r#"{"hideInFullscreen":false}"#);
        assert_eq!(base.read("Yuval/state/notes.json").unwrap(), "[notes]");
        assert_eq!(base.read("Yuval/state/reminders.json").unwrap(), "[reminders]");
        assert!(!base.exists("Yuval/logs"), "the running old app still owns its logs");
        assert!(!base.exists("Yuval/EBWebView"), "WebView2 caches are not copied");
        assert!(!base.exists("Yuval/EBWebView-Center"));
        assert!(!base.exists("Yuval/state/notes.json.tmp"));
        assert!(base.read("Yuval/migrated-from.txt").unwrap().starts_with("copied from CompanyIsland at "));
        // The old folder is exactly as it was.
        assert_eq!(base.read("CompanyIsland/settings.json").unwrap(), r#"{"hideInFullscreen":false}"#);
        assert_eq!(base.read("CompanyIsland/state/notes.json").unwrap(), "[notes]");
        assert!(base.exists("CompanyIsland/logs/companyisland.log"));
        assert!(base.exists("CompanyIsland/EBWebView/Default/Cache/data_0"));
        let leftovers: Vec<_> = std::fs::read_dir(base.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("migrating"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    #[cfg(windows)]
    fn migration_copies_when_the_old_app_really_holds_a_file_open() {
        use std::os::windows::fs::OpenOptionsExt;
        let base = Base::new("lock");
        base.seed_old("CompanyIsland");
        // Shared for reading only (no FILE_SHARE_DELETE): what a running app does with its profile.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(base.path().join("CompanyIsland").join("state").join("notes.json"))
            .unwrap();
        let outcome = migrate_data_dir(base.path());
        drop(held);
        assert!(matches!(outcome, Migration::Copied { .. }), "{outcome:?}");
        assert_eq!(base.read("Yuval/state/notes.json").unwrap(), "[notes]");
        assert_eq!(base.read("CompanyIsland/state/notes.json").unwrap(), "[notes]");
    }

    #[test]
    fn migration_never_touches_an_old_folder_when_the_new_one_exists() {
        let base = Base::new("both");
        base.seed_old("CompanyIsland");
        base.write("Yuval/settings.json", "new settings");
        assert_eq!(migrate_data_dir(base.path()), Migration::NotNeeded);
        assert_eq!(base.read("Yuval/settings.json").unwrap(), "new settings");
        assert!(!base.exists("Yuval/state"), "nothing was merged in");
        assert_eq!(base.read("CompanyIsland/settings.json").unwrap(), r#"{"hideInFullscreen":false}"#);
        assert_eq!(base.read("CompanyIsland/state/notes.json").unwrap(), "[notes]");
    }

    #[test]
    fn migration_with_nothing_installed_does_nothing() {
        let base = Base::new("none");
        assert_eq!(migrate_data_dir(base.path()), Migration::NothingToMigrate);
        assert!(!base.exists("Yuval"), "the folder is created later, by the first write");
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    }

    #[test]
    fn migration_never_runs_twice() {
        let moved = Base::new("twice-move");
        moved.seed_old("CompanyIsland");
        assert!(matches!(migrate_data_dir(moved.path()), Migration::Moved { .. }));
        moved.write("Yuval/settings.json", "edited after the move");
        // Something recreates the old folder (a stale shortcut, an old build): it is not pulled in.
        moved.write("CompanyIsland/settings.json", "stale");
        assert_eq!(migrate_data_dir(moved.path()), Migration::NotNeeded);
        assert_eq!(moved.read("Yuval/settings.json").unwrap(), "edited after the move");
        assert_eq!(moved.read("CompanyIsland/settings.json").unwrap(), "stale");

        let copied = Base::new("twice-copy");
        copied.seed_old("CompanyIsland");
        assert!(matches!(migrate_with(copied.path(), &locked), Migration::Copied { .. }));
        copied.write("Yuval/state/notes.json", "edited after the copy");
        assert_eq!(migrate_data_dir(copied.path()), Migration::NotNeeded);
        assert_eq!(copied.read("Yuval/state/notes.json").unwrap(), "edited after the copy");
        assert_eq!(copied.read("CompanyIsland/state/notes.json").unwrap(), "[notes]");
    }

    #[test]
    fn migration_treats_an_empty_new_folder_as_absent() {
        let base = Base::new("empty-shell");
        base.seed_old("CompanyIsland");
        std::fs::create_dir_all(base.path().join("Yuval")).unwrap();
        assert_eq!(migrate_data_dir(base.path()), Migration::Moved { from: "CompanyIsland" });
        assert_eq!(base.read("Yuval/state/notes.json").unwrap(), "[notes]");

        // A shell with anything inside is real data.
        let real = Base::new("not-empty-shell");
        real.seed_old("CompanyIsland");
        real.write("Yuval/logs/center.log", "the Center ran first");
        assert_eq!(migrate_data_dir(real.path()), Migration::NotNeeded);
        assert!(real.exists("CompanyIsland/settings.json"));
    }

    #[test]
    fn migration_reaches_back_to_the_pillar_folder_only_when_nothing_newer_exists() {
        let only_pillar = Base::new("pillar");
        only_pillar.write("PILLAR/logs/pillar.log", "very old");
        only_pillar.write("PILLAR/settings.json", "pillar settings");
        assert_eq!(migrate_data_dir(only_pillar.path()), Migration::Moved { from: "PILLAR" });
        assert_eq!(only_pillar.read("Yuval/settings.json").unwrap(), "pillar settings");
        assert!(!only_pillar.exists("PILLAR"));

        let both_old = Base::new("pillar-and-company");
        both_old.write("PILLAR/settings.json", "pillar settings");
        both_old.seed_old("CompanyIsland");
        assert_eq!(migrate_data_dir(both_old.path()), Migration::Moved { from: "CompanyIsland" });
        assert_eq!(both_old.read("PILLAR/settings.json").unwrap(), "pillar settings", "PILLAR is left alone");

        let with_yuval = Base::new("pillar-and-yuval");
        with_yuval.write("PILLAR/settings.json", "pillar settings");
        with_yuval.write("Yuval/settings.json", "current");
        assert_eq!(migrate_data_dir(with_yuval.path()), Migration::NotNeeded);
        assert_eq!(with_yuval.read("PILLAR/settings.json").unwrap(), "pillar settings");
    }

    #[test]
    fn migration_ignores_a_plain_file_with_the_old_name() {
        let base = Base::new("file-not-dir");
        base.write("CompanyIsland", "not a folder");
        assert_eq!(migrate_data_dir(base.path()), Migration::NothingToMigrate);
        assert_eq!(base.read("CompanyIsland").unwrap(), "not a folder");
    }

    #[test]
    fn migration_log_lines_do_not_carry_paths() {
        let line = Migration::Copied {
            from: "CompanyIsland",
            files: 3,
            skipped: 1,
            rename_error: "Access is denied. (os error 5)".into(),
        }
        .describe();
        assert!(line.contains("copied 3 files from CompanyIsland (1 skipped)"), "{line}");
        assert!(!line.contains('\\'), "{line}");
        assert_eq!(Migration::Moved { from: "PILLAR" }.describe(), "data folder: moved from PILLAR");
        assert!(!Migration::Moved { from: "PILLAR" }.is_warning());
        assert!(Migration::Failed { from: "PILLAR", error: "x".into() }.is_warning());
    }

    #[test]
    fn folder_errors_do_not_leak_the_profile_path() {
        let file = std::env::temp_dir().join(format!("yuval-paths-{}", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        let error = ensure_dir(file.join("logs")).unwrap_err();
        let _ = std::fs::remove_file(&file);
        assert!(error.starts_with("cannot create the logs folder"), "{error}");
        assert!(!error.contains(file.to_str().unwrap()), "{error}");
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
        assert!(p.exists(), "explorer.exe is resolved from the shell's Windows folder");
    }
}
