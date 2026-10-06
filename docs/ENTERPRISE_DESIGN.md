# CompanyIsland — enterprise design contract

This file is the single source of truth for the enterprise refactor of PILLAR.
Implementation agents MUST follow it. If something here is wrong, stop and report; do not diverge silently.

## 0. Decisions (made from the Phase 0 audit)

| Topic | Decision |
|---|---|
| Product name | `CompanyIsland` (placeholder; defined once in `src-tauri/tauri.conf.json` + `src-tauri/src/paths.rs` + `src/lib/appInfo.ts`; trivial to rename) |
| Identifier | `com.companyisland.app` |
| Executable | `CompanyIsland.exe` (cargo `[[bin]]`/`productName`) |
| Per-user data root | `%LOCALAPPDATA%\CompanyIsland\` (never `%APPDATA%`, never `.`/install dir). Sub-dirs: `logs\`, `state\`, `settings.json` |
| Installer | NSIS only, `installMode: perMachine`, x64, WebView2 `offlineInstaller` (embedded), MSI target dropped. Machine-wide autostart = HKLM `Run` value written by NSIS hook. App itself never writes HKLM. |
| Per-user autostart opt-out | Setting `launchWithWindows` (default true). Toggled by writing HKCU `Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run` value (same mechanism as Task Manager): `02 00 00 00 + 8 zero bytes` = enabled, `03 00 00 00 + 8 zero bytes` = disabled. No admin needed. |
| Outlook | Classic Outlook OOM via in-process Rust late-bound `IDispatch` (windows crate) on ONE dedicated STA worker thread. **Attach only** (`GetActiveObject` / ROT) — never `CoCreateInstance` of Outlook.Application. No sidecar, no .NET. |
| Calendar reads | Default Calendar only (`GetDefaultFolder(9)` of the *default store of the running session*). Properties read: EntryID(hashed), Subject, Start, End, Location, Organizer(display name), AllDayEvent, IsRecurring, BusyStatus, ResponseStatus, MeetingStatus (a number; canceled meetings 5/7 are skipped). Organizer, IsRecurring and the meeting URL are not used by any V1 screen (reserved for a Join action). **Never** Body, Recipients, SenderEmailAddress, attachments. Meeting URL = regex over Location only. |
| Cloud/AI/internet | None. Prism/Groq, `reqwest`, productivity/Focus, brightness, per-app mixer, battery, system monitor, SMTC music, foreground-app context are REMOVED. |
| Notifications | Keep Windows toast mirroring, best-effort: `UserNotificationListener`. Never call `RemoveNotification` (non-destructive). Access status is an enum, never a panic. Denied/policy/unsupported ⇒ feature off + diagnostic code, app continues. |
| Single instance | Per Windows session (named mutex `Local\CompanyIsland-<identifier-hash>` semantics; NOT `Global\`). Second launch in same session ⇒ signal first instance (toggle expand), exit. Different sessions run independently. |
| Focus | Window is non-activating: `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, never calls `set_focus`/`SetForegroundWindow`/`AllowSetForegroundWindow(ASFW_ANY)`. Island has no text inputs in V1, so no activation toggle is needed. |
| Reminder semantics | Fixed offset before event *start* (default 30 min, setting `reminderMinutes`; engine supports a list of offsets later: 30/15/5/0). Skipped: all-day events, `BusyStatus` = free(0), `ResponseStatus` = declined(4), events already started, events ended. Fired-set persisted per user in `state\reminders.json`, key `"<eventHash>|<startUtcIso>|<reminderType>"`, pruned after 7 days. A missed window (PC asleep) fires late only if event hasn't started and ≥ 1 min remains; otherwise marked skipped. |
| Locale | Two sources. Strings and text direction follow the Windows UI language (`navigator.language`, he/en table in `src/lib/i18n.ts`, other languages fall back to English strings). Dates, weekdays and times follow the Windows REGIONAL FORMAT (`GetUserDefaultLocaleName` via the `get_format_locale` command, read once before the first render; falls back to the UI language), formatted by Intl, so an English UI with an Israel region shows 6/10, as Windows does. RTL strings correct, but island layout is physically LTR: date left, weekday right (`dir="ltr"` on layout containers; text spans use `dir="auto"`/`unicode-bidi: plaintext`). |
| Notification/Alert priority | `meetingAlert (3) > notification (2) > userExpanded (1) > idle (0)`. Lower never interrupts higher; higher preempts lower and the preempted state is restored afterwards (except notification which is dropped if stale). |

## 1. Rust ⇄ TypeScript contract

All commands are registered in `lib.rs` `invoke_handler`; Rust modules live in `src-tauri/src/`.
Command errors are `Result<T, String>` where the string is `"CODE: short message"` with a stable code (see §3).

### Commands
- `get_system_info() -> SystemInfo`
- `get_format_locale() -> string | null` (BCP-47 regional format tag), `get_fullscreen_state() -> bool` (the event only reports changes; the page asks once on mount)
- `get_diagnostics() -> Diagnostics` (privacy-safe: no subjects, no emails)
- `copy_text_to_clipboard(text: String) -> ()` (Win32 clipboard; no webview focus needed)
- `open_log_dir() -> ()`
- `get_settings() -> Settings`, `update_settings(patch: SettingsPatch) -> Settings` (single Rust `SettingsStore`, validated, schema-versioned, atomic write, emits `settings-changed`)
- `calendar_get_snapshot() -> CalendarSnapshot`, `calendar_refresh() -> ()`
- `reminder_state_load() -> Record<string, number>` (key → firedAtUnixMs), `reminder_state_save(map)`.
- `notifications_get_status() -> NotificationStatus`, `notifications_request_access() -> NotificationStatus` (only on explicit user click), plus existing notification list/activate commands, trimmed.
- Window: `set_island_geometry(width, height, radius)` (flat logical px; native window is sized EXACTLY to the island, rounded SetWindowRgn, no global mouse hook), `get_monitors() -> [{id,name,primary,isPrimary,width,height,scale}]`; setting key is `monitorId` (null = primary).

### Events (Rust → JS)
- `calendar-snapshot` (payload `CalendarSnapshot`) — emitted on every state change and after each successful sync.
- `notification-received`, `notification-status`
- `settings-changed`, `fullscreen-changed`, `display-changed`, `island-toggle` (from tray / second instance)

### Types (camelCase over IPC; Rust uses `#[serde(rename_all = "camelCase")]`)
```ts
type SystemInfo = {
  computerName: string; localIpv4: string | null; ipAdapter: string | null; // adapter friendly name, for diagnostics only
  windowsUser: string;            // "DOMAIN\\USER"
  sessionId: number;
  osName: string; osDisplayVersion: string | null; // "Windows 10", "21H2"
  osBuild: number;                // 19044
  appVersion: string; webview2Version: string | null;
}
type CalendarStatus =
  | 'waiting'            // Outlook not running  (normal state, OUTLOOK-101 is informational)
  | 'connecting'
  | 'connected'
  | 'newOutlookOnly'     // olk.exe running, no classic COM
  | 'elevationMismatch'  // Outlook runs elevated, we don't (or vice versa)
  | 'unresponsive'       // watchdog / RPC_E_CALL_REJECTED persists
  | 'failed'
type CalendarSnapshot = {
  status: CalendarStatus; errorCode: string | null;   // e.g. "OUTLOOK-102"
  lastSyncUnixMs: number | null; cachedCount: number;
  nextRetryUnixMs: number | null;
  events: CalendarEventDto[];                         // sorted by start asc, horizon: now-0 .. +48h, max 50
}
type CalendarEventDto = {
  id: string;           // stable: sha256(EntryID + "|" + startUtc) truncated to 16 hex. Raw EntryID never leaves Rust.
  calendarId: string;   // hash of store+folder id; V1 always the default calendar. Multi-calendar-ready.
  subject: string; startUtc: string; endUtc: string;  // ISO-8601 UTC
  allDay: boolean; location: string | null; organizer: string | null;
  isRecurring: boolean; meetingUrl: string | null;
  busyStatus: 'free'|'tentative'|'busy'|'oof'|'workingElsewhere'; responseStatus: 'none'|'organized'|'tentative'|'accepted'|'declined'|'notResponded';
}
type NotificationStatus = 'allowed'|'denied'|'unspecified'|'unsupported'|'policy'|'error'
```

## 2. Frontend architecture

```
src/lib/appInfo.ts            product constants
src/lib/i18n.ts               string tables (en, he) + t(key, params) + locale detection + isRtl
src/lib/dateFormat.ts         Intl formatters (created once, cached per locale): shortDate, weekday, time, fullDate
src/lib/calendar/             CalendarProvider interface + registry, ClassicOutlookCalendarProvider (Tauri-backed), CalendarService (owns providers, merged snapshot), normalize, select, types
src/lib/reminders/            ReminderEngine (pure, testable) + ReminderStore (Tauri-persisted)
src/lib/island/               IslandStateManager (state.ts: pure reducer + priority), timing constants, geometryQueue
src/hooks/                    useClock (minute-aligned), useCalendar, useReminders, useSystemInfo, useIslandState (reducer + alert/toast timers), usePillState (boot + hover/leave intent)
src/components/Pill/          PillShell, CompactIsland, ExpandedIsland, MeetingAlert, NotificationToast (alert and toast render inside the island), panels/{DatetimeTab,CalendarTab,AboutTab}
```
Interaction: no global mouse hook. The native window is exactly the island's size (`set_island_geometry(width, height, radius)`), so DOM pointer events are the island's own: hover (120 ms intent) expands, leave collapses (500 ms; 4 s when pinned by a click/toggle), a meeting alert (8 s) or toast (4.5 s) pauses while hovered.
Rules: no Tauri `invoke` inside React components (only through `src/lib/*` / hooks). No Outlook logic in React. UI talks to `CalendarProvider` only.
Tabs: DATE & TIME, CALENDAR, ABOUT. Collapsed island: `d/M` physically left, weekday physically right, black pill, near-white text.

## 3. Error codes

`APP-001` unhandled panic caught · `APP-002` settings write failed · `APP-003` settings corrupt (quarantined) · `APP-010` log dir unavailable
`OUTLOOK-101` not running (informational) · `OUTLOOK-102` COM attach failed · `OUTLOOK-103` elevation mismatch · `OUTLOOK-104` New Outlook only · `OUTLOOK-105` busy/call rejected (retrying) · `OUTLOOK-106` MAPI namespace/profile unavailable · `OUTLOOK-107` default calendar unavailable · `OUTLOOK-108` reading items failed · `OUTLOOK-109` watchdog/unresponsive · `OUTLOOK-110` object model blocked by policy/prompt
`NOTIF-201` denied · `NOTIF-202` policy/unspecified · `NOTIF-203` unsupported · `NOTIF-204` listener error
`NET-301` no usable LAN IPv4 · `WIN-501` window geometry failed · `WIN-502` tray unavailable · `WIN-503` monitor enumeration failed · `WIN-504` clipboard busy/too large/write failed

## 4. Local IPv4 selection strategy (documented, deterministic)
1. `GetAdaptersAddresses` (IPv4, skip DNS suffix/multicast), keep adapters with `IfOperStatus == Up`, not loopback/tunnel (`IF_TYPE_SOFTWARE_LOOPBACK`, `IF_TYPE_TUNNEL`), unicast address not `127/8`, not `169.254/16`.
2. Drop adapters whose description/friendly name matches virtual patterns (Hyper-V vEthernet, VirtualBox, VMware, Tailscale, WSL, Docker, TAP/TUN, Npcap/loopback, Bluetooth PAN) unless nothing else remains.
3. Prefer the adapter that owns the lowest-metric default IPv4 route with a non-zero gateway (`GetBestRoute2`/`GetIpForwardTable2`); VPN adapters rank below physical ones.
4. Ties: Ethernet (type 6) > Wi-Fi (71) > other; then lowest interface metric; then lowest IfIndex.
5. If nothing qualifies, fall back to APIPA only if no other IPv4 exists, else `null` (`NET-301`). Result cached; recomputed on `NotifyIpInterfaceChange` or every 60 s while About is visible — never polled while collapsed.

## 5. Privacy rules (apply everywhere)
No meeting subjects/locations/organizers in logs, diagnostics, crash reports or persisted caches. Logs may contain: event hash (first 8 hex), counts, timings, error codes. The reminder store holds only hash|start|type|firedAt. There is no persistent event cache on disk in V1 (events are re-read from Outlook's own local cache on attach).

## 6. Phase gate (every phase)
`npx tsc --noEmit`, `npm test` (vitest, once added), `cargo check` + `cargo test` (PowerShell with `%USERPROFILE%\.cargo\bin` on PATH; Git Bash can't find cargo), review `git diff`, commit with message `phase N: …`.

## 7. Windows 10 21H2 (build 19044) compatibility and runtime hardening

Audited from the built exe's import table and the Rust sources. Nothing needs anything newer than Windows 10 1607 (build 14393); the one 1607 dependency is `UserNotificationListener`, which is best effort by design (§0).

| Area | API / feature | Minimum | Notes |
|---|---|---|---|
| Window | `SetWindowSubclass`/`DefSubclassProc` (comctl32 v6) | XP | needs the Common-Controls 6 manifest dependency (`windows-app.manifest`; tauri's `common-controls-v6` feature does the same for tao/muda) |
| Window | `WS_EX_NOACTIVATE \| WS_EX_TOOLWINDOW`, `SetWindowRgn`, `SetWindowPos`, `WM_STYLECHANGING` | Win2000 | identical on 10 and 11; tao's `DwmEnableBlurBehindWindow` (empty region) provides the transparent surface on Windows 10, WebView2 supplies the alpha-0 background (`DefaultBackgroundColor`, available in runtimes far older than the 111 baseline) |
| DPI | `GetDpiForMonitor` (shcore) | 8.1 | values are physical only with the PerMonitorV2 manifest (`windows-app.manifest`); without it every monitor reports 96 |
| Fullscreen | `SetWinEventHook` (out of context), `SHQueryUserNotificationState` | Win2000 / Vista | no injection, no admin; UWP fullscreen (ApplicationFrameWindow) relies on `QUNS_BUSY` plus the covers-monitor geometry |
| Network | `GetAdaptersAddresses` (skip flags only), `GetIpForwardTable2`, `NotifyIpInterfaceChange`, `NotifyUnicastIpAddressChange` | Vista | event driven, never polled |
| System | `RtlGetVersion`, `GetUserNameExW(NameSamCompatible)`, `GetComputerNameExW`, `RegGetValueW` (HKLM, read only) | Vista | no manifest version lie |
| COM | `CoRegisterMessageFilter`, `GetActiveObject`, `CLSIDFromProgID`, late-bound `IDispatch` | Win2000 | attach only |
| Locale | `GetDateFormatEx`, `GetTimeFormatEx`, `GetUserDefaultUILanguage` | Vista | |
| WinRT | `UserNotificationListener` (+ `NotificationChanged`) | 10.0.14393 | behaviour of an unpackaged exe is unverified on 21H2: every failure is a status (`unsupported`/`denied`/`error`), never a panic |
| Shell | `SHGetKnownFolderPath`, `ShellExecuteW("shell:AppsFolder\...")` | Vista / 8 | explorer.exe is resolved from the Windows known folder |
| Registry | HKCU `StartupApproved\Run` write; HKLM reads only | | the app never writes HKLM |

Frontend baseline is Chromium 111 (`build.target`, installer `minimumWebview2Version`). Used: `Intl.DateTimeFormat` (`formatToParts`, `hourCycle`), `Intl.RelativeTimeFormat`, `Intl.PluralRules`, `color-mix()`, `:where()`. Deliberately not used: `Intl.Locale.getWeekInfo` (not in 111), `backdrop-filter`, CSS nesting, `@layer`, container queries. Windows 11 extras are not used, so there is nothing to fall back from: no Mica/Acrylic, no `DWMWA_WINDOW_CORNER_PREFERENCE` (the pill shape is `SetWindowRgn`), no Segoe UI Variable dependency (it is only the first entry of a font stack that ends in Segoe UI).

Runtime hardening that is not in `tauri.conf.json` (see `lib.rs`): the WebView2 profile lives in `%LOCALAPPDATA%\CompanyIsland\EBWebView` (via `appDirectoriesOverride`), WebView2 runs with `--disable-background-networking` next to wry's defaults, and raw keyboard/mouse input registration is removed (`DeviceEventFilter::Always`). The page reaches Rust through `@tauri-apps/api` only (`window.__TAURI__` is not read), and the capability file grants `core:event:allow-listen/unlisten` only.
