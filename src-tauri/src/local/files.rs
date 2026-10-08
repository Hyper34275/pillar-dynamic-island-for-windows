//! File search: the Windows Search index through ADO (late bound, on one dedicated STA worker with
//! a hard deadline), and a bounded `std::fs` walk when the index is unavailable or finds nothing.
//! The SQL is built only from sanitized tokens and fixed known-folder scopes. Paths are never logged.

use super::{mint_key, reveal_in_explorer, search_roots, shell_open, FileHit, FileSearch};
use crate::com::{self, ComApartment, Dispatch};
use crate::intent::fold;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------------------------

const INDEX_DEADLINE_MS: u64 = 1_500;
const INDEX_TOP: usize = 50;
const INDEX_RETRY_AFTER: Duration = Duration::from_secs(60);

const MAX_GROUPS: usize = 6;
const MAX_ALTS: usize = 4;
const MAX_WORDS: usize = 3;
const MAX_TOKEN_CHARS: usize = 40;

const WALK_DEPTH: usize = 4;
const WALK_BUDGET_MS: u64 = 700;
const WALK_MAX_ENTRIES: usize = 20_000;

const KEY_CAP: usize = 300;

const ATTR_HIDDEN: u32 = 0x2;
const ATTR_SYSTEM: u32 = 0x4;
const ATTR_REPARSE: u32 = 0x400;

const SKIP_DIRS: [&str; 3] = ["node_modules", ".git", "appdata"];

const RISKY_EXT: [&str; 17] = [
    "exe", "bat", "cmd", "ps1", "vbs", "js", "jse", "wsf", "msi", "lnk", "scf", "hta", "com", "cpl", "reg", "vbe", "scr",
];

// ---------------------------------------------------------------------------------------------
// Pure: token sanitising and SQL building
// ---------------------------------------------------------------------------------------------

/// One search alternative: all its words must match (AND).
type Alt = Vec<String>;
/// Alternatives of one term (OR).
type Group = Vec<Alt>;

/// A token keeps letters, digits and `_ - .`; it needs at least one letter or digit and is
/// capped at 40 chars. Quotes, `%`, `[`, `]`, `*`, `;` and everything else are dropped.
pub(super) fn sanitize_token(raw: &str) -> Option<String> {
    let t: String = raw.chars().filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.')).take(MAX_TOKEN_CHARS).collect();
    t.chars().any(char::is_alphanumeric).then_some(t)
}

/// The caller's `terms` (AND of OR-groups) as sanitized groups: at most 6 groups of 4
/// alternatives of 3 words. Text with spaces becomes several words of one alternative.
pub(super) fn sanitize_terms(terms: &[Vec<String>]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for g in terms {
        let mut group: Group = Vec::new();
        for alt in g.iter().take(MAX_ALTS * 2) {
            let words: Alt = alt.split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))).filter_map(sanitize_token).take(MAX_WORDS).collect();
            if !words.is_empty() && !group.contains(&words) {
                group.push(words);
            }
            if group.len() >= MAX_ALTS {
                break;
            }
        }
        if !group.is_empty() {
            out.push(group);
        }
        if out.len() >= MAX_GROUPS {
            break;
        }
    }
    out
}

/// `'` doubled; the only escaping a quoted SQL string literal needs.
fn sql_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// `SCOPE='file:C:/Users/x/Desktop'` terms for fixed roots, joined with OR.
fn scope_clause(roots: &[PathBuf]) -> Option<String> {
    let parts: Vec<String> = roots
        .iter()
        .map(|r| format!("SCOPE='file:{}'", sql_quote(r.to_string_lossy().trim_end_matches('\\').replace('\\', "/").as_str())))
        .collect();
    match parts.len() {
        0 => None,
        1 => Some(parts[0].clone()),
        _ => Some(format!("({})", parts.join(" OR "))),
    }
}

/// An extension filter value: letters and digits only, at most 8, lowercase, with the dot.
pub(super) fn sanitize_ext(ext: &str) -> Option<String> {
    let e: String = ext.trim().trim_start_matches('.').chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_lowercase();
    (!e.is_empty()).then(|| format!(".{e}"))
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Tier {
    /// File name contains the word (ranked first).
    Name,
    /// Indexed content contains the word (ranked after names).
    Content,
}

/// Build one index query. `None` when there is nothing to ask (no roots, or neither terms nor ext).
pub(super) fn build_sql(roots: &[PathBuf], groups: &[Group], ext: Option<&str>, tier: Tier, top: usize) -> Option<String> {
    let scope = scope_clause(roots)?;
    let mut conds: Vec<String> = vec![scope];
    if let Some(e) = ext.and_then(sanitize_ext) {
        conds.push(format!("System.FileExtension = '{}'", sql_quote(&e)));
    }
    for g in groups {
        let alts: Vec<String> = g
            .iter()
            .map(|alt| match tier {
                Tier::Name => {
                    let ws: Vec<String> = alt.iter().map(|w| format!("System.FileName LIKE '%{}%'", sql_quote(w))).collect();
                    format!("({})", ws.join(" AND "))
                }
                Tier::Content => {
                    let ws: Vec<String> = alt.iter().map(|w| format!("\"{}*\"", w)).collect();
                    format!("CONTAINS('{}')", sql_quote(&ws.join(" AND ")))
                }
            })
            .collect();
        conds.push(if alts.len() == 1 { alts[0].clone() } else { format!("({})", alts.join(" OR ")) });
    }
    if groups.is_empty() && conds.len() < 2 {
        return None;
    }
    Some(format!(
        "SELECT TOP {} System.ItemPathDisplay FROM SystemIndex WHERE {} ORDER BY System.DateModified DESC",
        top.clamp(1, INDEX_TOP),
        conds.join(" AND ")
    ))
}

// ---------------------------------------------------------------------------------------------
// Pure: classification and matching
// ---------------------------------------------------------------------------------------------

pub(super) fn is_risky_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| RISKY_EXT.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// The label of the longest root that contains `path`.
pub(super) fn place_for(path: &Path, roots: &[(String, PathBuf)]) -> Option<String> {
    let p = path.to_string_lossy().to_lowercase();
    roots
        .iter()
        .filter(|(_, r)| {
            let r = r.to_string_lossy().to_lowercase();
            let r = r.trim_end_matches('\\');
            p.len() > r.len() && p.starts_with(r) && p.as_bytes()[r.len()] == b'\\'
        })
        .max_by_key(|(_, r)| r.as_os_str().len())
        .map(|(l, _)| l.clone())
}

/// Folded-name matcher: every group needs one alternative whose words are all contained.
pub(super) fn name_matches(folded_name: &str, groups: &[Vec<Vec<String>>]) -> bool {
    groups.iter().all(|g| g.iter().any(|alt| alt.iter().all(|w| folded_name.contains(w.as_str()))))
}

fn fold_groups(groups: &[Group]) -> Vec<Vec<Vec<String>>> {
    groups.iter().map(|g| g.iter().map(|a| a.iter().map(|w| fold(w)).collect()).collect()).collect()
}

// ---------------------------------------------------------------------------------------------
// Pure-ish: bounded walk
// ---------------------------------------------------------------------------------------------

pub(super) struct WalkLimits {
    pub depth: usize,
    pub budget: Duration,
    pub max_entries: usize,
}

#[derive(Debug)]
pub(super) struct WalkFound {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
pub(super) struct WalkOut {
    pub found: Vec<WalkFound>,
    pub partial: bool,
}

fn filetime_to_utc(t: std::time::SystemTime) -> Option<DateTime<Utc>> {
    Some(DateTime::<Utc>::from(t))
}

/// Breadth first over `roots` (shared visited set, so overlapping roots are read once). Skips
/// hidden/system entries, reparse points and a few heavy folders. Stops (partial) when the
/// time or entry budget is spent.
pub(super) fn walk(roots: &[PathBuf], matches: &dyn Fn(&str) -> bool, lim: &WalkLimits, want: usize) -> WalkOut {
    let start = Instant::now();
    let mut out = WalkOut::default();
    let mut queue: VecDeque<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    let mut visited: HashSet<String> = HashSet::new();
    let mut entries = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        if !visited.insert(dir.to_string_lossy().to_lowercase()) {
            continue;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            entries += 1;
            if entries > lim.max_entries || start.elapsed() >= lim.budget {
                out.partial = true;
                return out;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let attrs = meta.file_attributes();
            if attrs & (ATTR_HIDDEN | ATTR_SYSTEM | ATTR_REPARSE) != 0 {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = meta.is_dir();
            if is_dir && SKIP_DIRS.contains(&name.to_lowercase().as_str()) {
                continue;
            }
            if matches(&fold(&name)) {
                out.found.push(WalkFound { path: entry.path(), is_dir, size: if is_dir { 0 } else { meta.len() }, modified: meta.modified().ok().and_then(filetime_to_utc) });
                // Plenty of matches: no need to read the rest of the tree.
                if out.found.len() >= want.saturating_mul(4).max(200) {
                    out.partial = true;
                    return out;
                }
            }
            if is_dir && depth + 1 < lim.depth {
                queue.push_back((entry.path(), depth + 1));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Key map (opaque key -> path, bounded)
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct KeyMap {
    map: HashMap<String, PathBuf>,
    order: VecDeque<String>,
}

static KEYS: OnceLock<Mutex<KeyMap>> = OnceLock::new();
static KEY_COUNTER: AtomicU64 = AtomicU64::new(1);

fn keys() -> &'static Mutex<KeyMap> {
    KEYS.get_or_init(|| Mutex::new(KeyMap::default()))
}

fn remember(path: &Path) -> String {
    let n = KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let key = mint_key('f', n, &path.to_string_lossy());
    let mut k = keys().lock().unwrap_or_else(|e| e.into_inner());
    k.map.insert(key.clone(), path.to_path_buf());
    k.order.push_back(key.clone());
    while k.order.len() > KEY_CAP {
        if let Some(old) = k.order.pop_front() {
            k.map.remove(&old);
        }
    }
    key
}

fn lookup(key: &str) -> Option<PathBuf> {
    keys().lock().unwrap_or_else(|e| e.into_inner()).map.get(key).cloned()
}

// ---------------------------------------------------------------------------------------------
// ADO worker
// ---------------------------------------------------------------------------------------------

struct IndexRequest {
    sqls: Vec<String>,
    /// Stop after the first query when it already filled this many rows.
    need: usize,
    reply: Sender<Result<Vec<Vec<String>>, String>>,
}

static WORKER: OnceLock<Mutex<Option<Sender<IndexRequest>>>> = OnceLock::new();
/// A request is being served (possibly abandoned by its caller): nothing else is queued behind it.
static BUSY: AtomicBool = AtomicBool::new(false);

fn start_worker() -> Option<Sender<IndexRequest>> {
    let (tx, rx) = mpsc::channel::<IndexRequest>();
    std::thread::Builder::new().name("local-index".into()).spawn(move || worker_main(rx)).ok()?;
    Some(tx)
}

fn worker_main(rx: Receiver<IndexRequest>) {
    crate::debug_log::catch("local-index", || {
        let Ok(_apartment) = ComApartment::init_sta() else {
            dlog!("WARN", "local", "FILES-101 index worker: COM init failed");
            // Drain so callers get "unavailable" instead of waiting for the deadline.
            for req in rx {
                let _ = req.reply.send(Err("com".into()));
                BUSY.store(false, Ordering::SeqCst);
            }
            return;
        };
        let mut conn: Option<Dispatch> = None;
        let mut failed_at: Option<Instant> = None;
        for req in rx {
            let result = serve(&mut conn, &mut failed_at, &req);
            if result.is_err() {
                conn = None; // reopen on the next request
            }
            let _ = req.reply.send(result);
            BUSY.store(false, Ordering::SeqCst);
        }
    });
}

fn serve(conn: &mut Option<Dispatch>, failed_at: &mut Option<Instant>, req: &IndexRequest) -> Result<Vec<Vec<String>>, String> {
    if conn.is_none() {
        if failed_at.is_some_and(|t| t.elapsed() < INDEX_RETRY_AFTER) {
            return Err("index unavailable (recent failure)".into());
        }
        match open_connection() {
            Ok(c) => {
                *failed_at = None;
                *conn = Some(c);
            }
            Err(e) => {
                *failed_at = Some(Instant::now());
                dlog!("WARN", "local", "FILES-101 index open failed: {}", e);
                return Err(format!("open: {e}"));
            }
        }
    }
    let c = conn.as_mut().ok_or("no connection")?;
    let mut out: Vec<Vec<String>> = Vec::new();
    for (i, sql) in req.sqls.iter().enumerate() {
        if i > 0 && out.first().map(|r| r.len()).unwrap_or(0) >= req.need {
            break;
        }
        match run_query(c, sql) {
            Ok(rows) => out.push(rows),
            Err(e) => {
                dlog!("WARN", "local", "FILES-101 index query failed: {}", e);
                // A bad content query must not discard the name hits.
                if i == 0 {
                    return Err(format!("query: {e}"));
                }
                break;
            }
        }
    }
    Ok(out)
}

fn open_connection() -> Result<Dispatch, com::ComError> {
    let mut c = Dispatch::create("ADODB.Connection")?;
    c.call("Open", vec![com::variant_from_str("Provider=Search.CollatorDSO;Extended Properties='Application=Windows';")])?;
    Ok(c)
}

/// Run one query and return the first column of every row (capped).
fn run_query(conn: &mut Dispatch, sql: &str) -> Result<Vec<String>, com::ComError> {
    let Some(mut rs) = conn.call_object("Execute", vec![com::variant_from_str(sql)])? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let result = (|| {
        let mut fields = rs.get_object("Fields")?;
        while out.len() < INDEX_TOP {
            if com::variant_bool(&rs.get("EOF")?).unwrap_or(true) {
                break;
            }
            if let Some(mut f) = fields.call_object("Item", vec![com::variant_from_i32(0)])? {
                if let Some(p) = com::variant_string(&f.get("Value")?) {
                    out.push(p);
                }
            }
            rs.call("MoveNext", Vec::new())?;
        }
        Ok(())
    })();
    let _ = rs.call("Close", Vec::new());
    result.map(|()| out)
}

/// Ask the index. `Err` = unavailable (open failed, timed out, previous request still running).
fn query_index(sqls: Vec<String>, need: usize, deadline: Duration) -> Result<Vec<Vec<String>>, String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("index busy".into());
    }
    let tx = {
        let mut w = WORKER.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner());
        if w.is_none() {
            *w = start_worker();
        }
        w.clone()
    };
    let Some(tx) = tx else {
        BUSY.store(false, Ordering::SeqCst);
        return Err("worker did not start".into());
    };
    let (reply, rx) = mpsc::channel();
    if tx.send(IndexRequest { sqls, need, reply }).is_err() {
        // The worker died: forget it so the next call starts a new one.
        *WORKER.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner()) = None;
        BUSY.store(false, Ordering::SeqCst);
        return Err("worker gone".into());
    }
    match rx.recv_timeout(deadline) {
        Ok(r) => r,
        Err(_) => Err("index timed out".into()), // the worker clears BUSY when it finishes
    }
}

// ---------------------------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------------------------

fn to_hit(path: &Path, roots: &[(String, PathBuf)]) -> Option<FileHit> {
    let meta = std::fs::metadata(path).ok()?;
    let place = place_for(path, roots)?;
    let is_dir = meta.is_dir();
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(FileHit {
        key: remember(path),
        risky: !is_dir && is_risky_name(&name),
        name,
        place,
        modified: meta.modified().ok().and_then(filetime_to_utc),
        size: (!is_dir).then(|| meta.len()),
        is_dir,
    })
}

pub(super) fn search(terms: &[Vec<String>], ext: Option<&str>, limit: usize, budget_ms: u64) -> Result<FileSearch, String> {
    let started = Instant::now();
    let limit = limit.clamp(1, INDEX_TOP);
    let groups = sanitize_terms(terms);
    let ext = ext.and_then(sanitize_ext);
    if groups.is_empty() && ext.is_none() {
        return Ok(FileSearch::default());
    }
    let roots = search_roots();
    if roots.is_empty() {
        return Err("FILES-101: no searchable folders".into());
    }
    let root_paths: Vec<PathBuf> = roots.iter().map(|(_, p)| p.clone()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut hits: Vec<FileHit> = Vec::new();

    // 1. Windows Search index: names first, then content.
    let mut sqls: Vec<String> = Vec::new();
    if let Some(s) = build_sql(&root_paths, &groups, ext.as_deref(), Tier::Name, limit) {
        sqls.push(s);
    }
    if !groups.is_empty() {
        if let Some(s) = build_sql(&root_paths, &groups, ext.as_deref(), Tier::Content, limit) {
            sqls.push(s);
        }
    }
    let deadline = Duration::from_millis(INDEX_DEADLINE_MS.min(budget_ms.max(100)));
    let mut index_answered = false;
    match query_index(sqls, limit, deadline) {
        Ok(tiers) => {
            index_answered = true;
            for rows in tiers {
                for p in rows {
                    if hits.len() >= limit {
                        break;
                    }
                    let path = PathBuf::from(&p);
                    if !path.is_absolute() || !seen.insert(p.to_lowercase()) {
                        continue;
                    }
                    if let Some(h) = to_hit(&path, &roots) {
                        hits.push(h);
                    }
                }
            }
        }
        Err(e) => dlog!("INFO", "local", "FILES-101 index unavailable, using walk ({})", e),
    }

    // 2. Fallback walk: the index failed or found nothing (an empty index proves nothing).
    let mut partial = false;
    let mut index_used = index_answered;
    if hits.is_empty() {
        index_used = false;
        let remaining = budget_ms.saturating_sub(started.elapsed().as_millis() as u64);
        let lim = WalkLimits { depth: WALK_DEPTH, budget: Duration::from_millis(WALK_BUDGET_MS.min(remaining.max(100))), max_entries: WALK_MAX_ENTRIES };
        let fg = fold_groups(&groups);
        let ext_f = ext.clone();
        let matcher = move |folded: &str| {
            let ext_ok = ext_f.as_ref().map(|e| folded.ends_with(e.as_str())).unwrap_or(true);
            ext_ok && name_matches(folded, &fg)
        };
        let out = walk(&root_paths, &matcher, &lim, limit);
        partial = out.partial;
        let mut found = out.found;
        found.sort_by(|a, b| b.modified.cmp(&a.modified));
        for f in found {
            if hits.len() >= limit {
                break;
            }
            if !seen.insert(f.path.to_string_lossy().to_lowercase()) {
                continue;
            }
            if let Some(h) = to_hit(&f.path, &roots) {
                hits.push(h);
            }
        }
    }
    dlog!("INFO", "local", "files: {} hits, index={}, partial={}, {} ms", hits.len(), index_used, partial, started.elapsed().as_millis());
    Ok(FileSearch { hits, partial, index_used })
}

pub(super) fn open(key: &str) -> Result<(), String> {
    let path = lookup(key).ok_or_else(|| "FILES-104: file no longer available".to_string())?;
    open_path(&path)
}

fn open_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("FILES-104: file no longer available".into());
    }
    let meta = std::fs::metadata(path).map_err(|_| "FILES-104: file no longer available".to_string())?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let result = if !meta.is_dir() && is_risky_name(&name) { reveal_in_explorer(path) } else { shell_open(path) };
    result.map_err(|e| format!("FILES-109: could not open ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn groups(g: &[&[&str]]) -> Vec<Group> {
        sanitize_terms(&g.iter().map(|x| x.iter().map(|s| s.to_string()).collect()).collect::<Vec<_>>())
    }

    #[test]
    fn token_sanitiser_keeps_letters_digits_and_few_symbols() {
        assert_eq!(sanitize_token("budget").as_deref(), Some("budget"));
        assert_eq!(sanitize_token("תקציב2026").as_deref(), Some("תקציב2026"));
        assert_eq!(sanitize_token("a_b-c.d").as_deref(), Some("a_b-c.d"));
        assert_eq!(sanitize_token("x'; DROP--").as_deref(), Some("xDROP--"));
        assert_eq!(sanitize_token("50%[a]*\"b\"").as_deref(), Some("50ab"));
        assert_eq!(sanitize_token("'\"%;"), None);
        assert_eq!(sanitize_token("--.."), None);
        assert_eq!(sanitize_token(&"x".repeat(100)).unwrap().chars().count(), 40);
    }

    #[test]
    fn terms_are_capped_and_split() {
        let many: Vec<Vec<String>> = (0..10).map(|i| vec![format!("t{i}")]).collect();
        assert_eq!(sanitize_terms(&many).len(), 6);
        let g = groups(&[&["חשבונית מס", "invoice", "invoice", "a", "b", "c", "d"]]);
        assert_eq!(g[0].len(), 4);
        assert_eq!(g[0][0], vec!["חשבונית".to_string(), "מס".to_string()]);
        assert!(sanitize_terms(&[vec!["'\"".into()]]).is_empty());
    }

    #[test]
    fn sql_name_tier_shape_and_escaping() {
        let roots = vec![PathBuf::from(r"C:\Users\O'Neil\Desktop"), PathBuf::from(r"C:\Users\O'Neil\Documents\")];
        let sql = build_sql(&roots, &groups(&[&["budget", "תקציב"]]), Some(".PDF"), Tier::Name, 99).unwrap();
        assert!(sql.starts_with("SELECT TOP 50 System.ItemPathDisplay FROM SystemIndex WHERE "));
        assert!(sql.contains("(SCOPE='file:C:/Users/O''Neil/Desktop' OR SCOPE='file:C:/Users/O''Neil/Documents')"));
        assert!(sql.contains("System.FileExtension = '.pdf'"));
        assert!(sql.contains("((System.FileName LIKE '%budget%') OR (System.FileName LIKE '%תקציב%'))"));
        assert!(sql.ends_with("ORDER BY System.DateModified DESC"));
    }

    #[test]
    fn sql_content_tier_uses_contains() {
        let sql = build_sql(&[PathBuf::from(r"C:\x")], &groups(&[&["חשבונית מס"]]), None, Tier::Content, 10).unwrap();
        assert!(sql.contains("CONTAINS('\"חשבונית*\" AND \"מס*\"')"));
        assert!(!sql.contains("FileName"));
    }

    #[test]
    fn injection_attempts_cannot_break_out() {
        let evil = vec![vec!["x' OR 1=1 --".to_string(), "') ; DROP TABLE a;--".to_string(), "%_[a-z]".to_string()]];
        let g = sanitize_terms(&evil);
        let sql = build_sql(&[PathBuf::from(r"C:\x")], &g, Some("p'df"), Tier::Name, 5).unwrap();
        // Every quote in the statement belongs to our own literals: count is even and no
        // sanitized word contains one.
        assert_eq!(sql.matches('\'').count() % 2, 0);
        for alt in g.iter().flatten().flatten() {
            assert!(!alt.contains(['\'', '"', ';', '%', '[', ']', '*', ' ']), "{alt}");
        }
        assert!(!sql.contains("DROP TABLE a;"));
        assert!(sql.contains("System.FileExtension = '.pdf'"));
        let sql = build_sql(&[PathBuf::from(r"C:\x")], &g, None, Tier::Content, 5).unwrap();
        assert_eq!(sql.matches('\'').count() % 2, 0);
    }

    #[test]
    fn nothing_to_ask_gives_no_sql() {
        assert!(build_sql(&[PathBuf::from(r"C:\x")], &[], None, Tier::Name, 5).is_none());
        assert!(build_sql(&[], &groups(&[&["a"]]), None, Tier::Name, 5).is_none());
        // An extension alone is a valid question ("my pdfs").
        assert!(build_sql(&[PathBuf::from(r"C:\x")], &[], Some("pdf"), Tier::Name, 5).is_some());
    }

    #[test]
    fn risky_extensions() {
        for n in ["a.exe", "A.BAT", "x.ps1", "m.msi", "l.LNK", "s.js", "r.reg", "h.hta"] {
            assert!(is_risky_name(n), "{n}");
        }
        for n in ["a.pdf", "notes.txt", "exe", "archive.zip", "תקציב.xlsx"] {
            assert!(!is_risky_name(n), "{n}");
        }
    }

    #[test]
    fn place_uses_longest_root() {
        let roots = vec![
            ("OneDrive".to_string(), PathBuf::from(r"C:\Users\u\OneDrive")),
            ("Documents".to_string(), PathBuf::from(r"C:\Users\u\OneDrive\Documents")),
        ];
        assert_eq!(place_for(Path::new(r"C:\Users\u\OneDrive\Documents\a.txt"), &roots).as_deref(), Some("Documents"));
        assert_eq!(place_for(Path::new(r"c:\users\u\onedrive\b.txt"), &roots).as_deref(), Some("OneDrive"));
        assert_eq!(place_for(Path::new(r"C:\Users\u\OneDriveX\b.txt"), &roots), None);
        assert_eq!(place_for(Path::new(r"D:\b.txt"), &roots), None);
    }

    #[test]
    fn name_matching_is_and_of_or() {
        let g = fold_groups(&groups(&[&["budget", "תקציב"], &["2026"]]));
        assert!(name_matches("budget 2026.xlsx", &g));
        assert!(name_matches("תקציב-2026.pdf", &g));
        assert!(!name_matches("budget 2025.xlsx", &g));
        assert!(!name_matches("2026.txt", &g));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("companyisland-test-files-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn lim(depth: usize, ms: u64, entries: usize) -> WalkLimits {
        WalkLimits { depth, budget: Duration::from_millis(ms), max_entries: entries }
    }

    fn names(out: &WalkOut) -> Vec<String> {
        let mut v: Vec<String> = out.found.iter().map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn walk_respects_depth_and_skip_rules() {
        let root = scratch("walk");
        let mk = |rel: &str| {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, b"x").unwrap();
        };
        mk("budget1.txt");
        mk("a/budget2.txt");
        mk("a/b/budget3.txt");
        mk("a/b/c/budget4.txt");
        mk("a/b/c/d/budget5.txt"); // level 5: too deep
        mk("node_modules/budget_nm.txt");
        mk(".git/budget_git.txt");
        mk("AppData/budget_ad.txt");
        mk("other.txt");
        let m = |n: &str| n.contains("budget");
        let out = walk(&[root.clone()], &m, &lim(4, 5000, 20_000), 10);
        assert!(!out.partial);
        assert_eq!(names(&out), vec!["budget1.txt", "budget2.txt", "budget3.txt", "budget4.txt"]);

        // Hidden files and folders are skipped.
        let hidden = root.join("hid");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(hidden.join("budget_h.txt"), b"x").unwrap();
        let _ = std::process::Command::new("attrib").arg("+h").arg(&hidden).status();
        fs::write(root.join("budget_hf.txt"), b"x").unwrap();
        let _ = std::process::Command::new("attrib").arg("+h").arg(root.join("budget_hf.txt")).status();
        let out = walk(&[root.clone()], &m, &lim(4, 5000, 20_000), 10);
        let n = names(&out);
        assert!(!n.contains(&"budget_h.txt".to_string()) && !n.contains(&"budget_hf.txt".to_string()), "{n:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn walk_budgets_mark_partial() {
        let root = scratch("budget");
        for i in 0..30 {
            fs::write(root.join(format!("f{i}.txt")), b"x").unwrap();
        }
        let out = walk(&[root.clone()], &|_| true, &lim(4, 5000, 10), 100);
        assert!(out.partial);
        let out = walk(&[root.clone()], &|_| false, &lim(4, 0, 20_000), 10);
        assert!(out.partial);
        let out = walk(&[root.clone()], &|n| n.starts_with("f1"), &lim(4, 5000, 20_000), 10);
        assert!(!out.partial && out.found.len() == 11); // f1, f10..f19
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn walk_reads_overlapping_roots_once() {
        let root = scratch("overlap");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs").join("a.txt"), b"x").unwrap();
        let out = walk(&[root.join("docs"), root.clone()], &|n| n == "a.txt", &lim(4, 5000, 1000), 10);
        assert_eq!(out.found.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn walk_classifies_dirs() {
        let root = scratch("dirs");
        fs::create_dir_all(root.join("budget folder")).unwrap();
        let out = walk(&[root.clone()], &|n| n.contains("budget"), &lim(4, 5000, 1000), 10);
        assert_eq!(out.found.len(), 1);
        assert!(out.found[0].is_dir);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn open_refuses_unknown_keys_and_bad_paths() {
        assert!(open("f0000000000000000").unwrap_err().starts_with("FILES-104"));
        assert!(open("").unwrap_err().starts_with("FILES-104"));
        assert!(open("C:\\Windows\\notepad.exe").unwrap_err().starts_with("FILES-104"));
        assert!(open_path(Path::new("relative.txt")).unwrap_err().starts_with("FILES-104"));
        assert!(open_path(Path::new(r"C:\definitely\not\here.txt")).unwrap_err().starts_with("FILES-104"));
    }

    #[test]
    fn keys_resolve_then_age_out() {
        let k = remember(Path::new(r"C:\a\first.txt"));
        assert_eq!(lookup(&k), Some(PathBuf::from(r"C:\a\first.txt")));
        for i in 0..(KEY_CAP + 5) {
            remember(Path::new(&format!(r"C:\a\{i}.txt")));
        }
        assert_eq!(lookup(&k), None);
    }

    #[test]
    fn empty_question_returns_nothing_without_touching_anything() {
        let r = search(&[], None, 10, 1000).unwrap();
        assert!(r.hits.is_empty() && !r.index_used);
    }

    /// Live, read-only: opens the Windows Search index and times a file-name query.
    #[test]
    #[ignore]
    fn live_index_probe() {
        let roots = search_roots();
        println!("roots: {}", roots.len());
        let paths: Vec<PathBuf> = roots.iter().map(|(_, p)| p.clone()).collect();
        let g = groups(&[&["pdf", "txt", "docx", "md"]]);
        for (label, tier, ext) in [("name", Tier::Name, None), ("ext only", Tier::Name, Some("pdf")), ("content", Tier::Content, None)] {
            let g2 = if ext.is_some() { Vec::new() } else { g.clone() };
            let sql = build_sql(&paths, &g2, ext, tier, 50).unwrap();
            let t = Instant::now();
            let r = query_index(vec![sql], 50, Duration::from_millis(5_000));
            println!("{label}: {:?} rows, {} ms", r.as_ref().map(|t| t.iter().map(|x| x.len()).collect::<Vec<_>>()), t.elapsed().as_millis());
        }
        let t = Instant::now();
        let r = search(&[vec!["pdf".into()]], None, 20, 3_000).unwrap();
        println!("search_files: hits={} index_used={} partial={} {} ms", r.hits.len(), r.index_used, r.partial, t.elapsed().as_millis());
        let t = Instant::now();
        let r = search(&[vec!["pdf".into()]], None, 20, 3_000).unwrap();
        println!("search_files (warm): hits={} {} ms", r.hits.len(), t.elapsed().as_millis());
        let r = search(&[vec!["zzqqxxnotthere".into()]], None, 20, 3_000).unwrap();
        println!("miss -> walk: hits={} partial={} index_used={}", r.hits.len(), r.partial, r.index_used);
    }
}
