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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------------------------

const INDEX_DEADLINE_MS: u64 = 1_500;
const INDEX_TOP: usize = 50;
const INDEX_RETRY_AFTER: Duration = Duration::from_secs(60);
/// A request unanswered this long means the worker hangs: it is abandoned and replaced.
const INDEX_STALE: Duration = Duration::from_secs(10);
/// The worker (and its connection to the search service) exits after this long without a request.
const WORKER_IDLE: Duration = Duration::from_secs(300);

const MAX_GROUPS: usize = 6;
const MAX_ALTS: usize = 4;
const MAX_WORDS: usize = 3;
const MAX_TOKEN_CHARS: usize = 40;

const WALK_DEPTH: usize = 4;
const WALK_BUDGET_MS: u64 = 700;
const WALK_MAX_ENTRIES: usize = 20_000;

/// Keys kept for opening results: the assistant store keeps 20 queries (see `assistant::store`),
/// each with at most `INDEX_TOP` file hits, so every card still shown can open its file.
const STORE_QUERIES: usize = 20;
const KEY_CAP: usize = INDEX_TOP * STORE_QUERIES;

/// A whole file search (roots, index, walk, metadata) is abandoned after its budget plus this slack.
const SEARCH_SLACK_MS: u64 = 300;
/// A search thread older than this is treated as stuck (a dead share) and no longer blocks new ones.
const SEARCH_STALE: Duration = Duration::from_secs(20);
/// The folders are fixed, so the list (and the `is_dir` probes behind it) is cached.
const ROOTS_TTL: Duration = Duration::from_secs(60);

const ATTR_HIDDEN: u32 = 0x2;
const ATTR_SYSTEM: u32 = 0x4;
const ATTR_REPARSE: u32 = 0x400;

const SKIP_DIRS: [&str; 3] = ["node_modules", ".git", "appdata"];

/// Allowlist: only these types are opened with their default app on a click. Everything else
/// (executables, scripts, shortcuts, installers, `.url`, `.jar`, `.chm`, `.rdp`, ... and files
/// without an extension) is only revealed in Explorer. Macro-enabled Office files (`docm`, `xlsm`,
/// `pptm`, `dotm`, `xlam`, ...) are deliberately NOT listed: they run code on open. `html`/`htm`/
/// `svg` are listed because they open in the browser sandbox, not as a local program.
const OPEN_EXT: &[&str] = &[
    "docx", "xlsx", "pptx", "doc", "xls", "ppt", "dotx", "xltx", "potx", "pdf", "txt", "rtf", "csv", "tsv", "odt", "ods", "odp", "png", "jpg",
    "jpeg", "gif", "bmp", "tif", "tiff", "svg", "webp", "heic", "mp3", "mp4", "wav", "m4a", "flac", "ogg", "mov", "avi", "wmv", "mkv", "zip",
    "7z", "rar", "vsd", "vsdx", "msg", "eml", "one", "md", "json", "xml", "html", "htm", "log",
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

/// True unless the extension is on the [`OPEN_EXT`] allowlist (fails closed: no extension, a
/// trailing dot or space, or an unknown type is only revealed).
pub(super) fn is_risky_name(name: &str) -> bool {
    !Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| OPEN_EXT.contains(&e.to_ascii_lowercase().as_str()))
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

type IndexResult = Result<Vec<Vec<String>>, String>;

struct IndexRequest {
    sqls: Vec<String>,
    /// Stop after the first query when it already filled this many rows.
    need: usize,
    reply: Sender<IndexResult>,
    /// Identifies this request in [`Slot::busy`]; only the matching release clears it.
    token: u64,
}

/// The worker and the request it is serving, behind ONE lock so they can never disagree: a
/// request is queued only while `busy` is set (under the lock), a worker retires only while it is
/// not (under the lock), and a worker that died, panicked, retired or hung is replaced.
struct Slot {
    /// (worker id, sender). `None` = no worker running.
    worker: Option<(u64, Sender<IndexRequest>)>,
    /// (request token, started). `None` = idle.
    busy: Option<(u64, Instant)>,
    next: u64,
}

struct Index {
    slot: Mutex<Slot>,
}

static INDEX: Index = Index { slot: Mutex::new(Slot { worker: None, busy: None, next: 1 }) };

impl Index {
    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Queue a request. A request older than `stale_after` is presumed hung: its worker is
    /// abandoned (it may finish or stay stuck, nobody waits for it) and a new one is started.
    /// `spawn` starts the worker thread for (id, receiver) and says whether it did.
    fn submit(
        &self,
        sqls: Vec<String>,
        need: usize,
        stale_after: Duration,
        spawn: &dyn Fn(u64, Receiver<IndexRequest>) -> bool,
    ) -> Result<Receiver<IndexResult>, String> {
        let mut s = self.lock();
        if let Some((_, since)) = s.busy {
            if since.elapsed() < stale_after {
                return Err("index busy".into());
            }
            dlog!("WARN", "local", "FILES-101 index worker presumed hung, starting a new one");
            s.worker = None;
            s.busy = None;
        }
        if s.worker.is_none() {
            let id = s.next;
            s.next += 1;
            let (tx, rx) = mpsc::channel();
            if !spawn(id, rx) {
                return Err("worker did not start".into());
            }
            s.worker = Some((id, tx));
        }
        let token = s.next;
        s.next += 1;
        let (reply, reply_rx) = mpsc::channel();
        let sent = s.worker.as_ref().map(|(_, tx)| tx.send(IndexRequest { sqls, need, reply, token }).is_ok()).unwrap_or(false);
        if !sent {
            // The worker is gone: forget it so the next call starts a new one.
            s.worker = None;
            return Err("worker gone".into());
        }
        s.busy = Some((token, Instant::now()));
        Ok(reply_rx)
    }

    /// The worker finished request `token`. A late finish of an abandoned request is a no-op.
    fn release(&self, token: u64) {
        let mut s = self.lock();
        if s.busy.is_some_and(|(t, _)| t == token) {
            s.busy = None;
        }
    }

    /// Worker `id` has been idle: retire it when nothing is queued (a request is queued only
    /// together with `busy`, under this lock). True = the worker must exit now.
    fn retire_if_idle(&self, id: u64) -> bool {
        let mut s = self.lock();
        match &s.worker {
            Some((w, _)) if *w == id => {
                if s.busy.is_some() {
                    return false;
                }
                s.worker = None;
                true
            }
            _ => true, // replaced while idle: nobody sends to it any more
        }
    }

    /// Worker `id` is exiting (also after a panic): forget it and any request it held.
    fn worker_exit(&self, id: u64) {
        let mut s = self.lock();
        if s.worker.as_ref().is_some_and(|(w, _)| *w == id) {
            s.worker = None;
            s.busy = None;
        }
    }
}

struct ExitGuard {
    index: &'static Index,
    id: u64,
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.index.worker_exit(self.id);
    }
}

/// The worker's request loop: serve, reply, release; exit when the sender is gone or after
/// `idle` without a request (the open connection goes with the thread).
fn worker_loop(index: &'static Index, id: u64, rx: Receiver<IndexRequest>, idle: Duration, mut serve: impl FnMut(&IndexRequest) -> IndexResult) {
    let _exit = ExitGuard { index, id };
    loop {
        match rx.recv_timeout(idle) {
            Ok(req) => {
                let result = serve(&req);
                let _ = req.reply.send(result);
                index.release(req.token);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if index.retire_if_idle(id) {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn spawn_worker(id: u64, rx: Receiver<IndexRequest>) -> bool {
    std::thread::Builder::new().name("local-index".into()).spawn(move || worker_main(&INDEX, id, rx)).is_ok()
}

fn worker_main(index: &'static Index, id: u64, rx: Receiver<IndexRequest>) {
    crate::debug_log::catch("local-index", || {
        let Ok(_apartment) = ComApartment::init_sta() else {
            dlog!("WARN", "local", "FILES-101 index worker: COM init failed");
            // Answer "unavailable" at once instead of making callers wait for the deadline.
            worker_loop(index, id, rx, WORKER_IDLE, |_| Err("com".into()));
            return;
        };
        let mut conn: Option<Dispatch> = None;
        let mut failed_at: Option<Instant> = None;
        worker_loop(index, id, rx, WORKER_IDLE, |req| {
            let result = serve(&mut conn, &mut failed_at, req);
            if result.is_err() {
                conn = None; // reopen on the next request
            }
            result
        });
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
fn query_index(sqls: Vec<String>, need: usize, deadline: Duration) -> IndexResult {
    let rx = INDEX.submit(sqls, need, INDEX_STALE, &spawn_worker)?;
    match rx.recv_timeout(deadline) {
        Ok(r) => r,
        // Released when the worker finishes; presumed hung (and replaced) after INDEX_STALE.
        Err(_) => Err("index timed out".into()),
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

/// At most one bounded job runs at a time; a job older than the stale limit is presumed stuck
/// (a dead network share) and no longer blocks the next one.
struct Gate {
    running: Mutex<Option<(u64, Instant)>>,
}

static SEARCH_GATE: Gate = Gate { running: Mutex::new(None) };
static GATE_COUNTER: AtomicU64 = AtomicU64::new(1);

impl Gate {
    fn enter(&self, stale: Duration) -> Option<u64> {
        let mut r = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if r.is_some_and(|(_, since)| since.elapsed() < stale) {
            return None;
        }
        let id = GATE_COUNTER.fetch_add(1, Ordering::Relaxed);
        *r = Some((id, Instant::now()));
        Some(id)
    }

    fn leave(&self, id: u64) {
        let mut r = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if r.is_some_and(|(i, _)| i == id) {
            *r = None;
        }
    }
}

struct GateGuard {
    gate: &'static Gate,
    id: u64,
}

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.gate.leave(self.id);
    }
}

/// Run `f` on a helper thread and wait at most `wait` for it. `None` = it did not finish in time
/// (the thread is abandoned, it may be stuck on a share) or a previous job is still running.
fn run_bounded<T: Send + 'static>(gate: &'static Gate, stale: Duration, wait: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let id = gate.enter(stale)?;
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("local-files".into()).spawn(move || {
        let _guard = GateGuard { gate, id };
        let _ = tx.send(f());
    });
    if spawned.is_err() {
        gate.leave(id);
        return None;
    }
    rx.recv_timeout(wait).ok()
}

/// The search roots, cached: `search_roots()` probes every folder with `is_dir`, which can block
/// for the SMB timeout on a redirected folder. Only called on the helper thread.
fn cached_roots() -> Vec<(String, PathBuf)> {
    static ROOTS: Mutex<Option<(Instant, Vec<(String, PathBuf)>)>> = Mutex::new(None);
    if let Some((at, r)) = ROOTS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if at.elapsed() < ROOTS_TTL {
            return r.clone();
        }
    }
    let fresh = search_roots();
    if !fresh.is_empty() {
        *ROOTS.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), fresh.clone()));
    }
    fresh
}

/// Network (UNC) roots are answered by the index only: walking one can stall far past the budget.
fn is_unc(p: &Path) -> bool {
    p.to_string_lossy().starts_with(r"\\")
}

/// The whole search, with a hard wall-clock limit of `budget_ms` plus a small slack: the work
/// (folder probes, index, walk, metadata of every hit) runs on a helper thread that is abandoned
/// when the limit passes, because any of those calls can block on a stalled share.
pub(super) fn search(terms: &[Vec<String>], ext: Option<&str>, limit: usize, budget_ms: u64) -> Result<FileSearch, String> {
    if sanitize_terms(terms).is_empty() && ext.and_then(sanitize_ext).is_none() {
        return Ok(FileSearch::default());
    }
    let (terms, ext) = (terms.to_vec(), ext.map(str::to_string));
    let wait = Duration::from_millis(budget_ms.max(100) + SEARCH_SLACK_MS);
    match run_bounded(&SEARCH_GATE, SEARCH_STALE, wait, move || search_inner(&terms, ext.as_deref(), limit, budget_ms)) {
        Some(r) => r,
        None => {
            dlog!("WARN", "local", "FILES-102 file search did not finish in time (stalled folder?)");
            Ok(FileSearch { hits: Vec::new(), partial: true, index_used: false })
        }
    }
}

fn search_inner(terms: &[Vec<String>], ext: Option<&str>, limit: usize, budget_ms: u64) -> Result<FileSearch, String> {
    let started = Instant::now();
    let limit = limit.clamp(1, INDEX_TOP);
    let groups = sanitize_terms(terms);
    let ext = ext.and_then(sanitize_ext);
    if groups.is_empty() && ext.is_none() {
        return Ok(FileSearch::default());
    }
    let roots = cached_roots();
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
        let local_roots: Vec<PathBuf> = root_paths.iter().filter(|p| !is_unc(p)).cloned().collect();
        let out = walk(&local_roots, &matcher, &lim, limit);
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
        // The old deny list.
        for n in ["a.exe", "A.BAT", "x.ps1", "m.msi", "l.LNK", "s.js", "r.reg", "h.hta"] {
            assert!(is_risky_name(n), "{n}");
        }
        // Executable-equivalent types the old list missed, macro-enabled Office files, and
        // names that only look harmless (no extension, trailing dot or space, double extension).
        for n in [
            "i.url", "t.jar", "h.chm", "x.rdp", "a.appinstaller", "p.pif", "s.msc", "i.inf", "w.wsh", "w.ws", "c.wsc", "p.msp", "a.appx", "a.msix",
            "j.jnlp", "x.xll", "d.diagcab", "l.library-ms", "s.settingcontent-ms", "g.gadget", "a.application", "a.mht", "m.mde", "r.docm", "r.XLSM",
            "r.pptm", "r.dotm", "r.xlam", "exe", "README", "a.pdf.exe", "a.exe.", "a.exe ", "a.pdf ", ".hidden", "תקציב.xyz",
        ] {
            assert!(is_risky_name(n), "{n}");
        }
        for n in ["a.pdf", "notes.txt", "archive.zip", "תקציב.xlsx", "R.DOCX", "p.JPG", "v.mp4", "m.msg", "n.one", "d.vsdx", "x.md", "a.json", "i.html"] {
            assert!(!is_risky_name(n), "{n}");
        }
    }

    #[test]
    fn allowlist_has_no_executable_or_macro_types() {
        for e in OPEN_EXT {
            assert_eq!(*e, e.to_ascii_lowercase());
            assert!(!["exe", "bat", "cmd", "ps1", "vbs", "js", "lnk", "url", "jar", "docm", "xlsm", "pptm", "dotm", "xlam", "hta", "msi", "chm", "rdp"].contains(e), "{e}");
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
    fn key_map_holds_every_hit_of_every_stored_query() {
        // 20 stored queries x 50 hits (the assistant store and INDEX_TOP): none may age out early.
        assert!(KEY_CAP >= 20 * INDEX_TOP);
    }

    fn leak_index() -> &'static Index {
        Box::leak(Box::new(Index { slot: Mutex::new(Slot { worker: None, busy: None, next: 1 }) }))
    }

    /// A test worker thread; `handler` decides what a request does (answer, hang, panic).
    fn fake_spawn(index: &'static Index, idle: Duration, handler: fn(&IndexRequest) -> IndexResult) -> impl Fn(u64, Receiver<IndexRequest>) -> bool {
        move |id, rx| {
            std::thread::spawn(move || {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker_loop(index, id, rx, idle, handler)));
            });
            true
        }
    }

    fn ok_handler(_: &IndexRequest) -> IndexResult {
        Ok(vec![vec!["C:\\x\\a.txt".to_string()]])
    }

    fn hang_handler(_: &IndexRequest) -> IndexResult {
        std::thread::sleep(Duration::from_secs(30));
        Ok(Vec::new())
    }

    fn panic_handler(_: &IndexRequest) -> IndexResult {
        panic!("boom");
    }

    const LONG: Duration = Duration::from_secs(3600);

    #[test]
    fn index_recovers_after_a_hung_request() {
        let idx = leak_index();
        let hang = fake_spawn(idx, LONG, hang_handler);
        let rx = idx.submit(vec!["q".into()], 1, Duration::from_millis(150), &hang).unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        // Still within the stale limit: busy, nothing is queued behind the hung one.
        assert_eq!(idx.submit(vec!["q".into()], 1, Duration::from_millis(150), &hang).unwrap_err(), "index busy");
        std::thread::sleep(Duration::from_millis(200));
        // Past it: the hung worker is abandoned and a new one answers.
        let ok = fake_spawn(idx, LONG, ok_handler);
        let rx = idx.submit(vec!["q".into()], 1, Duration::from_millis(150), &ok).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap()[0][0], "C:\\x\\a.txt");
    }

    #[test]
    fn index_recovers_after_a_worker_panic() {
        let idx = leak_index();
        let bad = fake_spawn(idx, LONG, panic_handler);
        let rx = idx.submit(vec!["q".into()], 1, LONG, &bad).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_err()); // reply sender dropped by the unwind
        // No wait for a stale limit (LONG): the dead worker released the slot by itself.
        let ok = fake_spawn(idx, LONG, ok_handler);
        let mut answered = false;
        for _ in 0..50 {
            match idx.submit(vec!["q".into()], 1, LONG, &ok) {
                Ok(rx) => {
                    assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_ok());
                    answered = true;
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        assert!(answered, "slot stayed busy after the worker panicked");
    }

    #[test]
    fn idle_worker_retires_and_the_next_request_starts_a_new_one() {
        let idx = leak_index();
        let idle = Duration::from_millis(60);
        let ok = fake_spawn(idx, idle, ok_handler);
        let rx = idx.submit(vec!["q".into()], 1, LONG, &ok).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_ok());
        std::thread::sleep(Duration::from_millis(400));
        assert!(idx.lock().worker.is_none(), "idle worker still registered");
        let rx = idx.submit(vec!["q".into()], 1, LONG, &ok).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_ok());
    }

    #[test]
    fn late_finish_of_an_abandoned_request_does_not_free_the_new_one() {
        let idx = leak_index();
        idx.lock().busy = Some((7, Instant::now()));
        idx.release(8); // someone else's token
        assert!(idx.lock().busy.is_some());
        idx.release(7);
        assert!(idx.lock().busy.is_none());
    }

    #[test]
    fn bounded_job_returns_in_time_and_does_not_pile_up() {
        static GATE: Gate = Gate { running: Mutex::new(None) };
        let t = Instant::now();
        let r = run_bounded(&GATE, Duration::from_secs(60), Duration::from_millis(100), || {
            std::thread::sleep(Duration::from_secs(5));
            1
        });
        assert!(r.is_none() && t.elapsed() < Duration::from_secs(2));
        // The stuck job still holds the gate: the next one is refused at once, no second thread.
        let t = Instant::now();
        assert!(run_bounded(&GATE, Duration::from_secs(60), Duration::from_millis(100), || 2).is_none());
        assert!(t.elapsed() < Duration::from_millis(50));
        // A job that outlives the stale limit no longer blocks.
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(run_bounded(&GATE, Duration::from_millis(10), Duration::from_secs(2), || 3), Some(3));
    }

    #[test]
    fn bounded_job_frees_the_gate_when_it_finishes() {
        static GATE: Gate = Gate { running: Mutex::new(None) };
        assert_eq!(run_bounded(&GATE, Duration::from_secs(60), Duration::from_secs(2), || 1), Some(1));
        let mut again = None;
        for _ in 0..50 {
            again = run_bounded(&GATE, Duration::from_secs(60), Duration::from_secs(2), || 2);
            if again.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(again, Some(2));
    }

    #[test]
    fn unc_roots_are_not_walked() {
        assert!(is_unc(Path::new(r"\\server\share\Documents")));
        assert!(!is_unc(Path::new(r"C:\Users\u\Documents")));
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
