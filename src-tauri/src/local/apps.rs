//! App search: a cache of installed apps built from `shell:AppsFolder` (Shell.Application COM)
//! and the Start menu `.lnk` files, searched with Hebrew/English aliases. Launching is by opaque
//! key only; AUMIDs and paths are never logged.

use super::{mint_key, shell_open, AppHit};
use crate::com::{self, ComApartment, Dispatch};
use crate::intent::fold;
use crate::notifications::{is_valid_aumid, launch_aumid};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(600);
const MAX_APPS: usize = 3_000;
const LNK_DEPTH: usize = 5;
/// The longest a build waits for the shell's Apps folder (it normally takes about a second).
const COM_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    Aumid(String),
    /// An existing `.exe` or `.lnk`.
    Path(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AppEntry {
    pub key: String,
    pub name: String,
    pub folded: String,
    pub target: Target,
}

struct Cache {
    entries: Vec<AppEntry>,
    built: Option<Instant>,
    /// False when the Apps folder could not be read in time (Start menu shortcuts only).
    complete: bool,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { entries: Vec::new(), built: None, complete: false });
/// Serialises builds: a second caller waits for the first and then finds a fresh cache.
static BUILD: Mutex<()> = Mutex::new(());
static REFRESHING: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------------------------
// Pure: aliases and ranking
// ---------------------------------------------------------------------------------------------

/// Equivalent spellings of well-known apps (Hebrew UI names on Hebrew Windows, English elsewhere).
const ALIASES: &[&[&str]] = &[
    &["אקסל", "excel"],
    &["וורד", "ורד", "word"],
    &["פאוורפוינט", "פווארפוינט", "פאורפוינט", "powerpoint"],
    &["אאוטלוק", "אוטלוק", "outlook"],
    &["טימס", "teams", "microsoft teams"],
    &["מחשבון", "calculator"],
    &["פנקס רשימות", "notepad"],
    &["צייר", "paint"],
    &["כרום", "chrome", "google chrome"],
    &["אדג'", "אדג", "edge", "microsoft edge"],
    &["סייר הקבצים", "file explorer", "explorer"],
    &["שורת הפקודה", "command prompt", "cmd"],
    &["הגדרות", "settings"],
    &["וואנדרייב", "ואנדרייב", "onedrive", "one drive"],
    &["אקרובט", "acrobat", "adobe acrobat"],
    &["זום", "zoom"],
];

/// The folded query plus the folded members of every alias group it names.
pub(super) fn expand_query(q: &str) -> Vec<String> {
    let f = fold(q);
    if f.is_empty() {
        return Vec::new();
    }
    let mut out = vec![f.clone()];
    for group in ALIASES {
        let folded: Vec<String> = group.iter().map(|g| fold(g)).collect();
        let named = folded.iter().any(|m| *m == f || (f.chars().count() >= 3 && m.starts_with(f.as_str())));
        if named {
            for m in folded {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
        }
    }
    out
}

/// 4 exact, 3 prefix, 2 word prefix, 1 substring, 0 no match.
pub(super) fn match_score(name: &str, q: &str) -> u8 {
    if q.is_empty() {
        0
    } else if name == q {
        4
    } else if name.starts_with(q) {
        3
    } else if name.split(|c: char| !c.is_alphanumeric()).any(|w| !w.is_empty() && w.starts_with(q)) || name.contains(&format!(" {q}")) {
        2
    } else if name.contains(q) {
        1
    } else {
        0
    }
}

pub(super) fn rank_apps<'a>(entries: &'a [AppEntry], names: &[String], limit: usize) -> Vec<&'a AppEntry> {
    let queries: Vec<String> = names.iter().flat_map(|n| expand_query(n)).collect();
    if queries.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u8, &AppEntry)> = entries
        .iter()
        .filter_map(|e| {
            let s = queries.iter().map(|q| match_score(&e.folded, q)).max().unwrap_or(0);
            (s > 0).then_some((s, e))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.folded.chars().count().cmp(&b.1.folded.chars().count())).then_with(|| a.1.folded.cmp(&b.1.folded)));
    scored.into_iter().take(limit).map(|(_, e)| e).collect()
}

/// Uninstallers, help files and readmes are shortcuts nobody wants to launch from a search.
pub(super) fn is_noise_name(folded: &str) -> bool {
    folded.starts_with("uninstall")
        || folded.starts_with("הסרת")
        || folded.contains("readme")
        || folded.contains("release notes")
        || folded == "help"
        || folded.ends_with(" help")
        || folded.starts_with("help ")
        || folded.contains("documentation")
        || folded.contains("מדריך")
}

/// Merge candidate sources in priority order, deduplicating by folded name.
pub(super) fn build_entries(sources: Vec<(String, Target)>) -> Vec<AppEntry> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (name, target) in sources {
        let name = name.trim().to_string();
        let folded = fold(&name);
        if folded.is_empty() || is_noise_name(&folded) || !seen.insert(folded.clone()) {
            continue;
        }
        let payload = match &target {
            Target::Aumid(a) => a.clone(),
            Target::Path(p) => p.to_string_lossy().into_owned(),
        };
        out.push(AppEntry { key: mint_key('a', 0, &format!("{folded}|{payload}")), name, folded, target });
        if out.len() >= MAX_APPS {
            break;
        }
    }
    out
}

/// An AppsFolder `Path` is an AUMID, or (for some classic apps) a plain file path.
pub(super) fn classify_apps_folder_path(path: &str) -> Option<Target> {
    let p = Path::new(path);
    if p.is_absolute() {
        return launchable_path(p).then(|| Target::Path(p.to_path_buf()));
    }
    is_valid_aumid(path).then(|| Target::Aumid(path.to_string()))
}

fn launchable_path(p: &Path) -> bool {
    p.is_absolute() && is_exe_or_lnk(p) && p.is_file()
}

fn is_exe_or_lnk(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| matches!(e.to_ascii_lowercase().as_str(), "exe" | "lnk")).unwrap_or(false)
}

// ---------------------------------------------------------------------------------------------
// Sources
// ---------------------------------------------------------------------------------------------

/// `shell:AppsFolder` through Shell.Application. Needs an STA thread.
fn apps_folder() -> Result<Vec<(String, Target)>, com::ComError> {
    let mut shell = Dispatch::create("Shell.Application")?;
    let mut folder = shell
        .call_object("NameSpace", vec![com::variant_from_str("shell:AppsFolder")])?
        .ok_or_else(|| com::ComError::new("NameSpace", com::E_NOOBJECT))?;
    let mut items = folder.call_object("Items", Vec::new())?.ok_or_else(|| com::ComError::new("Items", com::E_NOOBJECT))?;
    let count = com::variant_i32(&items.get("Count")?).unwrap_or(0).clamp(0, MAX_APPS as i32);
    let mut out = Vec::new();
    for i in 0..count {
        let Ok(Some(mut item)) = items.call_object("Item", vec![com::variant_from_i32(i)]) else { continue };
        let name = item.get("Name").ok().and_then(|v| com::variant_string(&v));
        let path = item.get("Path").ok().and_then(|v| com::variant_string(&v));
        if let (Some(name), Some(path)) = (name, path) {
            if let Some(t) = classify_apps_folder_path(&path) {
                out.push((name, t));
            }
        }
    }
    Ok(out)
}

/// Recursive `.lnk` listing (name = file stem). Skips hidden files.
fn start_menu_links(roots: &[PathBuf]) -> Vec<(String, Target)> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth + 1 < LNK_DEPTH {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                if let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) {
                    out.push((stem, Target::Path(p)));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Enumerate `shell:AppsFolder` on a fresh STA thread (Shell.Application needs one).
fn apps_folder_sources() -> Vec<(String, Target)> {
    lower_priority();
    match ComApartment::init_sta() {
        Ok(_apt) => match apps_folder() {
            Ok(v) => v,
            Err(e) => {
                dlog!("WARN", "local", "APPS-101 AppsFolder enumeration failed: {}", e);
                Vec::new()
            }
        },
        Err(e) => {
            dlog!("WARN", "local", "APPS-101 COM init failed: {}", e);
            Vec::new()
        }
    }
}

/// An enumeration thread that has not returned yet (stuck): no second one is started behind it.
static COM_RUNNING: AtomicBool = AtomicBool::new(false);

struct ClearOnDrop(&'static AtomicBool);

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The Apps-folder sources from `com` (run on its own thread, waited for at most `timeout`) merged
/// behind the Start menu `lnk` sources. `complete` is false when `com` was skipped (the previous
/// one is still stuck) or timed out: the list is then only the Start menu and must not be cached
/// as fresh. The stuck thread is abandoned, never joined.
fn collect_sources(
    running: &'static AtomicBool,
    timeout: Duration,
    com: impl FnOnce() -> Vec<(String, Target)> + Send + 'static,
    lnk: impl FnOnce() -> Vec<(String, Target)>,
) -> (Vec<(String, Target)>, bool) {
    if running.swap(true, Ordering::SeqCst) {
        dlog!("WARN", "local", "APPS-102 previous AppsFolder enumeration still running, using the Start menu only");
        return (lnk(), false);
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("local-apps".into()).spawn(move || {
        let _clear = ClearOnDrop(running);
        let _ = tx.send(com());
    });
    if spawned.is_err() {
        running.store(false, Ordering::SeqCst);
        return (lnk(), false);
    }
    // The Start menu scan runs here while the shell enumerates.
    let lnk = lnk();
    match rx.recv_timeout(timeout) {
        Ok(mut v) => {
            v.extend(lnk);
            (v, true)
        }
        Err(_) => {
            dlog!("WARN", "local", "APPS-102 AppsFolder enumeration timed out, using the Start menu only");
            (lnk, false)
        }
    }
}

/// Build the whole list: `(entries, complete)`. Never waits longer than [`COM_TIMEOUT`] for the shell.
fn build_now() -> (Vec<AppEntry>, bool) {
    let started = Instant::now();
    let (sources, complete) = collect_sources(&COM_RUNNING, COM_TIMEOUT, apps_folder_sources, || start_menu_links(&super::start_menu_roots()));
    let entries = build_entries(sources);
    dlog!("INFO", "local", "apps cache: {} entries, complete={}, {} ms", entries.len(), complete, started.elapsed().as_millis());
    (entries, complete)
}

fn lower_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

fn store(cache: &Mutex<Cache>, (entries, complete): (Vec<AppEntry>, bool)) {
    // A failed rebuild (empty) never wipes a good cache, and neither does a Start-menu-only one.
    if entries.is_empty() {
        return;
    }
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    if !complete && c.complete {
        return;
    }
    c.entries = entries;
    c.complete = complete;
    c.built = Some(Instant::now());
}

/// Resets [`REFRESHING`] when the refresh thread ends, also if it panics.
struct RefreshGuard;

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESHING.store(false, Ordering::SeqCst);
    }
}

fn rebuild() {
    // The wait on the shell is bounded (COM_TIMEOUT), so holding BUILD cannot block for good.
    let _g = BUILD.lock().unwrap_or_else(|e| e.into_inner());
    store(&CACHE, build_now());
}

/// Entries to search: the cache, building it first when empty (blocking) or refreshing it in the
/// background when stale.
fn current() -> Vec<AppEntry> {
    let (entries, stale) = {
        let c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        // A Start-menu-only list (the shell was slow) is served but retried in the background.
        (c.entries.clone(), !c.complete || c.built.map(|t| t.elapsed() > TTL).unwrap_or(true))
    };
    if entries.is_empty() {
        let _g = BUILD.lock().unwrap_or_else(|e| e.into_inner());
        // Someone else may have finished while we waited.
        let again = CACHE.lock().unwrap_or_else(|e| e.into_inner()).entries.clone();
        if !again.is_empty() {
            return again;
        }
        store(&CACHE, build_now());
        return CACHE.lock().unwrap_or_else(|e| e.into_inner()).entries.clone();
    }
    if stale && !REFRESHING.swap(true, Ordering::SeqCst) {
        let spawned = std::thread::Builder::new().name("local-apps-refresh".into()).spawn(|| {
            let _reset = RefreshGuard;
            rebuild();
        });
        if spawned.is_err() {
            REFRESHING.store(false, Ordering::SeqCst);
        }
    }
    entries
}

pub(super) fn warm_up() {
    let _ = std::thread::Builder::new().name("local-apps-warm".into()).spawn(|| {
        crate::debug_log::catch("local-apps-warm", || {
            lower_priority();
            rebuild();
        });
    });
}

pub(super) fn search(names: &[String], limit: usize) -> Result<Vec<AppHit>, String> {
    let entries = current();
    Ok(rank_apps(&entries, names, limit.clamp(1, 50)).into_iter().map(|e| AppHit { key: e.key.clone(), name: e.name.clone() }).collect())
}

pub(super) fn launch(key: &str) -> Result<(), String> {
    let entry = {
        let c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        c.entries.iter().find(|e| e.key == key).cloned()
    };
    launch_entry(entry.as_ref())
}

fn launch_entry(entry: Option<&AppEntry>) -> Result<(), String> {
    let Some(entry) = entry else { return Err("APPS-104: app no longer available".into()) };
    match &entry.target {
        Target::Aumid(a) => launch_aumid(a).map_err(|_| "APPS-109: could not start the app".to_string()),
        Target::Path(p) => {
            if !launchable_path(p) {
                return Err("APPS-104: app no longer available".into());
            }
            shell_open(p).map_err(|_| "APPS-109: could not start the app".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(names: &[&str]) -> Vec<AppEntry> {
        build_entries(names.iter().map(|n| (n.to_string(), Target::Aumid(format!("Pkg.{}!App", n.replace(' ', ""))))).collect())
    }

    fn top(es: &[AppEntry], q: &str) -> Vec<String> {
        rank_apps(es, &[q.to_string()], 10).into_iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn hebrew_and_english_aliases_find_the_same_app() {
        let es = entries(&["Microsoft Excel", "Excel Viewer", "Word", "Microsoft Edge", "מחשבון", "Notepad", "Paint 3D", "Paint"]);
        assert!(top(&es, "אקסל").iter().any(|n| n == "Microsoft Excel"));
        assert!(top(&es, "וורד").contains(&"Word".to_string()));
        assert!(top(&es, "calculator").contains(&"מחשבון".to_string()));
        assert!(top(&es, "פנקס רשימות").contains(&"Notepad".to_string()));
        assert!(top(&es, "צייר").contains(&"Paint".to_string()));
        assert!(top(&es, "אדג'").contains(&"Microsoft Edge".to_string()));
    }

    #[test]
    fn ranking_prefix_then_word_prefix_then_substring() {
        let es = entries(&["Microsoft Word", "Wordpad", "Word", "Crossword Puzzle", "Sword Art"]);
        let r = top(&es, "word");
        assert_eq!(r[0], "Word"); // exact
        assert_eq!(r[1], "Wordpad"); // prefix
        assert_eq!(r[2], "Microsoft Word"); // word prefix
        assert!(r.iter().position(|n| n == "Crossword Puzzle").unwrap() > 2); // substring last
    }

    #[test]
    fn alias_prefix_of_three_chars() {
        let es = entries(&["Microsoft Excel"]);
        assert!(!top(&es, "אקס").is_empty());
        assert!(top(&es, "אק").is_empty()); // too short to trigger the alias
    }

    #[test]
    fn noise_and_duplicates_are_dropped() {
        let es = build_entries(vec![
            ("Zoom".into(), Target::Aumid("Zoom.App".into())),
            ("zoom".into(), Target::Path(PathBuf::from(r"C:\x\zoom.lnk"))),
            ("Uninstall Zoom".into(), Target::Aumid("U.App".into())),
            ("Zoom Readme".into(), Target::Aumid("R.App".into())),
            ("Help".into(), Target::Aumid("H.App".into())),
            ("   ".into(), Target::Aumid("E.App".into())),
        ]);
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].target, Target::Aumid("Zoom.App".into())); // the first source wins
    }

    #[test]
    fn keys_are_stable_for_the_same_entry() {
        let a = entries(&["Zoom"]);
        let b = entries(&["Zoom"]);
        assert_eq!(a[0].key, b[0].key);
        assert_ne!(a[0].key, entries(&["Teams"])[0].key);
    }

    #[test]
    fn apps_folder_paths_are_classified() {
        assert_eq!(classify_apps_folder_path("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"), Some(Target::Aumid("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into())));
        assert_eq!(classify_apps_folder_path("MSEdge"), Some(Target::Aumid("MSEdge".into())));
        assert_eq!(classify_apps_folder_path("bad;id|x"), None);
        assert_eq!(classify_apps_folder_path(r"C:\definitely\missing\app.exe"), None);
        assert_eq!(classify_apps_folder_path(r"C:\Windows\System32\notepad.exe"), Some(Target::Path(PathBuf::from(r"C:\Windows\System32\notepad.exe"))));
        assert_eq!(classify_apps_folder_path(r"C:\Windows\win.ini"), None); // not exe/lnk
    }

    #[test]
    fn launch_refuses_unknown_keys_and_missing_files() {
        assert!(launch("anything").unwrap_err().starts_with("APPS-104"));
        let gone = AppEntry { key: "k".into(), name: "x".into(), folded: "x".into(), target: Target::Path(PathBuf::from(r"C:\nope\x.exe")) };
        assert!(launch_entry(Some(&gone)).unwrap_err().starts_with("APPS-104"));
        let bad = AppEntry { key: "k".into(), name: "x".into(), folded: "x".into(), target: Target::Aumid("a;b".into()) };
        assert!(launch_entry(Some(&bad)).unwrap_err().starts_with("APPS-109"));
        assert!(launch_entry(None).is_err());
    }

    #[test]
    fn start_menu_links_are_listed_recursively() {
        let root = std::env::temp_dir().join(format!("companyisland-test-apps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Sub")).unwrap();
        std::fs::write(root.join("Alpha.lnk"), b"x").unwrap();
        std::fs::write(root.join("Sub").join("Beta.lnk"), b"x").unwrap();
        std::fs::write(root.join("Sub").join("site.url"), b"x").unwrap();
        let v = start_menu_links(&[root.clone()]);
        let names: Vec<&str> = v.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn lnk_list() -> Vec<(String, Target)> {
        vec![("Calc".to_string(), Target::Path(PathBuf::from(r"C:\x\Calc.lnk")))]
    }

    #[test]
    fn a_hung_apps_folder_does_not_block_the_build() {
        static RUNNING: AtomicBool = AtomicBool::new(false);
        let t = Instant::now();
        let (src, complete) = collect_sources(
            &RUNNING,
            Duration::from_millis(150),
            || {
                std::thread::sleep(Duration::from_secs(30));
                vec![("Never".to_string(), Target::Aumid("Never.App".into()))]
            },
            lnk_list,
        );
        assert!(t.elapsed() < Duration::from_secs(5), "build waited for the stuck enumeration");
        assert!(!complete);
        assert_eq!(src.len(), 1);
        assert_eq!(src[0].0, "Calc"); // the Start menu list still works
        // The stuck thread is still running: a second build does not start another one.
        let t = Instant::now();
        let (src, complete) = collect_sources(&RUNNING, Duration::from_secs(60), || panic!("second enumeration started"), lnk_list);
        assert!(!complete && src.len() == 1 && t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_finished_enumeration_comes_first_and_is_complete() {
        static RUNNING: AtomicBool = AtomicBool::new(false);
        let (src, complete) = collect_sources(&RUNNING, Duration::from_secs(5), || vec![("Excel".to_string(), Target::Aumid("Ex.App".into()))], lnk_list);
        assert!(complete);
        assert_eq!(src.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), vec!["Excel", "Calc"]);
        // The thread ends and frees the flag, so the next build can enumerate again.
        let mut freed = false;
        for _ in 0..100 {
            if !RUNNING.load(Ordering::SeqCst) {
                freed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(freed);
    }

    #[test]
    fn a_start_menu_only_list_never_replaces_a_complete_cache() {
        let cache = Mutex::new(Cache { entries: Vec::new(), built: None, complete: false });
        store(&cache, (entries(&["Excel", "Word"]), true));
        store(&cache, (entries(&["Calc"]), false));
        assert_eq!(cache.lock().unwrap().entries.len(), 2);
        store(&cache, (entries(&["Zoom"]), true));
        assert_eq!(cache.lock().unwrap().entries.len(), 1);
        // From nothing, an incomplete list is accepted (better than no apps at all) but stays stale.
        let fresh = Mutex::new(Cache { entries: Vec::new(), built: None, complete: false });
        store(&fresh, (entries(&["Calc"]), false));
        assert!(!fresh.lock().unwrap().complete && fresh.lock().unwrap().entries.len() == 1);
    }

    /// Live, read-only: enumerates shell:AppsFolder and the Start menu; prints counts and timings.
    #[test]
    #[ignore]
    fn live_apps_probe() {
        let t = Instant::now();
        let (entries, complete) = build_now();
        println!("apps cache: {} entries (complete={}) in {} ms", entries.len(), complete, t.elapsed().as_millis());
        let aumid = entries.iter().filter(|e| matches!(e.target, Target::Aumid(_))).count();
        println!("  aumid={} path={}", aumid, entries.len() - aumid);
        for q in ["אקסל", "excel", "מחשבון", "calc", "outlook", "אאוטלוק", "notepad", "settings", "הגדרות"] {
            let r = rank_apps(&entries, &[q.to_string()], 5);
            println!("  query#{}: {} hits", q.chars().count(), r.len());
        }
        let t = Instant::now();
        let r = search(&["word".to_string()], 5).unwrap();
        println!("search warm: {} hits, {} us", r.len(), t.elapsed().as_micros());
        let t = Instant::now();
        let lnk = start_menu_links(&super::super::start_menu_roots());
        println!("start menu lnk: {} in {} ms", lnk.len(), t.elapsed().as_millis());
    }
}
