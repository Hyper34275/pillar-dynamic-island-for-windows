//! Persisted "reminder already fired" set (`state\reminders.json`).
//!
//! Holds only `"<eventHash>|<startUtcIso>|<reminderType>" -> firedAtUnixMs`, never any
//! event content. Entries older than 7 days are dropped on load and on save. The file
//! is per user by construction (it lives under `%LOCALAPPDATA%`). A file that cannot
//! be parsed is quarantined (APP-003) and the app continues with an empty set.

use crate::paths;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const FILE_NAME: &str = "reminders.json";
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_ENTRIES: usize = 2_000;
const MAX_KEY_LEN: usize = 128;
const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Serializes load/save so concurrent commands cannot interleave tmp-file writes.
static IO_LOCK: Mutex<()> = Mutex::new(());

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `<hex>|<iso8601>|<type>`: 8-64 hex digits, an RFC 3339 timestamp, a short token.
fn valid_key(key: &str) -> bool {
    if key.len() > MAX_KEY_LEN {
        return false;
    }
    let mut parts = key.split('|');
    let (Some(hash), Some(start), Some(kind), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    (8..=64).contains(&hash.len())
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && chrono::DateTime::parse_from_rfc3339(start).is_ok()
        && (1..=24).contains(&kind.len())
        && kind.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Drop malformed keys, non-positive or expired timestamps; keep the newest
/// `MAX_ENTRIES`. Returns how many entries were removed.
fn sanitize(map: &mut HashMap<String, i64>, now: i64) -> usize {
    let before = map.len();
    map.retain(|k, fired_at| valid_key(k) && *fired_at > 0 && *fired_at >= now - RETENTION_MS);
    if map.len() > MAX_ENTRIES {
        let mut by_age: Vec<(String, i64)> = map.drain().collect();
        by_age.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        by_age.truncate(MAX_ENTRIES);
        map.extend(by_age);
    }
    before - map.len()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    // The same user can run one instance per Windows session against this one file.
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    let result = fs::rename(&tmp, path);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn quarantine(path: &Path) {
    let backup = path.with_extension("json.corrupt");
    let _ = fs::remove_file(&backup);
    let _ = fs::rename(path, &backup);
}

fn load_from(path: &Path, now: i64) -> HashMap<String, i64> {
    let too_big = fs::metadata(path).map(|m| m.len() > MAX_FILE_BYTES).unwrap_or(false);
    let content = if too_big {
        None
    } else {
        match fs::read_to_string(path) {
            Ok(c) => Some(c),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => None,
            Err(e) => {
                dlog!("WARN", "reminders", "reminder state unreadable ({}); starting empty", e);
                return HashMap::new();
            }
        }
    };
    match content.and_then(|c| serde_json::from_str::<HashMap<String, i64>>(&c).ok()) {
        Some(mut map) => {
            let dropped = sanitize(&mut map, now);
            if dropped > 0 {
                dlog!("INFO", "reminders", "pruned {} reminder entries on load", dropped);
            }
            map
        }
        None => {
            quarantine(path);
            dlog!("WARN", "reminders", "APP-003 reminder state corrupt; quarantined, starting empty");
            HashMap::new()
        }
    }
}

fn save_to(path: &Path, mut map: HashMap<String, i64>, now: i64) -> std::io::Result<HashMap<String, i64>> {
    let dropped = sanitize(&mut map, now);
    if dropped > 0 {
        dlog!("INFO", "reminders", "dropped {} invalid or expired reminder entries on save", dropped);
    }
    // Sorted keys keep the file stable between saves.
    let ordered: std::collections::BTreeMap<&String, &i64> = map.iter().collect();
    let bytes = serde_json::to_vec(&ordered).map_err(std::io::Error::other)?;
    write_atomic(path, &bytes)?;
    Ok(map)
}

fn state_file() -> Result<PathBuf, String> {
    paths::state_dir().map(|d| d.join(FILE_NAME))
}

#[tauri::command]
pub async fn reminder_state_load() -> Result<HashMap<String, i64>, String> {
    crate::rt::run_blocking("reminder_state_load", || {
        let Ok(path) = state_file() else { return Ok(HashMap::new()) };
        let _guard = IO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Ok(load_from(&path, now_ms()))
    })
    .await
}

#[tauri::command]
pub async fn reminder_state_save(map: HashMap<String, i64>) -> Result<(), String> {
    crate::rt::run_blocking("reminder_state_save", move || {
        let path = state_file().map_err(|e| format!("APP-002: reminder state unavailable: {e}"))?;
        let _guard = IO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        save_to(&path, map, now_ms())
            .map(|_| ())
            .map_err(|e| format!("APP-002: cannot write reminder state: {e}"))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const KEY: &str = "0123456789abcdef|2026-10-06T09:00:00Z|30";

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("companyisland-test-rem-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn key_validation() {
        assert!(valid_key(KEY));
        assert!(valid_key("0123abcd|2026-10-06T09:00:00+00:00|5"));
        assert!(!valid_key("nothex!!|2026-10-06T09:00:00Z|30"));
        assert!(!valid_key("0123abcd|yesterday|30"));
        assert!(!valid_key("0123abcd|2026-10-06T09:00:00Z|"));
        assert!(!valid_key("0123abcd|2026-10-06T09:00:00Z|30|extra"));
        assert!(!valid_key("0123abcd|2026-10-06T09:00:00Z"));
        assert!(!valid_key("abc|2026-10-06T09:00:00Z|30"));
        assert!(!valid_key(&format!("{}|2026-10-06T09:00:00Z|30", "a".repeat(120))));
        assert!(!valid_key("0123abcd|2026-10-06T09:00:00Z|bad type"));
    }

    #[test]
    fn sanitize_prunes_old_invalid_and_non_positive() {
        let mut m = HashMap::new();
        m.insert(KEY.to_string(), NOW - 1_000);
        m.insert("aaaaaaaa|2026-10-01T09:00:00Z|30".into(), NOW - RETENTION_MS - 1);
        m.insert("bbbbbbbb|2026-10-02T09:00:00Z|30".into(), NOW - RETENTION_MS);
        m.insert("garbage".into(), NOW);
        m.insert("cccccccc|2026-10-03T09:00:00Z|30".into(), 0);
        assert_eq!(sanitize(&mut m, NOW), 3);
        assert_eq!(m.len(), 2);
        assert!(m.contains_key(KEY));
    }

    #[test]
    fn sanitize_caps_entries_keeping_the_newest() {
        let mut m = HashMap::new();
        for i in 0..(MAX_ENTRIES + 10) {
            m.insert(format!("{:08x}|2026-10-06T09:00:00Z|30", i), NOW - 10_000 + i as i64);
        }
        sanitize(&mut m, NOW);
        assert_eq!(m.len(), MAX_ENTRIES);
        assert!(m.contains_key(&format!("{:08x}|2026-10-06T09:00:00Z|30", MAX_ENTRIES + 9)));
        assert!(!m.contains_key(&format!("{:08x}|2026-10-06T09:00:00Z|30", 0)));
    }

    #[test]
    fn save_then_load_round_trips_without_temp_files() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join(FILE_NAME);
        let mut m = HashMap::new();
        m.insert(KEY.to_string(), NOW - 5);
        save_to(&path, m.clone(), NOW).unwrap();
        assert_eq!(load_from(&path, NOW), m);
        let leftovers = fs::read_dir(&dir).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_empty_and_not_quarantined() {
        let dir = scratch_dir("missing");
        let path = dir.join(FILE_NAME);
        assert!(load_from(&path, NOW).is_empty());
        assert!(!path.with_extension("json.corrupt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_quarantined_and_empty() {
        let dir = scratch_dir("corrupt");
        let path = dir.join(FILE_NAME);
        fs::write(&path, "{ not json").unwrap();
        assert!(load_from(&path, NOW).is_empty());
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(path.with_extension("json.corrupt")).unwrap(), "{ not json");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_shape_is_corrupt() {
        let dir = scratch_dir("shape");
        let path = dir.join(FILE_NAME);
        fs::write(&path, r#"["a","b"]"#).unwrap();
        assert!(load_from(&path, NOW).is_empty());
        assert!(path.with_extension("json.corrupt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_file_is_quarantined() {
        let dir = scratch_dir("big");
        let path = dir.join(FILE_NAME);
        fs::write(&path, " ".repeat(MAX_FILE_BYTES as usize + 1)).unwrap();
        assert!(load_from(&path, NOW).is_empty());
        assert!(path.with_extension("json.corrupt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_prunes_expired_entries() {
        let dir = scratch_dir("prune");
        let path = dir.join(FILE_NAME);
        let json = format!(r#"{{"{KEY}": {}, "aaaaaaaa|2026-09-01T09:00:00Z|30": {}}}"#, NOW - 1, NOW - RETENTION_MS - 5);
        fs::write(&path, json).unwrap();
        let loaded = load_from(&path, NOW);
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains_key(KEY));
        let _ = fs::remove_dir_all(&dir);
    }
}
