# Yuval - IT deployment guide

Audience: desktop engineering / endpoint management. Build instructions, installer internals and the
signing hook live in [INSTALLER.md](INSTALLER.md); the design contract is
[ENTERPRISE_DESIGN.md](ENTERPRISE_DESIGN.md); the test plan is [QA_MATRIX.md](QA_MATRIX.md).

Yuval (called CompanyIsland up to 1.0.13; the same product, renamed in 1.0.14) is a small always-on-top overlay
("island") that shows the date, the time and the weekday, and reminds the
signed-in user of upcoming **Classic Outlook** meetings. It is offline by design, runs as a standard user,
and keeps all of its data in the user's own profile. Since 1.0.4 it also has user notes and a second program,
the **Island Center** (`<install folder>\center\Yuval.Center.exe`, a Windows app with Welcome, Settings,
Notes and a guided tour), which talks only to the island on the same PC. Sections 4, 7, 9, 11 and 12 say what
that changes for IT.

Values below were checked against the repository (file names are given so you can re-check after a rebuild).
Facts that could not be verified in the repository or on a test machine are marked **(unverified)** and are
collected in the "Not verified" list at the end.

## 1. Supported platforms

| Item | Supported | Notes |
|---|---|---|
| Windows 10 | 21H2 (build 19044), 22H2 (19045) | Minimum is build 19044 (`YUVAL_MIN_BUILD` in `src-tauri/installer-hooks.nsh`). |
| Windows 11 | all builds (22000+) | Win11 already ships the WebView2 runtime. |
| Architecture | x64 only | The installer refuses anything that is not 64-bit Windows. No ARM64 or x86 build. |
| Editions | Pro / Enterprise / Education | Windows 10 N editions and LTSC builds are not specifically tested **(unverified)**. LTSC 2021 is build 19044 and passes the installer gate. |
| Server SKUs, RDS, VDI | see section 12 | Not a primary target. |

Unsupported OS: the installer exits with code **1603** (silent) or shows a message box (interactive, Hebrew and
English) and installs nothing, before it touches WebView2 or running processes. Windows 10 22H2 is build 19045 and
passes.

## 2. Dependencies

| Dependency | Needed? | How it is satisfied |
|---|---|---|
| Microsoft Edge WebView2 Runtime (Evergreen), version 111.0.1661.41 or newer | Yes | See below. |
| Visual C++ redistributable | No | The exe is built with `+crt-static` (`src-tauri/.cargo/config.toml`). |
| .NET, Java, Node, Outlook PIAs | No: nothing to install on the PC | Outlook is automated late-bound from Rust; no interop assemblies. The Island Center (1.0.4) is a .NET 10 / Windows App SDK 1.8 program, but it is **self-contained**: its runtime files are inside `center\`, so no .NET runtime, no Windows App SDK and no MSIX/AppX registration is needed, and nothing machine-wide is installed or patched separately. |
| Internet access | No | Not at install time (runtime is embedded) and not at run time. |
| Classic Outlook | Optional | Without it the island still shows date and weekday; the Calendar tab says Outlook is not available. See section 10. |

### WebView2: how the embedded offline installer works

`tauri.conf.json` sets `webviewInstallMode` to `offlineInstaller` (silent). The ~200 MB setup file
therefore contains Microsoft's *Evergreen Standalone x64* installer. During setup the NSIS script:

1. Reads the installed runtime version (`pv` under `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`, then HKCU).
2. **No runtime found**: extracts the standalone installer to `%TEMP%` and runs it silently (`/silent /install`). Aborts setup if that fails.
3. **Runtime found but older than 111.0.1661.41**: runs the machine's own EdgeUpdate (`EdgeUpdate\path`) to update it (needs the updater to be present and not blocked; it may need Microsoft update endpoints **(unverified)**; if it fails an interactive install offers Abort/Retry/Ignore).
4. **Runtime found and new enough**: does nothing.

| | Windows 10 21H2 | Windows 11 |
|---|---|---|
| Typical state | Runtime may be absent on a clean or locked-down image; present if Microsoft 365 Apps or recent Edge updates installed it | Present (part of the OS) |
| What setup does | Installs it from the embedded copy (needs admin, which setup has) | Nothing |
| Internet needed | No | No |

Patching ownership with Evergreen: the runtime updates itself through Microsoft's EdgeUpdate service. If you
block or pin that (WebView2 update policies are keyed by the GUID above), **you** own security patching of
the runtime.

**Fixed-version runtime (alternative).** Possible only if you rebuild the installer with Tauri
`webviewInstallMode: fixedRuntime` (the runtime folder is then shipped inside the installer) or point the
WebView2 loader at your own folder with the documented `BrowserExecutableFolder` policy / the
`WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` environment variable **(unverified with Yuval)**. IT then owns
patching: every WebView2 security release must be repackaged and redeployed. The shipped configuration is
Evergreen; the NSIS script's Evergreen check would still try to install Evergreen if it finds none, so a
fixed-runtime-only estate needs the rebuild.

## 3. Permissions

| Phase | Account | What it touches |
|---|---|---|
| Install / upgrade / uninstall | Administrator (UAC elevation or SYSTEM) | `%ProgramFiles%\Yuval`, HKLM `Run` and `Uninstall` keys, Start menu and desktop shortcuts, WebView2 runtime |
| Run | **Standard user. No UAC prompt.** The exe manifest is `asInvoker` (`src-tauri/windows-app.manifest`) | Read/write `%LOCALAPPDATA%\Yuval\**` and `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run` only. Reads (never writes) a few HKLM values (OS version, notification policy). Never writes HKLM. |

The island must not be run elevated: it would then no longer match a normal-integrity Outlook (section 10).

## 4. Installation

Installer: `Yuval_1.0.14_x64-setup.exe` (NSIS, per-machine, about 207 MiB because it embeds WebView2).
Language follows the Windows UI language (English or Hebrew, no selector); dates and weekdays follow the Windows regional format.

| Task | Command (run elevated) |
|---|---|
| Interactive | double-click the setup file, accept UAC |
| Silent (new install, **update** or migration from CompanyIsland) | `Yuval_1.0.14_x64-setup.exe /S` |
| Silent, custom folder | `Yuval_1.0.14_x64-setup.exe /S /D=D:\Apps\Yuval` |
| Passive (progress bar, no questions) | `Yuval_1.0.14_x64-setup.exe /P` |
| No Start menu / desktop shortcuts | add `/NS` |
| Start the app for the current user after install | add `/R` (silent or passive only) |

Notes:

- `/D=` must be the **last** argument, with no quotes, even if the path has spaces (standard NSIS rule).
  The default folder is `%ProgramFiles%\Yuval` (`C:\Program Files\Yuval`). Detection rules and
  the uninstall command below assume the default; adjust them if you use `/D=`.
- Flags `/S /D= /P /NS /R /UPDATE` come from the Tauri-generated `installer.nsi` (generated into
  `src-tauri/target/release/nsis/x64/installer.nsi` at build time). `/UPDATE` is for in-app updaters and is
  not needed for deployment.
- Silent installs create the shortcuts (Start menu folder `Yuval`, all-users desktop) unless `/NS` is
  given (an update refreshes the desktop shortcut only if it still exists). A first install does **not** launch
  the app: without `/R` it first starts at the user's next logon through the HKLM `Run` value. An update or a
  migration starts it again **if it was running when setup closed it** (section 6), and `/R` always starts it, both as the
  interactively logged-on user and never elevated. When the setup runs as SYSTEM with nobody logged on nothing
  is started (**unverified**): the app starts at the next logon, which is also what to expect in managed rollouts.
- Exit codes: `0` success (installed, updated or repaired), `1603` unsupported OS (nothing installed), `1638` a
  **newer** version is already installed and downgrades are refused (nothing changed), `1` cancelled by the user,
  `2` aborted by the script (for example a program that could not be closed); other non-zero values are standard
  NSIS codes. A running app is closed by the installer itself (section 6), not by Restart Manager.
- What is installed: `Yuval.exe` (about 6.5 MiB) and `uninstall.exe` in the install folder, and, since
  1.0.4, the folder `center\` next to them: the Island Center (`Yuval.Center.exe`), its self-contained
  .NET 10 and Windows App SDK files (DLLs, `.winmd`, `.pri`, language folders) and `center\web\` (the tour's pages).
  About 86 MiB and 347 files in 92 folders on disk (measured on the build output for 1.0.4, not on an installed
  copy). No DLL sits next to `Yuval.exe` itself. No services, no scheduled tasks, no drivers, no firewall
  rules, no Start menu entry or autostart value for the Center (the island opens it on demand, and once by
  itself the first time, see section 5).
- Installer size: about 207 MiB for 1.0.0 (section 13); 1.0.4 adds the Center's files, which the installer
  compresses. The new size has not been measured because the 1.0.4 setup has not been built yet **(unverified)**:
  take it from `scripts/verify-installer.ps1` on the file you deploy.
- Registry written by setup (64-bit view):
  - `HKLM\Software\Microsoft\Windows\CurrentVersion\Run` : `Yuval` = `"C:\Program Files\Yuval\Yuval.exe"`
  - `HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Yuval` (Add/Remove Programs entry, `DisplayVersion`, `UninstallString`)
  - `HKLM\Software\<publisher>\Yuval` (install folder, used for upgrades; `<publisher>` is `bundle.publisher` of
    `tauri.conf.json`; uninstall leaves it unless the interactive "delete app data" box is ticked)
  - A migration from CompanyIsland additionally **removes** `Run\CompanyIsland`, `Uninstall\CompanyIsland` and
    `HKLM\Software\CompanyIsland\CompanyIsland` (section 6).

### 4.1 Intune (Win32 app) and SCCM / Configuration Manager

Package the setup file as-is (for Intune, wrap the folder with `IntuneWinAppUtil.exe -c <folder> -s Yuval_1.0.14_x64-setup.exe -o <out>`).

| Setting | Value |
|---|---|
| Install command | `Yuval_1.0.14_x64-setup.exe /S` |
| Uninstall command | `"%ProgramFiles%\Yuval\uninstall.exe" /S` |
| Install behaviour / run as | System (SCCM: "whether or not a user is logged on", "run with administrative rights") |
| Device restart behaviour | No specific action; no reboot is required |
| Requirements | 64-bit OS, Windows 10 build 19044 or newer |
| Max run time | 30 minutes is generous: WebView2 installation is the slow part on a clean Windows 10 |
| Detection (simple) | File: `C:\Program Files\Yuval\Yuval.exe`, version (string) greater than or equal to `1.0.0` |
| Detection (stronger) | The same file rule **and** registry value `HKLM\Software\Microsoft\Windows\CurrentVersion\Run` name `Yuval` exists (see script below) |

Do not tick "associated with a 32-bit app on 64-bit systems" in file or registry detection rules: the file and
the value are in the native 64-bit locations.

Custom detection script (Intune "script" rule or a ConfigMgr script detection; run it as **64-bit**
PowerShell, otherwise the HKLM and Program Files paths are redirected):

```powershell
$exe = Join-Path $env:ProgramW6432 'Yuval\Yuval.exe'
$run = (Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' -Name Yuval -ErrorAction SilentlyContinue).Yuval
if ((Test-Path $exe) -and $run -and ([version](Get-Item $exe).VersionInfo.ProductVersion -ge [version]'1.0.0')) {
    Write-Output 'Installed'; exit 0
}
exit 1
```

The dev build reports `ProductVersion` / `FileVersion` `1.0.0`. The version is set once for the build (it must
match in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`).

Uninstall timing: an NSIS uninstaller normally copies itself to `%TEMP%` and returns immediately, so a tool
that waits for the process to end can see "success" before files are gone. (Setup itself never runs an
uninstaller any more: updates and the replacement of CompanyIsland are done in place, section 6.) Use a
detection rule on the exe file so the tool re-evaluates after the uninstall.

**Replacing a CompanyIsland deployment (1.0.13 or older).** Run the Yuval setup with the same command as for a
new install (`/S`): it removes CompanyIsland itself (section 6), so no uninstall step and no supersedence with
"uninstall previous version" is needed. Retire the old app in your tool at the same time, or it will fail its
detection rule (`...\CompanyIsland\CompanyIsland.exe` is gone) and reinstall the old version: in Intune, remove the
assignment of the old Win32 app, or let the new app supersede it **without** "uninstall previous version" (the old
uninstall command points to a file that no longer exists); in SCCM, retire the old application or give it the new
detection method. Order does not matter for the setup: if the old app is still installed it is replaced, if it
is gone nothing is done. A PC where the old app is still assigned and gets reinstalled **after** Yuval ends up with
both installed side by side.

### 4.2 Uninstall

| Method | Command / location |
|---|---|
| Silent | `"%ProgramFiles%\Yuval\uninstall.exe" /S` (elevated) |
| Interactive | Settings > Apps > Yuval, or run `uninstall.exe` |

Uninstall closes running instances (a polite `taskkill` without `/F`, up to about 5 seconds, then `taskkill /F`,
in any session; Tauri's own check then finds nothing left), deletes the exe, uninstaller, shortcuts, the `center\`
folder and the HKLM `Run` and `Uninstall` values. An open Island Center is closed first the same way
(`taskkill /IM Yuval.Center.exe`), otherwise it would lock its own files. It **keeps per-user data** (section 7).
The "delete app data" checkbox of the interactive uninstaller only cleans the
administrator's own `%LOCALAPPDATA%\com.companyisland.app`, which is empty.

## 5. Startup behaviour

| Layer | Mechanism | Who controls it |
|---|---|---|
| Machine-wide autostart | HKLM `Run` value `Yuval` (written by setup, removed by uninstall) | IT |
| Per-user opt-out | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`, value `Yuval`, `REG_BINARY` of 12 bytes: first byte `02` = enabled, `03` = disabled, the rest zero (`src-tauri/src/autostart.rs`) | The user (Settings tab > "Launch with Windows", or Task Manager > Startup apps) |
| Setting | `launchWithWindows` in `settings.json`, default `true` | The user |

Behaviour: the app adopts a change made outside it at its next start (Task Manager writes the same value).
With no per-user value at all, Windows runs the machine-wide entry, i.e. enabled by default for every user.
If the user's setting says "off" but the registry value has gone missing, the app writes the opt-out again.

**IT: switch autostart off for one user** (logon script, GPP registry item or Intune remediation, in the
user's context):

```powershell
$k = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
New-Item -Path $k -Force | Out-Null
Set-ItemProperty -Path $k -Name Yuval -Type Binary -Value ([byte[]](3,0,0,0,0,0,0,0,0,0,0,0))
```

Use `2,0,0,...` to switch it back on. The user can still re-enable it in the app, so this is a default, not a
lock. **IT: switch autostart off for everyone**: delete the HKLM `Run` value `Yuval` (a GPP registry
item with action Delete); note that re-running or upgrading the installer writes it again. The Windows
"Do not process the legacy run list" policy (`DisableLocalMachineRun`) would also suppress HKLM `Run`
entries **(unverified here)**; it blocks every program in that key, not only this one.

A second launch in the same Windows session does not start a second island: it tells the running one to
toggle open and exits.

**First run and the Welcome window (1.0.4).** Roughly 1.5 seconds after the island starts for a user whose
settings do not say `onboardingDone: true`, the island opens the Island Center on its Welcome page (a normal
window, about 1000x700, in Hebrew, with a guided tour button) and then records `onboardingDone: true` in that
user's `settings.json`. It happens **once per user**, for a new user after the first logon and for an existing
user after the first start following an upgrade from a version without the flag. It does not repeat at later
logons. If the Center is not installed or cannot start, nothing is shown (`APP-030` in the log) and it is tried
again at the next start. The Center is never started at logon by itself; afterwards it opens only when the user
clicks "מרכז האי" in the tray menu or a button in the island. To suppress the window for a managed rollout,
pre-create `%LOCALAPPDATA%\Yuval\settings.json` containing `{"onboardingDone": true}` (other fields fall
back to their defaults; partial files load with defaults by a unit test, the recipe itself has not been run
**(unverified)**), or accept it as the product's first-run explanation.

## 6. Upgrade behaviour

The setup **updates in place and never runs an old uninstaller**, in every mode (interactive, `/P`, `/S`); details
and the reasons are in [INSTALLER.md](INSTALLER.md) "Upgrades and migration".

- **Same product, newer or same version (update / repair).** Run the new setup with the same commands as for a new
  install. A running `Yuval.Center.exe` and `Yuval.exe` are closed first (any session; asked politely, forced after
  about 5 seconds), files are replaced in place, `center\` is removed and re-copied so no file of an older Center
  version is left behind, the Add/Remove Programs entry (`DisplayVersion`) and the HKLM `Run` value are rewritten
  (a deleted value comes back), the Start menu shortcut is refreshed, and a desktop shortcut the user deleted is
  not recreated by a silent update. The interactive setup has no "reinstall / uninstall first" page, and it does not
  ask for the folder: it updates the registered one (`/D=` still wins).
- **Older setup over a newer install (downgrade).** Refused: exit code **1638**, nothing is closed or changed;
  interactive shows a Hebrew/English message. To go back, uninstall first (per-user data is kept) and run the older
  setup.
- **CompanyIsland 1.0.13 or older installed (the product before the rename).** The Yuval setup replaces it: it closes
  `CompanyIsland.Center.exe` and `CompanyIsland.exe`, deletes the old program files (`CompanyIsland.exe`, its
  `uninstall.exe` **without running it**, `center\`, and the folder if it is then empty), the HKLM `Run` value
  `CompanyIsland`, `Uninstall\CompanyIsland`, `Software\CompanyIsland\CompanyIsland` and the old all-users Start menu
  and desktop shortcuts (only those that point at the old exe), then installs Yuval into `%ProgramFiles%\Yuval`
  (or `/D=`). Nothing under any `%LOCALAPPDATA%` or in HKCU is touched by the setup. Silent mode does the same.
- **Per-user data on that migration.** The app moves `%LOCALAPPDATA%\CompanyIsland` to `%LOCALAPPDATA%\Yuval` itself
  at its first start for each user, so settings, notes and reminder state are kept. Until a user starts Yuval for
  the first time their old folder is simply still there.
- **Is the app started again?** Interactive: the finish page offers "Run Yuval" (checked). Silent or passive: yes,
  when the app (Yuval, or CompanyIsland for a migration) **was running when setup closed it**, and always with `/R`;
  it is started as the interactive user through Explorer's token, never elevated as the installing administrator,
  and only for the console user. With no Explorer (setup run as SYSTEM, nobody logged on) nothing is started and users
  get the app at the next logon (HKLM `Run`). An open Island Center is closed and not reopened: a user with the
  Center open loses the window (an unsaved note being typed in it would be lost).
- The first start after an upgrade from a version before 1.0.4 opens the Welcome window once per user
  (section 5): their `settings.json` has no `onboardingDone` yet. Notes start empty; nothing else is migrated.
- Per-user data is forward compatible by design: `settings.json` is schema-versioned, unknown or invalid
  fields fall back to defaults, a file that cannot be read is renamed `settings.json.corrupt` and defaults are
  used.
- Autostart opt-outs: the per-user `StartupApproved\Run` opt-out is stored under the name of the `Run` value, which is
  now `Yuval`. The app is expected to carry an old `CompanyIsland` opt-out over; if it does not, a user who had
  switched autostart off gets the island at the next logon until they switch it off again (section 5).

## 7. Per-user data

All under `%LOCALAPPDATA%\Yuval\` (resolved with the shell known-folder API, not environment variables;
never `%APPDATA%`, never the install folder).

| Path | Content | Notes |
|---|---|---|
| `settings.json` | User settings (reminder on/off, 30-minute offset, monitor, launch with Windows, hide in fullscreen, notifications, debug logging, and since 1.0.4 `onboardingDone` and `islandDisplay`) | Atomic write (temp file + rename). Corrupt file is kept as `settings.json.corrupt`. |
| `state\notes.json` | The user's notes (1.0.4): `{"schemaVersion":1,"notes":[{id, text, createdAt, updatedAt, pinned}]}` | **User-typed content, in clear text, per user.** At most 500 notes of 10,000 characters and 16 MiB in all. Atomic write. A corrupt or oversize file is kept as `notes.json.corrupt` and the list starts empty. Never logged, never sent anywhere, **kept on uninstall**. Only the island writes it (the Center asks the island over the pipe). |
| `EBWebView-Center\` | WebView2 profile of the Center's tour page (1.0.4) | Created the first time the Tour is shown; cache and crash dumps only; the island's own profile is `EBWebView`. |
| `state\reminders.json` | Which reminders already fired: `eventHash\|startUtc\|reminderType` -> time fired | No meeting text. Entries older than 7 days are dropped; at most 2000 entries; files over 256 KB are quarantined as `reminders.json.corrupt`. |
| `logs\companyisland.log` (+ `.1.log` ... `.4.log`) | Diagnostic log (section 8) | Rotates at 1 MB, 5 files kept (about 5 MB maximum). |
| `EBWebView\` | WebView2 profile (cache, crash dumps) | The app points WebView2 here, so nothing is created under `%LOCALAPPDATA%\com.companyisland.app`. |

There is no on-disk calendar cache: events live in memory and are re-read from Outlook after a restart.

**Wipe** (per user; as that user, or from a logon script, with the app closed):

```bat
rmdir /s /q "%LOCALAPPDATA%\Yuval"
rmdir /s /q "%LOCALAPPDATA%\CompanyIsland"
```

(The second line only matters on a PC where the user has not started Yuval since the rename; the app moves that
folder to `%LOCALAPPDATA%\Yuval` at its first start, and neither the installer nor the uninstaller touches either.)

To reset only the settings: delete `settings.json` (this also shows the Welcome window again once). To make
reminders re-fire for meetings still ahead: delete `state\reminders.json`. To remove only the user's notes:
delete `state\notes.json` (and `state\notes.json.corrupt` if present); decide with the user or your data policy
first, because notes are personal content that cannot be recovered. To also remove the autostart opt-out: delete the `Yuval` value under the
`StartupApproved\Run` key shown in section 5.

If the folder cannot be created (profile on a read-only or full volume), the app keeps running with
in-memory settings and without a log file (codes `APP-002`, `APP-010`).

## 8. Logs

| | |
|---|---|
| File | `%LOCALAPPDATA%\Yuval\logs\companyisland.log` |
| Open | tray menu "Open logs", or Settings tab > "Open logs" |
| Rotation | at 1 MB the file becomes `.1.log`; five files in total (current + `.1` to `.4`) |
| Level | `INFO` and above. Debug is opt-in: setting `debugLogging` or environment variable `COMPANYISLAND_LOG=debug` |
| Format | one line per entry: `2026-10-06 16:23:01.289 INFO  [calendar] status waiting -> waiting code=OUTLOOK-101 events=0` |
| Crash marker | the last line `---- clean exit ----` is written on normal exit; if missing at next start the log says `previous session did not exit cleanly` |

Logged: app version, OS and architecture, a random session id, status transitions with error codes, counts and
timings, an 8-hex hash prefix of an event id when a reminder fires, UI view/tab/size changes, window and
display events. User profile paths are replaced with `%USERPROFILE%`. Control characters in a log entry are
neutralized so one entry is always one line.

Never logged: meeting subjects, locations, organizers, attendees, bodies, notification titles or text, e-mail
addresses, Outlook profile names (only a hash in debug mode), and note text (1.0.4; the log has only counts such
as `[notes] saved 12 notes`, and `[center]` lines with page names, process ids and codes; a note's id is never
written). The Center's own error reports reach the same log as short codes through the pipe (scope `center`).

"Copy diagnostics" (Settings tab, explicit click, clipboard only) is separate from the log: its first line is a one-line summary for tickets, then it contains Windows
user (`DOMAIN\USER`), computer name, local IPv4 and adapter name, OS, build, WebView2 version, session id,
Outlook status and mode, error code, cached-event count, last sync time and the last error codes. No meeting
content. Treat it as containing personal identifiers when users paste it into tickets.

Error codes are listed in [ARCHITECTURE.md](ARCHITECTURE.md) section 8.

## 9. Network, proxy, firewall

None required. The app makes no network connections by design: no telemetry, no update check, no cloud or AI
calls, no HTTP client library. The page's Content-Security-Policy only allows its own origin and the local
Tauri IPC. WebView2 is started with `--disable-background-networking` (plus wry's defaults and
`msSmartScreenProtection` disabled), so the runtime does not phone home for component or safe-browsing
updates from this app's profile. The only network-related code reads the local adapter list to show the
local IPv4 address in the About tab; it sends nothing.

The Island Center (1.0.4) changes none of this. It makes no network connection: it talks to the island over a
**local named pipe** (`\\.\pipe\CompanyIsland.Center.<session id>.<hash of the user's SID>`) that exists only on
that PC, is readable and writable by that user alone (a protected access list naming only the user's SID),
refuses remote clients and refuses connections from another Windows session. There is no listening TCP or UDP
port and no firewall rule is needed. Its tour page is local files in `center\web\`, served to the Center's
WebView2 through a virtual host name (`tour.companyisland.invalid`, which never resolves in DNS; it is a mapping
inside WebView2, not a network name). The page's Content-Security-Policy has `connect-src 'none'`, and the host
cancels any navigation to another address, any new window and any download. The Center's WebView2 starts with the
same `--disable-background-networking` argument as the island's. A traffic capture of the Center has not been done
**(unverified)**: include `Yuval.Center.exe` and its `msedgewebview2.exe` children in the 10-minute check
above.

To confirm in your environment, capture traffic for `Yuval.exe` and its `msedgewebview2.exe` children
(parent = `Yuval.exe`) during a 10-minute run; the expected result is no outbound connections.
(Automated coverage only pins the WebView2 command-line flags; a traffic capture has not been done in this
repository **(unverified)**.)

Outlook itself does its own network traffic (Exchange / Microsoft 365). Yuval reads Outlook's local
object model and adds none.

## 10. Outlook requirements

| Requirement | Detail |
|---|---|
| Product | **Classic Outlook for Windows** (`OUTLOOK.EXE`): Microsoft 365 Apps (Click-to-Run) or Office 2016 / 2019 / 2021 (MSI or C2R). Version 2016+ is the supported baseline, 64-bit and 32-bit Office alike (out-of-process COM through the Running Object Table). Only the object-model calls are verified in code; each Office build still needs the QA run **(unverified)**. |
| Not supported | **New Outlook** (`olk.exe`, the "Outlook (new)" app) has no COM automation. If only `olk.exe` runs, the Calendar tab says "New Outlook isn't supported - Switch to classic Outlook to see your meetings" and diagnostics show `OUTLOOK-104`. |
| Running | Outlook must be running. Yuval never starts Outlook, never picks a profile, never calls Logon. It attaches to the running instance through the Running Object Table. Until Outlook runs: "Waiting for Outlook" (`OUTLOOK-101`, informational). |
| Same user, session, integrity | It only considers `OUTLOOK.EXE` in the same Windows session, owned by the same user SID, at the same elevation. Elevated Outlook with a normal island (or the reverse) shows "Outlook runs with different permissions" (`OUTLOOK-103`). Do not run either elevated. |
| Calendar | The **default calendar of the default store** only, today to +48 hours, at most 50 events. Other mailboxes, shared and secondary calendars are not read in v1. |
| Prompts | None expected. Only these properties are read: EntryID (hashed immediately), Subject, Start, End, Location, Organizer (display name), AllDayEvent, IsRecurring, BusyStatus, ResponseStatus. Never body, attendees/recipients, e-mail addresses, attachments or user properties. No Trust Center "programmatic access" dialog is expected for these **(unverified against each Office build)**. Organizer is protected by the object model guard: where policy (or an antivirus Outlook does not recognise) blocks it, the events are shown without the organizer and Organizer is not read again for 30 minutes (logged as `OUTLOOK-110 Organizer blocked`). If the guard blocks the calendar itself, the code is `OUTLOOK-110` and the app retries with backoff. |
| Cached Exchange mode | Reads Outlook's local data; works offline. |
| Modal dialogs / startup | If Outlook rejects calls (modal dialog, still starting) the app retries with backoff (`OUTLOOK-105`); after repeated busy answers the island says "Outlook isn't responding". |

Meeting join links are recognised only inside the Location text and only for teams.microsoft.com,
teams.live.com, zoom.us, webex.com and meet.google.com.

## 11. Group Policy, AppLocker, WDAC, security software

Only behaviour justified by the code or by Microsoft documentation is listed. "Doc" means Microsoft
documentation, not tested here.

| Area | What matters | Basis |
|---|---|---|
| Notification access policy | If `HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy\LetAppsAccessNotifications` is `2` (force deny), notification mirroring reports `policy` / `NOTIF-202` and is off. Calendar, date and reminders are unaffected. Other Windows denials report `denied` / `NOTIF-201`. | Code (`notifications.rs`) |
| Notification mirroring on unpackaged apps | `UserNotificationListener` often cannot subscribe for an unpackaged exe (`NOTIF-204 NotificationChanged unavailable (0x80070490)` was observed on Windows 11 25H2). The app then falls back to reading the Action Center list every 5 seconds (30 seconds after repeated failures) and `notificationMode` in diagnostics reports `polling`. | Code (`notifications.rs`) + dev-box log |
| Autostart | `HKLM\...\Run` is a normal Run value; blocking policies for Run keys stop it. `StartupApproved` is per user and needs no policy. | Code, doc |
| AppLocker / WDAC | The exes are **unsigned** unless you sign them (section 14). Allow by path (`%ProgramFiles%\Yuval\*` covers the island and the Center; AppLocker's default "everything in Program Files" rule already covers it, but a rule that allows only `Yuval.exe` does not: since 1.0.4 add `%ProgramFiles%\Yuval\center\*`, in particular `Yuval.Center.exe` and the DLLs beside it), by file hash (changes every build, and now covers about 350 files, so prefer path or publisher) or by publisher once signed (sign `Yuval.Center.exe` and, for a WDAC publisher rule that also covers libraries, the Yuval-built `Yuval.Center.dll` and `Yuval.Center.Core.dll` as well; the many Microsoft files in `center\` are expected to carry Microsoft's signature; the signatures of the published files have not been inspected **(unverified)**). If DLL rules are enforced, the DLLs in `center\` need to be allowed too. Setup (`Yuval_..._x64-setup.exe`) runs from a temp location and may need its own allow rule. | Code, doc |
| What is executed | The island exe (alone in the install folder with the uninstaller, **no DLL next to it**) and, since 1.0.4, `center\Yuval.Center.exe`. The Center is a self-contained .NET 10 / Windows App SDK 1.8 program: it loads the DLLs from its own `center\` folder (its own `Yuval.Center*.dll`, the .NET runtime and Windows App SDK libraries) plus system DLLs and Microsoft's WebView2 runtime. The WinUI libraries are Microsoft's; .NET is loaded from the same folder, never from the machine. | `installer.nsi` copies `Yuval.exe` and the resource list of `center\` (347 files for 1.0.4); ENTERPRISE_DESIGN section 7 import-table audit for the island only |
| Island Center | A second process per user session, started by the island (or by the user from `center\`), `asInvoker`, never elevated, no services, tasks or autostart of its own. Starts only after a click or once at the first run (section 5). Its parent is `Yuval.exe` when the island started it (an EDR rule on "unsigned child process" sees this). It reads and writes no file of the island: it asks the island through the named pipe (section 9). | Code |
| WebView2 child processes | The island hosts WebView2, so it starts `msedgewebview2.exe` (browser, renderer, GPU, utility, crashpad) from the runtime folder (typically `C:\Program Files (x86)\Microsoft\EdgeWebView\Application\<version>\`). The Island Center starts the same processes, with the Center as parent, while the Tour page has been shown (profile `%LOCALAPPDATA%\Yuval\EBWebView-Center`). These are Microsoft-signed and **must be allowed** by AppLocker/WDAC and EDR, otherwise the island never renders (and the tour shows a message). | Doc |
| Defender ASR | "Block executable files from running unless they meet a prevalence, age, or trusted list criterion" can block a brand-new unsigned exe: add an exclusion or sign it. "Block process creations originating from PSExec and WMI" and the Office rules ("Office applications creating child processes", etc.) are not relevant: Yuval is not started from Office and uses no WMI. | Doc |
| Child processes started | `explorer.exe` (absolute path from the Windows folder) for "Open logs", the shell for activating the app behind a mirrored toast (only AUMIDs seen in toasts), and, since 1.0.4, `<install folder>\center\Yuval.Center.exe --page <page>` (absolute path, no shell; the page is one of a fixed list, a note shows as `note:<id>` with a validated id). The installer runs `taskkill`. | Code |
| Key-logger heuristics | Raw keyboard/mouse input registration is disabled (`DeviceEventFilter::Always`), there is no global mouse or keyboard hook, and no code injection: the fullscreen detector uses out-of-context `SetWinEventHook` only (foreground / minimize / location events). | Code |
| SmartScreen | A downloaded (mark-of-the-web) unsigned installer triggers "Windows protected your PC". Software-distribution tools do not add the mark. Signing fixes it for the publisher over time. | Doc |
| Privileges | `asInvoker`; no services, drivers, scheduled tasks, firewall rules, or HKLM writes at run time. | Code |
| Outlook programmatic access | See section 10 "Prompts". | Code, doc |

## 12. Multi-user, Fast User Switching, RDS / VDI

- **One instance per Windows session.** The single-instance mutex has no `Global\` prefix, so it is
  session-local: each interactive session (local user, Fast User Switching, RDP) runs its own island with its
  own data (`%LOCALAPPDATA%` of that user). A second launch in the same session toggles the running island.
- **The Island Center is per user and session too.** One Center per session (a second launch hands its page to
  the first and exits), and its pipe name contains the Windows session id and a hash of the user's SID, so two
  users or two sessions on one PC each have their own pipe, and a session can never connect to another's (the
  island also checks the client's session id). Another user, even an administrator, cannot open the pipe: its
  access list names only the owner **(not yet tested with a second account, see QA_MATRIX E43)**.
- **Outlook discovery is filtered by session and SID**: another user's Outlook on the same machine is never
  attached, and does not affect the status ("Waiting").
- **Fast User Switching**: a disconnected session keeps its island and keeps polling for Outlook every 15 s in
  the background; the user's reminders are shown when the session is active again. (Behaviour on switch-back
  is covered by QA scenario 9, hardware needed.)
- **RDS / VDI**: autostart works through the HKLM `Run` key. In non-persistent VDI the profile is recreated, so
  `settings.json`, `reminders.json` and the user's notes (`state\notes.json`) are lost, and the Welcome window
  shows again at every new profile, unless the profile solution (FSLogix, UPD) roams
  `%LOCALAPPDATA%\Yuval`; roaming `EBWebView` and `EBWebView-Center` is not needed and can be excluded. Many concurrent
  sessions each run an island plus WebView2 processes: size memory accordingly **(unverified, see QA matrix)**.

## 13. Known limitations

- Windows x64 only; Windows 10 21H2 or newer.
- Classic Outlook only. New Outlook and Outlook on the web are not supported; Microsoft Graph is a future
  provider (see ARCHITECTURE.md section 10).
- Default calendar of the default store only; no shared or secondary calendars; 48-hour horizon; 50 events max.
- Reminder offsets in the UI are 5 / 10 / 15 / 30 minutes (default 30, stored value clamped to 0 to 120).
- The island sits at the very top of the monitor (on a top-docked taskbar it overlays the taskbar's middle).
  The window is non-activating (`WS_EX_NOACTIVATE`) and hidden from Alt+Tab: it never takes keyboard focus.
- Notification mirroring is best effort (section 11) and can be off by policy.
- No auto-update. Roll out new versions by running the new setup.
- Installer is about 207 MiB (1.0.0) because it carries the WebView2 runtime, and 1.0.4 adds the Island Center's
  files (about 86 MiB uncompressed on disk). Unsigned by default.
- Hebrew and English UI strings; other languages get English text with locale-correct dates. The island and the
  Island Center always show Hebrew.
- Notes are plain text in the user's profile, with no encryption, sync or backup; roaming profiles or FSLogix
  containers carry them with `%LOCALAPPDATA%\Yuval` (section 12), otherwise they exist on one PC only.
- The Island Center has not been run on Windows 10 21H2 or at a display scale above 100% (QA_MATRIX E44, E45).

## 14. Code signing recommendation

Sign **all three** artifacts so AppLocker/WDAC publisher rules, SmartScreen and EDR reputation work:
`Yuval.exe`, `uninstall.exe` and the setup `.exe`. Tauri signs all three when a certificate is
configured under `bundle.windows` in `src-tauri/tauri.conf.json` (`certificateThumbprint` + `digestAlgorithm`
+ `timestampUrl`, or a `signCommand`); see [INSTALLER.md](INSTALLER.md) "Code signing" for the exact keys.
Nothing is signed or configured today, on purpose. Since 1.0.4 there is a fourth thing to sign for a publisher
rule: `center\Yuval.Center.exe` (plus the two `Yuval.Center*.dll` if DLL rules are enforced). Tauri's
signing step does not cover bundle resources, so it has to be done on `center/publish/` before the setup is built
(INSTALLER.md "Code signing") **(unverified)**.

| Choice | Recommendation |
|---|---|
| Certificate | Your corporate code-signing certificate from an internal or public CA. For an internal-only rollout, an internal CA that is already trusted by the fleet (and referenced by your WDAC publisher rule) is enough. For distribution outside the managed fleet, a public OV or EV certificate. |
| OV vs EV | OV is sufficient for managed devices (policy trusts the publisher). EV gives immediate SmartScreen reputation, which only matters for unmanaged downloads. |
| Key handling | Prefer an HSM or cloud signing service via `signCommand` (for example `{ "cmd": "signtool", "args": ["sign", "/fd", "sha256", "/tr", "<timestamp-url>", "/td", "sha256", "/a", "%1"] }`). Never commit a `.pfx`. |
| Timestamp | Always timestamp (RFC 3161, SHA-256) so signatures stay valid after the certificate expires. |
| Verify | `Get-AuthenticodeSignature` on all three files, and `scripts/verify-installer.ps1` for size, hash and PE type (it does not check the signature). |

## 15. One-page deployment checklist

Before the pilot

- [ ] Target PCs: 64-bit, Windows 10 build 19044+ or Windows 11. Fleet uses **Classic** Outlook (not New Outlook).
- [ ] Decide signing: sign the three artifacts, or add AppLocker/WDAC and Defender exclusions for the unsigned build.
- [ ] Allow `msedgewebview2.exe` (Microsoft-signed) in AppLocker/WDAC/EDR if application control is on.
- [ ] Allow the Island Center: `%ProgramFiles%\Yuval\center\*` (path rule) or its publisher once signed; it is a second exe with about 350 files (section 11).
- [ ] Tell users about the Welcome window that opens once at the first start after install or upgrade, and that notes they write are kept in their profile (`state\notes.json`, section 7).
- [ ] Decide autostart default (HKLM `Run`, on) and whether to pre-set per-user opt-outs.
- [ ] Check `LetAppsAccessNotifications` policy if toast mirroring is wanted (value 2 turns it off).
- [ ] Confirm no policy forces Outlook to run elevated or as a different user than the desktop user.

Package and deploy

- [ ] Verify the setup file: `powershell -File scripts/verify-installer.ps1 <setup.exe>`; record SHA256.
- [ ] Intune / SCCM: install `Yuval_1.0.14_x64-setup.exe /S`, uninstall `"%ProgramFiles%\Yuval\uninstall.exe" /S`, run as System, no reboot.
- [ ] Detection: `C:\Program Files\Yuval\Yuval.exe` version >= 1.0.14 (+ HKLM `Run` value `Yuval`).
- [ ] Replacing CompanyIsland 1.0.13 or older: the same install command does it (no uninstall step); retire or re-point the old app's assignment so its detection rule does not reinstall it (section 4.1).
- [ ] Exit codes in the deployment tool: `0` success, `1603` unsupported OS, `1638` a newer version is installed (treat as "already compliant" or investigate, nothing was changed).
- [ ] Pilot ring first: 10 to 20 users covering Windows 10 21H2 and Windows 11, at least one 125% / 150% DPI laptop and one dual-monitor desk.

After install (per pilot user, after next logon)

- [ ] Island visible at the top centre; shows `d/M`, the time and the weekday; no UAC prompt; not elevated in Task Manager (Details > Elevated = No).
- [ ] The Welcome window of the Island Center opened once (about 2 seconds after the island started) and does not open again at the next logon; `<install folder>\center\Yuval.Center.exe` exists; the tray item "מרכז האי" opens it; the Notes tab and the Center's Notes page show the same notes.
- [ ] With Classic Outlook running: Calendar tab shows meetings; Settings tab (diagnostics) shows Outlook "Running", mode "Classic Outlook".
- [ ] Meeting 30 minutes ahead produces one alert, once.
- [ ] No outbound traffic from `Yuval.exe`, `Yuval.Center.exe` and their WebView2 children.
- [ ] Logs exist in `%LOCALAPPDATA%\Yuval\logs`; no meeting text and no note text in them.
- [ ] Help desk knows: Settings tab > "Copy diagnostics" for tickets; error codes in ARCHITECTURE.md section 8; per-user wipe command (section 7).

Operations

- [ ] Upgrade = run the new setup silently; it updates in place, and the island is started again for the logged-on user if it was running (section 6).
- [ ] Offboarding: uninstall command above, plus per-user data wipe via logon script if required.

## 16. Not verified

Everything marked **(unverified)** above, collected. Re-test on a real Windows 10 21H2 machine and a Windows 11
machine with Classic Outlook before broad rollout ([QA_MATRIX.md](QA_MATRIX.md)).

- The installer itself has not been executed end to end for this guide (no install exists on the dev PC):
  exit codes, shortcuts, the `Run` / `Uninstall` / `Software\<publisher>\Yuval` keys and the 64-bit registry view
  are read from `installer-hooks.nsh` and the generated `installer.nsi`. What was run: the installer script compiles,
  and its logic (version decision with the exit codes 1638 and 1603, the migration's file and shortcut removal, closing
  a real process) passes in a temp folder (`node scripts/test-installer.cjs`). **Not run**: the migration from a real
  CompanyIsland 1.0.13 and the in-place update of a real Yuval, interactive and silent (INSTALLER.md, "Manual test
  plan"), the registry cleanup, and the restart of the app as the logged-on user.
- Windows 10 N, LTSC and ARM64-with-emulation behaviour.
- The EdgeUpdate-based update of a too-old WebView2 runtime, and fixed-runtime operation
  (`BrowserExecutableFolder` / `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`) with Yuval.
- `/R` and the automatic restart of the app when setup runs as SYSTEM (expected: nothing starts, the `Run` value
  starts it at the next logon); `RunAsUser` is read from the plugin's imports, not run.
- The `DisableLocalMachineRun` policy effect; Defender ASR, AppLocker default-rule coverage of the WebView2
  folder, and SmartScreen behaviour (Microsoft documentation, not tested).
- Zero outbound connections (no traffic capture has been done).
- Office 2016 / 2019 / 2021 / Microsoft 365 builds, 32-bit vs 64-bit Office, and "no Trust Center prompt" for
  the property set that is read.
- Fast User Switching, RDS and VDI behaviour and memory per session.
- Whether a Windows logoff leaves the `previous session did not exit cleanly` marker.
- `UserNotificationListener` on Windows 10 21H2 for an unpackaged exe (observed on Windows 11 25H2 only).
- The `MeetingStatus` read that hides canceled meetings (read-only number; failures are tolerated) has not been
  run against a real Outlook.
- The Island Center (1.0.4): it has not been run on Windows 10 21H2, at a display scale above 100%, or from an
  installed copy against the real island's pipe in a recorded QA run; the 1.0.4 installer has not been built, so
  its real size, the exact file list under `center\`, the closing of an open Center on upgrade and uninstall, and
  the publisher signatures of the files in `center\` are unchecked; no traffic capture of the Center exists; the
  pipe's refusal of another user, session or remote client rests on the access list and a code check, not on a
  test with a second account (QA_MATRIX E33 to E46).
- The installer's wizard text for Hebrew: NSIS's own pages are Hebrew, and since 1.0.14 `installer-hooks.nsh` adds
  Hebrew for Tauri's custom messages and for its own (the build checks that none is missing); how the Hebrew
  (right-to-left) pages and message boxes look has not been seen on a screen.
