# Classic Outlook shared calendars

The island follows the calendars the employee has checked in classic Outlook's Calendar
navigation pane: My Calendars, Shared Calendars, Other Calendars and groups the employee made
themselves. Nothing is configured in the island. Outlook is the only place where calendars are
chosen, and the island never changes anything in Outlook.

Code:
- `src-tauri/src/outlook_nav.rs`: discovery, the checked-state rules, the selection memory and
  the live notifications.
- `src-tauri/src/outlook.rs` (`OutlookSource`): reading several calendars, the time budget,
  quarantine and dedup.
- `src-tauri/src/calendar.rs`: the contract and the supervisor.
- `src/lib/calendar/*`: normalisation and presentation helpers.
- `src/components/Pill/panels/CalendarSources.tsx`: the list of calendars.

## Object model used (verified)

Every member below was checked against Microsoft Learn and the installed Outlook PIA
(Microsoft.Office.Interop.Outlook 15). It was also exercised read-only against a running
Outlook 16.0.17932.

| Step | Member | Notes |
|---|---|---|
| Explorer | `Application.ActiveExplorer` | When it is null (no main window), the island uses the last discovery. |
| Calendar module | `Explorer.NavigationPane.Modules.GetNavigationModule(1)` | `olModuleCalendar = 1`. Reading it does **not** switch the visible module. |
| Groups | `CalendarModule.NavigationGroups.Item(i)`, `.GroupType` | `OlGroupType`: 0 custom, 1 My, 2 People ("Shared Calendars"), 3 Other, 5 Rooms. |
| Calendars | `NavigationGroup.NavigationFolders.Item(j)` | `DisplayName`, `IsSelected`, `Folder`. |
| Folder | `NavigationFolder.Folder` → `EntryID`, `StoreID`, `DefaultItemType` | Only `olAppointmentItem (1)` folders count. |
| Re-open | `NameSpace.GetFolderFromID(EntryID, StoreID)` | Each sync opens folders by ID, so no Folder object is kept between syncs. |
| Live changes | `NavigationGroupsEvents_12` `{000630F4-0000-0000-C000-000000000046}` | SelectedChange 64458, NavigationFolderAdd 64459, NavigationFolderRemove 64460. |
| Explorer closed | `ExplorerEvents_10` `{0006300F-0000-0000-C000-000000000046}` | Close 61448. |

**Never called: `NavigationGroups.GetDefaultNavigationGroup`.** It *creates* the group when it
does not exist. A probe on the development machine added empty "Shared Calendars" and "Other
Calendars" groups to the user's pane this way.

`NameSpace.GetSharedDefaultFolder` is not used. Every shared calendar the employee opened or was
given is already in the navigation pane. Resolving recipients would mean guessing people, and
the island must not look for calendars the employee never opened.

## Groups without localized names

Groups are classified by `GroupType`, an enum that is the same in every Outlook language. The
group names ("Shared Calendars", "יומנים משותפים" and so on) are never compared.

Employees can drag calendars between groups and create groups, so a calendar's *kind* also looks
at its store (`kind_of` in outlook_nav.rs):

| Group | Folder in the employee's own store | Folder in another store |
|---|---|---|
| (the profile's default calendar) | primary | n/a |
| My Calendars (1), custom groups (0) | personal | shared |
| Shared Calendars (2) | shared | shared |
| Other Calendars (3), Rooms (5) | other | other |

Limitation: a shared calendar that Exchange synchronizes *into* the employee's own mailbox and
that the employee then drags into My Calendars is shown as "personal". No group type identifies
it in that position, and its store is the employee's own.

## Checked calendars

`NavigationFolder.IsSelected` means "checked" only while the explorer shows the Calendar module
in a calendar view (`CurrentModule.NavigationModuleType = 1` and `CurrentView.ViewType = 2`). In
other modules it means "selected and displayed" (Microsoft Learn, NavigationFolder.IsSelected).
So:

- **On the calendar:** the readout is used as is and replaces the remembered selection.
- **On Mail, Contacts and so on, or with no main window:** the last trusted readout stands
  (`SelectionOrigin::Remembered`). It is kept in memory and in
  `%LOCALAPPDATA%\CompanyIsland\state\calendar_selection.json`. That file holds hashed ids and a
  checked flag per Outlook profile, no names and no events. It only matters after an island
  restart while Outlook is on Mail.
- **Nothing known yet:** only the primary calendar (`primaryOnly`). The sources list asks the
  employee to open the calendar in Outlook once.

The profile's default calendar is **always** active, even when it is unchecked in Outlook. This
keeps the existing personal-calendar behaviour and its reminders unchanged.

The island sets `IsSelected` only for a switch the employee turned in the island, and only while
Outlook shows its checkboxes (below). It never switches module or view and never activates Outlook.

## Switching calendars from the island

Since 1.0.9 every calendar in the sources list except the default one has a switch. A switch is a
request to Outlook, not a second selection:

1. `outlook_set_calendar_selected(id, selected)` queues the request and asks for a sync. The id is
   a calendar id from the last report (16 hex). Requests for the default calendar or an unknown id
   are dropped.
2. The next regular sync puts it into the selection memory (`SelectionMemory::request`): it counts
   in the island at once, and is kept as *pending* (also in `calendar_selection.json`, so it
   survives a restart). The source reports `pendingInOutlook: true`, and the list says "Outlook
   follows when its calendar is open".
3. The first scan that sees Outlook's checkboxes (the trusted state above) sets
   `NavigationFolder.IsSelected` on that entry. That is exactly what the employee's own click on
   the checkbox does. The readout then replaces the memory and nothing is pending any more. If
   Outlook refuses (it keeps the last checked calendar checked) or the calendar is gone, Outlook's
   checkbox wins.

`IsSelected` is never set in any other state: outside the Calendar module it means "select and
display" and could move Outlook away from what the employee is looking at. The island still never
switches module or view and never activates Outlook. The page holds a switch it just turned until a
report reflects it, at most 15 s, and turns it back with a message if the request could not be sent.

## Live updates

`NavWatcher` advises `NavigationGroupsEvents_12` on the active explorer's calendar groups and
`ExplorerEvents_10` on that explorer. Both subscriptions are needed, or neither is kept.

- The sink is a hand-built IDispatch (`com.rs`, `EventSink`). It answers QueryInterface for the
  dispinterface IID, which a connection point asks for. Callbacks only set a flag.
- The STA worker checks the flag after dispatching messages, outside Outlook's callback. A change
  sends `NavChanged` to the supervisor, which reads again right away, rate-limited to one read
  per 2 s like a manual refresh.
- Reconciliation: every regular sync (60 s) rescans the pane anyway, so a missed event costs at
  most a minute.
- Explorer `Close` releases the watcher: Unadvise, then the explorer and groups are released.
  This also happens on a disconnect, on a dead explorer (Outlook restarted, checked each sync),
  on a profile change, and when the worker stops. While the watcher exists, it holds only that
  explorer and its calendar NavigationGroups.

## Reading events

Per active calendar, the existing reader runs: `Items.Sort("[Start]")`, then
`IncludeRecurrences = True`, then `Restrict("[End] >= from AND [Start] <= to")`. That order is
documented. Outlook then expands recurring series into occurrences, including modified and
deleted ones. `Count` is never used, and every item is re-checked against the window.

- **Primary calendar:** read on every sync (60 s). A failure fails the sync, as before.
- **Other active calendars:** read when newly checked, then every 5 minutes. In between, their
  last events are reused. Unchecking a calendar drops its cache, so checking it again reads it
  fresh.
- **Time budget:** after 6 s of a sync, no further calendar read starts. The calendars left over
  keep their last events, are marked `pending` (CAL-SHARED-105) and are read next time. The
  watchdog is 10 s.
- **Quarantine:** if a read hangs past the watchdog while a shared calendar is being read, that
  calendar is skipped for 15 minutes (CAL-SHARED-104). The employee's own calendar is unaffected.
- **Items read:** the same properties as before (no Body, no Inbox). `GlobalAppointmentID` is
  added only while more than one calendar is active.
- **Limited permission:** with free/busy-only access Outlook usually cannot open the folder.
  Such a calendar shows as unavailable (CAL-SHARED-101), not as an error of the island. With
  "titles and locations" access, a missing subject is shown as "(No subject)".

## Duplicate meetings

The same meeting can appear in the employee's calendar and in a shared calendar. Two events are
treated as one meeting only when all of the following agree:

- `GlobalAppointmentID` (the same in every copy of a meeting, per Microsoft Learn; hashed, never
  sent to the page);
- start;
- end.

The first copy in source order wins: primary first, then Outlook's pane order. A meeting
therefore keeps its event id, and so its reminder key, from one sync to the next. Events without
a GlobalAppointmentID are never merged. Occurrences of one series share the id but not the
times, so they stay separate. The subject is never used as an identity.

## Failure isolation and support codes

One calendar's problem never fails the others. Raw HRESULTs go to the local log only. The UI
and copied diagnostics show these codes:

| Code | Meaning |
|---|---|
| CAL-SHARED-101 | No permission (any more): access denied |
| CAL-SHARED-102 | The folder is gone, or its reference is no longer valid |
| CAL-SHARED-103 | Its mailbox or server cannot be reached (offline, not synchronized yet) |
| CAL-SHARED-104 | It hung a read and is skipped for 15 minutes |
| CAL-SHARED-105 | Not read in this sync (time budget); its last events are shown |
| CAL-SHARED-106 | The navigation pane entry cannot be opened |
| CAL-SHARED-109 | Any other read failure |

Outlook-level failures (busy, disconnected) keep the existing OUTLOOK-1xx handling for the whole
read. An access denied on any calendar other than the primary one (E_ACCESSDENIED is also
MAPI_E_NO_ACCESS, a calendar the user may not or no longer open) is that calendar's
CAL-SHARED-101 or CAL-SHARED-106, never OUTLOOK-110: in a large organization the pane lists
such calendars routinely. The pane's folders are opened once and kept for 10 minutes; a scan
opens new ones for at most 3 s and leaves the rest to the next syncs (not trusted for the
checkboxes until it is complete).

## Reminders

- Reminder keys stay `<eventId>|<startUtc>|<type>`. Because duplicates are removed before
  reminders and the winning copy is stable, one meeting reminds once even if three calendars
  show it. Adding the calendar id to the key would instead remind once per calendar.
- Alerts carry `calendarId`, `calendarName` and `sourceKind`. The reminder shows the calendar
  name after the countdown ("Meeting in 30 minutes · Team Schedule").
- **Policy** (`ReminderSourcePolicy`, separate from discovery):
  - `all`: the default, every active calendar.
  - `own`: primary and personal calendars only.
  - `calendars`: a list of calendar ids, kept for a later per-calendar setting.

  The setting "Reminders from shared calendars" (`sharedCalendarReminders`) switches between
  `all` and `own`. Shared events stay visible either way.
- **No storms:** at most 3 reminders are shown for one moment, own calendars first. The rest are
  marked fired and remain in the calendar list. The island's alert queue still shows them one
  after another without re-expanding in between.

## Privacy, users and sessions

- Discovery runs only inside the current Windows session, against the classic Outlook process
  of the same user SID and elevation. This is the existing process discovery. There is no
  global or cross-session COM connection.
- Calendar names and events live in memory only. The one file written is the hashed selection
  memory, in the user's own `%LOCALAPPDATA%`, keyed by a hash of the Outlook profile name.
- The log carries counts, hashed calendar ids, group types and codes. It never carries names
  or subjects.
- The copied diagnostics list calendars by number and type ("Calendar source #2: type=shared
  …"), never by name.

## Outlook closed or restarted

- The last events and the last discovery stay on screen. The status line says what is wrong
  ("Waiting for Outlook") and "Last updated HH:MM", so stale data never looks fresh.
- A new Outlook process gets a new attach, a new scan of the pane and a new watcher. Objects
  from the previous process are dropped: dead-explorer check, disconnect handling, or the
  worker being replaced.
- Nothing of Outlook's is held while its main window is closed, so Outlook can exit normally.

## Test matrix

Unit tests (cargo / vitest) cover:
- group and kind mapping
- the IsSelected trust rules and the selection memory (per profile, tamper-proof)
- the event sink's COM contract
- dedup: same meeting, occurrences, attendee copies, no-id appointments
- error-code mapping
- the reminder policy and the burst limit
- source labels, including long Hebrew names (ellipsis plus tooltip, bidi-isolated)
- the sources list
- the "last updated" line
- anonymized diagnostics

The live test `cargo test --lib outlook_live -- --ignored --nocapture` runs read-only against the
running Outlook. It reports the discovery and the listener state, then checks 200
attach/read/release cycles for handle leaks. Set `CI_LIVE_WATCH_SECS=60` and click calendars in
Outlook meanwhile to see SelectedChange arrive.

| # | Case | How | Status (2026-10-07) |
|---|---|---|---|
| 1 | Personal calendar only | live test | ✓ (groups=3, primary active, listener active, no handle growth) |
| 2–3 | Personal + 1 / + 5 shared | unit (read path, dedup), manual | needs an Exchange profile with shared calendars |
| 4–7 | Checked / unchecked / check and uncheck live | unit (selection rules), live watch | manual click test pending |
| 8–9 | Calendar → Mail → Calendar | unit (remembered selection) | manual pending |
| 10–11 | Close / restart Outlook | existing machine tests + watcher release | manual pending |
| 12–13 | Permission removed / calendar unavailable | unit (codes), isolation in the read loop | needs Exchange |
| 14 | Recurring shared event | same Restrict path as the primary calendar (existing tests) | needs Exchange |
| 15 | All-day shared event | existing all-day handling ("All day") | ✓ unit |
| 16 | Duplicate meeting in two calendars | unit | ✓ |
| 17 | Long Hebrew calendar name | unit (ellipsis, `<bdi dir="rtl">`, tooltip) | ✓ |
| 18–19 | English / Hebrew Outlook | GroupType enum, no name comparison | ✓ by design; Hebrew Outlook manual pending |
| 20–21 | Windows 10 21H2 / Windows 11 | plain COM and IDispatch, nothing new in the OS API surface | Windows 11 ✓ live; Windows 10 pending |
| 22 | Standard user | no elevation, per-user files | ✓ (dev machine runs non-elevated) |
| 23–24 | Another user / fast user switching | existing SID + session discovery; per-user state | ✓ unit (classify) |
| 25 | Cached mode, network off | CAL-SHARED-103 per calendar, primary unaffected | needs Exchange |
