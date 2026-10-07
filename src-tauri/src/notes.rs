//! Island notes (`state\notes.json`).
//!
//! `{"schemaVersion":1,"notes":[...]}`; a bare array is also accepted on load. This module is
//! the only writer: the island's Notes tab uses the commands below and the Island Center uses
//! the same `save` through the pipe. Every note is sanitized on the way in (see `sanitize`), so
//! whatever a client sends, the file and the answer hold valid notes in canonical order.
//! A file that cannot be parsed, or is larger than 16 MiB, is quarantined (APP-003) and the app
//! continues with no notes. Note text is user content: it is never logged, only counts are.

use crate::{center_ipc, paths, reminder_state, rt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

const FILE_NAME: &str = "notes.json";
const SCHEMA_VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_NOTES: usize = 500;
const MAX_TEXT_CHARS: usize = 10_000;
const MAX_ID_LEN: usize = 64;
const READ_ATTEMPTS: u32 = 4;
const STALE_TMP_AGE: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Serializes load/save so concurrent commands (island, Center) cannot interleave tmp-file writes.
static IO_LOCK: Mutex<()> = Mutex::new(());

/// Missing fields default (and are then sanitized), so a partial note from a client is repaired
/// or dropped instead of failing the whole call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub pinned: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileOut<'a> {
    schema_version: u32,
    notes: &'a [Note],
}

/// The two shapes a notes file may have.
#[derive(Deserialize)]
#[serde(untagged)]
enum FileIn {
    Wrapped { notes: Vec<Note> },
    Bare(Vec<Note>),
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 1..64 characters of `[A-Za-z0-9_-]`. Also the rule for the id inside a `note:<id>` Center page.
pub fn valid_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Pinned first, then newest update first, then id.
fn canonical_cmp(a: &Note, b: &Note) -> std::cmp::Ordering {
    b.pinned
        .cmp(&a.pinned)
        .then_with(|| b.updated_at.cmp(&a.updated_at))
        .then_with(|| a.id.cmp(&b.id))
}

/// Make any list of notes valid: bad ids and blank notes are dropped, text is cut to 10,000
/// characters, timestamps are repaired, duplicate ids keep the newest update, at most 500 notes
/// are kept (the newest by update time) and the result is in canonical order.
pub fn sanitize(notes: Vec<Note>, now: i64) -> Vec<Note> {
    let mut by_id: HashMap<String, Note> = HashMap::new();
    for mut note in notes {
        if !valid_id(&note.id) || note.text.trim().is_empty() {
            continue;
        }
        if let Some((cut, _)) = note.text.char_indices().nth(MAX_TEXT_CHARS) {
            note.text.truncate(cut);
        }
        if note.created_at <= 0 {
            note.created_at = now;
        }
        if note.updated_at <= 0 {
            note.updated_at = now;
        }
        if note.updated_at < note.created_at {
            note.updated_at = note.created_at;
        }
        match by_id.get(&note.id) {
            Some(kept) if kept.updated_at >= note.updated_at => {}
            _ => {
                by_id.insert(note.id.clone(), note);
            }
        }
    }
    let mut list: Vec<Note> = by_id.into_values().collect();
    if list.len() > MAX_NOTES {
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| a.id.cmp(&b.id)));
        list.truncate(MAX_NOTES);
    }
    list.sort_by(canonical_cmp);
    list
}

/// Read the file, retrying the errors that clear up on their own (a scanner or indexer briefly
/// holding it). `Ok(None)` is "no file".
fn read_file(path: &Path) -> std::io::Result<Option<String>> {
    let mut result = Ok(None);
    for attempt in 0..READ_ATTEMPTS {
        result = match fs::read_to_string(path) {
            Ok(c) => Ok(Some(c)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        };
        match &result {
            Err(e) if reminder_state::is_transient(e) => {
                std::thread::sleep(std::time::Duration::from_millis(40 * (attempt as u64 + 1)))
            }
            _ => break,
        }
    }
    result
}

/// Remove `notes.<pid>.tmp` leftovers of a crashed instance (older than 5 minutes, so a write in
/// progress in another session is never touched).
fn remove_stale_tmp(path: &Path) {
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|n| n.to_str())) else { return };
    let prefix = format!("{stem}.");
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.starts_with(&prefix) && name.ends_with(".tmp")) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE_TMP_AGE);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Notes in canonical order. A missing file is an empty list; a corrupt one is quarantined
/// (APP-003) and also empty. A file that exists but cannot be read (permissions, a lock that does
/// not clear) is an error, never an empty list: a client that took it for "no notes" would
/// overwrite the real file on its next save.
fn load_from(path: &Path, now: i64) -> Result<Vec<Note>, String> {
    let too_big = fs::metadata(path).map(|m| m.len() > MAX_FILE_BYTES).unwrap_or(false);
    let content = if too_big {
        None
    } else {
        match read_file(path) {
            Ok(Some(c)) => Some(c),
            Ok(None) => return Ok(Vec::new()),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => None,
            Err(e) => {
                dlog!("WARN", "notes", "APP-002 notes file unreadable ({}); not treating it as empty", e.kind());
                return Err("APP-002: cannot read notes".to_string());
            }
        }
    };
    match content.and_then(|c| serde_json::from_str::<FileIn>(&c).ok()) {
        Some(FileIn::Wrapped { notes } | FileIn::Bare(notes)) => {
            let loaded = notes.len();
            let notes = sanitize(notes, now);
            if notes.len() != loaded {
                dlog!("INFO", "notes", "repaired notes on load: {} -> {}", loaded, notes.len());
            }
            Ok(notes)
        }
        None => {
            reminder_state::quarantine(path);
            dlog!("WARN", "notes", "APP-003 notes file corrupt; quarantined, starting empty");
            Ok(Vec::new())
        }
    }
}

fn save_to(path: &Path, notes: Vec<Note>, now: i64) -> Result<Vec<Note>, String> {
    let notes = sanitize(notes, now);
    let bytes = serde_json::to_vec(&FileOut { schema_version: SCHEMA_VERSION, notes: &notes })
        .map_err(|_| "APP-002: cannot write notes".to_string())?;
    // A file the next load would refuse (and quarantine) must never be written.
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("APP-002: cannot write notes (too large)".to_string());
    }
    // Saving replaces the whole list. If the file that is there cannot be read, writing over it
    // would destroy notes nobody has seen, so refuse (the client keeps its draft and can retry).
    if let Err(e) = read_file(path) {
        if e.kind() != std::io::ErrorKind::InvalidData {
            dlog!("WARN", "notes", "APP-002 existing notes unreadable ({}); not overwriting", e.kind());
            return Err("APP-002: cannot read existing notes".to_string());
        }
    }
    remove_stale_tmp(path);
    reminder_state::write_atomic(path, &bytes).map_err(|e| {
        dlog!("WARN", "notes", "APP-002 notes write failed ({})", e.kind());
        "APP-002: cannot write notes".to_string()
    })?;
    Ok(notes)
}

fn file() -> Result<PathBuf, String> {
    paths::state_dir().map(|d| d.join(FILE_NAME))
}

/// All notes in canonical order. Missing or corrupt (quarantined) files give an empty list; a
/// file that exists but cannot be read is an `APP-002` error so the client never saves over it.
pub fn load() -> Result<Vec<Note>, String> {
    let path = file().map_err(|_| "APP-002: cannot read notes".to_string())?;
    let _guard = IO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    load_from(&path, now_ms())
}

/// Replace all notes with the sanitized `notes`, then tell every window and every pipe client
/// (`notes-changed`). Used by the island's command and by the Center's `notesSave`.
pub fn save(app: &AppHandle, notes: Vec<Note>) -> Result<Vec<Note>, String> {
    let saved = {
        let path = file().map_err(|_| "APP-002: cannot write notes".to_string())?;
        let _guard = IO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        save_to(&path, notes, now_ms())?
    };
    dlog!("INFO", "notes", "saved {} notes", saved.len());
    if let Err(e) = app.emit("notes-changed", &saved) {
        dlog!("WARN", "notes", "emit notes-changed failed: {}", e);
    }
    center_ipc::broadcast("notes-changed", &saved);
    Ok(saved)
}

#[tauri::command]
pub async fn notes_load() -> Result<Vec<Note>, String> {
    rt::run_blocking("notes_load", load).await
}

#[tauri::command]
pub async fn notes_save(app: AppHandle, notes: Vec<Note>) -> Result<Vec<Note>, String> {
    rt::run_blocking("notes_save", move || save(&app, notes)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn note(id: &str, text: &str, created: i64, updated: i64, pinned: bool) -> Note {
        Note { id: id.into(), text: text.into(), created_at: created, updated_at: updated, pinned }
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("companyisland-test-notes-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn id_rules() {
        assert!(valid_id("0123456789abcdef"));
        assert!(valid_id("A_b-9"));
        assert!(valid_id(&"a".repeat(64)));
        assert!(!valid_id(""));
        assert!(!valid_id(&"a".repeat(65)));
        assert!(!valid_id("has space"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id("שלום"));
        assert!(!valid_id("a:b"));
    }

    #[test]
    fn invalid_ids_and_blank_notes_are_dropped_text_is_kept_as_typed() {
        let out = sanitize(
            vec![
                note("ok1", "  keep my spaces \n", 10, 20, false),
                note("bad id", "x", 10, 20, false),
                note("", "x", 10, 20, false),
                note("blank", " \n\t ", 10, 20, false),
                note("empty", "", 10, 20, false),
            ],
            NOW,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "  keep my spaces \n");
    }

    #[test]
    fn long_text_is_cut_on_a_char_boundary() {
        let text: String = "ש".repeat(MAX_TEXT_CHARS + 50);
        let out = sanitize(vec![note("a", &text, 1, 1, false)], NOW);
        assert_eq!(out[0].text.chars().count(), MAX_TEXT_CHARS);
        let exact: String = "😀".repeat(MAX_TEXT_CHARS);
        let out = sanitize(vec![note("a", &exact, 1, 1, false)], NOW);
        assert_eq!(out[0].text, exact);
    }

    #[test]
    fn timestamps_are_repaired() {
        let out = sanitize(
            vec![note("a", "x", 0, 0, false), note("b", "x", 500, 100, false), note("c", "x", -5, 700, false)],
            NOW,
        );
        let get = |id: &str| out.iter().find(|n| n.id == id).unwrap().clone();
        assert_eq!((get("a").created_at, get("a").updated_at), (NOW, NOW));
        assert_eq!((get("b").created_at, get("b").updated_at), (500, 500));
        assert_eq!((get("c").created_at, get("c").updated_at), (NOW, NOW));
    }

    #[test]
    fn duplicate_ids_keep_the_newest_update() {
        let out = sanitize(
            vec![note("a", "old", 1, 10, false), note("a", "new", 1, 30, true), note("a", "mid", 1, 20, false)],
            NOW,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "new");
    }

    #[test]
    fn canonical_order_is_pinned_then_newest_then_id() {
        let out = sanitize(
            vec![
                note("c", "x", 1, 50, false),
                note("b", "x", 1, 50, false),
                note("p1", "x", 1, 10, true),
                note("p2", "x", 1, 90, true),
                note("d", "x", 1, 70, false),
            ],
            NOW,
        );
        let ids: Vec<&str> = out.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["p2", "p1", "d", "b", "c"]);
    }

    #[test]
    fn at_most_500_notes_keeping_the_newest() {
        let many: Vec<Note> = (0..MAX_NOTES + 20).map(|i| note(&format!("n{i}"), "x", 1, 1 + i as i64, false)).collect();
        let out = sanitize(many, NOW);
        assert_eq!(out.len(), MAX_NOTES);
        assert!(out.iter().any(|n| n.id == format!("n{}", MAX_NOTES + 19)));
        assert!(!out.iter().any(|n| n.id == "n0"));
        // newest-by-update decides who stays, even when an old note is pinned
        let mut mixed: Vec<Note> = (0..MAX_NOTES).map(|i| note(&format!("n{i}"), "x", 1, 100 + i as i64, false)).collect();
        mixed.push(note("oldpinned", "x", 1, 1, true));
        let out = sanitize(mixed, NOW);
        assert_eq!(out.len(), MAX_NOTES);
        assert!(!out.iter().any(|n| n.id == "oldpinned"));
    }

    #[test]
    fn wire_names_are_camel_case_and_missing_fields_default() {
        let json = serde_json::to_value(note("a", "x", 1, 2, true)).unwrap();
        assert_eq!(json["createdAt"], 1);
        assert_eq!(json["updatedAt"], 2);
        assert_eq!(json["pinned"], true);
        let partial: Note = serde_json::from_str(r#"{"id":"a","text":"hi"}"#).unwrap();
        assert_eq!((partial.created_at, partial.updated_at, partial.pinned), (0, 0, false));
    }

    #[test]
    fn save_then_load_round_trips_a_versioned_file() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join(FILE_NAME);
        let saved = save_to(&path, vec![note("a", "one", 1, 5, false), note("b", "two", 1, 9, true)], NOW).unwrap();
        assert_eq!(saved.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        let raw: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["schemaVersion"], 1);
        assert_eq!(raw["notes"].as_array().unwrap().len(), 2);
        assert_eq!(load_from(&path, NOW).unwrap(), saved);
        let leftovers = fs::read_dir(&dir).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bare_array_loads_and_is_repaired() {
        let dir = scratch_dir("bare");
        let path = dir.join(FILE_NAME);
        fs::write(&path, r#"[{"id":"a","text":"x","createdAt":5,"updatedAt":9,"pinned":false},{"id":"bad id","text":"y"}]"#).unwrap();
        let loaded = load_from(&path, NOW).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "a");
        assert!(!path.with_extension("json.corrupt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_empty_and_not_quarantined() {
        let dir = scratch_dir("missing");
        let path = dir.join(FILE_NAME);
        assert!(load_from(&path, NOW).unwrap().is_empty());
        assert!(!path.with_extension("json.corrupt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_and_wrong_shape_files_are_quarantined() {
        for (name, content) in [("junk", "{ not json"), ("shape", r#"{"notes": "nope"}"#), ("scalar", "42")] {
            let dir = scratch_dir(name);
            let path = dir.join(FILE_NAME);
            fs::write(&path, content).unwrap();
            assert!(load_from(&path, NOW).unwrap().is_empty(), "{name}");
            assert!(!path.exists(), "{name}");
            assert_eq!(fs::read_to_string(path.with_extension("json.corrupt")).unwrap(), content);
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn oversized_file_is_quarantined() {
        let dir = scratch_dir("big");
        let path = dir.join(FILE_NAME);
        fs::write(&path, " ".repeat(MAX_FILE_BYTES as usize + 1)).unwrap();
        assert!(load_from(&path, NOW).unwrap().is_empty());
        assert!(path.with_extension("json.corrupt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_failure_is_app_002_without_content() {
        let dir = scratch_dir("nowrite");
        // The parent "folder" is a file: the temp file cannot be created.
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"x").unwrap();
        let err = save_to(&blocker.join(FILE_NAME), vec![note("a", "secret text", 1, 1, false)], NOW).unwrap_err();
        assert_eq!(err, "APP-002: cannot write notes");
        assert!(!err.contains("secret"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_file_is_an_error_not_an_empty_list_and_is_never_overwritten() {
        let dir = scratch_dir("unreadable");
        let path = dir.join(FILE_NAME);
        // A directory named notes.json: it exists, but reading it fails (not NotFound, not InvalidData).
        fs::create_dir_all(&path).unwrap();
        assert_eq!(load_from(&path, NOW).unwrap_err(), "APP-002: cannot read notes");
        assert!(path.is_dir(), "nothing quarantined or moved");
        assert!(save_to(&path, vec![note("a", "new", 1, 1, false)], NOW).is_err());
        assert!(path.is_dir(), "save did not replace it");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_held_without_sharing_fails_load_and_save_and_the_real_file_survives() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = scratch_dir("held");
        let path = dir.join(FILE_NAME);
        save_to(&path, vec![note("old", "keep me", 1, 1, false)], NOW).unwrap();
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        assert!(load_from(&path, NOW).is_err());
        let err = save_to(&path, vec![note("new", "x", 1, 1, false)], NOW).unwrap_err();
        assert!(err.starts_with("APP-002"));
        drop(held);
        assert_eq!(load_from(&path, NOW).unwrap()[0].id, "old", "the real file survived");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_transient_rename_failure_is_retried_until_the_holder_lets_go() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = scratch_dir("retry");
        let path = dir.join(FILE_NAME);
        fs::write(&path, b"{}").unwrap();
        // Shared for read/write but not delete: renaming onto the file fails with a sharing error.
        let held = fs::OpenOptions::new().write(true).share_mode(1 | 2).open(&path).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(held);
        });
        reminder_state::write_atomic(&path, b"{\"schemaVersion\":1,\"notes\":[]}").expect("retry should outlast the hold");
        releaser.join().unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("schemaVersion"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_tmp_files_are_removed_but_fresh_ones_and_other_files_stay() {
        let dir = scratch_dir("staletmp");
        let path = dir.join(FILE_NAME);
        let stale = dir.join("notes.json.111.tmp");
        let fresh = dir.join("notes.json.222.tmp");
        let other = dir.join("reminders.json.333.tmp");
        for f in [&stale, &fresh, &other] {
            fs::write(f, b"x").unwrap();
        }
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options().write(true).open(&stale).unwrap().set_modified(old).unwrap();
        remove_stale_tmp(&path);
        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(other.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
