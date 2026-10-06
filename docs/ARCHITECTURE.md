# CompanyIsland - architecture

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
|   PillShell > CompactIsland | ExpandedIsland (tabs: DATE & TIME, CALENDAR, ABOUT)             |
|   MeetingAlert | NotificationToast | ContextMenu       "dir=ltr" layout, dir=auto text        |
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
|  paths.rs, settings.rs, reminder_state.rs, autostart.rs                    (per-user persistence) |
|  window.rs, monitors.rs, fullscreen.rs, tray.rs, clipboard.rs                    (Win32 shell)     |
+--------------------------------------------------------------------------------------------------+
```

| Layer | Responsibility | Key files |
|---|---|---|
| UI | Renders the island; pointer hover/leave intent; announces to screen readers; never calls Outlook or `invoke` directly | `src/components/Pill/*`, `src/hooks/*` |
| Island State Manager | Decides what the island shows by priority; remembers the user's expanded tab; queues alerts | `src/lib/island/state.ts`, `src/hooks/useIslandState.ts`, `timing.ts` |
| Localization | Strings (en, he) and RTL flag from the UI language; plurals; Intl date/time formatting from the Windows regional format (`get_format_locale`); locale-correct short date | `src/lib/i18n.ts`, `src/lib/dateFormat.ts` |
| CalendarProvider | Source-agnostic calendar contract and merge | `src/lib/calendar/*` |
| ClassicOutlookCalendarProvider | Frontend half of the Outlook source: initial read, pushed snapshots, throttled refresh (5 s) | `src/lib/calendar/classicOutlook.ts` |
| Outlook worker (Rust) | The only code that talks to Outlook (late-bound COM on its own STA thread) | `src-tauri/src/outlook.rs`, `com.rs`, `calendar.rs` |
| Reminder Engine | Pure scheduler: fire-once reminders from events + settings; persisted fired set | `src/lib/reminders/*` |
| System Info | Machine/user/OS/WebView2/IPv4 for About and diagnostics | `src-tauri/src/system.rs`, `src/hooks/useSystemInfo.ts` |
| Notification Provider | Mirror Windows toasts (read-only), status as an enum | `src-tauri/src/notifications.rs`, `src/hooks/useNotifications.ts` |
| Diagnostics | Privacy-safe status + last 10 error codes; "Copy diagnostics" text | `src-tauri/src/diagnostics.rs`, `src/lib/diagnostics.ts` |
| Logging | One rotating file, one line per entry, scrubbed; UI forwards through `write_logs` | `src-tauri/src/debug_log.rs`, `src/lib/debugLog.ts`, `logger.ts` |
| Settings | Typed, validated, schema-versioned store; autostart opt-out | `src-tauri/src/settings.rs`, `autostart.rs`, `src/hooks/useSettings.ts` |

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

## 2. Thread model

| Thread | Created by | Runs | Notes |
|---|---|---|---|
| UI / main (tao event loop, WebView2 UI) | process | Window creation and every window mutation (`run_on_main_thread`), tray, event dispatch | Never blocked by Outlook or WinRT: slow commands run elsewhere |
| Tokio blocking pool | Tauri `async_runtime` | `rt::run_blocking(...)`: settings, system info, diagnostics, clipboard, notification status, log-folder open | A command taking over 400 ms logs a `slow backend command` warning |
| `companyisland-calendar` (supervisor + **watchdog**) | `calendar::start` | Owns the `Machine`; cheap process discovery; hands reads to the worker; waits at most 10 s for a reply (`WATCHDOG_MS`); wakes at least every 30 s to notice sleep/midnight | Pure scheduling logic; no COM here |
| `companyisland-outlook-<generation>` (STA worker) | supervisor, on demand | `ComApartment::init_sta`, message filter, pumps messages; per job: attach -> read -> release | One worker at a time. A hung worker is **abandoned** (not killed) and replaced; if it ever returns it releases its COM objects and exits. Dropped when Outlook is not running |
| `companyisland-fullscreen` | `fullscreen::start` (from a startup thread) | Message loop + out-of-context WinEvent hooks (foreground, minimize, location of the foreground window); 250 ms debounce; `SHQueryUserNotificationState` + geometry | Emits `fullscreen-changed`; hides/restores the window only if it hid it |
| Notification threads | `notifications::start` / `spawn_sync` | `companyisland-notif-init` / `-sync` (short-lived): query access, subscribe or start the poller | `NotificationChanged` handlers run on WinRT callback threads and are wrapped in `catch` |
| `companyisland-notif-poll` | `notifications` when `NotificationChanged` cannot be subscribed (typical for an unpackaged exe) | Reads the Action Center every 5 s (30 s after 3 failures), forwards only unseen ids; stops through a channel | Reports `notificationMode = polling` |
| `startup` thread | `lib.rs` setup | `settings::sync_autostart` (registry) then `fullscreen::start` | Keeps registry work off the UI thread |
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
   `CalendarEventDto`s (id = first 16 hex of sha256(EntryID + "|" + start)), releases every object.
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
   preempting a toast, queuing behind another alert); the shell resizes the native window through the ordered
   geometry queue (`set_island_geometry`); the alert counts down 8 s (paused while hovered or while a
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
| any | blocked (E_ACCESSDENIED, 0x800A....) | `failed` | OUTLOOK-110 | failure backoff |
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

Durations (`src/lib/island/timing.ts`): alert 8 s, toast 4.5 s, hover intent 120 ms, leave grace 500 ms
(4 s when opened by click or toggle). Hover or a fullscreen-hidden island pauses the countdown.

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

## 5. File map

```
src/
  App.tsx, main.tsx, index.css
  components/CrashBoundary.tsx
  components/Pill/        PillShell, CompactIsland, ExpandedIsland, MeetingAlert, NotificationToast,
                          ContextMenu, TabDock, TabBoundary, tabs.ts, animations.ts, alertLayout.ts,
                          usePillGeometry.ts, useCompactLayout.ts, panels/{DatetimeTab,CalendarTab,AboutTab}
  hooks/                  useIslandState, usePillState, useIslandEvents, useCalendar, useReminders,
                          useSettings, useSystemInfo, useNotifications, useClock, useScreenReader,
                          useDesktopGestures, useCrashRecovery
  lib/appInfo.ts          product constants (name, identifier, version)
  lib/i18n.ts             string tables en/he, t(), detectLocale, isRtl, plurals
  lib/dateFormat.ts       cached Intl formatters, minute/day helpers
  lib/ipc.ts, tauri.ts    typed wrappers: commands + events (non-global @tauri-apps/api; null outside Tauri)
  lib/errors.ts, logger.ts, debugLog.ts, diagnostics.ts   code extraction, log forwarding, diagnostics text
  lib/calendar/           provider.ts (interface), registry.ts, service.ts, classicOutlook.ts,
                          normalize.ts, select.ts, types.ts
  lib/reminders/          engine.ts, store.ts, types.ts
  lib/island/             state.ts, timing.ts, geometryQueue.ts, compactLayout.ts
src-tauri/
  tauri.conf.json         product, window (transparent, no decorations, non-focusable), CSP, NSIS bundle
  windows-app.manifest    asInvoker, PerMonitorV2, Common-Controls v6, long paths
  installer-hooks.nsh     OS gate (1603), HKLM Run value, pre-uninstall close
  capabilities/default.json   core:event:allow-listen / allow-unlisten only
  .cargo/config.toml      +crt-static
  src/lib.rs              builder, plugins (single-instance, core plugin: log + state), command table, WebView2 args
  src/calendar.rs         contract types, Machine, supervisor/watchdog, commands
  src/outlook.rs          discovery, COM source, STA worker
  src/com.rs              IDispatch wrapper, apartment, message filter, HRESULT classes
  src/calendar_diag.rs, diagnostics.rs
  src/notifications.rs, system.rs, monitors.rs, window.rs, fullscreen.rs, tray.rs, clipboard.rs
  src/paths.rs, settings.rs, reminder_state.rs, autostart.rs, debug_log.rs, rt.rs
docs/                     ENTERPRISE_DESIGN, INSTALLER, DEPLOYMENT, QA_MATRIX, ARCHITECTURE
scripts/                  check-versions.cjs, verify-installer.ps1, make-icon.ps1
```

IPC commands (registered in `lib.rs`): `write_logs`, `log_frontend_error`, `open_log_dir`, `get_settings`,
`update_settings`, `calendar_get_snapshot`, `calendar_refresh`, `reminder_state_load`, `reminder_state_save`,
`set_island_geometry`, `get_monitors`, `get_fullscreen_state`, `get_system_info`, `get_format_locale`, `get_diagnostics`,
`copy_text_to_clipboard`, `notifications_get_status`, `notifications_request_access`, `activate_notification`,
`activate_app_by_aumid`. Events (Rust to JS): `calendar-snapshot`, `notification-received`,
`notification-status`, `settings-changed`, `fullscreen-changed`, `display-changed`, `island-toggle`.

## 6. Security and hardening summary

- Never elevated (`asInvoker`); no HKLM writes at run time; per-user data only.
- Capability file grants only event listen/unlisten; CSP is `default-src 'self'` with `connect-src` limited to
  the Tauri IPC; `window.__TAURI__` is not exposed (`withGlobalTauri: false`).
- No global mouse/keyboard hook, no raw-input registration (`DeviceEventFilter::Always`), no injection;
  fullscreen detection uses out-of-context WinEvent hooks.
- Outlook: attach-only (`GetActiveObject`), same session + SID + elevation, read-only, safe properties only,
  every object released after each read.
- Notifications: read-only (`RemoveNotification` is never called); activation of a toast's app is limited to
  AUMIDs the app has itself seen (allow-list of 64) and validated against shell metacharacters.
- External programs are started by absolute path (`explorer.exe` from the Windows known folder).
- WebView2 profile under `%LOCALAPPDATA%\CompanyIsland\EBWebView`, `--disable-background-networking`.

## 7. Settings

`settings.json` (schema version 1), camelCase, validated on load and on every patch:
`launchWithWindows` (true), `hideInFullscreen` (true), `meetingReminderEnabled` (true), `reminderMinutes`
(30, clamped 0-120; the UI offers 5/10/15/30), `monitorId` (null = primary; otherwise a zero-based index as a
string, missing monitor falls back to primary), `notificationsEnabled` (true),
`debugLogging` (false). A change emits `settings-changed`; `launchWithWindows` writes the HKCU StartupApproved
value; `debugLogging` switches the log level; `monitorId` re-places the island; `hideInFullscreen`
re-evaluates fullscreen; `notificationsEnabled` starts or stops delivery.

## 8. Error codes

Codes appear in the log, in the Calendar tab (for calendar statuses), in About > diagnostics and in "Copy
diagnostics" (last 10, repeats collapsed). Any WARN/ERROR log line containing a known code feeds the recent
list; `OUTLOOK-101` and the notification status codes are INFO conditions (the latter are recorded
explicitly). No code is fatal: the app keeps running in every case except a failure to start (`APP-001
failed to start`, process exits with code 1).

| Code | Meaning | Raised by | User-visible effect | Typical action |
|---|---|---|---|---|
| APP-001 | Unhandled panic caught, a background command task failed, UI render error in a tab, or the app failed to start | `debug_log::catch`, panic hook, `rt::run_blocking`, `TabBoundary`, `lib.rs` | A tab shows "Unavailable / Try again"; the rest keeps working | Collect the log; report |
| APP-002 | Settings or reminder state could not be written / data folder unavailable | `settings.rs`, `reminder_state.rs` | Settings kept in memory only; reminders may repeat after a restart | Check profile volume and permissions |
| APP-003 | Settings or reminder file corrupt or unreadable | `settings.rs`, `reminder_state.rs` | Defaults / empty set; bad file kept as `*.corrupt` | Delete or inspect `*.corrupt` |
| APP-010 | Log folder unavailable or cannot be opened | `debug_log.rs`, `paths.rs` | No log file; "Open logs" fails | Check `%LOCALAPPDATA%\CompanyIsland\logs` |
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
   meeting content. Its first line is a one-line summary (`CompanyIsland 1.0.0 / Windows 10 21H2 Build 19044 /
   Computer / User / Outlook: Connected|Waiting for Outlook|Connection Failed (Internal Error: CODE) / Calendar /
   Cached events / Notifications: Available|Restricted by policy ...`), followed by every field including
   `Notification delivery: events|polling|none` and the recent error codes.
8. No network: no telemetry, no update check, no cloud calls.

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
   Store tokens with DPAPI under `%LOCALAPPDATA%\CompanyIsland`, never in `localStorage`.
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
2. TypeScript: `Settings`, `SETTINGS_DEFAULTS` and `normalizeSettings` in `src/lib/ipc.ts`; the About >
   Settings UI (`REMINDER_MINUTE_OPTIONS`).
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
3. Settings: a list of enabled calendar ids (`settings.rs`, `ipc.ts`), plus a chooser in About > Settings and
   a way to list calendars (a new command that returns `calendarId` + a display name, never logged).
4. Frontend: no change to `CalendarService`, `select.ts` (it keys by `calendarId|id|startUtc`) or the engine.
5. Privacy: calendar display names are user content; treat them like subjects (UI only, never logged).
6. Tests: extend the fake source in `calendar.rs` tests and `outlook.rs` tests; add a QA row for multiple
   accounts (scenario 10 changes from "default only" to "selected calendars").
