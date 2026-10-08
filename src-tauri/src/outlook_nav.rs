//! Outlook's Calendar navigation pane: which calendars the user has (My Calendars, Shared
//! Calendars, Other Calendars, groups of their own) and which of them are checked.
//!
//! Read-only, with one exception: a calendar switched on or off in the island is checked or
//! unchecked in Outlook too, and only while Outlook shows those checkboxes ([`scan`]). Nothing
//! here adds, removes, renames or reorders a calendar, switches a module or a view, or brings
//! Outlook forward. In particular
//! `NavigationGroups.GetDefaultNavigationGroup` is never called: it *creates* a missing
//! group in the user's pane.
//!
//! Object path (Outlook 2007+): `Explorer.NavigationPane.Modules.GetNavigationModule(olModuleCalendar)`
//! -> `NavigationGroups` -> `NavigationGroup` (`GroupType`) -> `NavigationFolders` -> `NavigationFolder`
//! (`DisplayName`, `IsSelected`, `Folder`). Getting the module does not switch the pane to it.
//!
//! Groups are told apart by `NavigationGroup.GroupType` (`OlGroupType`), a language-neutral enum,
//! never by their localized names. A calendar's kind also looks at its folder's store, because
//! users can drag calendars between groups and make groups of their own.
//!
//! `IsSelected` means "checked" only while the explorer shows the Calendar module in a calendar
//! view; elsewhere it means "selected and displayed" (Microsoft Learn, NavigationFolder.IsSelected).
//! So it is believed only then. Otherwise the last believed readout stands ([`SelectionMemory`]),
//! which is also kept, as hashed ids only, in the per-user state folder.
//!
//! Live changes come from `NavigationGroupsEvents_12` (SelectedChange, NavigationFolderAdd,
//! NavigationFolderRemove) on the calendar module's groups; `ExplorerEvents_10.Close` tells when
//! that explorer goes, so nothing of Outlook's is held after its window closes ([`NavWatcher`]).
//! IIDs and DISPIDs are those of the Outlook type library (checked against the installed
//! Microsoft.Office.Interop.Outlook 15 PIA).

use crate::calendar::{SelectionOrigin, SourceGroup, SourceKind};
use crate::com::{self, ComError, ComResult, Connection, Dispatch};
use crate::outlook::{bool_prop, hash16, i32_prop, optional, str_prop};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;
use windows::core::GUID;

/// `OlNavigationModuleType.olModuleCalendar`
const OL_MODULE_CALENDAR: i32 = 1;
/// `OlViewType.olCalendarView`
const OL_CALENDAR_VIEW: i32 = 2;
/// `OlItemType.olAppointmentItem` (a folder's DefaultItemType)
const OL_APPOINTMENT_ITEM: i32 = 1;
/// Far above any real pane; they bound the walk.
const MAX_GROUPS: i32 = 20;
const MAX_FOLDERS_PER_GROUP: i32 = 60;
pub const MAX_CALENDARS: usize = 40;
const MAX_NAME_CHARS: usize = 120;

/// `NavigationGroupsEvents_12` and its members.
const DIID_NAVIGATION_GROUPS_EVENTS: GUID = GUID::from_u128(0x000630F4_0000_0000_C000_000000000046);
const DISPID_SELECTED_CHANGE: i32 = 64458;
const DISPID_NAVIGATION_FOLDER_ADD: i32 = 64459;
const DISPID_NAVIGATION_FOLDER_REMOVE: i32 = 64460;
/// `ExplorerEvents_10.Close`
const DIID_EXPLORER_EVENTS: GUID = GUID::from_u128(0x0006300F_0000_0000_C000_000000000046);
const DISPID_EXPLORER_CLOSE: i32 = 61448;

// =============================================================================
// Pure classification
// =============================================================================

/// `OlGroupType` -> group. 1 olMyFoldersGroup, 2 olPeopleFoldersGroup ("Shared Calendars"),
/// 3 olOtherFoldersGroup, 5 olRoomsGroup; 0 olCustomFoldersGroup and anything else are the
/// user's own groups.
pub fn group_of(ol_group_type: i32) -> SourceGroup {
    match ol_group_type {
        1 => SourceGroup::My,
        2 => SourceGroup::Shared,
        3 => SourceGroup::Other,
        5 => SourceGroup::Rooms,
        _ => SourceGroup::Custom,
    }
}

/// What a calendar is. The group decides for Shared/Other/Rooms; in My Calendars and the
/// user's own groups, a calendar from someone else's mailbox is still a shared one.
pub fn kind_of(group: SourceGroup, primary: bool, own_store: bool) -> SourceKind {
    if primary {
        return SourceKind::Primary;
    }
    match group {
        SourceGroup::Shared => SourceKind::Shared,
        SourceGroup::Other | SourceGroup::Rooms => SourceKind::Other,
        SourceGroup::My | SourceGroup::Custom | SourceGroup::Unknown => {
            if own_store {
                SourceKind::Personal
            } else {
                SourceKind::Shared
            }
        }
    }
}

/// Support code for a calendar that cannot be opened or read. Raw HRESULTs stay in the log.
/// - CAL-SHARED-101 no permission (any more)
/// - CAL-SHARED-102 the folder is gone or its reference is no longer valid
/// - CAL-SHARED-103 its mailbox or server cannot be reached (offline, not synchronized yet)
/// - CAL-SHARED-109 any other read failure
pub fn shared_error_code(e: &ComError) -> &'static str {
    match e.effective() as u32 {
        // E_ACCESSDENIED (= MAPI_E_NO_ACCESS)
        0x8007_0005 => "CAL-SHARED-101",
        // MAPI_E_NOT_FOUND, MAPI_E_INVALID_ENTRYID, MAPI_E_OBJECT_DELETED
        0x8004_010F | 0x8004_0107 | 0x8004_0117 => "CAL-SHARED-102",
        // MAPI_E_NETWORK_ERROR, MAPI_E_LOGON_FAILED, MAPI_E_UNCONFIGURED, MAPI_E_FAILONEPROVIDER,
        // MAPI_E_END_OF_SESSION, MAPI_E_TIMEOUT
        0x8004_0115 | 0x8004_0111 | 0x8004_011C | 0x8004_011D | 0x8004_0200 | 0x8004_0401 => "CAL-SHARED-103",
        _ => "CAL-SHARED-109",
    }
}

/// Only a call that failed because Outlook is gone or busy fails the whole scan. Anything else,
/// an access denied included, stays with the one group or calendar it happened on: in a large
/// organization the pane routinely lists calendars the user may no longer open
/// (E_ACCESSDENIED = MAPI_E_NO_ACCESS), and one of them must never hide the others.
fn is_outlook_level(e: &ComError) -> bool {
    e.is_disconnected() || e.is_busy()
}

/// Outlook-level failures bubble up; anything else is handed back to the caller to deal with.
fn contained<T>(r: ComResult<T>) -> ComResult<ComResult<T>> {
    match r {
        Err(e) if is_outlook_level(&e) => Err(e),
        r => Ok(r),
    }
}

fn clip_name(name: &str) -> String {
    name.trim().chars().take(MAX_NAME_CHARS).collect()
}

// =============================================================================
// Reading the pane
// =============================================================================

/// One calendar of the navigation pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavCalendar {
    /// hash16(StoreID|EntryID); for a folder that cannot be opened, a hash of where it sits.
    pub id: String,
    /// Empty when the folder could not be opened. Never leaves the COM worker.
    pub entry_id: String,
    pub store_id: String,
    pub name: String,
    pub group: SourceGroup,
    /// `IsSelected` as read (only meaningful when [`NavScan::trusted`]).
    pub selected: bool,
    /// The folder lives in the profile's default store.
    pub own_store: bool,
    /// Why the folder behind the entry could not be opened.
    pub error: Option<&'static str>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavScan {
    pub calendars: Vec<NavCalendar>,
    pub groups: usize,
    /// `IsSelected` means "checked" right now (Calendar module, calendar view).
    pub trusted: bool,
}

/// The Calendar module's NavigationGroups of `explorer`. Does not switch modules.
fn calendar_groups(explorer: &mut Dispatch) -> ComResult<Dispatch> {
    let mut pane = explorer.get_object("NavigationPane")?;
    let mut modules = pane.get_object("Modules")?;
    let mut module = modules
        .call_object("GetNavigationModule", vec![com::variant_from_i32(OL_MODULE_CALENDAR)])?
        .ok_or_else(|| ComError::new("GetNavigationModule", com::E_NOOBJECT))?;
    module.get_object("NavigationGroups")
}

/// True while `explorer` shows the Calendar module in a calendar (day/week/month) view: the one
/// state in which `IsSelected` is the checkbox. Unreadable means "no".
fn selection_is_trustworthy(explorer: &mut Dispatch) -> ComResult<bool> {
    Ok(contained(read_trustworthy(explorer))?.unwrap_or(false))
}

fn read_trustworthy(explorer: &mut Dispatch) -> ComResult<bool> {
    let Some(mut pane) = optional(explorer.get_object("NavigationPane"))? else {
        return Ok(false);
    };
    let Some(mut current) = optional(pane.get_object("CurrentModule"))? else {
        return Ok(false);
    };
    if i32_prop(&mut current, "NavigationModuleType")? != Some(OL_MODULE_CALENDAR) {
        return Ok(false);
    }
    let Some(mut view) = optional(explorer.get_object("CurrentView"))? else {
        return Ok(false);
    };
    Ok(i32_prop(&mut view, "ViewType")? == Some(OL_CALENDAR_VIEW))
}

#[derive(Clone)]
enum FolderRef {
    Calendar { entry_id: String, store_id: String },
    /// A Tasks or Mail folder dragged into the pane: not ours to read.
    NotCalendar,
    Unavailable(&'static str),
}

fn open_folder(nav: &mut Dispatch) -> ComResult<FolderRef> {
    let read = contained((|| -> ComResult<Option<(String, String)>> {
        let mut folder = nav.get_object("Folder")?;
        if i32_prop(&mut folder, "DefaultItemType")?.is_some_and(|t| t != OL_APPOINTMENT_ITEM) {
            return Ok(None);
        }
        Ok(Some((str_prop(&mut folder, "EntryID")?.unwrap_or_default(), str_prop(&mut folder, "StoreID")?.unwrap_or_default())))
    })())?;
    Ok(match read {
        Ok(None) => FolderRef::NotCalendar,
        Ok(Some((entry_id, _))) if entry_id.is_empty() => FolderRef::Unavailable("CAL-SHARED-106"),
        Ok(Some((entry_id, store_id))) => FolderRef::Calendar { entry_id, store_id },
        Err(e) => {
            dlog!("DEBUG", "outlook", "navigation folder not opened: {}", e);
            FolderRef::Unavailable(match shared_error_code(&e) {
                "CAL-SHARED-109" => "CAL-SHARED-106",
                code => code,
            })
        }
    })
}

/// A pane entry's folder is opened again after this long; until then the last result stands.
const FOLDER_REFRESH_SECS: u64 = 600;
/// Opening the folders of entries not seen before stops after this much of one scan; the
/// next syncs open the rest. Another mailbox's folder can take seconds to open, and the whole
/// sync has to answer within the watchdog's 10 s.
const SCAN_BUDGET_MS: u128 = 3_000;

/// What opening each pane entry's folder found, so the pane can be re-read on every sync
/// without opening every colleague's calendar again. Memory only.
#[derive(Default)]
pub struct FolderCache {
    /// "group type|position|display name" -> the folder, and when it was opened.
    entries: HashMap<String, (FolderRef, Instant)>,
}

impl FolderCache {
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Every calendar in `explorer`'s Calendar navigation pane, in pane order, each once.
/// `own_store_id` is the profile's default store. Only an Outlook-level failure is an error: a
/// group or calendar that cannot be read is skipped or listed as unavailable. A scan that ran
/// out of time before opening every new entry is partial and not [`NavScan::trusted`], so it
/// never replaces the remembered checkboxes. `apply` (calendar id -> checked) is written to
/// Outlook's checkboxes when they are on screen; the scan reports the result.
///
/// This is the one place the island changes anything in Outlook's pane, and only on the user's
/// own request from the island.
pub fn scan(explorer: &mut Dispatch, own_store_id: &str, cache: &mut FolderCache, apply: &HashMap<String, bool>) -> ComResult<NavScan> {
    let started = Instant::now();
    let over_budget = || started.elapsed().as_millis() > SCAN_BUDGET_MS;
    let trusted = selection_is_trustworthy(explorer)?;
    let mut groups = calendar_groups(explorer)?;
    let group_count = i32_prop(&mut groups, "Count")?.unwrap_or(0).clamp(0, MAX_GROUPS);
    let mut out = NavScan { calendars: Vec::new(), groups: group_count as usize, trusted };
    let mut seen = Vec::new();
    let mut partial = false;
    'groups: for gi in 1..=group_count {
        let Ok(Some(mut group)) = contained(optional(groups.call_object("Item", vec![com::variant_from_i32(gi)])).map(Option::flatten))? else {
            continue;
        };
        let group_type = contained(i32_prop(&mut group, "GroupType"))?.ok().flatten().unwrap_or(0);
        let Ok(Some(mut folders)) = contained(optional(group.get_object("NavigationFolders")))? else {
            continue;
        };
        let folder_count = contained(i32_prop(&mut folders, "Count"))?.ok().flatten().unwrap_or(0).clamp(0, MAX_FOLDERS_PER_GROUP);
        for fi in 1..=folder_count {
            if out.calendars.len() >= MAX_CALENDARS {
                break 'groups;
            }
            let Ok(Some(mut nav)) = contained(optional(folders.call_object("Item", vec![com::variant_from_i32(fi)])).map(Option::flatten))? else {
                continue;
            };
            let name = clip_name(&contained(str_prop(&mut nav, "DisplayName"))?.ok().flatten().unwrap_or_default());
            let selected = contained(bool_prop(&mut nav, "IsSelected"))?.unwrap_or(false);
            let group = group_of(group_type);
            let key = format!("{group_type}|{fi}|{name}");
            let folder = match cache.entries.get(&key) {
                Some((folder, at)) if at.elapsed().as_secs() < FOLDER_REFRESH_SECS || over_budget() => folder.clone(),
                None if over_budget() => {
                    partial = true;
                    continue;
                }
                _ => {
                    let folder = open_folder(&mut nav)?;
                    cache.entries.insert(key.clone(), (folder.clone(), Instant::now()));
                    folder
                }
            };
            seen.push(key);
            let mut calendar = match folder {
                FolderRef::NotCalendar => continue,
                FolderRef::Calendar { entry_id, store_id } => NavCalendar {
                    id: hash16(&format!("{store_id}|{entry_id}")),
                    own_store: !own_store_id.is_empty() && store_id == own_store_id,
                    entry_id,
                    store_id,
                    name,
                    group,
                    selected,
                    error: None,
                },
                FolderRef::Unavailable(code) => NavCalendar {
                    id: hash16(&format!("nav|{group_type}|{fi}|{name}")),
                    entry_id: String::new(),
                    store_id: String::new(),
                    name,
                    group,
                    selected,
                    own_store: false,
                    error: Some(code),
                },
            };
            // A switch turned in the island. Written only while the checkboxes are on screen,
            // where setting IsSelected is exactly the user's click on one; elsewhere it would
            // mean "select and display" and could move Outlook away from what it shows.
            if let Some(&want) = apply.get(&calendar.id).filter(|&&want| trusted && want != calendar.selected) {
                match contained(nav.put("IsSelected", com::variant_from_bool(want)))? {
                    Ok(()) => {
                        dlog!("INFO", "outlook", "calendar {} {} in Outlook from the island", crate::debug_log::hash_id(&calendar.id), if want { "checked" } else { "unchecked" });
                        calendar.selected = want;
                    }
                    // E.g. the last checked calendar, which Outlook keeps checked.
                    Err(e) => dlog!("WARN", "outlook", "calendar {} not changed in Outlook: {}", crate::debug_log::hash_id(&calendar.id), e),
                }
            }
            if !out.calendars.iter().any(|c| c.id == calendar.id) {
                out.calendars.push(calendar);
            }
        }
    }
    if partial {
        out.trusted = false;
        dlog!(
            "INFO",
            "outlook",
            "calendar navigation scan stopped after {} ms with {} calendars; the rest follow on the next syncs",
            started.elapsed().as_millis(),
            out.calendars.len()
        );
    } else {
        // Entries gone from the pane are forgotten.
        cache.entries.retain(|k, _| seen.contains(k));
    }
    Ok(out)
}

// =============================================================================
// Which calendars are checked
// =============================================================================

/// The last trustworthy readout of the checkboxes: calendar id -> checked. Reconciled with
/// Outlook on every read that can see them; never a second selection system. A switch turned
/// in the island is a request to Outlook: it counts at once, and is written to Outlook's own
/// checkbox by the next scan that sees the checkboxes ([`scan`]); after that, Outlook wins again.
#[derive(Debug, Default)]
pub struct SelectionMemory {
    checked: HashMap<String, bool>,
    /// Turned in the island, not yet written to Outlook: calendar id -> checked.
    pending: HashMap<String, bool>,
    /// Hash of the Outlook profile the readout belongs to.
    profile: Option<String>,
    /// Changed since it was last saved.
    dirty: bool,
}

#[derive(Serialize, Deserialize)]
struct SavedSelection {
    v: u32,
    profile: String,
    checked: HashMap<String, bool>,
    #[serde(default)]
    pending: HashMap<String, bool>,
}

const SELECTION_FILE: &str = "calendar_selection.json";
const MAX_SELECTION_BYTES: u64 = 64 * 1024;

impl SelectionMemory {
    /// Use the readout saved for `profile` (another profile's is ignored). Called when the
    /// profile is first seen or changes.
    pub fn switch_profile(&mut self, profile: &str, dir: Option<&Path>) {
        if self.profile.as_deref() == Some(profile) {
            return;
        }
        self.profile = Some(profile.to_string());
        self.dirty = false;
        let saved = dir.and_then(|d| load_selection(&d.join(SELECTION_FILE), profile)).unwrap_or_default();
        (self.checked, self.pending) = saved;
    }

    /// The island asks for calendar `id` to be checked or not. It counts from now on; Outlook
    /// itself follows when a scan can see its checkboxes.
    pub fn request(&mut self, id: &str, checked: bool) {
        self.checked.insert(id.to_string(), checked);
        self.pending.insert(id.to_string(), checked);
        self.dirty = true;
    }

    /// The requests not written to Outlook yet.
    pub fn pending(&self) -> &HashMap<String, bool> {
        &self.pending
    }

    /// Which calendars are checked, and how that is known. A trusted scan replaces the memory;
    /// it has also written every pending request it could (see [`scan`]), so none is left.
    pub fn resolve(&mut self, scan: Option<&NavScan>) -> (HashMap<String, bool>, SelectionOrigin) {
        match scan {
            Some(s) if s.trusted => {
                let readout: HashMap<String, bool> = s.calendars.iter().map(|c| (c.id.clone(), c.selected)).collect();
                if readout != self.checked || !self.pending.is_empty() {
                    self.checked = readout;
                    self.pending.clear();
                    self.dirty = true;
                }
                (self.checked.clone(), SelectionOrigin::Outlook)
            }
            _ if !self.checked.is_empty() => (self.checked.clone(), SelectionOrigin::Remembered),
            _ => (HashMap::new(), SelectionOrigin::PrimaryOnly),
        }
    }

    /// Save the readout when it changed. Ids are hashes; no names, no events.
    pub fn save_if_dirty(&mut self, dir: Option<&Path>) {
        let (Some(dir), Some(profile)) = (dir, self.profile.as_ref()) else { return };
        if !self.dirty {
            return;
        }
        let saved = SavedSelection { v: 1, profile: profile.clone(), checked: self.checked.clone(), pending: self.pending.clone() };
        match serde_json::to_vec(&saved).map_err(std::io::Error::other).and_then(|b| crate::reminder_state::write_atomic(&dir.join(SELECTION_FILE), &b)) {
            Ok(()) => self.dirty = false,
            Err(e) => dlog!("WARN", "outlook", "calendar selection not saved: {}", e),
        }
    }
}

/// A calendar id as the scan makes them: 16 hex digits.
pub fn is_calendar_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The saved checkboxes and pending requests for `profile`.
fn load_selection(path: &Path, profile: &str) -> Option<(HashMap<String, bool>, HashMap<String, bool>)> {
    if std::fs::metadata(path).ok()?.len() > MAX_SELECTION_BYTES {
        return None;
    }
    let saved: SavedSelection = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let valid = saved.checked.keys().chain(saved.pending.keys()).all(|id| is_calendar_id(id));
    (saved.v == 1 && saved.profile == profile && valid).then_some((saved.checked, saved.pending))
}

/// The per-user folder the selection readout is kept in, when there is one.
pub fn state_dir() -> Option<PathBuf> {
    crate::paths::state_dir().ok()
}

// =============================================================================
// Live notifications
// =============================================================================

pub const CHANGED: u8 = 1;
pub const CLOSED: u8 = 2;

/// Outlook's own notifications for one explorer's calendar pane. Holds that explorer and its
/// calendar NavigationGroups (nothing else) and lets go of both, after unadvising, when the
/// explorer closes, Outlook goes away or the worker stops.
pub struct NavWatcher {
    // Field order is drop order: unadvise first, then release the objects.
    connections: Vec<Connection>,
    _groups: Dispatch,
    explorer: Dispatch,
    signals: Rc<Cell<u8>>,
}

impl NavWatcher {
    /// Subscribe to `explorer`'s calendar pane. Both subscriptions or none: without Close we
    /// could not know when to let go of the explorer.
    pub fn arm(mut explorer: Dispatch) -> ComResult<NavWatcher> {
        let groups = calendar_groups(&mut explorer)?;
        let signals = Rc::new(Cell::new(0u8));
        let on_nav = Rc::clone(&signals);
        let nav = groups.advise(
            DIID_NAVIGATION_GROUPS_EVENTS,
            Box::new(move |dispid| {
                if matches!(dispid, DISPID_SELECTED_CHANGE | DISPID_NAVIGATION_FOLDER_ADD | DISPID_NAVIGATION_FOLDER_REMOVE) {
                    on_nav.set(on_nav.get() | CHANGED);
                }
            }),
        )?;
        let on_explorer = Rc::clone(&signals);
        let close = explorer.advise(
            DIID_EXPLORER_EVENTS,
            Box::new(move |dispid| {
                if dispid == DISPID_EXPLORER_CLOSE {
                    on_explorer.set(on_explorer.get() | CLOSED);
                }
            }),
        )?;
        Ok(NavWatcher { connections: vec![nav, close], _groups: groups, explorer, signals })
    }

    /// The notifications since the last call ([`CHANGED`], [`CLOSED`] bits).
    pub fn take(&self) -> u8 {
        self.signals.replace(0)
    }

    /// False once the explorer's Outlook is gone (restarted, crashed): its objects are dead.
    pub fn alive(&mut self) -> bool {
        match self.explorer.get("Class") {
            Ok(_) => true,
            Err(e) => !e.is_disconnected(),
        }
    }
}

impl Drop for NavWatcher {
    fn drop(&mut self) {
        self.connections.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_types_follow_ol_group_type() {
        assert_eq!(group_of(1), SourceGroup::My);
        assert_eq!(group_of(2), SourceGroup::Shared);
        assert_eq!(group_of(3), SourceGroup::Other);
        assert_eq!(group_of(5), SourceGroup::Rooms);
        for custom in [0, 4, 6, 99, -1] {
            assert_eq!(group_of(custom), SourceGroup::Custom, "{custom}");
        }
    }

    #[test]
    fn kinds_come_from_group_and_store_never_from_names() {
        assert_eq!(kind_of(SourceGroup::Shared, true, true), SourceKind::Primary);
        assert_eq!(kind_of(SourceGroup::My, false, true), SourceKind::Personal);
        assert_eq!(kind_of(SourceGroup::My, false, false), SourceKind::Shared);
        assert_eq!(kind_of(SourceGroup::Shared, false, true), SourceKind::Shared);
        assert_eq!(kind_of(SourceGroup::Shared, false, false), SourceKind::Shared);
        assert_eq!(kind_of(SourceGroup::Other, false, false), SourceKind::Other);
        assert_eq!(kind_of(SourceGroup::Rooms, false, false), SourceKind::Other);
        assert_eq!(kind_of(SourceGroup::Custom, false, true), SourceKind::Personal);
        assert_eq!(kind_of(SourceGroup::Custom, false, false), SourceKind::Shared);
    }

    #[test]
    fn read_errors_map_to_support_codes() {
        let err = |hr: u32| ComError::new("Folder", hr as i32);
        assert_eq!(shared_error_code(&err(0x8007_0005)), "CAL-SHARED-101");
        assert_eq!(shared_error_code(&err(0x8004_010F)), "CAL-SHARED-102");
        assert_eq!(shared_error_code(&err(0x8004_0107)), "CAL-SHARED-102");
        assert_eq!(shared_error_code(&err(0x8004_0115)), "CAL-SHARED-103");
        assert_eq!(shared_error_code(&err(0x8004_011D)), "CAL-SHARED-103");
        assert_eq!(shared_error_code(&err(0x8000_4005)), "CAL-SHARED-109");
    }

    fn cal(id: &str, selected: bool) -> NavCalendar {
        NavCalendar {
            id: id.into(),
            entry_id: "e".into(),
            store_id: "s".into(),
            name: "n".into(),
            group: SourceGroup::Shared,
            selected,
            own_store: false,
            error: None,
        }
    }

    fn scan_of(trusted: bool, cals: Vec<NavCalendar>) -> NavScan {
        NavScan { groups: 2, trusted, calendars: cals }
    }

    #[test]
    fn selection_follows_outlook_only_when_the_checkboxes_are_visible() {
        let mut m = SelectionMemory::default();
        // Nothing known, Outlook on Mail: only the primary calendar.
        let (sel, origin) = m.resolve(Some(&scan_of(false, vec![cal("aaaaaaaaaaaaaaaa", false)])));
        assert!(sel.is_empty());
        assert_eq!(origin, SelectionOrigin::PrimaryOnly);
        // On the calendar: read as is.
        let (sel, origin) = m.resolve(Some(&scan_of(true, vec![cal("aaaaaaaaaaaaaaaa", true), cal("bbbbbbbbbbbbbbbb", false)])));
        assert_eq!(origin, SelectionOrigin::Outlook);
        assert_eq!(sel["aaaaaaaaaaaaaaaa"], true);
        assert_eq!(sel["bbbbbbbbbbbbbbbb"], false);
        // Back on Mail, IsSelected reads false everywhere: the last readout stands.
        let (sel, origin) = m.resolve(Some(&scan_of(false, vec![cal("aaaaaaaaaaaaaaaa", false), cal("bbbbbbbbbbbbbbbb", false)])));
        assert_eq!(origin, SelectionOrigin::Remembered);
        assert_eq!(sel["aaaaaaaaaaaaaaaa"], true);
        // No explorer at all: same.
        assert_eq!(m.resolve(None).1, SelectionOrigin::Remembered);
        // Unchecked in Outlook: follows.
        let (sel, _) = m.resolve(Some(&scan_of(true, vec![cal("aaaaaaaaaaaaaaaa", false), cal("bbbbbbbbbbbbbbbb", true)])));
        assert_eq!(sel["aaaaaaaaaaaaaaaa"], false);
        assert_eq!(sel["bbbbbbbbbbbbbbbb"], true);
    }

    #[test]
    fn a_switch_in_the_island_counts_at_once_and_waits_for_outlooks_checkboxes() {
        let (a, b) = ("aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb");
        let mut m = SelectionMemory::default();
        m.resolve(Some(&scan_of(true, vec![cal(a, true), cal(b, false)])));
        // Switched on in the island while Outlook shows Mail: on now, still owed to Outlook.
        m.request(b, true);
        let (sel, origin) = m.resolve(Some(&scan_of(false, vec![cal(a, false), cal(b, false)])));
        assert_eq!(origin, SelectionOrigin::Remembered);
        assert!(sel[a] && sel[b]);
        assert_eq!(m.pending().get(b), Some(&true));
        // Outlook shows its calendar: the scan wrote it (the readout has it checked), nothing is owed.
        let (sel, origin) = m.resolve(Some(&scan_of(true, vec![cal(a, true), cal(b, true)])));
        assert_eq!(origin, SelectionOrigin::Outlook);
        assert!(sel[b]);
        assert!(m.pending().is_empty());
        // Outlook refused (or the calendar went): its own checkbox wins, and nothing is owed either.
        m.request(a, false);
        let (sel, _) = m.resolve(Some(&scan_of(true, vec![cal(a, true), cal(b, true)])));
        assert!(sel[a]);
        assert!(m.pending().is_empty());
    }

    #[test]
    fn a_switch_owed_to_outlook_survives_a_restart() {
        let dir = scratch("pending");
        let mut m = SelectionMemory::default();
        m.switch_profile("p1hash", Some(&dir));
        m.request("cccccccccccccccc", true);
        m.save_if_dirty(Some(&dir));
        let mut again = SelectionMemory::default();
        again.switch_profile("p1hash", Some(&dir));
        assert_eq!(again.pending().get("cccccccccccccccc"), Some(&true));
        assert_eq!(again.resolve(None).0["cccccccccccccccc"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn calendar_ids_are_16_hex_digits() {
        assert!(is_calendar_id("0123456789abcdef"));
        assert!(!is_calendar_id("0123456789abcde"));
        assert!(!is_calendar_id("Support Team 123"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ci-nav-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_readout_is_saved_per_profile_as_hashes_only() {
        let dir = scratch("save");
        let mut m = SelectionMemory::default();
        m.switch_profile("p1hash", Some(&dir));
        m.resolve(Some(&scan_of(true, vec![cal("aaaaaaaaaaaaaaaa", true)])));
        m.save_if_dirty(Some(&dir));
        let text = std::fs::read_to_string(dir.join(SELECTION_FILE)).unwrap();
        assert!(text.contains("aaaaaaaaaaaaaaaa") && !text.contains("\"n\""), "{text}");

        // A restart while Outlook shows Mail: remembered.
        let mut again = SelectionMemory::default();
        again.switch_profile("p1hash", Some(&dir));
        let (sel, origin) = again.resolve(None);
        assert_eq!(origin, SelectionOrigin::Remembered);
        assert_eq!(sel["aaaaaaaaaaaaaaaa"], true);

        // Another Outlook profile never sees it.
        let mut other = SelectionMemory::default();
        other.switch_profile("p2hash", Some(&dir));
        assert_eq!(other.resolve(None).1, SelectionOrigin::PrimaryOnly);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tampered_selection_file_is_ignored() {
        let dir = scratch("bad");
        std::fs::write(dir.join(SELECTION_FILE), br#"{"v":1,"profile":"p","checked":{"Support Team":true}}"#).unwrap();
        let mut m = SelectionMemory::default();
        m.switch_profile("p", Some(&dir));
        assert_eq!(m.resolve(None).1, SelectionOrigin::PrimaryOnly);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
