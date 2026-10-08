//! Windows Sticky Notes, READ ONLY.
//!
//! What is supported. Sticky Notes 3.x/4.x and later (the Store app `Microsoft.MicrosoftStickyNotes`) keep
//! their notes in a SQLite file, `%LOCALAPPDATA%\Packages\Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe\
//! LocalState\plum.sqlite`, table `Note` (`Id`, `Text`, `Theme`, `CreatedAt`, `UpdatedAt`, `DeletedAt`, ...).
//! That is the only layout read here. Everything else is a calm "not available", never an error:
//!   * `notInstalled`: no `Packages\Microsoft.MicrosoftStickyNotes_...` folder at all;
//!   * `noData`: the app is installed but has no `plum.sqlite` (never used, or the notes live only in the
//!     cloud / OneNote and were never synced to this PC);
//!   * `unsupported`: a file that is not the layout above (a `Note` table without `Id`/`Text`, or the
//!     Windows 7-era `StickyNotes.snt` when the Store app is absent);
//!   * `unavailable`: `winsqlite3.dll` cannot be loaded, or the file cannot be copied or opened.
//! On the development machine of this change the app is installed (4.0.6104.0) but `LocalState` is empty, so
//! the real-world path is covered by a fixture built with the same `winsqlite3.dll` (see the tests).
//!
//! How it is read. No SQLite crate: `winsqlite3.dll` ships in `System32` on Windows 10 and 11 and is loaded
//! dynamically (`LOAD_LIBRARY_SEARCH_SYSTEM32`, so a planted DLL is never picked up). The live file is never
//! opened. `plum.sqlite` and its `-wal` are copied into `%LOCALAPPDATA%\CompanyIsland\sticky-tmp\<pid>-<n>\`
//! and that copy is opened with `SQLITE_OPEN_READONLY`, then deleted. The `-wal` carries the newest notes
//! (the main file is only updated at a checkpoint), which is why it is copied. The `-shm` is deliberately NOT
//! copied: it is a cache of the WAL index, and a copy taken while Sticky Notes writes can disagree with the
//! copied WAL; without it SQLite rebuilds the index from the WAL itself, which is always consistent.
//!
//! When it is read. The result is cached and re-read only when the size or modification time of
//! `plum.sqlite` or `plum.sqlite-wal` changed (two `stat` calls, no polling inside Rust). Callers: the
//! Notes tab (the page asks every >= 10 s while the tab is open) and smart search (`notes.search`).
//! Note text is user content: it is never logged, only counts and state names are.

use crate::{notifications, paths, rt};
use serde::Serialize;
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use windows::core::{w, PCSTR};
use windows::Win32::Foundation::{HANDLE, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

/// The Store app's identity: the package family and the `shell:AppsFolder` id the Start menu launches.
const PACKAGE_FAMILY: &str = "Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe";
pub const AUMID: &str = "Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe!App";
/// `AssistantItem.source` of a search hit.
pub const SOURCE_LABEL: &str = "Sticky Notes";
/// A search hit id is `sticky:<note id>`; a note of the island itself never has a ':' in its id.
const HIT_PREFIX: &str = "sticky:";

/// At most this many notes (newest first) and characters per note reach the page and the search.
const MAX_NOTES: usize = 500;
const MAX_TEXT_CHARS: usize = 10_000;
const MAX_ROWS: usize = 5_000;
const TITLE_MAX_CHARS: usize = 80;
/// After a failed read the same file state is not copied again for this long.
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(30);
/// ... or this long when an earlier good read can keep being shown.
const RETRY_AFTER_STALE: Duration = Duration::from_secs(5);
/// A leftover copy of a crashed instance is removed once it is this old.
const STALE_COPY_AGE: Duration = Duration::from_secs(10 * 60);

// =============================================================================
// Wire types
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    Ok,
    NoData,
    NotInstalled,
    Unsupported,
    Unavailable,
}

impl Availability {
    fn as_str(self) -> &'static str {
        match self {
            Availability::Ok => "ok",
            Availability::NoData => "noData",
            Availability::NotInstalled => "notInstalled",
            Availability::Unsupported => "unsupported",
            Availability::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StickyNote {
    /// The note's own id (a GUID); only meaningful for display keys and search hits.
    pub id: String,
    /// Plain text: the `\id=` paragraph prefixes and formatting markers are gone, Hebrew intact.
    pub text: String,
    /// The first non-empty line, clipped.
    pub title: String,
    /// `yellow` `green` `blue` `purple` `pink` `gray` `charcoal`.
    pub colour: String,
    /// Unix ms.
    pub updated_at: i64,
    /// Unix ms (0 when the file does not say).
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StickySnapshot {
    pub availability: Availability,
    /// Newest first; empty unless `availability` is `ok`.
    pub notes: Vec<StickyNote>,
    /// Changes whenever availability or notes changed, so the page can skip identical answers.
    pub revision: u64,
}

// =============================================================================
// Note text
// =============================================================================

/// `\id=<token>` starts every paragraph of a note's text.
const ID_MARKER: &str = "\\id=";

/// The plain text of a stored note: paragraph prefixes (`\id=<guid> `) removed, lines kept in order,
/// inline markers (`**bold**`, `__u__`, `~~s~~`, `*italic*`, `![image](...)`, `[link](...)`) reduced to
/// their text, control characters dropped. Direction marks and all other characters are untouched.
pub fn parse_text(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut paragraphs: Vec<String> = Vec::new();
    let mut rest = text.as_str();
    // Text before the first marker (a note without any marker is all of it).
    match rest.find(ID_MARKER) {
        Some(0) => {}
        Some(pos) => {
            let head = rest[..pos].trim_matches(['\n', ' ']);
            if !head.is_empty() {
                paragraphs.push(head.to_string());
            }
            rest = &rest[pos..];
        }
        None => {
            paragraphs.push(rest.trim_matches(['\n', ' ']).to_string());
            rest = "";
        }
    }
    while let Some(after) = rest.strip_prefix(ID_MARKER) {
        // The token runs to the first whitespace; one space after it separates it from the text.
        let token_end = after.find(char::is_whitespace).unwrap_or(after.len());
        let mut body = &after[token_end..];
        if let Some(b) = body.strip_prefix(' ') {
            body = b;
        }
        let next = body.find(ID_MARKER).unwrap_or(body.len());
        paragraphs.push(body[..next].trim_end_matches(['\n', ' ']).to_string());
        rest = &body[next..];
    }
    let mut lines: Vec<String> = Vec::new();
    for p in &paragraphs {
        for line in p.split('\n') {
            lines.push(strip_inline(line));
        }
    }
    // Blank lines at both ends go; a run of blank lines inside is capped at one.
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let blank = line.trim().is_empty();
        if blank && (out.is_empty() || out.last().is_some_and(|l| l.is_empty())) {
            continue;
        }
        out.push(if blank { String::new() } else { line });
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

/// Removes the markers of one line (see [`parse_text`]).
fn strip_inline(line: &str) -> String {
    let cleaned: String = line.chars().filter(|c| !c.is_control() || *c == '\t').collect();
    let mut s = strip_links(&cleaned);
    for marker in ["**", "__", "~~", "*"] {
        s = strip_pairs(&s, marker);
    }
    // A bullet written as `* item`.
    if let Some(rest) = s.trim_start().strip_prefix("* ") {
        let indent = s.len() - s.trim_start().len();
        s = format!("{}\u{2022} {}", &s[..indent], rest);
    }
    s.trim_end().to_string()
}

/// `![alt](url)` disappears, `[text](url)` becomes `text`.
fn strip_links(s: &str) -> String {
    if !s.contains("](") {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let image = chars[i] == '!' && chars.get(i + 1) == Some(&'[');
        let open = if image { i + 1 } else { i };
        if chars[open] == '[' {
            if let Some(close_text) = (open + 1..chars.len()).find(|&j| chars[j] == ']') {
                if chars.get(close_text + 1) == Some(&'(') {
                    if let Some(close_url) = (close_text + 2..chars.len()).find(|&j| chars[j] == ')') {
                        if !image {
                            out.extend(&chars[open + 1..close_text]);
                        }
                        i = close_url + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Removes matched `marker` pairs: the opener is not followed by a space, the closer not preceded by one
/// and, for the single `*`, neither sits inside a word (so `2*3*4` and `a * b * c` stay as typed).
fn strip_pairs(s: &str, marker: &str) -> String {
    if !s.contains(marker) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let m: Vec<char> = marker.chars().collect();
    let at = |i: usize| chars.get(i..i + m.len()).is_some_and(|w| w == m.as_slice());
    let single = m.len() == 1;
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if at(i) {
            let after = chars.get(i + m.len()).copied();
            let before_ok = i == 0 || !is_word(chars[i - 1]) || !single;
            let opens = before_ok && after.is_some_and(|c| !c.is_whitespace() && !m.contains(&c));
            if opens {
                let closer = (i + m.len() + 1..chars.len()).find(|&j| {
                    at(j)
                        && !chars[j - 1].is_whitespace()
                        && (!single || chars.get(j + 1).is_none_or(|c| !is_word(*c)))
                });
                if let Some(j) = closer {
                    out.extend(&chars[i + m.len()..j]);
                    i = j + m.len();
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The first non-empty line, clipped to [`TITLE_MAX_CHARS`] characters.
pub fn title_of(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() <= TITLE_MAX_CHARS {
        line.to_string()
    } else {
        let mut t: String = line.chars().take(TITLE_MAX_CHARS - 1).collect();
        t.push('\u{2026}');
        t
    }
}

fn clip_chars(text: String, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => text[..cut].to_string(),
        None => text,
    }
}

/// The Sticky Notes theme as one of seven colour names (the page maps them to its own tokens).
pub fn colour_of(theme: &str) -> &'static str {
    match theme.trim().to_ascii_lowercase().as_str() {
        "green" => "green",
        "blue" => "blue",
        "purple" => "purple",
        "pink" => "pink",
        "gray" | "grey" => "gray",
        "charcoal" => "charcoal",
        _ => "yellow",
    }
}

/// .NET ticks (100 ns since 0001-01-01, what Sticky Notes writes) as unix ms. A value that is plainly unix
/// seconds or milliseconds is accepted too; 0 for anything unusable.
pub fn to_unix_ms(v: i64) -> i64 {
    const TICKS_AT_UNIX_EPOCH: i64 = 621_355_968_000_000_000;
    if v <= 0 {
        0
    } else if v >= TICKS_AT_UNIX_EPOCH {
        (v - TICKS_AT_UNIX_EPOCH) / 10_000
    } else if v >= 100_000_000_000 {
        v
    } else {
        v.saturating_mul(1000)
    }
}

// =============================================================================
// Search hits
// =============================================================================

/// The id a search hit carries: opening it launches Sticky Notes instead of the Island Center.
pub fn hit_id(note: &StickyNote) -> String {
    format!("{HIT_PREFIX}{}", note.id)
}

pub fn is_hit_id(id: &str) -> bool {
    id.starts_with(HIT_PREFIX)
}

/// What smart search looks at. Never an error: whenever Sticky Notes is not available (or reading it
/// panicked) the snapshot simply has no notes.
pub fn for_search() -> Arc<StickySnapshot> {
    std::panic::catch_unwind(current)
        .unwrap_or_else(|_| Arc::new(StickySnapshot { availability: Availability::Unavailable, notes: Vec::new(), revision: 0 }))
}

// =============================================================================
// winsqlite3.dll, loaded dynamically
// =============================================================================

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_NULL: c_int = 5;
const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
#[cfg(test)]
const SQLITE_OPEN_READWRITE_CREATE: c_int = 0x0000_0002 | 0x0000_0004;

type OpenV2 = unsafe extern "C" fn(*const c_char, *mut *mut c_void, c_int, *const c_char) -> c_int;
type Close = unsafe extern "C" fn(*mut c_void) -> c_int;
type PrepareV2 = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void, *mut *const c_char) -> c_int;
type Step = unsafe extern "C" fn(*mut c_void) -> c_int;
type Finalize = unsafe extern "C" fn(*mut c_void) -> c_int;
type ColumnText = unsafe extern "C" fn(*mut c_void, c_int) -> *const u8;
type ColumnBytes = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type ColumnInt64 = unsafe extern "C" fn(*mut c_void, c_int) -> i64;
type ColumnType = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type BusyTimeout = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
#[cfg(test)]
type Exec = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_void, *mut c_void, *mut *mut c_char) -> c_int;

/// The handful of entry points used. The library is never unloaded.
struct Api {
    open_v2: OpenV2,
    close: Close,
    prepare_v2: PrepareV2,
    step: Step,
    finalize: Finalize,
    column_text: ColumnText,
    column_bytes: ColumnBytes,
    column_int64: ColumnInt64,
    column_type: ColumnType,
    busy_timeout: BusyTimeout,
    /// Only the tests write (to build a fixture); the app code has no way to.
    #[cfg(test)]
    exec: Exec,
}

/// Looks up one export of `lib`; `name` must end in a NUL.
unsafe fn symbol<T: Copy>(lib: HMODULE, name: &'static str) -> Result<T, String> {
    debug_assert!(name.ends_with('\0'));
    debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
    match GetProcAddress(lib, PCSTR(name.as_ptr())) {
        Some(f) => Ok(std::mem::transmute_copy::<_, T>(&f)),
        None => Err(format!("{} is missing from winsqlite3.dll", name.trim_end_matches('\0'))),
    }
}

fn load_api() -> Result<Api, String> {
    unsafe {
        let lib = LoadLibraryExW(w!("winsqlite3.dll"), HANDLE::default(), LOAD_LIBRARY_SEARCH_SYSTEM32)
            .map_err(|_| "winsqlite3.dll cannot be loaded".to_string())?;
        Ok(Api {
            open_v2: symbol(lib, "sqlite3_open_v2\0")?,
            close: symbol(lib, "sqlite3_close\0")?,
            prepare_v2: symbol(lib, "sqlite3_prepare_v2\0")?,
            step: symbol(lib, "sqlite3_step\0")?,
            finalize: symbol(lib, "sqlite3_finalize\0")?,
            column_text: symbol(lib, "sqlite3_column_text\0")?,
            column_bytes: symbol(lib, "sqlite3_column_bytes\0")?,
            column_int64: symbol(lib, "sqlite3_column_int64\0")?,
            column_type: symbol(lib, "sqlite3_column_type\0")?,
            busy_timeout: symbol(lib, "sqlite3_busy_timeout\0")?,
            #[cfg(test)]
            exec: symbol(lib, "sqlite3_exec\0")?,
        })
    }
}

static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn api() -> Result<&'static Api, String> {
    API.get_or_init(load_api).as_ref().map_err(|e| e.clone())
}

/// One open connection; closed on drop.
struct Db {
    api: &'static Api,
    handle: *mut c_void,
}

impl Drop for Db {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.handle);
        }
    }
}

/// The current row of a running statement.
struct Row<'a> {
    api: &'static Api,
    stmt: *mut c_void,
    _db: std::marker::PhantomData<&'a Db>,
}

impl Row<'_> {
    fn is_null(&self, i: c_int) -> bool {
        unsafe { (self.api.column_type)(self.stmt, i) == SQLITE_NULL }
    }

    fn text(&self, i: c_int) -> Option<String> {
        unsafe {
            if self.is_null(i) {
                return None;
            }
            // column_text first, then column_bytes (the documented order).
            let p = (self.api.column_text)(self.stmt, i);
            if p.is_null() {
                return None;
            }
            let n = (self.api.column_bytes)(self.stmt, i).max(0) as usize;
            Some(String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned())
        }
    }

    fn int(&self, i: c_int) -> i64 {
        unsafe { (self.api.column_int64)(self.stmt, i) }
    }
}

struct Statement {
    api: &'static Api,
    stmt: *mut c_void,
}

impl Drop for Statement {
    fn drop(&mut self) {
        unsafe {
            (self.api.finalize)(self.stmt);
        }
    }
}

impl Db {
    fn open(path: &Path, flags: c_int) -> Result<Db, String> {
        let api = api()?;
        let c_path = CString::new(path.to_str().ok_or("the database path is not valid text")?).map_err(|_| "the database path has a NUL")?;
        let mut handle: *mut c_void = std::ptr::null_mut();
        let rc = unsafe { (api.open_v2)(c_path.as_ptr(), &mut handle, flags, std::ptr::null()) };
        if handle.is_null() {
            return Err(format!("sqlite open failed ({rc})"));
        }
        let db = Db { api, handle };
        if rc != SQLITE_OK {
            return Err(format!("sqlite open failed ({rc})"));
        }
        unsafe {
            (api.busy_timeout)(handle, 250);
        }
        Ok(db)
    }

    /// Runs one SELECT/PRAGMA; `each` returns false to stop early.
    fn query(&self, sql: &str, mut each: impl FnMut(&Row) -> bool) -> Result<(), String> {
        let c_sql = CString::new(sql).map_err(|_| "bad sql")?;
        let mut stmt: *mut c_void = std::ptr::null_mut();
        let rc = unsafe { (self.api.prepare_v2)(self.handle, c_sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) };
        if rc != SQLITE_OK || stmt.is_null() {
            return Err(format!("sqlite prepare failed ({rc})"));
        }
        let statement = Statement { api: self.api, stmt };
        loop {
            match unsafe { (self.api.step)(statement.stmt) } {
                SQLITE_ROW => {
                    if !each(&Row { api: self.api, stmt: statement.stmt, _db: std::marker::PhantomData }) {
                        return Ok(());
                    }
                }
                SQLITE_DONE => return Ok(()),
                rc => return Err(format!("sqlite step failed ({rc})")),
            }
        }
    }

    /// Test fixtures only: runs statements that change the database.
    #[cfg(test)]
    fn exec(&self, sql: &str) -> Result<(), String> {
        let c_sql = CString::new(sql).map_err(|_| "bad sql")?;
        let rc = unsafe { (self.api.exec)(self.handle, c_sql.as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut()) };
        if rc == SQLITE_OK {
            Ok(())
        } else {
            Err(format!("sqlite exec failed ({rc})"))
        }
    }
}

// =============================================================================
// Reading the database
// =============================================================================

#[derive(Debug, PartialEq)]
enum ReadError {
    /// A database, but not the Sticky Notes layout.
    Unsupported,
    /// winsqlite3 missing, copy or open failed.
    Failed(String),
}

impl From<String> for ReadError {
    fn from(e: String) -> Self {
        ReadError::Failed(e)
    }
}

/// The `Note` table of an open database, as notes (newest first). The query only names columns that
/// exist, so an older or newer layout that kept `Id` and `Text` still works.
fn read_notes(db: &Db) -> Result<Vec<StickyNote>, ReadError> {
    let mut columns: Vec<String> = Vec::new();
    db.query("PRAGMA table_info(Note)", |row| {
        if let Some(name) = row.text(1) {
            columns.push(name.to_ascii_lowercase());
        }
        true
    })?;
    let has = |name: &str| columns.iter().any(|c| c == name);
    if !has("text") {
        return Err(ReadError::Unsupported);
    }
    let sql = format!(
        "SELECT {id}, Text, {theme}, {updated}, {created} FROM Note WHERE Text IS NOT NULL{deleted} ORDER BY {order} DESC LIMIT {MAX_ROWS}",
        id = if has("id") { "Id" } else { "CAST(rowid AS TEXT)" },
        theme = if has("theme") { "Theme" } else { "NULL" },
        updated = if has("updatedat") { "UpdatedAt" } else { "0" },
        created = if has("createdat") { "CreatedAt" } else { "0" },
        deleted = if has("deletedat") { " AND (DeletedAt IS NULL OR DeletedAt = 0)" } else { "" },
        order = if has("updatedat") { "UpdatedAt" } else { "rowid" },
    );
    let mut notes: Vec<StickyNote> = Vec::new();
    db.query(&sql, |row| {
        let raw = row.text(1).unwrap_or_default();
        let text = clip_chars(parse_text(&raw), MAX_TEXT_CHARS);
        if text.trim().is_empty() {
            return true;
        }
        let created = to_unix_ms(row.int(4));
        let updated = to_unix_ms(row.int(3)).max(created);
        notes.push(StickyNote {
            id: row.text(0).unwrap_or_default(),
            title: title_of(&text),
            colour: colour_of(&row.text(2).unwrap_or_default()).to_string(),
            text,
            updated_at: updated,
            created_at: created,
        });
        notes.len() < MAX_NOTES * 2
    })?;
    // Ordered by the stored UpdatedAt; the converted value is what the page shows, so sort by it.
    notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| a.id.cmp(&b.id)));
    notes.truncate(MAX_NOTES);
    Ok(notes)
}

static COPY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A temporary folder that is removed (with the copy inside) when dropped.
struct TempCopy {
    dir: PathBuf,
}

impl TempCopy {
    fn create(work_dir: &Path) -> Result<TempCopy, String> {
        remove_stale_copies(work_dir);
        let n = COPY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = work_dir.join(format!("{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create the temporary folder: {}", e.kind()))?;
        Ok(TempCopy { dir })
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Copies left by a crashed instance (older than [`STALE_COPY_AGE`]; a copy in use is younger).
fn remove_stale_copies(work_dir: &Path) {
    remove_copies_older_than(work_dir, STALE_COPY_AGE);
}

fn remove_copies_older_than(work_dir: &Path, limit: Duration) {
    let Ok(entries) = std::fs::read_dir(work_dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn wal_of(db: &Path) -> PathBuf {
    let mut s = db.as_os_str().to_os_string();
    s.push("-wal");
    PathBuf::from(s)
}

/// Copies the database (and its WAL when there is one) into a temporary folder, opens the copy read-only,
/// reads it and removes the copy. The live files are only ever read.
fn read_database(src: &Path, work_dir: &Path) -> Result<Vec<StickyNote>, ReadError> {
    api()?;
    // First with the WAL (the newest notes); when that copy cannot be opened or read, with the main
    // file alone (older notes are better than none).
    let attempt = |with_wal: bool| -> Result<Vec<StickyNote>, ReadError> {
        let copy = TempCopy::create(work_dir)?;
        let target = copy.dir.join("plum.sqlite");
        std::fs::copy(src, &target).map_err(|e| format!("cannot copy the notes file: {}", e.kind()))?;
        if with_wal {
            let wal = wal_of(src);
            if wal.is_file() {
                std::fs::copy(&wal, wal_of(&target)).map_err(|e| format!("cannot copy the notes journal: {}", e.kind()))?;
            }
        }
        let db = Db::open(&target, SQLITE_OPEN_READONLY)?;
        let notes = read_notes(&db);
        // `db` closes first (reverse declaration order), then the folder goes.
        notes
    };
    match attempt(true) {
        Err(ReadError::Failed(_)) if wal_of(src).is_file() => attempt(false),
        other => other,
    }
}

// =============================================================================
// Locating the data, the cache
// =============================================================================

/// Where things are; a value so the tests can point it at a fixture folder.
#[derive(Clone, Debug)]
struct Location {
    /// `...\Packages\Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe`
    package_dir: PathBuf,
    /// The Windows 7-era file, only to tell "unsupported" from "not installed".
    legacy_snt: Option<PathBuf>,
    /// Temporary copies go here.
    work_dir: PathBuf,
}

impl Location {
    fn real() -> Option<Location> {
        let root = paths::root().ok()?;
        let local_app_data = root.parent()?.to_path_buf();
        Some(Location {
            package_dir: local_app_data.join("Packages").join(PACKAGE_FAMILY),
            legacy_snt: local_app_data.parent().map(|p| p.join("Roaming").join("Microsoft").join("Sticky Notes").join("StickyNotes.snt")),
            work_dir: root.join("sticky-tmp"),
        })
    }

    fn database(&self) -> PathBuf {
        self.package_dir.join("LocalState").join("plum.sqlite")
    }
}

/// Size and modification time of the database and its WAL: "did anything change", without reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    db: (u64, Option<SystemTime>),
    wal: Option<(u64, Option<SystemTime>)>,
}

fn stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| (m.len(), m.modified().ok()))
}

fn fingerprint(db: &Path) -> Option<Fingerprint> {
    Some(Fingerprint { db: stamp(db)?, wal: stamp(&wal_of(db)) })
}

/// What can be said without opening the database.
enum Probe {
    Gone(Availability),
    File(Fingerprint),
}

fn probe(loc: &Location) -> Probe {
    if !loc.package_dir.is_dir() {
        let legacy = loc.legacy_snt.as_ref().is_some_and(|p| p.is_file());
        return Probe::Gone(if legacy { Availability::Unsupported } else { Availability::NotInstalled });
    }
    match fingerprint(&loc.database()) {
        Some(fp) => Probe::File(fp),
        None => Probe::Gone(Availability::NoData),
    }
}

struct State {
    fingerprint: Option<Fingerprint>,
    snapshot: Arc<StickySnapshot>,
    /// Do not copy again before this instant (after a failed read of the same file state).
    retry_at: Option<Instant>,
    revision: u64,
    logged: Option<&'static str>,
}

impl State {
    fn new() -> State {
        State {
            fingerprint: None,
            snapshot: Arc::new(StickySnapshot { availability: Availability::Unavailable, notes: Vec::new(), revision: 0 }),
            retry_at: None,
            revision: 0,
            logged: None,
        }
    }

    /// Makes `snapshot` match the files, copying and reading only when they changed.
    fn refresh(&mut self, loc: &Location, now: Instant) -> Arc<StickySnapshot> {
        let outcome = match probe(loc) {
            Probe::Gone(a) => {
                self.fingerprint = None;
                self.retry_at = None;
                Some((a, Vec::new()))
            }
            Probe::File(fp) => {
                let waiting = self.retry_at.is_some_and(|t| now < t);
                // Nothing to do when the files are as last read (and no retry is due), or when a failed
                // re-read of a changed file is still in its short back-off with a good list on screen.
                let skip = if self.fingerprint == Some(fp) {
                    self.retry_at.is_none() || waiting
                } else {
                    waiting && self.snapshot.availability == Availability::Ok
                };
                if skip {
                    None
                } else {
                    match read_database(&loc.database(), &loc.work_dir) {
                        Ok(notes) => {
                            self.fingerprint = Some(fp);
                            self.retry_at = None;
                            Some((Availability::Ok, notes))
                        }
                        Err(ReadError::Unsupported) => {
                            self.fingerprint = Some(fp);
                            self.retry_at = None;
                            Some((Availability::Unsupported, Vec::new()))
                        }
                        Err(ReadError::Failed(why)) => {
                            self.log_once("unavailable", &why);
                            if self.snapshot.availability == Availability::Ok {
                                // Keep showing the last good list; try again soon.
                                self.retry_at = Some(now + RETRY_AFTER_STALE);
                                None
                            } else {
                                self.fingerprint = Some(fp);
                                self.retry_at = Some(now + RETRY_AFTER_FAILURE);
                                Some((Availability::Unavailable, Vec::new()))
                            }
                        }
                    }
                }
            }
        };
        if let Some((availability, notes)) = outcome {
            if self.revision == 0 || self.snapshot.availability != availability || self.snapshot.notes != notes {
                self.revision += 1;
                self.snapshot = Arc::new(StickySnapshot { availability, notes, revision: self.revision });
                if availability != Availability::Unavailable {
                    self.log_once(availability.as_str(), "");
                }
            }
        }
        self.snapshot.clone()
    }

    /// One INFO line per change of state (counts and names only, never note text).
    fn log_once(&mut self, state: &'static str, detail: &str) {
        if self.logged == Some(state) {
            return;
        }
        self.logged = Some(state);
        if detail.is_empty() {
            dlog!("INFO", "sticky", "sticky notes: {}", state);
        } else {
            dlog!("INFO", "sticky", "sticky notes: {} ({})", state, detail);
        }
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::new()));

/// The current notes: cached, re-read when the files changed. Blocking (a file copy at worst).
pub fn current() -> Arc<StickySnapshot> {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    match Location::real() {
        Some(loc) => state.refresh(&loc, Instant::now()),
        None => state.snapshot.clone(),
    }
}

// =============================================================================
// Commands
// =============================================================================

/// The Windows Sticky Notes for the Notes tab: `{availability, notes, revision}`. Never fails for a
/// missing, locked or unreadable Sticky Notes: that is `availability`.
#[tauri::command]
pub async fn sticky_notes_list() -> Result<StickySnapshot, String> {
    rt::run_blocking("sticky_notes_list", || Ok((*current()).clone())).await
}

/// Launches the Sticky Notes app. Only ever from an explicit click; the target is fixed here, the page
/// cannot name another app.
#[tauri::command]
pub async fn sticky_notes_open() -> Result<(), String> {
    rt::run_blocking("sticky_notes_open", open_app).await
}

pub fn open_app() -> Result<(), String> {
    rt::ensure_com_initialized();
    notifications::launch_aumid(AUMID).map_err(|_| "STICKY-201: could not start Sticky Notes".to_string())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- text ----

    #[test]
    fn paragraph_prefixes_are_removed_and_lines_keep_their_order() {
        let raw = "\\id=0f5c8ae4-5d3a-4c11-8e63-1a5a1c3d7c2e Grocery list\r\\id=3a1b2c3d-0000-4000-8000-000000000001 Milk\r\\id=3a1b2c3d-0000-4000-8000-000000000002 Bread";
        assert_eq!(parse_text(raw), "Grocery list\nMilk\nBread");
    }

    #[test]
    fn paragraphs_without_line_breaks_between_markers_still_split() {
        let raw = "\\id=aaaa-1 first\\id=bbbb-2 second\\id=cccc-3 third";
        assert_eq!(parse_text(raw), "first\nsecond\nthird");
    }

    #[test]
    fn hebrew_and_direction_marks_are_untouched() {
        let raw = "\\id=1 \u{05E8}\u{05E9}\u{05D9}\u{05DE}\u{05EA} \u{05E7}\u{05E0}\u{05D9}\u{05D5}\u{05EA}\r\\id=2 \u{200F}\u{05D7}\u{05DC}\u{05D1} 3% and bread\r\\id=3 \u{05E4}\u{05D2}\u{05D9}\u{05E9}\u{05D4} at 14:30";
        assert_eq!(parse_text(raw), "\u{05E8}\u{05E9}\u{05D9}\u{05DE}\u{05EA} \u{05E7}\u{05E0}\u{05D9}\u{05D5}\u{05EA}\n\u{200F}\u{05D7}\u{05DC}\u{05D1} 3% and bread\n\u{05E4}\u{05D2}\u{05D9}\u{05E9}\u{05D4} at 14:30");
    }

    #[test]
    fn text_without_markers_and_blank_edges() {
        assert_eq!(parse_text("plain note"), "plain note");
        assert_eq!(parse_text("\\id=1 \r\\id=2 hello\r\\id=3 \r\\id=4 \r\\id=5 world\r\\id=6 "), "hello\n\nworld");
        assert_eq!(parse_text(""), "");
        assert_eq!(parse_text("\\id=1 "), "");
    }

    #[test]
    fn formatting_markers_are_reduced_to_text() {
        assert_eq!(strip_inline("**Bold** and __underlined__ and ~~gone~~ and *italic*"), "Bold and underlined and gone and italic");
        assert_eq!(strip_inline("**\u{05E9}\u{05DC}\u{05D5}\u{05DD}** \u{05E2}\u{05D5}\u{05DC}\u{05DD}"), "\u{05E9}\u{05DC}\u{05D5}\u{05DD} \u{05E2}\u{05D5}\u{05DC}\u{05DD}");
        assert_eq!(strip_inline("see [the site](https://example.com) now"), "see the site now");
        assert_eq!(strip_inline("before ![photo](ms-appdata:///local/img.png) after"), "before  after");
    }

    #[test]
    fn things_that_only_look_like_markers_stay() {
        for keep in ["a * b * c", "2*3*4", "snake_case_name", "5 ** 2", "price: 3*", "a__b", "~ approx ~", "[not a link]", "a ] ( b )"] {
            assert_eq!(strip_inline(keep), keep, "{keep}");
        }
        // An unmatched opener is kept as typed.
        assert_eq!(strip_inline("**unfinished"), "**unfinished");
    }

    #[test]
    fn star_bullets_become_bullets() {
        assert_eq!(strip_inline("* milk"), "\u{2022} milk");
        assert_eq!(strip_inline("  * nested"), "  \u{2022} nested");
        assert_eq!(strip_inline("- dash stays"), "- dash stays");
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(strip_inline("a\u{0}b\u{7}c\u{1b}d"), "abcd");
        assert_eq!(strip_inline("tab\there"), "tab\there");
    }

    #[test]
    fn title_is_the_first_line_clipped() {
        assert_eq!(title_of("\n  Hello\nWorld"), "Hello");
        assert_eq!(title_of(""), "");
        let long = "x".repeat(200);
        let t = title_of(&long);
        assert_eq!(t.chars().count(), TITLE_MAX_CHARS);
        assert!(t.ends_with('\u{2026}'));
    }

    #[test]
    fn colours_map_and_unknown_is_yellow() {
        assert_eq!(colour_of("Yellow"), "yellow");
        assert_eq!(colour_of("GREEN"), "green");
        assert_eq!(colour_of(" Blue "), "blue");
        assert_eq!(colour_of("Purple"), "purple");
        assert_eq!(colour_of("Pink"), "pink");
        assert_eq!(colour_of("Gray"), "gray");
        assert_eq!(colour_of("grey"), "gray");
        assert_eq!(colour_of("Charcoal"), "charcoal");
        assert_eq!(colour_of(""), "yellow");
        assert_eq!(colour_of("Teal"), "yellow");
    }

    #[test]
    fn times_accept_ticks_milliseconds_and_seconds() {
        // 2026-10-08T00:00:00Z
        let ms = 1_791_417_600_000_i64;
        let ticks = ms * 10_000 + 621_355_968_000_000_000;
        assert_eq!(to_unix_ms(ticks), ms);
        assert_eq!(to_unix_ms(ms), ms);
        assert_eq!(to_unix_ms(ms / 1000), ms);
        assert_eq!(to_unix_ms(0), 0);
        assert_eq!(to_unix_ms(-5), 0);
    }

    #[test]
    fn search_hit_ids_are_recognised() {
        let n = StickyNote { id: "abc-1".into(), text: "t".into(), title: "t".into(), colour: "yellow".into(), updated_at: 1, created_at: 1 };
        assert_eq!(hit_id(&n), "sticky:abc-1");
        assert!(is_hit_id("sticky:abc-1"));
        assert!(!is_hit_id("0123456789abcdef"));
        // The island's own note ids can never start with it (valid ids have no ':').
        assert!(!crate::notes::valid_id("sticky:abc-1"));
    }

    #[test]
    fn snapshot_wire_shape_is_camel_case() {
        let s = StickySnapshot {
            availability: Availability::NoData,
            notes: vec![StickyNote { id: "i".into(), text: "t".into(), title: "t".into(), colour: "blue".into(), updated_at: 5, created_at: 4 }],
            revision: 3,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["availability"], "noData");
        assert_eq!(v["revision"], 3);
        assert_eq!(v["notes"][0]["updatedAt"], 5);
        assert_eq!(v["notes"][0]["colour"], "blue");
        for a in [Availability::Ok, Availability::NoData, Availability::NotInstalled, Availability::Unsupported, Availability::Unavailable] {
            assert_eq!(serde_json::to_value(a).unwrap(), a.as_str());
        }
    }

    // ---- fixture database, built with the very same winsqlite3.dll ----

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new() -> Sandbox {
            let n = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!("companyisland-sticky-test-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("Packages").join(PACKAGE_FAMILY).join("LocalState")).unwrap();
            Sandbox { root }
        }
        fn location(&self) -> Location {
            Location {
                package_dir: self.root.join("Packages").join(PACKAGE_FAMILY),
                legacy_snt: Some(self.root.join("Roaming").join("StickyNotes.snt")),
                work_dir: self.root.join("work"),
            }
        }
        fn database(&self) -> PathBuf {
            self.location().database()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// The Sticky Notes 3.x/4.x `Note` table.
    const CREATE_NOTE: &str = "CREATE TABLE Note (Text varchar, WindowPosition varchar, IsOpen integer, IsAlwaysOnTop integer, \
        CreationNoteIdAnchor varchar, Theme varchar, IsFutureNote integer, RemoteId varchar, ChangeKey varchar, \
        LastServerVersion varchar, RemoteSchemaVersion integer, IsRemoteDataInvalid integer, PendingInsightsScan integer, \
        Type varchar, Id varchar primary key not null, ParentId varchar, CreatedAt integer, DeletedAt integer, UpdatedAt integer);";

    fn ticks(unix_ms: i64) -> i64 {
        unix_ms * 10_000 + 621_355_968_000_000_000
    }

    fn sql_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    fn insert(db: &Db, id: &str, text: &str, theme: &str, updated_ms: i64, deleted: Option<i64>) {
        db.exec(&format!(
            "INSERT INTO Note (Text, Theme, IsOpen, Id, CreatedAt, UpdatedAt, DeletedAt) VALUES ({}, {}, 0, {}, {}, {}, {});",
            sql_quote(text),
            sql_quote(theme),
            sql_quote(id),
            ticks(updated_ms - 1000),
            ticks(updated_ms),
            deleted.map(|d| ticks(d).to_string()).unwrap_or_else(|| "NULL".to_string()),
        ))
        .unwrap();
    }

    fn have_sqlite() -> bool {
        match api() {
            Ok(_) => true,
            Err(e) => {
                eprintln!("skipped: {e}");
                false
            }
        }
    }

    const T0: i64 = 1_790_000_000_000;

    #[test]
    fn reads_notes_from_a_fixture_skips_deleted_and_orders_newest_first() {
        if !have_sqlite() {
            return;
        }
        let sb = Sandbox::new();
        {
            let db = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
            db.exec(CREATE_NOTE).unwrap();
            insert(&db, "id-old", "\\id=a1 Old note\r\\id=a2 second line", "Blue", T0, None);
            insert(&db, "id-new", "\\id=b1 \u{05E4}\u{05EA}\u{05E7} \u{05D7}\u{05D3}\u{05E9}\r\\id=b2 **\u{05D7}\u{05DC}\u{05D1}** \u{05D5}\u{05DC}\u{05D7}\u{05DD}", "Green", T0 + 5_000, None);
            insert(&db, "id-deleted", "\\id=c1 Deleted one", "Pink", T0 + 9_000, Some(T0 + 9_500));
            insert(&db, "id-blank", "\\id=d1 \r\\id=d2 ", "Yellow", T0 + 7_000, None);
            insert(&db, "id-odd-theme", "\\id=e1 Odd theme", "Teal", T0 + 2_000, None);
        }
        let notes = read_database(&sb.database(), &sb.location().work_dir).unwrap();
        let ids: Vec<&str> = notes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["id-new", "id-odd-theme", "id-old"]);
        assert_eq!(notes[0].title, "\u{05E4}\u{05EA}\u{05E7} \u{05D7}\u{05D3}\u{05E9}");
        assert_eq!(notes[0].text, "\u{05E4}\u{05EA}\u{05E7} \u{05D7}\u{05D3}\u{05E9}\n\u{05D7}\u{05DC}\u{05D1} \u{05D5}\u{05DC}\u{05D7}\u{05DD}");
        assert_eq!(notes[0].colour, "green");
        assert_eq!(notes[0].updated_at, T0 + 5_000);
        assert_eq!(notes[0].created_at, T0 + 4_000);
        assert_eq!(notes[1].colour, "yellow");
        assert_eq!(notes[2].text, "Old note\nsecond line");
        assert_eq!(notes[2].colour, "blue");
        // The copy is gone.
        let left: Vec<_> = std::fs::read_dir(sb.location().work_dir).map(|d| d.filter_map(Result::ok).collect()).unwrap_or_default();
        assert!(left.is_empty(), "temporary copy left behind: {left:?}");
    }

    #[test]
    fn the_wal_is_read_and_the_live_files_are_not_touched() {
        if !have_sqlite() {
            return;
        }
        let sb = Sandbox::new();
        // Sticky Notes is "running": a connection stays open on a WAL database whose rows are only in the WAL.
        let live = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
        live.exec("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
        live.exec(CREATE_NOTE).unwrap();
        live.exec("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        insert(&live, "in-wal", "\\id=f1 Only in the journal", "Purple", T0, None);
        let wal = wal_of(&sb.database());
        assert!(std::fs::metadata(&wal).unwrap().len() > 0, "the row must be in the -wal for this test to mean anything");
        let before = (std::fs::read(sb.database()).unwrap(), std::fs::read(&wal).unwrap(), std::fs::metadata(sb.database()).unwrap().modified().unwrap());

        let notes = read_database(&sb.database(), &sb.location().work_dir).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "Only in the journal");
        assert_eq!(notes[0].colour, "purple");

        let after = (std::fs::read(sb.database()).unwrap(), std::fs::read(&wal).unwrap(), std::fs::metadata(sb.database()).unwrap().modified().unwrap());
        assert_eq!(before, after, "reading must not change the live database or its journal");

        // While Sticky Notes still holds the file open, a new note shows up in the cheap "did it change" check.
        let fp_before = fingerprint(&sb.database()).unwrap();
        insert(&live, "second", "\\id=f2 Written later", "Blue", T0 + 1000, None);
        let fp_after = fingerprint(&sb.database()).unwrap();
        assert_ne!(fp_before, fp_after, "an append to the -wal must change the fingerprint without the writer closing the file");
        assert_eq!(read_database(&sb.database(), &sb.location().work_dir).unwrap().len(), 2);
        drop(live);
    }

    #[test]
    fn a_layout_without_the_note_table_is_unsupported_and_garbage_is_unavailable() {
        if !have_sqlite() {
            return;
        }
        let sb = Sandbox::new();
        {
            let db = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
            db.exec("CREATE TABLE Other (a integer);").unwrap();
        }
        assert_eq!(read_database(&sb.database(), &sb.location().work_dir), Err(ReadError::Unsupported));

        let sb = Sandbox::new();
        std::fs::write(sb.database(), vec![0x41u8; 5000]).unwrap();
        assert!(matches!(read_database(&sb.database(), &sb.location().work_dir), Err(ReadError::Failed(_))));
    }

    #[test]
    fn a_note_table_with_fewer_columns_still_reads() {
        if !have_sqlite() {
            return;
        }
        let sb = Sandbox::new();
        {
            let db = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
            db.exec("CREATE TABLE Note (Text varchar); INSERT INTO Note (Text) VALUES ('\\id=1 bare note');").unwrap();
        }
        let notes = read_database(&sb.database(), &sb.location().work_dir).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "bare note");
        assert_eq!(notes[0].colour, "yellow");
        assert_eq!(notes[0].updated_at, 0);

        let sb = Sandbox::new();
        {
            let db = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
            db.exec("CREATE TABLE Note (Id varchar, Theme varchar);").unwrap();
        }
        assert_eq!(read_database(&sb.database(), &sb.location().work_dir), Err(ReadError::Unsupported));
    }

    #[test]
    fn many_notes_are_capped_at_the_newest() {
        if !have_sqlite() {
            return;
        }
        let sb = Sandbox::new();
        {
            let db = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
            db.exec(CREATE_NOTE).unwrap();
            db.exec("BEGIN;").unwrap();
            for i in 0..(MAX_NOTES as i64 + 40) {
                insert(&db, &format!("n{i:04}"), &format!("\\id=x note {i}"), "Yellow", T0 + i * 1000, None);
            }
            db.exec("COMMIT;").unwrap();
        }
        let notes = read_database(&sb.database(), &sb.location().work_dir).unwrap();
        assert_eq!(notes.len(), MAX_NOTES);
        assert_eq!(notes[0].id, format!("n{:04}", MAX_NOTES + 39));
    }

    #[test]
    fn long_text_is_clipped() {
        if !have_sqlite() {
            return;
        }
        let sb = Sandbox::new();
        {
            let db = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
            db.exec(CREATE_NOTE).unwrap();
            insert(&db, "big", &format!("\\id=1 {}", "\u{05D0}".repeat(MAX_TEXT_CHARS + 500)), "Yellow", T0, None);
        }
        let notes = read_database(&sb.database(), &sb.location().work_dir).unwrap();
        assert_eq!(notes[0].text.chars().count(), MAX_TEXT_CHARS);
    }

    // ---- states and the cache ----

    #[test]
    fn availability_follows_what_is_on_disk() {
        let sb = Sandbox::new();
        let loc = sb.location();
        let mut st = State::new();
        // Installed, no database yet.
        let s = st.refresh(&loc, Instant::now());
        assert_eq!((s.availability, s.notes.len()), (Availability::NoData, 0));
        // No package folder at all.
        let mut gone = loc.clone();
        gone.package_dir = sb.root.join("Packages").join("nothing-here");
        assert_eq!(st.refresh(&gone, Instant::now()).availability, Availability::NotInstalled);
        // ... but the Windows 7 file is there: not supported (rather than "not installed").
        std::fs::create_dir_all(sb.root.join("Roaming")).unwrap();
        std::fs::write(sb.root.join("Roaming").join("StickyNotes.snt"), b"x").unwrap();
        assert_eq!(st.refresh(&gone, Instant::now()).availability, Availability::Unsupported);
    }

    #[test]
    fn refresh_reads_once_per_file_change_and_survives_a_bad_moment() {
        if !have_sqlite() {
            return;
        }
        let sb = Sandbox::new();
        let loc = sb.location();
        {
            let db = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
            db.exec(CREATE_NOTE).unwrap();
            insert(&db, "one", "\\id=1 first", "Yellow", T0, None);
        }
        let mut st = State::new();
        let t = Instant::now();
        let a = st.refresh(&loc, t);
        assert_eq!((a.availability, a.notes.len()), (Availability::Ok, 1));
        // Nothing changed: the very same snapshot comes back without another read.
        let b = st.refresh(&loc, t + Duration::from_secs(1));
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.revision, b.revision);
        // A note is added (a different size): re-read.
        {
            let db = Db::open(&sb.database(), SQLITE_OPEN_READWRITE_CREATE).unwrap();
            insert(&db, "two", &format!("\\id=2 second {}", "x".repeat(12_000)), "Blue", T0 + 1000, None);
        }
        let c = st.refresh(&loc, t + Duration::from_secs(2));
        assert_eq!(c.notes.len(), 2);
        assert!(c.revision > a.revision);
        // The database becomes unreadable: the last good list stays on screen.
        std::fs::write(sb.database(), vec![0x41u8; 6000]).unwrap();
        let d = st.refresh(&loc, t + Duration::from_secs(3));
        assert_eq!(d.availability, Availability::Ok);
        assert_eq!(d.notes.len(), 2);
        // The file is removed: noData.
        std::fs::remove_file(sb.database()).unwrap();
        assert_eq!(st.refresh(&loc, t + Duration::from_secs(4)).availability, Availability::NoData);
    }

    #[test]
    fn an_unreadable_database_is_unavailable_and_not_copied_again_at_once() {
        let sb = Sandbox::new();
        let loc = sb.location();
        std::fs::write(sb.database(), vec![0x41u8; 5000]).unwrap();
        let mut st = State::new();
        let t = Instant::now();
        let a = st.refresh(&loc, t);
        if api().is_err() {
            assert_eq!(a.availability, Availability::Unavailable);
            return;
        }
        assert_eq!(a.availability, Availability::Unavailable);
        let b = st.refresh(&loc, t + Duration::from_secs(1));
        assert!(Arc::ptr_eq(&a, &b));
        assert!(st.retry_at.is_some());
    }

    #[test]
    fn stale_copies_of_a_crashed_instance_are_removed() {
        let sb = Sandbox::new();
        let work = sb.location().work_dir;
        let leftover = work.join("99999-0");
        std::fs::create_dir_all(&leftover).unwrap();
        std::fs::write(leftover.join("plum.sqlite"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(30));
        // Younger than the limit: a copy that may be in use is never touched.
        remove_stale_copies(&work);
        assert!(leftover.exists());
        remove_copies_older_than(&work, Duration::from_millis(10));
        assert!(!leftover.exists());
    }

    #[test]
    fn the_fixed_launch_target_is_a_valid_aumid() {
        assert!(notifications::is_valid_aumid(AUMID));
        assert!(AUMID.starts_with(PACKAGE_FAMILY));
    }

    /// Live and read-only: whatever this machine has, the answer is one of the states and never a panic.
    #[test]
    fn live_machine_gives_a_state() {
        let s = current();
        println!("sticky notes on this machine: {} ({} notes)", s.availability.as_str(), s.notes.len());
    }
}
