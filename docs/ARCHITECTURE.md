# Yuval - architecture

Final architecture of the enterprise build (Tauri 2 + React + Rust, Windows 10 21H2+ / 11, x64, standard user,
offline). It summarises the code as built; the contract it was built against is
[ENTERPRISE_DESIGN.md](ENTERPRISE_DESIGN.md), deployment is [DEPLOYMENT.md](DEPLOYMENT.md), testing is
[QA_MATRIX.md](QA_MATRIX.md).

Principles: no network; no admin; no HKLM writes at run time; attach to Outlook, never launch it; read the
minimum of each meeting; no meeting content outside memory and the screen; every failure is a status or a
code, never a crash; the UI talks to a provider interface, not to Outlook.

## 1. Layers

```
+----------------------------------------------------------------------------------------------+
| UI  (React 18, WebView2; src/components/Pill, src/hooks)                                     |
|   PillShell > CompactIsland | ExpandedIsland (tabs: CALENDAR, NOTIFICATIONS, NOTES,           |
|   ABOUT, SETTINGS)                                                                           |
|   MeetingAlert | NotificationToast | ContextMenu   RTL layout, tokens: src/design (DESIGN_SYSTEM) |
+--------------+---------------------------+---------------------------+-----------------------+
               | view = selectView(state)  | t(), Intl dates           | settings, system info
+--------------v-------------+  +----------v-------------+  +----------v-----------------------+
| Island State Manager       |  | Localization           |  | Settings (Rust SettingsStore)     |
| src/lib/island/state.ts    |  | src/lib/i18n.ts        |  | settings.json, validated, atomic  |
| pure reducer + priority    |  | src/lib/dateFormat.ts  |  | `settings-changed` event          |
| useIslandState: timers     |  | en + he, Intl, RTL     |  +-----------------------------------+
+--------------^-------------+  +------------------------+
               | ALERT_SHOW / NOTIFICATION_SHOW
+--------------+----------------+            +-----------------------------------------------+
| Reminder Engine               |  events    | Notification Provider (Rust notifications.rs)  |
| src/lib/reminders/engine.ts   |<-----------+  UserNotificationListener: events or poller   |
| ReminderStore -> state\       |   (via     |  `notification-received`, `notification-status`|
|   reminders.json (Rust)       |  CalendarService)  +----------------------------------------+
+--------------^----------------+
               | CalendarSnapshot (merged, normalised)
+--------------+----------------------------------------------------------------------------------+
| CalendarService  src/lib/calendar/service.ts      (owns provider lifecycle, merges snapshots)      |
|   CalendarProvider interface (provider.ts)  <-- registry.ts: registerCalendarProvider(id, factory)  |
|   +-- ClassicOutlookCalendarProvider (classicOutlook.ts)  = IPC: calendar_get_snapshot,            |
|   |      calendar_refresh, event `calendar-snapshot`                                                |
|   +-- (future) MicrosoftGraphCalendarProvider                                                       |
+--------------+----------------------------------------------------------------------------------+
               | Tauri IPC (commands + events; CSP: self + ipc only)
+--------------v----------------------------------------------------------------------------------+
| Rust backend  src-tauri/src                                                                       |
|  calendar.rs   supervisor thread + pure Machine (status, backoff, resume, midnight, watchdog)     |
|       |  jobs / replies (channel)                                                                 |
|  outlook.rs    STA worker thread: attach (ROT) -> read -> release; discovery by session + SID     |
|  com.rs        late-bound IDispatch, STA apartment, message filter, HRESULT classifiers           |
|  system.rs     computer / user / OS / WebView2 / local IPv4 (event-driven cache)                   |
|  diagnostics.rs + calendar_diag.rs   privacy-safe snapshot and recent error-code ring (10)         |
|  debug_log.rs  rotating log, scrubbing, panic hook, clean-exit marker                              |
|  paths.rs, settings.rs, reminder_state.rs, autostart.rs, notes.rs          (per-user persistence) |
|  window.rs, monitors.rs, fullscreen.rs, tray.rs, clipboard.rs                    (Win32 shell)     |
|  center.rs     open the Yuval Center (navigate event, or start center\Yuval.Center.exe)   |
|  center_ipc.rs named-pipe server for the Center (tokio tasks; DACL, session check, JSON lines)    |
+----------------------------^---------------------------------------------------------------------+
                             | \\.\pipe\CompanyIsland.Center.<session>.<sidhash>  (local, this user only)
+----------------------------v---------------------------------------------------------------------+
| Yuval Center  center/  (WinUI 3, C# / .NET 10, a second process: <install>\center\*.exe)         |
|   CompanyIsland.Center.Core: IslandClient (hello, requests by id, events, reconnect), models,      |
|   note helpers, Hebrew strings          Pages: Welcome | Settings | Notes | Tour (WebView2)         |
|   Tour page = tour.html (React, mock data, no IPC) served from center\web\ by a virtual host       |
+--------------------------------------------------------------------------------------------------+
```

| Layer | Responsibility | Key files |
|---|---|---|
| UI | Renders the island; pointer hover/leave intent; announces to screen readers; never calls Outlook or `invoke` directly | `src/components/Pill/*`, `src/hooks/*` |
| Island State Manager | Decides what the island shows by priority; remembers the user's expanded tab; queues alerts | `src/lib/island/state.ts`, `src/hooks/useIslandState.ts`, `timing.ts` |
| Localization | UI pinned to Hebrew at startup (`setFixedLocale("he")` in `main.tsx`, whatever the Windows language); plurals; date words (weekday/month names from fixed Hebrew tables, "tomorrow", "in 5 min") in Hebrew; numbers, their order and the 12/24-hour clock from the Windows regional format (`get_format_locale`); locale-correct short date | `src/lib/i18n.ts`, `src/lib/dateFormat.ts`, `src/main.tsx` |
| CalendarProvider | Source-agnostic calendar contract and merge | `src/lib/calendar/*` |
| ClassicOutlookCalendarProvider | Frontend half of the Outlook source: initial read, pushed snapshots, throttled refresh (5 s) | `src/lib/calendar/classicOutlook.ts` |
| Outlook worker (Rust) | The only code that talks to Outlook (late-bound COM on its own STA thread): events with their category color, unread meeting requests (when the setting is on); plus `open_calendar` on an invite click (own short-lived STA thread) | `src-tauri/src/outlook.rs`, `com.rs`, `calendar.rs` |
| Reminder Engine | Pure scheduler: fire-once reminders from events + settings; persisted fired set | `src/lib/reminders/*` |
| System Info | Machine/user/OS/WebView2/IPv4 for About and diagnostics | `src-tauri/src/system.rs`, `src/hooks/useSystemInfo.ts` |
| Notification Provider | Mirror Windows toasts (read-only), status as an enum; new Outlook meeting requests from the calendar snapshot become island notifications too (once per session, only those received after start) | `src-tauri/src/notifications.rs`, `src/hooks/useNotifications.ts`, `useMeetingInvites.ts` |
| Diagnostics | Privacy-safe status + last 10 error codes; "Copy diagnostics" text | `src-tauri/src/diagnostics.rs`, `src/lib/diagnostics.ts` |
| Logging | One rotating file, one line per entry, scrubbed; UI forwards through `write_logs` | `src-tauri/src/debug_log.rs`, `src/lib/debugLog.ts`, `logger.ts` |
| Settings | Typed, validated, schema-versioned store; autostart opt-out | `src-tauri/src/settings.rs`, `autostart.rs`, `src/hooks/useSettings.ts` |
| Notes | The island's Notes tab (list, pin, copy, delete) over a store with optimistic updates; Rust sanitises and is the only writer of `state\notes.json`; both the island's page and the Center save through the same `notes::save` | `src-tauri/src/notes.rs`, `src/lib/notes/store.ts`, `src/hooks/useNotes.ts`, `src/components/Pill/panels/NotesTab.tsx` |
| Center bridge (Rust) | Opens the Center (navigate event to a connected one, else starts the exe) and serves it over a named pipe: commands in, `settings-changed` / `notes-changed` / `navigate` out | `src-tauri/src/center.rs`, `center_ipc.rs` |
| Yuval Center | A WinUI 3 app (second process): Welcome, Settings, Notes, Tour pages; a client of the pipe only, it never reads or writes the island's files | `center/CompanyIsland.Center`, `center/CompanyIsland.Center.Core` |
| Tour | The 12-step guided tour: the island's real components with mock data, no IPC, shown in the Center's locked-down WebView2 | `tour.html`, `src/tour/*`, `center/.../Pages/TourPage.xaml.cs` |

### 1.1 Why in-process Rust COM and not a sidecar

Outlook is read through late-bound `IDispatch` from Rust, on one dedicated STA thread, in the same process as the
island. A separate .NET (VSTO / interop) sidecar was considered and rejected:

- No extra runtime: a sidecar needs the .NET runtime or a self-contained copy (tens of MiB), which IT must
  patch and allow-list. The installer carries one binary plus WebView2.
- One executable to sign, allow in AppLocker/WDAC and uninstall; no IPC channel between two processes that
  could be spoofed, restarted out of order or left running.
- The risky part is the COM call, not the language. It is contained the same way in either design: a
  watchdog (10 s) abandons a hung call and starts a fresh STA worker; attach-only discovery by session, SID
  and elevation; every object released after each read; backoff on failure. Those are unit-tested with a fake
  source (section 4.1).
- Late binding needs no Outlook primary interop assembly and no Office version pin, so Office 2016 to
  Microsoft 365 builds, 32 and 64-bit, use the same code path.
- Trade-off accepted: a COM fault that takes the process down would take the island with it. The worker
  runs under panic catching and a watchdog, and a sidecar would only move that crash to another process.

### 1.2 The Yuval Center is not that sidecar

Since 1.0.4 the product does ship a .NET program, and the reasoning above still holds because it is a different
thing: a user-facing window, not a bridge to Outlook.

- **Why a second process at all**: the island is non-activating and has no text input by design (ENTERPRISE_DESIGN
  section 0, Focus). Notes, a settings page and a first-run explanation need a normal window with a keyboard.
  The user chose a real WinUI 3 app (2026-10-06) over putting a text box into the island.
- **What it may do**: ask the island for settings and notes and change them, ask it to open the island on a tab,
  report a code, all over one local pipe (protocol in ENTERPRISE_DESIGN section 1, "Yuval Center pipe"). It
  never touches Outlook, the calendar, Windows notifications, `settings.json` or `notes.json`, and makes no
  network connection.
- **If it is missing or dies**: the island is unaffected (`APP-030` when an open fails, `APP-031` when the pipe
  cannot be served). Nothing in the island waits for the Center.
- **Cost**: about 86 MiB more in the install folder (self-contained .NET 10 + Windows App SDK 1.8, trimmed),
  one more exe and a folder of DLLs to allow-list (DEPLOYMENT section 11), and the installer must close it
  before it replaces files (`installer-hooks.nsh`).

## 2. Thread model

| Thread | Created by | Runs | Notes |
|---|---|---|---|
| UI / main (tao event loop, WebView2 UI) | process | Window creation and every window mutation (`run_on_main_thread`), tray, event dispatch | Never blocked by Outlook or WinRT: slow commands run elsewhere |
| Tokio blocking pool | Tauri `async_runtime` | `rt::run_blocking(...)`: settings, system info, diagnostics, clipboard, notification status, log-folder open | A command taking over 400 ms logs a `slow backend command` warning |
| `companyisland-calendar` (supervisor + **watchdog**) | `calendar::start` | Owns the `Machine`; cheap process discovery; hands reads to the worker; waits at most 10 s for a reply (`WATCHDOG_MS`); wakes at least every 30 s to notice sleep/midnight | Pure scheduling logic; no COM here |
| `companyisland-outlook-<generation>` (STA worker) | supervisor, on demand | `ComApartment::init_sta`, message filter, pumps messages; per job: attach -> read -> release | One worker at a time. A hung worker is **abandoned** (not killed) and replaced; if it ever returns it releases its COM objects and exits. Dropped when Outlook is not running |
| `companyisland-outlook-respond` (STA, short-lived) | `outlook_respond_invite` (Accept / Maybe / Decline on an invitation) | Attach, `GetItemFromID`, `Respond`, `Send` | Waits at most 60 s (an Outlook security prompt may be on screen) |
| `companyisland-outlook-open` (STA, short-lived) | `outlook_open_calendar` (a click on a meeting invitation) | Attach, switch the active explorer to the calendar, `Activate`, `GoToDate` | The command waits at most 10 s (`OUTLOOK-109`); a fresh thread because blocking-pool threads may already be MTA |
| `companyisland-fullscreen` | `fullscreen::start` (from a startup thread) | Message loop + out-of-context WinEvent hooks (foreground, minimize, location of the foreground window); 250 ms debounce; `SHQueryUserNotificationState` + geometry | Emits `fullscreen-changed`; hides/restores the window only if it hid it. Forwards every foreground change at once (not debounced) as `foreground-changed`, so a click on the desktop, taskbar or another app closes the island |
| Notification threads | `notifications::start` / `spawn_sync` | `companyisland-notif-init` / `-sync` (short-lived): query access, subscribe or start the poller | `NotificationChanged` handlers run on WinRT callback threads and are wrapped in `catch` |
| `companyisland-notif-poll` | `notifications` when `NotificationChanged` cannot be subscribed (typical for an unpackaged exe) | Reads the Action Center every 5 s (30 s after 3 failures), forwards only unseen ids; stops through a channel | Reports `notificationMode = polling` |
| `startup` thread | `lib.rs` setup | `settings::sync_autostart` (registry) then `fullscreen::start` | Keeps registry work off the UI thread |
| Center pipe tasks (tokio runtime, `tauri::async_runtime`) | `center_ipc::start` from setup | One accept task (`accept_loop`) creating pipe instances (at most 4), and per client one task reading lines and one writing them (queue of 256 lines; a client that falls behind is dropped) | Async only: file and Win32 work (settings, notes, monitors, log folder, `showIsland`) is handed to the blocking pool, never run on the reactor. Requests of one connection run one after another. A creation failure (name taken) logs `APP-031` and ends the task; the island carries on |
| `companyisland-onboarding` | `lib.rs` setup, only while `onboardingDone` is false | Sleeps 1.5 s, `center::open("welcome")`, on success `mark_onboarding_done` | Short-lived. A failed open is logged and retried at the next start |
| Center process (separate exe) | `center::open` (`Command::spawn`, absolute path, not waited on) or the user | WinUI 3 UI thread (STA, `DispatcherQueue`); a pool task for the pipe client and its 3 s reconnect loop; WebView2 processes only while the Tour page has been shown | One per user session (`AppInstance.FindOrRegisterForKey`): a second launch redirects its arguments to the first on its own thread and exits. Its WebView2 uses `EBWebView-Center`, not the island's profile |
| Network change callbacks | Windows (`NotifyIpInterfaceChange`, `NotifyUnicastIpAddressChange`) | Invalidate the cached IPv4 selection | Nothing polls |
| Webview JS thread | WebView2 | React, reminder engine timer (one timer, <= 60 s sleeps), clock store (minute aligned) | No per-second timers while collapsed and idle |

Every thread entry and native callback is wrapped in `debug_log::catch`, which logs `APP-001` instead of
unwinding across the boundary. Panics are also logged with a (scrubbed) backtrace by the panic hook.

## 3. Data flow: the 30-minute reminder

1. **Discovery** (supervisor, every 15 s while idle or connected): `outlook::discover` lists `OUTLOOK.EXE` /
   `olk.exe` processes and keeps only those in our session and owned by our SID; classifies as
   `Waiting`, `NewOutlookOnly`, `ElevationMismatch` or `Classic(pid)`.
2. **Read** (every 60 s while connected, or on refresh): the supervisor sends a `FetchWindow` (now .. +48 h)
   to the STA worker. The worker calls `GetActiveObject("Outlook.Application")`, `Session.GetDefaultFolder(9)`,
   sorts `Items` by `[Start]`, sets `IncludeRecurrences`, `Restrict`s with a locale-formatted filter (falls
   back to US format if empty or implausible), reads the allow-listed properties per item, builds
   `CalendarEventDto`s (id = first 16 hex of sha256(EntryID + "|" + start); `color` from the first of the
   item's categories that has one, looked up in the master category list re-read every 5 min). When
   `meetingInvitesEnabled` is on it also reads the newest unread meeting requests of the default Inbox (a
   date-free `Restrict`; a failure here never fails the sync). Then it releases every object.
3. **State**: `Machine::on_fetch_ok` stores the events (sorted, de-duplicated, max 50), status `connected`,
   next read in 60 s. `publish` stores the snapshot and emits `calendar-snapshot` if it changed; events that
   already ended are filtered out of what is published.
4. **Frontend provider**: `ClassicOutlookCalendarProvider` normalises the payload (`normalizeSnapshot`),
   keeps the events array identity when only the sync time changed, and notifies `CalendarService`, which
   merges providers (`mergeSnapshots`).
5. **Reminder Engine**: `useReminders` calls `engine.update(events, { enabled, offsetsMinutes: [reminderMinutes] })`.
   For each real meeting (timed, not free, not declined, not started, not ended) it computes
   `due = start - 30 min`, key `eventId|startUtc|minutes-30`, skips keys already in the fired set, and arms
   **one** timer for the earliest due instant (at most 60 s ahead, so a resume from sleep is noticed).
6. **Fire**: at the due time the key is added to the in-memory fired set first, then persisted
   (`reminder_state_save` -> `state\reminders.json`, entries older than 7 days dropped), then
   `onFire(alert)`. Late reminders (PC was asleep, Outlook attached late): fire with the real minutes
   remaining if the meeting has not started and at least 1 minute is left, otherwise mark skipped.
7. **Island**: `showAlert` dispatches `ALERT_SHOW`; the reducer makes `meetingAlert` the view (priority 3,
   preempting a toast, queuing behind another alert); the island's shape springs to the alert's size inside
   the fixed stage window, and the window region follows through the ordered geometry queue
   (`set_island_geometry`); the alert counts down 8 s (paused while hovered or while a
   fullscreen app hides the island) and then `ALERT_DONE` restores whatever the user had open.
8. **After a restart**: the fired set is loaded before anything is scheduled, so nothing fires twice.

Meeting text only travels Outlook -> worker memory -> IPC event -> React state -> screen. It is never written
to disk and never logged.

## 4. State machines

### 4.1 Calendar status (`src-tauri/src/calendar.rs`, `Machine`)

Statuses: `waiting`, `connecting`, `connected`, `newOutlookOnly`, `elevationMismatch`, `unresponsive`, `failed`.

| Current | Input | Next | Code | Next action |
|---|---|---|---|---|
| any | discovery: no Outlook of ours | `waiting` | OUTLOOK-101 | poll in 15 s; worker dropped; cached events kept |
| any | discovery: only `olk.exe` | `newOutlookOnly` | OUTLOOK-104 | poll in 15 s |
| any | discovery: Outlook at another elevation or uninspectable | `elevationMismatch` | OUTLOOK-103 | poll in 15 s |
| waiting / newOutlookOnly / elevationMismatch | discovery: `Classic(pid)` | `connecting` | cleared | fetch now |
| failed / unresponsive | discovery: `Classic(other pid)` (Outlook restarted) | `connecting` | cleared | backoff and abandoned count reset, fetch now |
| any | process list unreadable | `failed` | OUTLOOK-102 | failure backoff |
| connecting / connected | fetch ok | `connected` | cleared | re-read in 60 s, discover every 15 s |
| connecting / connected | not in ROT or disconnected, attempts <= 8 | `connecting` | cleared | retry after 1, 2, 4, 8, 16, 30, 30, 30 s |
| connecting / connected | not in ROT or disconnected, attempts > 8 | `failed` | OUTLOOK-102 | failure backoff |
| connecting / connected | busy (call rejected), streak < 3 | unchanged (`connected` stays `connected`) | OUTLOOK-105 | failure backoff |
| connecting / connected | busy, streak >= 3 | `unresponsive` | OUTLOOK-105 | failure backoff |
| any | blocked on the primary calendar (E_ACCESSDENIED, 0x800A....) | `failed` | OUTLOOK-110 | failure backoff |
| any | other read error | `failed` | the stage code: OUTLOOK-106 / 107 / 108 / 102 | failure backoff |
| any | no answer within 10 s (watchdog), abandoned < 5 | `unresponsive` | OUTLOOK-109 | worker abandoned, fresh one next time |
| any | watchdog, abandoned = 5 | `failed` | OUTLOOK-109 | at least 5 min before the next try; refresh is ignored |
| any | wall clock jumped ahead of the monotonic clock (resume) | unchanged | - | counters reset, rediscover at once |
| connected | local date changed | unchanged | - | one-shot re-read |
| any | user refresh (expanding the island, throttled to 5 s in the UI and 2 s in Rust) | unchanged | - | try now unless abandoned >= 5 |

Failure backoff: 5, 10, 20, 40, 60, 120, 300 s (then 300 s), each +-20% jitter. A successful read resets all
counters.

### 4.2 Island priority (`src/lib/island/state.ts`)

Priority: `meetingAlert (3) > notification (2) > userExpanded (1) > idle (0)`. The user layer (expanded, tab,
pinned) is never modified by temporary states; they sit on top of it.

| Showing now | `ALERT_SHOW` | `NOTIFICATION_SHOW` | `USER_EXPAND` / `PIN` | `USER_COLLAPSE` |
|---|---|---|---|---|
| idle | meetingAlert | notification | userExpanded | idle |
| userExpanded | meetingAlert (expanded state kept underneath) | notification | userExpanded (tab updated) | idle |
| notification | meetingAlert (toast waits, shown afterwards if younger than 15 s) | replaces the toast | stays notification (user layer updated underneath) | user layer collapsed underneath |
| meetingAlert | queued (max 8, no duplicate keys) | held as "waiting" (latest only) | stays meetingAlert | user layer collapsed underneath |

| End event | Result |
|---|---|
| `ALERT_DONE` with queued alerts | next alert; no toast between two alerts |
| `ALERT_DONE`, queue empty | waiting toast if fresher than 15 s, else the user layer (expanded tab, or idle) |
| `NOTIFICATION_DONE` | back to the user layer |
| `TICK` | drops a waiting toast that went stale |

Durations (`src/lib/island/timing.ts`): alert 8 s, toast 4.5 s, hover intent 120 ms, leave grace 400 ms
once the pointer has been on the expanded island (clicked or not); 4 s for an island opened from the tray or a
second launch that the pointer has not reached yet. Another window becoming active (`foreground-changed`, i.e. a
click outside the island) collapses it at once unless the pointer is on it or it opened less than 700 ms ago.
Hover or a fullscreen-hidden island pauses the countdown.

### 4.3 Reminder decision (`src/lib/reminders/engine.ts`)

| Condition | Outcome |
|---|---|
| all-day, `busyStatus = free`, `responseStatus = declined`, already started, already ended | no reminder, **not** marked fired (accepting later still reminds) |
| due in the future | scheduled; one timer, <= 60 s sleeps |
| due now (within 5 s) | fire |
| overdue, meeting not started, >= 60 s left | fire late with real remaining minutes |
| overdue, < 60 s left | skipped and marked fired |
| key already fired | nothing (restart-safe) |
| reminders disabled in settings | nothing scheduled; picked up when enabled |

### 4.4 Island motion (`src/lib/island/islandMotion.ts`, `src/components/Pill/useIslandMotion.ts`)

One owner per animated property, one target, no timers in the choreography:

- **Shape** (width, height, corner radius): only the island motion engine writes it. The view decides the
  target; `setTarget` (in a layout effect of the commit that changed the view) retargets from the current
  size and velocity, last writer wins. Two closed-form springs (`islandSprings`, `spring.ts`) are stepped by
  motion's frame loop, so a late frame (32 Hz in an RDP session) lands exactly where it should. Each axis stops
  at its target instead of crossing it, never leaves [launch dot, stage], and every frame is checked
  (`frameViolations`, logged once per transition). The radius is derived from how far the size has travelled.
- **Native window**: a fixed stage (`stageSize()`, the largest island within the monitor's limits, plus the
  8 DIP top inset; it starts at the monitor's top, the island is drawn 8 DIP down, and the region adds an
  island-wide bridge up to the screen edge) placed once; it is never resized or
  moved during a morph (a resized WebView2 window shows its previous frame at the new size, measured as a
  ~12 px sliver at the end of every close, and stalls its frame pipeline ~110 ms). Only the window region
  follows the island's resting shape: grown at once, shrunk once the animated shape fits inside it.
- **Content**: every layer is laid out at its final size, centred, riding the shape's vertical centre; its
  opacity is a function of the shape's progress (`layerFade`). The expanded island's parts ride their own
  edges (`IslandPart`: header with the top-left corner, body in the middle, dock with the bottom edge) and
  fade on `partFade`.
- **Tabs**: one capsule (a spring on the slot index that never swings past its slot) and one content progress
  spring, started in the same frame; the content's cross-fade (`layerFade.tab`) follows the progress, which is
  slower than the capsule, so content never runs ahead of the selection.
- Diagnostics: set `window.__ISLAND_TRACE__ = []` (DevTools) to record every frame of the shape
  (time, transition, size, radius, velocity, progress, settled, target).

## 5. File map

```
src/
  App.tsx, main.tsx, index.css
  components/CrashBoundary.tsx
  components/Pill/        PillShell, CompactIsland, ExpandedIsland, MeetingAlert, NotificationToast,
                          ContextMenu, TabDock, TabBoundary, tabs.ts, animations.ts, alertLayout.ts,
                          usePillGeometry.ts, useCompactLayout.ts, panels/{CalendarTab,DayTimeline,WeekStrip,NotificationsTab,NotesTab,AboutTab,SettingsTab},
                          RingerPill, IslandLayer (+ IslandPart), drivenTransition.ts, useIslandMotion.ts,
                          ui/{meetingActions,eventColor}
  hooks/                  useIslandState, usePillState, useIslandEvents, useCalendar, useReminders,
                          useSettings, useSystemInfo, useNotifications, useMeetingInvites, useMeetingSilence, useClock, useNotes,
                          useScreenReader, useDesktopGestures, useCrashRecovery
  lib/appInfo.ts          product constants (name, identifier, version)
  lib/i18n.ts             string tables en/he, t(), detectLocale, isRtl, plurals, setFixedLocale/getWordTag
  lib/dateFormat.ts       cached Intl formatters, Hebrew weekday/month tables, minute/day helpers
  lib/ipc.ts, tauri.ts    typed wrappers: commands + events (non-global @tauri-apps/api; null outside Tauri)
  lib/errors.ts, logger.ts, debugLog.ts, diagnostics.ts   code extraction, log forwarding, diagnostics text
  lib/calendar/           provider.ts (interface), registry.ts, service.ts, classicOutlook.ts,
                          normalize.ts, select.ts, types.ts, dayRange.ts (on-demand days), meetingStatus.ts,
                          inviteAnswers.ts
  lib/reminders/          engine.ts, store.ts, types.ts
  lib/island/             state.ts, timing.ts, geometryQueue.ts, compactLayout.ts, morph.ts, silence.ts,
                          spring.ts (closed-form spring), islandMotion.ts (the one owner of the island's shape)
  lib/notifications/      history.ts (this session's notifications, memory only)
  lib/notes/              store.ts (createNotesStore, sortNotes, newNoteId), store.test.ts,
                          sticky.ts (Windows Sticky Notes: wire types, normaliser, 10 s store), stickyColour.ts, useSticky.ts
  tour/                   main.tsx, TourApp.tsx, TourStage.tsx, steps.tsx, crossFade.tsx, compact.ts, mockData.ts,
                          host.ts (messages to the Center's WebView2), params.ts, tour.css, tour.test.tsx
tour.html                 second Vite page (CSP meta, connect-src 'none'); built into dist/ next to index.html
center/                   Yuval Center (WinUI 3), versioned by center/Directory.Build.props
  CompanyIsland.Center/       WinExe: Program.cs (single instance), App, MainWindow (navigation, title bar, disconnected bar),
                              Pages/{Welcome,Settings,Notes,Tour}Page, Services/{CenterModel,NoteViewModel}, Theme/CenterTheme.xaml,
                              Controls/SettingRow, app.manifest (asInvoker, PerMonitorV2), Assets/icon.ico
  CompanyIsland.Center.Core/  pipe name, IslandClient, models + source-generated JSON context, NoteOps, CenterPage, Strings (Hebrew)
  CompanyIsland.Center.Tests/ xunit: Core and IslandClient against an in-process fake pipe server
  publish/                    build output (gitignored): the folder installed as <install>\center\
src-tauri/
  tauri.conf.json         product, window (transparent, no decorations, non-focusable), CSP, NSIS bundle
  windows-app.manifest    asInvoker, PerMonitorV2, Common-Controls v6, long paths
  tauri.installer.conf.json   merged only by `npm run build:installer`: beforeBuildCommand builds the web app and the Center, bundle.resources maps center/publish to <install>\center
  installer-hooks.nsh     OS gate (1603), HKLM Run value, pre-install and pre-uninstall close of the island and of Yuval.Center.exe
  capabilities/default.json   core:event:allow-listen / allow-unlisten only
  .cargo/config.toml      +crt-static
  src/lib.rs              builder, plugins (single-instance, core plugin: log + state), command table, WebView2 args
  src/calendar.rs         contract types, Machine, supervisor/watchdog, commands
  src/outlook.rs          discovery, COM source, STA worker
  src/com.rs              IDispatch wrapper, apartment, message filter, HRESULT classes
  src/calendar_diag.rs, diagnostics.rs
  src/notifications.rs, system.rs, monitors.rs, window.rs, fullscreen.rs, tray.rs, clipboard.rs
  src/paths.rs, settings.rs, reminder_state.rs, autostart.rs, debug_log.rs, rt.rs
  src/notes.rs            notes.json: sanitize, canonical order, load / save, commands notes_load / notes_save
  src/sticky_notes.rs     Windows Sticky Notes, read only: winsqlite3 loaded dynamically, plum.sqlite copy, cache, commands sticky_notes_list / sticky_notes_open (section 11)
  src/center.rs           valid_page, open (navigate or spawn), command open_center
  src/center_ipc.rs       pipe name + DACL, accept loop, per-connection protocol, Hub (broadcast, navigate)
docs/                     ENTERPRISE_DESIGN, INSTALLER, DEPLOYMENT, QA_MATRIX, ARCHITECTURE
scripts/                  check-versions.cjs (four versions), build-center.cjs, verify-installer.ps1, make-icon.ps1
```

IPC commands (registered in `lib.rs`): `write_logs`, `log_frontend_error`, `open_log_dir`, `get_settings`,
`update_settings`, `calendar_get_snapshot`, `calendar_refresh`, `reminder_state_load`, `reminder_state_save`,
`set_island_geometry`, `get_monitors`, `get_fullscreen_state`, `get_system_info`, `get_format_locale`, `get_diagnostics`,
`copy_text_to_clipboard`, `notifications_get_status`, `notifications_request_access`, `activate_notification`,
`activate_app_by_aumid`, `outlook_open_calendar`, `outlook_respond_invite`, `open_meeting_url`, `calendar_get_range`, `notes_load`, `notes_save`,
`open_center`, `sticky_notes_list`, `sticky_notes_open`. Events (Rust to JS): `calendar-snapshot`, `notification-received`,
`notification-status`, `settings-changed`, `fullscreen-changed`, `display-changed`, `island-toggle`, `foreground-changed`, `notes-changed`.

Pipe commands (Rust to the Center, not Tauri IPC; `center_ipc.rs`; details in ENTERPRISE_DESIGN section 1): `hello`,
`getSettings`, `updateSettings`, `notesLoad`, `notesSave`, `getMonitors`, `getNotificationStatus`,
`requestNotificationAccess`, `openLogDir`, `showIsland`, `log`. Pipe events: `settings-changed`, `notes-changed`
(every client that said hello), `navigate` (Center clients only). The page cannot call the pipe and the Center cannot
call Tauri IPC: they meet only in Rust (`settings::apply_patch`, `notes::save`, `window::show`).

## 6. Security and hardening summary

- Never elevated (`asInvoker`); no HKLM writes at run time; per-user data only.
- Capability file grants only event listen/unlisten; CSP is `default-src 'self'` with `connect-src` limited to
  the Tauri IPC; `window.__TAURI__` is not exposed (`withGlobalTauri: false`).
- No global mouse/keyboard hook, no raw-input registration (`DeviceEventFilter::Always`), no injection;
  fullscreen detection uses out-of-context WinEvent hooks.
- Outlook: attach-only (`GetActiveObject`), same session + SID + elevation, read-only, safe properties only,
  every object released after each read. The write-like calls, each only after a click in the island: showing the
  user's own Outlook on its calendar, answering an invitation, and checking or unchecking a calendar in Outlook's
  Calendar pane (only while that pane is on screen; see CALENDAR_SHARED.md). Showing Outlook uses `AllowSetForegroundWindow` for that Outlook PID only, never `ASFW_ANY`.
- Notifications: read-only (`RemoveNotification` is never called); activation of a toast's app is limited to
  AUMIDs the app has itself seen (allow-list of 64) and validated against shell metacharacters.
- External programs are started by absolute path (`explorer.exe` from the Windows known folder).
- WebView2 profile under `%LOCALAPPDATA%\Yuval\EBWebView`, `--disable-background-networking`.
- Center pipe: `\\.\pipe\CompanyIsland.Center.<session>.<sidhash>` is created with a protected DACL
  (`D:P(A;;GA;;;<user SID>)`: this user only, not Administrators), `PIPE_REJECT_REMOTE_CLIENTS`, a first-instance
  flag (a name that already exists means `APP-031` and no server, so a squatter cannot be served) and at most 4
  instances. Each connection must come from the island's own Windows session (`GetNamedPipeClientSessionId`),
  must say `hello` (protocol 1) before anything else, and is limited to 16 MiB per line. Only a fixed list of
  commands exists, arguments are validated, `showIsland` accepts only the five tab names, `log` messages are
  clipped to 300 characters. The client uses `PipeOptions.CurrentUserOnly`. Nothing is logged from request or
  response bodies.
- Foreground: the island's `AllowSetForegroundWindow` calls are for one named pid each (the user's Outlook after
  a click on an invitation; the Center's pid after a click on an island button or the tray item, or at the first
  run); never `ASFW_ANY`. `navigate` is only sent after that grant.
- Yuval Center process: `asInvoker`, started by absolute path (`<dir of Yuval.exe>\center\...`), no shell,
  no arguments except `--page <valid page>` (validated by `center::valid_page` before it is passed).
- Tour host (the Center's WebView2): the page's own CSP forbids every connection (`connect-src 'none'`); the host
  maps `tour.companyisland.invalid` to `center\web\` only, cancels any navigation to another origin, new windows,
  downloads and permission requests, turns off dev tools, the context menu, the status bar, zoom, browser
  accelerator keys, autofill and error pages, ignores page messages from another host, and accepts only
  `navigate` (to welcome, settings or notes) and `done`. Its profile (`EBWebView-Center`) is separate from the island's.

## 7. Settings

`settings.json` (schema version 1), camelCase, validated on load and on every patch:
`launchWithWindows` (true), `hideInFullscreen` (true), `meetingReminderEnabled` (true), `reminderMinutes`
(30, clamped 0-120; the UI offers 5/10/15/30), `monitorId` (null = primary; otherwise a zero-based index as a
string, missing monitor falls back to primary), `notificationsEnabled` (true), `meetingSilencePrompt` (true; the ring / silent pill at a meeting's start), `meetingInvitesEnabled` (true; off
means the Inbox is not read at all, picked up at the next sync), `debugLogging` (false), `onboardingDone` (false; set
to true by the island itself once the Center's Welcome page was opened at the first run, so an older file without the
key shows it once; the Center's Settings page has no control for it) and `islandDisplay` (`full` | `clock` | `date`,
default `full`, anything else becomes `full`; chosen in the island's Settings tab or the Center). A change (from the
island's page or from the Center over the pipe, which both end in `settings::apply_patch`) emits the Tauri event
`settings-changed` and the same pipe event, so each side redraws; `launchWithWindows` writes the HKCU StartupApproved
value; `debugLogging` switches the log level; `monitorId` re-places the island; `hideInFullscreen`
re-evaluates fullscreen; `notificationsEnabled` starts or stops delivery.

## 8. Error codes

Codes appear in the log, in the Calendar tab (for calendar statuses), in Settings > diagnostics and in "Copy
diagnostics" (last 10, repeats collapsed). Any WARN/ERROR log line containing a known code feeds the recent
list; `OUTLOOK-101` and the notification status codes are INFO conditions (the latter are recorded
explicitly). No code is fatal: the app keeps running in every case except a failure to start (`APP-001
failed to start`, process exits with code 1).

| Code | Meaning | Raised by | User-visible effect | Typical action |
|---|---|---|---|---|
| APP-001 | Unhandled panic caught, a background command task failed, UI render error in a tab, or the app failed to start | `debug_log::catch`, panic hook, `rt::run_blocking`, `TabBoundary`, `lib.rs` | A tab shows "Unavailable / Try again"; the rest keeps working | Collect the log; report |
| APP-002 | Settings, reminder state or notes could not be written / data folder unavailable (also: the notes file would exceed 16 MiB) | `settings.rs`, `reminder_state.rs`, `notes.rs` | Settings kept in memory only; reminders may repeat after a restart; a note change is reverted and the Notes tab / Center says it could not be saved | Check profile volume and permissions |
| APP-003 | Settings, reminder or notes file corrupt or unreadable | `settings.rs`, `reminder_state.rs`, `notes.rs` | Defaults / empty set / no notes; bad file kept as `*.corrupt` | Delete or inspect `*.corrupt` (`notes.json.corrupt` holds the user's notes: do not delete without asking) |
| APP-030 | Yuval Center missing (`center\Yuval.Center.exe` not next to the island) or could not be started; also the first-run thread could not start | `center.rs`, `lib.rs` | The Center button / tray item does nothing; the first-run Welcome is retried at the next start | Reinstall; check that application control allows `center\Yuval.Center.exe` |
| APP-031 | Center connection: the pipe could not be created (name taken), cannot listen again or accept, a connection was refused (another session), a client is too slow, a request line is too long, or a protocol error (`hello required`, unsupported protocol, unknown command, invalid arguments) | `center_ipc.rs` | The Center shows "the island is not running" or an error bar; the island is unaffected | Informational; collect the log if the Center never connects |
| APP-032 | `open_center` with a page name that is not `welcome`, `tour`, `settings`, `notes`, `notes-new` or `note:<valid id>` | `center.rs` | Nothing opens | Report (a bug, not a user error) |
| APP-020 | A Join click with a link the island did not find itself, or no handler could open it | `outlook.rs` (`open_meeting_url`) | Nothing opens | Check the default browser / Teams install |
| APP-010 | Log folder unavailable or cannot be opened | `debug_log.rs`, `paths.rs` | No log file; "Open logs" fails | Check `%LOCALAPPDATA%\Yuval\logs` |
| OUTLOOK-101 | Outlook not running (informational) | `calendar.rs` | "Waiting for Outlook" | Start Classic Outlook |
| OUTLOOK-102 | Attach failed / Outlook not (yet) in the Running Object Table / connection lost / process list unreadable / worker could not start / COM apartment failed | `outlook.rs`, `calendar.rs` | "Connecting" then "Couldn't read the calendar" after about 2 minutes | Usually self-heals; check Outlook health |
| OUTLOOK-103 | Outlook runs at a different elevation (or cannot be inspected) | `outlook.rs` classification | "Outlook runs with different permissions" | Run both normally, not "as administrator" |
| OUTLOOK-104 | Only New Outlook is running | `outlook.rs` classification | "New Outlook isn't supported" | Use Classic Outlook |
| OUTLOOK-105 | Outlook rejected the call (busy: modal dialog, start-up) | `outlook.rs` mapping | After 3 in a row: "Outlook isn't responding" | Close the dialog |
| OUTLOOK-106 | MAPI session / profile unavailable | `outlook.rs` (`Session`) | "Couldn't read the calendar" | Check the Outlook profile |
| OUTLOOK-107 | Default calendar folder or its items unavailable | `outlook.rs` | same | Check the default data file |
| OUTLOOK-108 | Reading items failed, internal error while reading, or the backend sent an unknown status | `outlook.rs`, `normalize.ts` | same | Collect the log |
| OUTLOOK-109 | Watchdog: no answer from the Outlook worker within 10 s | `calendar.rs` | "Outlook isn't responding"; after 5 hung reads "Couldn't read the calendar", retry every 5 min or more | Restart Outlook |
| OUTLOOK-110 | Object model blocked (access denied or a 0x800A.... runtime error: guard, policy, prompt) | `outlook.rs`, `com.rs` | "Couldn't read the calendar" | Review Outlook security policies |
| NOTIF-201 | Notification access denied in Windows | `notifications.rs` | Notifications off | Allow in Settings > Privacy > Notifications |
| NOTIF-202 | Access unspecified or forced off by policy (`LetAppsAccessNotifications = 2`) | `notifications.rs` | Notifications off | Policy decision |
| NOTIF-203 | Notification listener not supported | `notifications.rs` | Notifications off | None |
| NOTIF-204 | Listener error: subscription, read, access query or thread failure | `notifications.rs` | Falls back to polling, or off | Informational |
| NET-301 | No usable LAN IPv4, or network-change notifications unavailable | `system.rs` | "Local IP" shows n/a | Informational |
| WIN-501 | Window geometry, styles, region or fullscreen hooks failed | `window.rs`, `fullscreen.rs` | Island may mis-position; fullscreen detection may be off | Report |
| WIN-502 | System tray unavailable | `tray.rs` | No tray icon | None (second launch still toggles) |
| WIN-503 | Monitor enumeration failed / no monitors | `monitors.rs`, `window.rs` | Display chooser unavailable | Report |
| WIN-504 | Clipboard busy, text too large, or write failed | `clipboard.rs` | "Copy failed" | Retry |
| STICKY-201 | The Sticky Notes app could not be started (`sticky_notes_open`, or a click on a Sticky Notes search hit) | `sticky_notes.rs` | Nothing opens; no toast. (An unreadable Sticky Notes database is not an error: it is the `unavailable` state of the section, logged at INFO only) | Check that the Sticky Notes app is installed |

## 9. Privacy rules

1. No meeting subjects, locations, organizers, attendees, bodies or e-mail addresses in logs, diagnostics,
   crash output or any file. Notification titles and text are never logged or stored.
2. Allowed in logs: app version, OS and architecture, random session id, status names, error codes, counts,
   timings, an 8-hex hash prefix of an event id, view/tab names, window sizes.
3. The only persisted calendar-derived data is `state\reminders.json`: `eventHash|startUtc|reminderType ->
   firedAt`. There is no event cache on disk; events are re-read from Outlook after a restart.
4. Event ids are hashes (EntryID never leaves `outlook.rs`; the Outlook profile name is only ever hashed, and
   only at debug level).
5. Allow-listed Outlook properties only: EntryID (hashed), Subject, Start, End, Location, Organizer (display
   name), AllDayEvent, IsRecurring, BusyStatus, ResponseStatus, MeetingStatus (a number: canceled meetings are skipped). Organizer, IsRecurring and the meeting URL are carried to the UI but no V1 screen uses them. Subject and location are clipped to 200
   characters for the UI; the meeting link is extracted from Location only (known providers).
6. User profile paths in log text are replaced with `%USERPROFILE%`; error messages that reach the UI name a
   folder, never a full path.
7. "Copy diagnostics" is user-initiated and contains system identifiers (user, computer, local IP) but no
   meeting content. Its first line is a one-line summary (`Yuval 1.0.0 / Windows 10 21H2 Build 19044 /
   Computer / User / Outlook: Connected|Waiting for Outlook|Connection Failed (Internal Error: CODE) / Calendar /
   Cached events / Notifications: Available|Restricted by policy ...`), followed by every field including
   `Notification delivery: events|polling|none` and the recent error codes.
8. No network: no telemetry, no update check, no cloud calls. The Yuval Center and its Tour make none either
   (the Tour is local files behind a WebView2 virtual host; its page cannot connect anywhere).
9. Notes (1.0.4) are the one persisted piece of user-typed content: `state\notes.json`, per user, at most 500 notes
   of 10,000 characters and 16 MiB, atomic writes, a corrupt file quarantined as `notes.json.corrupt` (`APP-003`).
   Note text never appears in a log, in diagnostics, in the pipe's `log` command (the Center sends codes and names
   only) or in the Center's own error reports; logs hold counts only ("saved 12 notes", "repaired notes on load: 3
   -> 2"), and an `open_center` log line names the page kind (`note`), never the note id. Notes are kept when the
   product is uninstalled, like all per-user data.
10. The pipe carries only what the user asked for in the Center (settings, notes, monitors, notification status,
    `showIsland`, `openLogDir`) and nothing else; request and response bodies are never logged. The Tour has no IPC
    (no pipe, no Tauri bridge) and no network, and shows invented sample data.
11. Windows Sticky Notes (section 11) are read, never written: their text reaches the Notes tab and smart search
    (memory only) and is never logged. The one place it touches disk is a temporary copy of `plum.sqlite` (and its
    `-wal`) in `%LOCALAPPDATA%\CompanyIsland\sticky-tmp\`, deleted as soon as the read ends (a leftover of a crash,
    older than 10 minutes, is removed at the next read).

## 10. Extending

### 10.1 Add MicrosoftGraphCalendarProvider (New Outlook / Microsoft 365) later

1. **Provider**: create `src/lib/calendar/microsoftGraph.ts` implementing `CalendarProvider` (`id`,
   `subscribe`, `getSnapshot`, `refresh` that never rejects, `dispose`). Emit snapshots in the contract shape
   and pass every foreign payload through `normalizeSnapshot`.
2. **Register**: in `registry.ts` add `registerCalendarProvider("microsoft-graph", () => createGraphProvider())`
   inside `registerDefaultProviders` (ideally behind a setting so it is opt-in). `CalendarService` then merges
   it with Classic Outlook: events de-duplicated by `calendarId|id|startUtc`, sorted by start, best status
   wins (a `connected` provider leads), counts add.
3. **Ids**: `event.id` must be globally unique and must not expose the raw Graph id (hash it, e.g. sha256 of
   `calendarId|eventId|start`, 16 hex). The reminder key is `eventId|startUtc|type` and does **not** contain
   `calendarId`, so uniqueness across providers must come from the id itself.
4. **Where HTTP happens**: keep network and tokens in Rust (a new module next to `outlook.rs`) and expose
   them over IPC exactly like `calendar-snapshot`; do not loosen the page CSP (`connect-src`) for this.
   Store tokens with DPAPI under `%LOCALAPPDATA%\Yuval`, never in `localStorage`.
5. **Statuses**: reuse `waiting` (not signed in), `connecting`, `connected`, `failed`; a sign-in-required
   state needs a new `CalendarStatus` value in `src/lib/calendar/types.ts`, the `STATUSES` list in
   `normalize.ts`, strings in `i18n.ts` (both languages) and a case in `CalendarTab`. Add new error codes to
   section 8 and to `CODE_PREFIXES` in `diagnostics.rs` if you add a family.
6. **Enterprise review**: this ends the "zero network connections" guarantee. Update DEPLOYMENT.md sections 9
   and 13 (proxy, firewall, consent), the privacy rules above (token storage, scopes: read-only calendar), and
   re-run QA scenarios 18-20.
7. **Tests**: provider unit tests with a fake backend (pattern: `src/lib/calendar/classicOutlook.test.ts`),
   merge tests in `select.test.ts`.

### 10.2 Add a reminder type (15 / 5 / 0 minutes)

The engine already schedules a list of offsets, one reminder each, and persists them under separate keys
(`...|minutes-15`, `...|minutes-5`, `...|start`). What is missing is the setting:

1. Rust `settings.rs`: add `reminder_offsets: Vec<u32>` (clamped 0-120, sorted, de-duplicated, small maximum)
   to `Settings` and `SettingsPatch` with a default of `[30]`; keep `reminderMinutes` as the migration source.
2. TypeScript: `Settings`, `SETTINGS_DEFAULTS` and `normalizeSettings` in `src/lib/ipc.ts`; the
   Settings tab UI (`REMINDER_MINUTE_OPTIONS`).
3. `src/hooks/useReminders.ts`: pass `offsetsMinutes: settings.reminderOffsets` instead of `[reminderMinutes]`.
4. Nothing to change in the engine: `0` produces the "Meeting starting now" copy (`reminder.startingNow`) and
   a late fire reports the real remaining minutes; `valid_key` in `reminder_state.rs` already accepts
   `minutes-N` and `start`.
5. A new *kind* (for example "at end"): extend `ReminderType`, `reminderTypeId` (`types.ts`) and the due-time
   computation in `candidates()` (`engine.ts`). The persisted key token must stay `[A-Za-z0-9_-]{1,24}`.
6. Tests: `engine.test.ts` already covers several offsets per meeting ("supports several offsets per meeting,
   each firing once"); add the new type next to it.

### 10.3 Multi-calendar

1. Rust `outlook.rs`: `OutlookSource::fetch` already iterates a `calendars` list (today it holds the default
   calendar only) and `read_calendar` produces a stable per-folder `calendarId`
   (`sha256(StoreID|EntryID)`, 16 hex). Add more folders there (for example the default calendar of each store
   from `Session.Stores`). Do not resolve other people's calendars through `Recipient` objects without a
   security review: that is where Outlook's object-model guard prompts appear.
2. The 50-event cap and the 500-item scan cap apply to the merged result; revisit them if more calendars are
   read. Events are de-duplicated by id across calendars in `normalize_events`.
3. Settings: a list of enabled calendar ids (`settings.rs`, `ipc.ts`), plus a chooser in the Settings tab and
   a way to list calendars (a new command that returns `calendarId` + a display name, never logged).
4. Frontend: no change to `CalendarService`, `select.ts` (it keys by `calendarId|id|startUtc`) or the engine.
5. Privacy: calendar display names are user content; treat them like subjects (UI only, never logged).
6. Tests: extend the fake source in `calendar.rs` tests and `outlook.rs` tests; add a QA row for multiple
   accounts (scenario 10 changes from "default only" to "selected calendars").

## 11. Windows Sticky Notes (read only)

The island's Notes tab shows the user's Windows Sticky Notes under their own notes, and smart search
(`notes.search`) finds them too (`docs/AI_SEARCH.md`, section 8). The island never writes to them: editing
happens in the Sticky Notes app, which a click opens.

1. **Where the data is.** Sticky Notes 3.x/4.x and later (Store package `Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe`)
   keep their notes in `%LOCALAPPDATA%\Packages\<package>\LocalState\plum.sqlite`, table `Note` (`Id`, `Text`,
   `Theme`, `CreatedAt`, `UpdatedAt`, `DeletedAt`, ...; times are .NET ticks). That is the only layout read.
   `Text` holds the paragraphs as `\id=<guid> line` markers, with markdown-like inline markers; `sticky_notes::parse_text`
   returns plain text (Hebrew and direction marks untouched). Notes with `DeletedAt` set are skipped.
   On the development machine the app is installed (4.0.6104.0) but `LocalState` is empty (never used), so the
   real-database path is covered by a fixture built with the same `winsqlite3.dll` in the tests.
2. **How it is read.** No SQLite crate. `winsqlite3.dll` (in `System32` on Windows 10 and 11) is loaded with
   `LoadLibraryExW(LOAD_LIBRARY_SEARCH_SYSTEM32)` and ten entry points are bound (`sqlite3_open_v2`, `close`, `prepare_v2`,
   `step`, `finalize`, `column_text` / `column_bytes` / `column_int64` / `column_type`, `busy_timeout`). The live file is never opened: `plum.sqlite` and `plum.sqlite-wal` (where the newest
   notes are) are copied to `%LOCALAPPDATA%\CompanyIsland\sticky-tmp\<pid>-<n>\`, the copy is opened with
   `SQLITE_OPEN_READONLY`, read, closed and deleted (a copy older than 10 minutes left by a crash is removed at the
   next read). The `-shm` is not copied: SQLite rebuilds the WAL index from the copied `-wal`, which a stale copy
   of the index could contradict. The columns are discovered with `PRAGMA table_info(Note)`, so a layout that
   kept `Id` and `Text` still reads.
3. **Cache.** The answer is cached; it is read again only when the size or modification time of `plum.sqlite` or
   its `-wal` changed (two `stat` calls). The page asks (`sticky_notes_list`) at once and then every 10 s, only while
   the Notes tab is open and the page is visible (`lib/notes/sticky.ts`); a search asks too. The page sends the revision it
   shows (`since`) and gets `unchanged: true` without the notes while nothing changed. A failed re-read keeps the last good
   list and retries after 5 s; a first failure is retried after 30 s.
4. **States, never an error.** `ok`, `noData` (installed, no `plum.sqlite`: never used, or the notes are only in the
   cloud), `notInstalled` (the Notes tab shows no section), `unsupported` (a `Note` table without `Id`/`Text`, or only the
   Windows 7-era `StickyNotes.snt`), `unavailable` (`winsqlite3.dll` missing, copy or open failed). The section shows a
   calm empty / "not available" state from `ui/states.tsx`; nothing is toasted. State changes are logged at INFO (state
   names only).
5. **Privacy.** Note text goes to the page and to smart search only (memory), like the island's own notes; it is never
   logged and never written anywhere except the temporary copy of the database, which is deleted at once. At most 500
   notes of 10,000 characters are returned.
6. **Opening.** `sticky_notes_open` (and a click on a Sticky Notes search hit, id `sticky:<guid>`) launches
   `shell:AppsFolder\Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe!App` through `notifications::launch_aumid`. The target is a
   constant: the page cannot name another app.
