//! User settings: one typed, validated, schema-versioned store.
//!
//! `settings.json` lives in the per-user data folder (see `paths`). Writes go to a
//! temp file that is renamed over the target, so a crash can never leave a
//! truncated file. A file that cannot be parsed is quarantined as
//! `settings.json.corrupt` (APP-003) and defaults are used. If the data folder is
//! unavailable the store works in memory only.

use crate::{autostart, center_ipc, debug_log, fullscreen, notifications, paths, rt, search_bar, window};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

pub const SCHEMA_VERSION: u32 = 1;
const DEFAULT_REMINDER_MINUTES: u32 = 30;
const MAX_REMINDER_MINUTES: u32 = 120;
const MAX_MONITOR_INDEX: u32 = 15;
const PRIMARY_MONITOR: &str = "primary";
const RENAME_ATTEMPTS: u32 = 5;
const DEFAULT_ISLAND_DISPLAY: &str = "full";
const ISLAND_DISPLAYS: [&str; 3] = ["full", "clock", "date"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    pub launch_with_windows: bool,
    pub hide_in_fullscreen: bool,
    pub meeting_reminder_enabled: bool,
    #[serde(deserialize_with = "de_minutes")]
    pub reminder_minutes: u32,
    /// `"primary"` or a zero-based monitor index as a string. On the wire this is
    /// `monitorId` (`null` = primary, the id `get_monitors` returns otherwise);
    /// the legacy name `monitor` is still accepted.
    #[serde(
        rename = "monitorId",
        alias = "monitor",
        serialize_with = "ser_monitor",
        deserialize_with = "de_monitor"
    )]
    pub monitor: String,
    pub notifications_enabled: bool,
    /// When a meeting starts the island offers to silence notifications until it ends.
    pub meeting_silence_prompt: bool,
    /// New Outlook meeting requests pop up in the island. Off means the Inbox is not read at all.
    pub meeting_invites_enabled: bool,
    /// Meeting reminders also for events of checked calendars other than the user's own
    /// (shared, other). Off keeps showing their events, without reminders.
    pub shared_calendar_reminders: bool,
    /// Opt-in debug-level logging (`COMPANYISLAND_LOG=debug` does the same).
    pub debug_logging: bool,
    /// The Welcome page of the Island Center was shown once (first run, or the first start after
    /// an upgrade from a version without it).
    pub onboarding_done: bool,
    /// What the collapsed island shows: `"full"` (date, time, weekday), `"clock"` or `"date"`.
    #[serde(deserialize_with = "de_island_display")]
    pub island_display: String,
    /// Smart search (AI Mode): questions typed in the search bar overlay are answered in the island.
    /// Off turns the whole feature off (no button, no hotkey, no overlay).
    pub ai_search_enabled: bool,
    /// The small AI button on the Windows 10 taskbar search box.
    pub ai_search_button: bool,
    /// Ctrl+Alt+Space opens the smart search input (also where there is no search box).
    pub ai_search_hotkey: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            launch_with_windows: true,
            hide_in_fullscreen: true,
            meeting_reminder_enabled: true,
            reminder_minutes: DEFAULT_REMINDER_MINUTES,
            monitor: PRIMARY_MONITOR.to_string(),
            notifications_enabled: true,
            meeting_invites_enabled: true,
            shared_calendar_reminders: true,
            meeting_silence_prompt: true,
            debug_logging: false,
            onboarding_done: false,
            island_display: DEFAULT_ISLAND_DISPLAY.to_string(),
            ai_search_enabled: true,
            ai_search_button: true,
            ai_search_hotkey: true,
        }
    }
}

/// Partial update from the UI; absent fields are left unchanged.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    launch_with_windows: Option<bool>,
    hide_in_fullscreen: Option<bool>,
    meeting_reminder_enabled: Option<bool>,
    #[serde(default, deserialize_with = "de_minutes_opt")]
    reminder_minutes: Option<u32>,
    /// Present-but-null selects the primary monitor.
    #[serde(default, rename = "monitorId", alias = "monitor", deserialize_with = "de_monitor_patch")]
    monitor: Option<String>,
    notifications_enabled: Option<bool>,
    meeting_invites_enabled: Option<bool>,
    shared_calendar_reminders: Option<bool>,
    meeting_silence_prompt: Option<bool>,
    debug_logging: Option<bool>,
    onboarding_done: Option<bool>,
    island_display: Option<String>,
    ai_search_enabled: Option<bool>,
    ai_search_button: Option<bool>,
    ai_search_hotkey: Option<bool>,
}

fn clamp_minutes(value: f64) -> u32 {
    if value.is_nan() {
        DEFAULT_REMINDER_MINUTES
    } else {
        value.round().clamp(0.0, MAX_REMINDER_MINUTES as f64) as u32
    }
}

fn de_minutes<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    f64::deserialize(d).map(clamp_minutes)
}

fn de_minutes_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    Option::<f64>::deserialize(d).map(|v| v.map(clamp_minutes))
}

fn normalize_monitor(value: &str) -> String {
    match value.trim().parse::<u32>() {
        Ok(index) if index <= MAX_MONITOR_INDEX => index.to_string(),
        _ => PRIMARY_MONITOR.to_string(),
    }
}

/// A monitor id as the UI or an older file may spell it. Anything unusable (a bad
/// type must never make the whole settings file unreadable) means "primary".
#[derive(Deserialize)]
#[serde(untagged)]
enum MonitorValue {
    Text(String),
    Number(u32),
    Other(serde::de::IgnoredAny),
}

fn monitor_from(value: Option<MonitorValue>) -> String {
    match value {
        Some(MonitorValue::Text(s)) => normalize_monitor(&s),
        Some(MonitorValue::Number(n)) => normalize_monitor(&n.to_string()),
        _ => PRIMARY_MONITOR.to_string(),
    }
}

fn de_monitor<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Option::<MonitorValue>::deserialize(d).map(monitor_from)
}

fn de_monitor_patch<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<MonitorValue>::deserialize(d).map(|v| Some(monitor_from(v)))
}

/// An unknown value (or a wrong type, which must not make the file unreadable) means "full".
fn normalize_island_display(value: &str) -> String {
    if ISLAND_DISPLAYS.contains(&value) {
        value.to_string()
    } else {
        DEFAULT_ISLAND_DISPLAY.to_string()
    }
}

fn de_island_display<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Text(String),
        Other(serde::de::IgnoredAny),
    }
    Option::<Value>::deserialize(d).map(|v| match v {
        Some(Value::Text(s)) => normalize_island_display(&s),
        _ => DEFAULT_ISLAND_DISPLAY.to_string(),
    })
}

fn ser_monitor<S: Serializer>(value: &str, s: S) -> Result<S::Ok, S::Error> {
    if value == PRIMARY_MONITOR {
        s.serialize_none()
    } else {
        s.serialize_str(value)
    }
}

impl Settings {
    fn sanitized(mut self) -> Self {
        self.schema_version = SCHEMA_VERSION;
        self.reminder_minutes = self.reminder_minutes.min(MAX_REMINDER_MINUTES);
        self.monitor = normalize_monitor(&self.monitor);
        self.island_display = normalize_island_display(&self.island_display);
        self
    }

    fn patched(&self, patch: SettingsPatch) -> Self {
        let mut next = self.clone();
        if let Some(v) = patch.launch_with_windows {
            next.launch_with_windows = v;
        }
        if let Some(v) = patch.hide_in_fullscreen {
            next.hide_in_fullscreen = v;
        }
        if let Some(v) = patch.meeting_reminder_enabled {
            next.meeting_reminder_enabled = v;
        }
        if let Some(v) = patch.reminder_minutes {
            next.reminder_minutes = v;
        }
        if let Some(v) = patch.monitor {
            next.monitor = v;
        }
        if let Some(v) = patch.notifications_enabled {
            next.notifications_enabled = v;
        }
        if let Some(v) = patch.meeting_silence_prompt {
            next.meeting_silence_prompt = v;
        }
        if let Some(v) = patch.meeting_invites_enabled {
            next.meeting_invites_enabled = v;
        }
        if let Some(v) = patch.shared_calendar_reminders {
            next.shared_calendar_reminders = v;
        }
        if let Some(v) = patch.debug_logging {
            next.debug_logging = v;
        }
        if let Some(v) = patch.onboarding_done {
            next.onboarding_done = v;
        }
        if let Some(v) = patch.island_display {
            next.island_display = v;
        }
        if let Some(v) = patch.ai_search_enabled {
            next.ai_search_enabled = v;
        }
        if let Some(v) = patch.ai_search_button {
            next.ai_search_button = v;
        }
        if let Some(v) = patch.ai_search_hotkey {
            next.ai_search_hotkey = v;
        }
        next.sanitized()
    }
}

/// Windows errors that clear up on their own: access denied / sharing / lock
/// violation (antivirus or a backup agent briefly holding the file).
fn is_transient(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    let mut result = Ok(());
    for attempt in 0..RENAME_ATTEMPTS {
        result = fs::rename(&tmp, path);
        match &result {
            Err(e) if is_transient(e) => std::thread::sleep(Duration::from_millis(40 * (attempt as u64 + 1))),
            _ => break,
        }
    }
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

fn load_from(path: &Path) -> Settings {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(e) => {
            dlog!("WARN", "settings", "APP-003 settings unreadable ({}); using defaults", e);
            return Settings::default();
        }
    };
    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => settings.sanitized(),
        Err(e) => {
            quarantine(path);
            dlog!("WARN", "settings", "APP-003 settings corrupt ({}); quarantined, using defaults", e);
            Settings::default()
        }
    }
}

pub struct SettingsStore {
    path: Option<PathBuf>,
    inner: Mutex<Settings>,
}

struct Update {
    old: Settings,
    new: Settings,
    persist_error: Option<String>,
}

impl SettingsStore {
    pub fn load() -> Self {
        let (path, settings) = match paths::settings_file() {
            Ok(path) => {
                let settings = load_from(&path);
                (Some(path), settings)
            }
            Err(e) => {
                dlog!("WARN", "settings", "APP-002 data folder unavailable ({}); settings are in-memory only", e);
                (None, Settings::default())
            }
        };
        Self { path, inner: Mutex::new(settings) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Settings> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self) -> Settings {
        self.lock().clone()
    }

    fn update(&self, patch: SettingsPatch) -> Update {
        // The lock is held across the write so concurrent updates cannot reorder.
        let mut current = self.lock();
        let old = current.clone();
        let new = old.patched(patch);
        if new == old {
            return Update { old, new, persist_error: None };
        }
        *current = new.clone();
        let persist_error = match &self.path {
            None => Some("data folder unavailable".to_string()),
            Some(path) => serde_json::to_vec_pretty(&new)
                .map_err(|e| e.to_string())
                .and_then(|bytes| write_atomic(path, &bytes).map_err(|e| e.to_string()))
                .err(),
        };
        Update { old, new, persist_error }
    }
}

/// Apply a patch: validate, persist, run side effects, notify the UI.
pub fn apply_patch(app: &AppHandle, patch: SettingsPatch) -> Settings {
    let store = app.state::<SettingsStore>();
    let Update { old, new, persist_error } = store.update(patch);
    if let Some(e) = persist_error {
        dlog!("WARN", "settings", "APP-002 settings write failed ({}); keeping in memory", e);
    }
    if old.launch_with_windows != new.launch_with_windows {
        if let Err(e) = autostart::set_enabled(new.launch_with_windows) {
            dlog!("WARN", "settings", "autostart update failed: {}", e);
        }
    }
    if old.debug_logging != new.debug_logging {
        debug_log::set_debug(new.debug_logging);
    }
    if old != new {
        if let Err(e) = app.emit("settings-changed", &new) {
            dlog!("WARN", "settings", "emit settings-changed failed: {}", e);
        }
        center_ipc::broadcast("settings-changed", &new);
    }
    if old.monitor != new.monitor {
        window::reflow_on_main(app);
    }
    if old.hide_in_fullscreen != new.hide_in_fullscreen {
        fullscreen::reevaluate();
    }
    if old.notifications_enabled != new.notifications_enabled {
        notifications::on_enabled_changed(app);
    }
    if (old.ai_search_enabled, old.ai_search_button, old.ai_search_hotkey)
        != (new.ai_search_enabled, new.ai_search_button, new.ai_search_hotkey)
    {
        search_bar::apply_settings(app, &new);
        if old.ai_search_enabled && !new.ai_search_enabled {
            // Switched off: the kept questions and answers go too (privacy off switch).
            crate::assistant::on_disabled();
        }
    }
    new
}

/// Adopt the per-user autostart state when the user changed it outside the app
/// (Task Manager > Startup apps writes the same registry value).
pub fn sync_autostart(app: &AppHandle) {
    let current = app.state::<SettingsStore>().get();
    match autostart::read_state() {
        Some(actual) if actual != current.launch_with_windows => {
            dlog!("INFO", "settings", "autostart changed outside the app; now {}", actual);
            apply_patch(app, SettingsPatch { launch_with_windows: Some(actual), ..Default::default() });
        }
        // No per-user value means Windows runs the machine-wide entry: re-assert an opt-out
        // (e.g. the registry value was cleaned up or the profile was reset).
        None if !current.launch_with_windows => {
            if let Err(e) = autostart::set_enabled(false) {
                dlog!("WARN", "settings", "autostart opt-out could not be re-applied: {}", e);
            }
        }
        _ => {}
    }
}

/// The Welcome page of the Island Center has been shown: never open it by itself again.
pub fn mark_onboarding_done(app: &AppHandle) {
    apply_patch(app, SettingsPatch { onboarding_done: Some(true), ..Default::default() });
}

#[tauri::command(async)]
pub fn get_settings(store: State<'_, SettingsStore>) -> Result<Settings, String> {
    Ok(store.get())
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, patch: SettingsPatch) -> Result<Settings, String> {
    rt::run_blocking("update_settings", move || Ok(apply_patch(&app, patch))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("companyisland-settings-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn patch(json: &str) -> SettingsPatch {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn defaults_match_contract() {
        let s = Settings::default();
        assert_eq!(s.schema_version, 1);
        assert!(s.launch_with_windows && s.hide_in_fullscreen && s.meeting_reminder_enabled);
        assert_eq!(s.reminder_minutes, 30);
        assert_eq!(s.monitor, "primary");
        assert!(s.notifications_enabled && s.meeting_invites_enabled && s.meeting_silence_prompt);
        assert!(!s.debug_logging);
        assert!(s.ai_search_enabled && s.ai_search_button && s.ai_search_hotkey);
    }

    #[test]
    fn ai_search_settings_round_trip_and_default_on_for_old_files() {
        let old: Settings = serde_json::from_str(r#"{"schemaVersion":1,"launchWithWindows":true}"#).unwrap();
        assert!(old.ai_search_enabled && old.ai_search_button && old.ai_search_hotkey);
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json.get("aiSearchEnabled"), Some(&serde_json::Value::Bool(true)));
        let next = Settings::default().patched(patch(r#"{"aiSearchButton": false, "aiSearchHotkey": false}"#));
        assert!(next.ai_search_enabled && !next.ai_search_button && !next.ai_search_hotkey);
    }

    #[test]
    fn serializes_camel_case() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert!(json.get("launchWithWindows").is_some());
        assert!(json.get("reminderMinutes").is_some());
        assert!(json.get("schemaVersion").is_some());
    }

    #[test]
    fn patch_clamps_reminder_minutes() {
        let base = Settings::default();
        assert_eq!(base.patched(patch(r#"{"reminderMinutes": 9999}"#)).reminder_minutes, 120);
        assert_eq!(base.patched(patch(r#"{"reminderMinutes": -5}"#)).reminder_minutes, 0);
        assert_eq!(base.patched(patch(r#"{"reminderMinutes": 14.6}"#)).reminder_minutes, 15);
        assert_eq!(base.patched(patch(r#"{}"#)).reminder_minutes, 30);
    }

    #[test]
    fn patch_validates_monitor() {
        let base = Settings::default();
        assert_eq!(base.patched(patch(r#"{"monitor": "2"}"#)).monitor, "2");
        assert_eq!(base.patched(patch(r#"{"monitor": "02"}"#)).monitor, "2");
        assert_eq!(base.patched(patch(r#"{"monitor": "99"}"#)).monitor, "primary");
        assert_eq!(base.patched(patch(r#"{"monitor": "left"}"#)).monitor, "primary");
        assert_eq!(base.patched(patch(r#"{"monitor": "-1"}"#)).monitor, "primary");
    }

    #[test]
    fn monitor_is_monitor_id_on_the_wire() {
        let base = Settings::default();
        assert!(serde_json::to_value(&base).unwrap()["monitorId"].is_null());
        let second = base.patched(patch(r#"{"monitorId": "1"}"#));
        assert_eq!(second.monitor, "1");
        assert_eq!(serde_json::to_value(&second).unwrap()["monitorId"], "1");
        // null selects the primary monitor again; an absent key changes nothing
        assert_eq!(second.patched(patch(r#"{"monitorId": null}"#)).monitor, "primary");
        assert_eq!(second.patched(patch(r#"{"debugLogging": true}"#)).monitor, "1");
        // numbers and junk are tolerated
        assert_eq!(base.patched(patch(r#"{"monitorId": 2}"#)).monitor, "2");
        assert_eq!(base.patched(patch(r#"{"monitorId": {"a": 1}}"#)).monitor, "primary");
    }

    #[test]
    fn legacy_monitor_key_and_bad_types_still_load() {
        let legacy: Settings = serde_json::from_str(r#"{"monitor": "3"}"#).unwrap();
        assert_eq!(legacy.monitor, "3");
        let junk: Settings = serde_json::from_str(r#"{"monitorId": [1, 2], "reminderMinutes": 15}"#).unwrap();
        assert_eq!(junk.monitor, "primary");
        assert_eq!(junk.reminder_minutes, 15);
        let round_trip: Settings = serde_json::from_str(&serde_json::to_string(&legacy).unwrap()).unwrap();
        assert_eq!(round_trip, legacy);
    }

    #[test]
    fn patch_only_touches_given_fields() {
        let next = Settings::default().patched(patch(r#"{"hideInFullscreen": false, "unknown": 1}"#));
        assert!(!next.hide_in_fullscreen);
        assert!(next.launch_with_windows && next.notifications_enabled);
    }

    #[test]
    fn meeting_invites_toggle_and_old_files_default_to_on() {
        let off = Settings::default().patched(patch(r#"{"meetingInvitesEnabled": false}"#));
        assert!(!off.meeting_invites_enabled);
        assert!(off.notifications_enabled);
        assert_eq!(serde_json::to_value(&off).unwrap()["meetingInvitesEnabled"], false);
        let old: Settings = serde_json::from_str(r#"{"schemaVersion": 1, "notificationsEnabled": false}"#).unwrap();
        assert!(old.meeting_invites_enabled);
    }

    #[test]
    fn shared_calendar_reminders_toggle_and_old_files_default_to_on() {
        let off = Settings::default().patched(patch(r#"{"sharedCalendarReminders": false}"#));
        assert!(!off.shared_calendar_reminders);
        assert!(off.meeting_reminder_enabled);
        assert_eq!(serde_json::to_value(&off).unwrap()["sharedCalendarReminders"], false);
        let old: Settings = serde_json::from_str(r#"{"schemaVersion": 1}"#).unwrap();
        assert!(old.shared_calendar_reminders);
    }

    #[test]
    fn center_settings_default_and_use_camel_case_names() {
        let s = Settings::default();
        assert!(!s.onboarding_done);
        assert_eq!(s.island_display, "full");
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["onboardingDone"], false);
        assert_eq!(json["islandDisplay"], "full");
    }

    #[test]
    fn old_files_without_the_center_keys_get_the_defaults() {
        let old: Settings =
            serde_json::from_str(r#"{"schemaVersion": 1, "launchWithWindows": false, "debugLogging": true}"#).unwrap();
        let old = old.sanitized();
        assert!(!old.onboarding_done, "upgraders see the Welcome page once");
        assert_eq!(old.island_display, "full");
        assert!(!old.launch_with_windows && old.debug_logging);
    }

    #[test]
    fn patch_sets_onboarding_and_island_display() {
        let base = Settings::default();
        let next = base.patched(patch(r#"{"onboardingDone": true, "islandDisplay": "clock"}"#));
        assert!(next.onboarding_done);
        assert_eq!(next.island_display, "clock");
        // fields that are absent stay as they were
        let again = next.patched(patch(r#"{"debugLogging": true}"#));
        assert!(again.onboarding_done);
        assert_eq!(again.island_display, "clock");
        assert_eq!(again.patched(patch(r#"{"islandDisplay": "date"}"#)).island_display, "date");
        assert!(!again.patched(patch(r#"{"onboardingDone": false}"#)).onboarding_done);
    }

    #[test]
    fn island_display_is_sanitised_everywhere() {
        let base = Settings::default();
        assert_eq!(base.patched(patch(r#"{"islandDisplay": "huge"}"#)).island_display, "full");
        assert_eq!(base.patched(patch(r#"{"islandDisplay": ""}"#)).island_display, "full");
        assert_eq!(base.patched(patch(r#"{"islandDisplay": "CLOCK"}"#)).island_display, "full");
        let unknown: Settings = serde_json::from_str(r#"{"islandDisplay": "weekday"}"#).unwrap();
        assert_eq!(unknown.island_display, "full");
        // a wrong type neither breaks the file nor loses the other values
        let junk: Settings = serde_json::from_str(r#"{"islandDisplay": 7, "reminderMinutes": 15}"#).unwrap();
        assert_eq!(junk.island_display, "full");
        assert_eq!(junk.reminder_minutes, 15);
        let kept: Settings = serde_json::from_str(r#"{"islandDisplay": "date"}"#).unwrap();
        assert_eq!(kept.island_display, "date");
        let direct = Settings { island_display: "nope".into(), ..Settings::default() }.sanitized();
        assert_eq!(direct.island_display, "full");
    }

    #[test]
    fn partial_file_gets_defaults_and_current_schema() {
        let s: Settings = serde_json::from_str(r#"{"schemaVersion": 0, "reminderMinutes": 500}"#).unwrap();
        let s = s.sanitized();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert_eq!(s.reminder_minutes, 120);
        assert!(s.launch_with_windows);
    }

    #[test]
    fn atomic_write_round_trips_and_leaves_no_temp() {
        let dir = scratch_dir("write");
        let path = dir.join("settings.json");
        let first = Settings::default().patched(patch(r#"{"reminderMinutes": 15}"#));
        write_atomic(&path, &serde_json::to_vec_pretty(&first).unwrap()).unwrap();
        let second = first.patched(patch(r#"{"debugLogging": true}"#));
        write_atomic(&path, &serde_json::to_vec_pretty(&second).unwrap()).unwrap();
        assert_eq!(load_from(&path), second);
        assert!(!path.with_extension("json.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_quarantined() {
        let dir = scratch_dir("corrupt");
        let path = dir.join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path), Settings::default());
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(path.with_extension("json.corrupt")).unwrap(), "{ not json");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = scratch_dir("missing");
        assert_eq!(load_from(&dir.join("settings.json")), Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn in_memory_store_still_updates() {
        let store = SettingsStore { path: None, inner: Mutex::new(Settings::default()) };
        let update = store.update(patch(r#"{"monitor": "1"}"#));
        assert_eq!(update.new.monitor, "1");
        assert!(update.persist_error.is_some());
        assert_eq!(store.get().monitor, "1");
        let unchanged = store.update(patch(r#"{"monitor": "1"}"#));
        assert!(unchanged.persist_error.is_none());
    }
}
