//! Outlook's Calendar navigation pane: which calendars the user has (My Calendars, Shared
//! Calendars, Other Calendars, groups of their own) and which of them are checked.
//!
//! Read-only. Nothing here selects, adds, removes, renames or reorders a calendar, switches a
//! module or a view, or brings Outlook forward. In particular
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

/// A call that failed because Outlook is gone, busy or guarded fails the whole read; anything
/// else only affects the one calendar.
fn is_outlook_level(e: &ComError) -> bool {
    e.is_disconnected() || e.is_busy() || e.is_blocked()
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

enum FolderRef {
    Calendar { entry_id: String, store_id: String },
    /// A Tasks or Mail folder dragged into the pane: not ours to read.
    NotCalendar,
    Unavailable(&'static str),
}

fn open_folder(nav: &mut Dispatch) -> ComResult<FolderRef> {
    let mut folder = match nav.get_object("Folder") {
        Ok(f) => f,
        Err(e) if is_outlook_level(&e) => return Err(e),
        Err(e) => {
            dlog!("DEBUG", "outlook", "navigation folder not opened: {}", e);
            return Ok(FolderRef::Unavailable(match shared_error_code(&e) {
                "CAL-SHARED-109" => "CAL-SHARED-106",
                code => code,
            }));
        }
    };
    if i32_prop(&mut folder, "DefaultItemType")?.is_some_and(|t| t != OL_APPOINTMENT_ITEM) {
        return Ok(FolderRef::NotCalendar);
    }
    let entry_id = str_prop(&mut folder, "EntryID")?.unwrap_or_default();
    let store_id = str_prop(&mut folder, "StoreID")?.unwrap_or_default();
    if entry_id.is_empty() {
        return Ok(FolderRef::Unavailable("CAL-SHARED-106"));
    }
    Ok(FolderRef::Calendar { entry_id, store_id })
}

/// Every calendar in `explorer`'s Calendar navigation pane, in pane order, each once.
/// `own_store_id` is the profile's default store. Only an Outlook-level failure is an error.
pub fn scan(explorer: &mut Dispatch, own_store_id: &str) -> ComResult<NavScan> {
    let trusted = selection_is_trustworthy(explorer)?;
    let mut groups = calendar_groups(explorer)?;
    let group_count = i32_prop(&mut groups, "Count")?.unwrap_or(0).clamp(0, MAX_GROUPS);
    let mut out = NavScan { calendars: Vec::new(), groups: group_count as usize, trusted };
    for gi in 1..=group_count {
        let Some(mut group) = optional(groups.call_object("Item", vec![com::variant_from_i32(gi)]))?.flatten() else {
            continue;
        };
        let group_type = i32_prop(&mut group, "GroupType")?.unwrap_or(0);
        let Some(mut folders) = optional(group.get_object("NavigationFolders"))? else {
            continue;
        };
        let folder_count = i32_prop(&mut folders, "Count")?.unwrap_or(0).clamp(0, MAX_FOLDERS_PER_GROUP);
        for fi in 1..=folder_count {
            if out.calendars.len() >= MAX_CALENDARS {
                return Ok(out);
            }
            let Some(mut nav) = optional(folders.call_object("Item", vec![com::variant_from_i32(fi)]))?.flatten() else {
                continue;
            };
            let name = clip_name(&str_prop(&mut nav, "DisplayName")?.unwrap_or_default());
            let selected = bool_prop(&mut nav, "IsSelected")?;
            let group = group_of(group_type);
            let calendar = match open_folder(&mut nav)? {
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
            if !out.calendars.iter().any(|c| c.id == calendar.id) {
                out.calendars.push(calendar);
            }
        }
    }
    Ok(out)
}

// =============================================================================
// Which calendars are checked
// =============================================================================

/// The last trustworthy readout of the checkboxes: calendar id -> checked. Reconciled with
/// Outlook on every read that can see them; never a second selection system (nothing here is
/// ever written back to Outlook, and Outlook always wins).
#[derive(Debug, Default)]
pub struct SelectionMemory {
    checked: HashMap<String, bool>,
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
        self.checked = dir.and_then(|d| load_selection(&d.join(SELECTION_FILE), profile)).unwrap_or_default();
    }

    /// Which calendars are checked, and how that is known. A trusted scan replaces the memory.
    pub fn resolve(&mut self, scan: Option<&NavScan>) -> (HashMap<String, bool>, SelectionOrigin) {
        match scan {
            Some(s) if s.trusted => {
                let readout: HashMap<String, bool> = s.calendars.iter().map(|c| (c.id.clone(), c.selected)).collect();
                if readout != self.checked {
                    self.checked = readout;
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
        let saved = SavedSelection { v: 1, profile: profile.clone(), checked: self.checked.clone() };
        match serde_json::to_vec(&saved).map_err(std::io::Error::other).and_then(|b| crate::reminder_state::write_atomic(&dir.join(SELECTION_FILE), &b)) {
            Ok(()) => self.dirty = false,
            Err(e) => dlog!("WARN", "outlook", "calendar selection not saved: {}", e),
        }
    }
}

fn load_selection(path: &Path, profile: &str) -> Option<HashMap<String, bool>> {
    if std::fs::metadata(path).ok()?.len() > MAX_SELECTION_BYTES {
        return None;
    }
    let saved: SavedSelection = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let valid = |id: &String| id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit());
    (saved.v == 1 && saved.profile == profile && saved.checked.keys().all(valid)).then_some(saved.checked)
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
