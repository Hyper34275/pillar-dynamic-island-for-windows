//! Crash safety for the Windows 10 only parts of the search bar.
//!
//! - [`guarded`]: wraps the body of every `extern "system"` callback so a panic never crosses the
//!   FFI boundary (that is undefined behaviour and kills the process). Logged once as APP-001.
//! - [`StartupGuard`]: a marker file written before the AI button and the WinEvent hooks start and
//!   deleted after they have run for a while. A marker left behind means the previous run died in
//!   that phase: this session starts without the button and the hooks (the hotkeys stay on), and
//!   the next one tries again.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static PANIC_LOGGED: AtomicBool = AtomicBool::new(false);

/// Run `f`; on a panic return `default` (and log APP-001 the first time only, so a callback that
/// panics on every event cannot flood the log).
pub fn guarded<R>(scope: &str, default: R, f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(_) => {
            if !PANIC_LOGGED.swap(true, Ordering::Relaxed) {
                dlog!("ERROR", "search_bar", "APP-001 panic caught in {} (further ones are not logged)", scope);
            }
            default
        }
    }
}

/// Delay between the app start and the AI button / hooks (the island must be up first).
pub const START_DELAY: Duration = Duration::from_secs(6);
/// The marker is deleted after the button and hooks have run this long.
pub const CONFIRM_AFTER: Duration = Duration::from_secs(30);

/// The file system seam of the guard.
pub trait MarkerIo {
    fn exists(&self) -> bool;
    /// False when the marker could not be written (the guard then cannot protect; run anyway).
    fn write(&self) -> bool;
    fn remove(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Start the button and the hooks.
    Run,
    /// The previous start died while they were starting: stay hotkey-only this session.
    SkipThisSession,
}

pub struct StartupGuard<'a> {
    io: &'a dyn MarkerIo,
}

impl<'a> StartupGuard<'a> {
    pub fn new(io: &'a dyn MarkerIo) -> Self {
        StartupGuard { io }
    }

    /// Before starting the button and hooks. A stale marker is removed so the following start
    /// tries again.
    pub fn begin(&self) -> Decision {
        if self.io.exists() {
            self.io.remove();
            return Decision::SkipThisSession;
        }
        let _ = self.io.write();
        Decision::Run
    }

    /// After the button and hooks ran for [`CONFIRM_AFTER`].
    pub fn confirm(&self) {
        self.io.remove();
    }
}

pub struct FileMarker(pub PathBuf);

impl FileMarker {
    pub fn default_path() -> Option<FileMarker> {
        crate::paths::state_dir().ok().map(|d| FileMarker(d.join("searchbar_start.marker")))
    }
}

impl MarkerIo for FileMarker {
    fn exists(&self) -> bool {
        self.0.exists()
    }
    fn write(&self) -> bool {
        std::fs::write(&self.0, b"1").is_ok()
    }
    fn remove(&self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Fake {
        present: Cell<bool>,
        writable: Cell<bool>,
        log: RefCell<Vec<&'static str>>,
    }

    impl MarkerIo for Fake {
        fn exists(&self) -> bool {
            self.present.get()
        }
        fn write(&self) -> bool {
            self.log.borrow_mut().push("write");
            if self.writable.get() {
                self.present.set(true);
            }
            self.writable.get()
        }
        fn remove(&self) {
            self.log.borrow_mut().push("remove");
            self.present.set(false);
        }
    }

    #[test]
    fn a_clean_start_writes_the_marker_and_confirm_deletes_it() {
        let io = Fake::default();
        io.writable.set(true);
        let g = StartupGuard::new(&io);
        assert_eq!(g.begin(), Decision::Run);
        assert!(io.present.get());
        g.confirm();
        assert!(!io.present.get());
        // the next start runs again
        assert_eq!(StartupGuard::new(&io).begin(), Decision::Run);
    }

    #[test]
    fn a_marker_left_by_a_dead_run_skips_once_then_retries() {
        let io = Fake::default();
        io.writable.set(true);
        // run 1 died before confirm()
        assert_eq!(StartupGuard::new(&io).begin(), Decision::Run);
        // run 2 sees the marker
        assert_eq!(StartupGuard::new(&io).begin(), Decision::SkipThisSession);
        assert!(!io.present.get(), "the stale marker is cleared so run 3 retries");
        // run 3 tries again
        assert_eq!(StartupGuard::new(&io).begin(), Decision::Run);
    }

    #[test]
    fn an_unwritable_marker_does_not_block_the_start() {
        let io = Fake::default();
        assert_eq!(StartupGuard::new(&io).begin(), Decision::Run);
    }

    #[test]
    fn guarded_returns_the_default_on_a_panic() {
        let v = guarded("test", 7, || -> i32 { panic!("boom") });
        assert_eq!(v, 7);
        assert_eq!(guarded("test", 7, || 3), 3);
    }

    #[test]
    fn the_file_marker_round_trips() {
        let path = std::env::temp_dir().join(format!("ci_searchbar_marker_{}", std::process::id()));
        let m = FileMarker(path.clone());
        m.remove();
        assert!(!m.exists());
        assert!(m.write());
        assert!(m.exists());
        m.remove();
        assert!(!path.exists());
    }
}
