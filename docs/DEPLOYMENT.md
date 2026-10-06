# CompanyIsland - IT deployment guide

Audience: desktop engineering / endpoint management. Build instructions, installer internals and the
signing hook live in [INSTALLER.md](INSTALLER.md); the design contract is
[ENTERPRISE_DESIGN.md](ENTERPRISE_DESIGN.md); the test plan is [QA_MATRIX.md](QA_MATRIX.md).

CompanyIsland is a small always-on-top overlay ("island") that shows the date and weekday, and reminds the
signed-in user of upcoming **Classic Outlook** meetings. It is offline by design, runs as a standard user,
and keeps all of its data in the user's own profile.

Values below were checked against the repository (file names are given so you can re-check after a rebuild).
Facts that could not be verified in the repository or on a test machine are marked **(unverified)** and are
collected in the "Not verified" list at the end.

## 1. Supported platforms

| Item | Supported | Notes |
|---|---|---|
| Windows 10 | 21H2 (build 19044), 22H2 (19045) | Minimum is build 19044 (`COMPANYISLAND_MIN_BUILD` in `src-tauri/installer-hooks.nsh`). |
| Windows 11 | all builds (22000+) | Win11 already ships the WebView2 runtime. |
| Architecture | x64 only | The installer refuses anything that is not 64-bit Windows. No ARM64 or x86 build. |
| Editions | Pro / Enterprise / Education | Windows 10 N editions and LTSC builds are not specifically tested **(unverified)**. LTSC 2021 is build 19044 and passes the installer gate. |
| Server SKUs, RDS, VDI | see section 12 | Not a primary target. |

Unsupported OS: the installer exits with code **1603** (silent) or shows a message box (interactive) and
installs nothing, before it touches WebView2 or running processes.

## 2. Dependencies

| Dependency | Needed? | How it is satisfied |
|---|---|---|
| Microsoft Edge WebView2 Runtime (Evergreen), version 111.0.1661.41 or newer | Yes | See below. |
| Visual C++ redistributable | No | The exe is built with `+crt-static` (`src-tauri/.cargo/config.toml`). |
| .NET, Java, Node, Outlook PIAs | No | Outlook is automated late-bound from Rust; no interop assemblies. |
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
`WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` environment variable **(unverified with CompanyIsland)**. IT then owns
patching: every WebView2 security release must be repackaged and redeployed. The shipped configuration is
Evergreen; the NSIS script's Evergreen check would still try to install Evergreen if it finds none, so a
fixed-runtime-only estate needs the rebuild.

## 3. Permissions

| Phase | Account | What it touches |
|---|---|---|
| Install / upgrade / uninstall | Administrator (UAC elevation or SYSTEM) | `%ProgramFiles%\CompanyIsland`, HKLM `Run` and `Uninstall` keys, Start menu and desktop shortcuts, WebView2 runtime |
| Run | **Standard user. No UAC prompt.** The exe manifest is `asInvoker` (`src-tauri/windows-app.manifest`) | Read/write `%LOCALAPPDATA%\CompanyIsland\**` and `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run` only. Reads (never writes) a few HKLM values (OS version, notification policy). Never writes HKLM. |

The island must not be run elevated: it would then no longer match a normal-integrity Outlook (section 10).

## 4. Installation

Installer: `CompanyIsland_1.0.0_x64-setup.exe` (NSIS, per-machine, about 207 MiB because it embeds WebView2).
Language follows the Windows UI language (English or Hebrew, no selector); dates and weekdays follow the Windows regional format.

| Task | Command (run elevated) |
|---|---|
| Interactive | double-click the setup file, accept UAC |
| Silent | `CompanyIsland_1.0.0_x64-setup.exe /S` |
| Silent, custom folder | `CompanyIsland_1.0.0_x64-setup.exe /S /D=D:\Apps\CompanyIsland` |
| Passive (progress bar, no questions) | `CompanyIsland_1.0.0_x64-setup.exe /P` |
| No Start menu / desktop shortcuts | add `/NS` |
| Start the app for the current user after install | add `/R` (silent or passive only) |

Notes:

- `/D=` must be the **last** argument, with no quotes, even if the path has spaces (standard NSIS rule).
  The default folder is `%ProgramFiles%\CompanyIsland` (`C:\Program Files\CompanyIsland`). Detection rules and
  the uninstall command below assume the default; adjust them if you use `/D=`.
- Flags `/S /D= /P /NS /R /UPDATE` come from the Tauri-generated `installer.nsi` (generated into
  `src-tauri/target/release/nsis/x64/installer.nsi` at build time). `/UPDATE` is for in-app updaters and is
  not needed for deployment.
- Silent installs create the shortcuts (Start menu folder `CompanyIsland`, all-users desktop) unless `/NS` is
  given. The setup does **not** launch the app: without `/R` it first starts at the user's next logon through
  the HKLM `Run` value. `/R` starts the app as the interactively logged-on user; its behaviour when the setup
  runs as SYSTEM with nobody logged on is **(unverified)**, so prefer "starts at next logon" in managed
  rollouts.
- Exit codes: `0` success, `1603` unsupported OS (nothing installed), `2` aborted, other non-zero values are
  standard NSIS codes. A silent install that finds the app running closes it via Restart Manager; if it cannot,
  it aborts with a message on the console.
- What is installed: `CompanyIsland.exe` (about 6.5 MiB) and `uninstall.exe`. No resource files, no DLLs, no
  services, no scheduled tasks, no drivers, no firewall rules.
- Registry written by setup (64-bit view):
  - `HKLM\Software\Microsoft\Windows\CurrentVersion\Run` : `CompanyIsland` = `"C:\Program Files\CompanyIsland\CompanyIsland.exe"`
  - `HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\CompanyIsland` (Add/Remove Programs entry, `DisplayVersion`, `UninstallString`)
  - `HKLM\Software\CompanyIsland\CompanyIsland` (install folder, used for upgrades; uninstall leaves it unless the interactive "delete app data" box is ticked)

### 4.1 Intune (Win32 app) and SCCM / Configuration Manager

Package the setup file as-is (for Intune, wrap the folder with `IntuneWinAppUtil.exe -c <folder> -s CompanyIsland_1.0.0_x64-setup.exe -o <out>`).

| Setting | Value |
|---|---|
| Install command | `CompanyIsland_1.0.0_x64-setup.exe /S` |
| Uninstall command | `"%ProgramFiles%\CompanyIsland\uninstall.exe" /S` |
| Install behaviour / run as | System (SCCM: "whether or not a user is logged on", "run with administrative rights") |
| Device restart behaviour | No specific action; no reboot is required |
| Requirements | 64-bit OS, Windows 10 build 19044 or newer |
| Max run time | 30 minutes is generous: WebView2 installation is the slow part on a clean Windows 10 |
| Detection (simple) | File: `C:\Program Files\CompanyIsland\CompanyIsland.exe`, version (string) greater than or equal to `1.0.0` |
| Detection (stronger) | The same file rule **and** registry value `HKLM\Software\Microsoft\Windows\CurrentVersion\Run` name `CompanyIsland` exists (see script below) |

Do not tick "associated with a 32-bit app on 64-bit systems" in file or registry detection rules: the file and
the value are in the native 64-bit locations.

Custom detection script (Intune "script" rule or a ConfigMgr script detection; run it as **64-bit**
PowerShell, otherwise the HKLM and Program Files paths are redirected):

```powershell
$exe = Join-Path $env:ProgramW6432 'CompanyIsland\CompanyIsland.exe'
$run = (Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' -Name CompanyIsland -ErrorAction SilentlyContinue).CompanyIsland
if ((Test-Path $exe) -and $run -and ([version](Get-Item $exe).VersionInfo.ProductVersion -ge [version]'1.0.0')) {
    Write-Output 'Installed'; exit 0
}
exit 1
```

The dev build reports `ProductVersion` / `FileVersion` `1.0.0`. The version is set once for the build (it must
match in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`).

Uninstall timing: an NSIS uninstaller normally copies itself to `%TEMP%` and returns immediately, so a tool
that waits for the process to end can see "success" before files are gone. The installer's own upgrade path
uses the in-place form `uninstall.exe /S _?=<install folder>` (visible in the generated `installer.nsi`),
which blocks until done; that form may leave `uninstall.exe` and the folder behind **(unverified)**. Use a
detection rule on the exe file so the tool re-evaluates after the uninstall.

### 4.2 Uninstall

| Method | Command / location |
|---|---|
| Silent | `"%ProgramFiles%\CompanyIsland\uninstall.exe" /S` (elevated) |
| Interactive | Settings > Apps > CompanyIsland, or run `uninstall.exe` |

Uninstall closes running instances (a polite `taskkill` without `/F`, then Tauri force-closes whatever is left
in any session), deletes the exe, uninstaller, shortcuts and the HKLM `Run` and `Uninstall` values. It **keeps
per-user data** (section 7). The "delete app data" checkbox of the interactive uninstaller only cleans the
administrator's own `%LOCALAPPDATA%\com.companyisland.app`, which is empty.

## 5. Startup behaviour

| Layer | Mechanism | Who controls it |
|---|---|---|
| Machine-wide autostart | HKLM `Run` value `CompanyIsland` (written by setup, removed by uninstall) | IT |
| Per-user opt-out | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`, value `CompanyIsland`, `REG_BINARY` of 12 bytes: first byte `02` = enabled, `03` = disabled, the rest zero (`src-tauri/src/autostart.rs`) | The user (Settings tab > "Launch with Windows", or Task Manager > Startup apps) |
| Setting | `launchWithWindows` in `settings.json`, default `true` | The user |

Behaviour: the app adopts a change made outside it at its next start (Task Manager writes the same value).
With no per-user value at all, Windows runs the machine-wide entry, i.e. enabled by default for every user.
If the user's setting says "off" but the registry value has gone missing, the app writes the opt-out again.

**IT: switch autostart off for one user** (logon script, GPP registry item or Intune remediation, in the
user's context):

```powershell
$k = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
New-Item -Path $k -Force | Out-Null
Set-ItemProperty -Path $k -Name CompanyIsland -Type Binary -Value ([byte[]](3,0,0,0,0,0,0,0,0,0,0,0))
```

Use `2,0,0,...` to switch it back on. The user can still re-enable it in the app, so this is a default, not a
lock. **IT: switch autostart off for everyone**: delete the HKLM `Run` value `CompanyIsland` (a GPP registry
item with action Delete); note that re-running or upgrading the installer writes it again. The Windows
"Do not process the legacy run list" policy (`DisableLocalMachineRun`) would also suppress HKLM `Run`
entries **(unverified here)**; it blocks every program in that key, not only this one.

A second launch in the same Windows session does not start a second island: it tells the running one to
toggle open and exits.

## 6. Upgrade behaviour

- Run the new setup over the old one (same commands). Files are replaced in place; `HKLM\...\Run` is rewritten.
- A running instance is closed first (Restart Manager). The app is **not** restarted afterwards (unless `/R`),
  so users get it back at next logon or from the Start menu. The island is gone from the screen until then.
- The downgrade check is off (`ALLOWDOWNGRADES` is `true` in the generated installer), so installing an older
  setup over a newer one is allowed.
- Per-user data is forward compatible by design: `settings.json` is schema-versioned, unknown or invalid
  fields fall back to defaults, a file that cannot be read is renamed `settings.json.corrupt` and defaults are
  used.
- The interactive setup shows a "reinstall / uninstall first" page when the product is already installed; the
  silent path skips it.

## 7. Per-user data

All under `%LOCALAPPDATA%\CompanyIsland\` (resolved with the shell known-folder API, not environment variables;
never `%APPDATA%`, never the install folder).

| Path | Content | Notes |
|---|---|---|
| `settings.json` | User settings (reminder on/off, 30-minute offset, monitor, launch with Windows, hide in fullscreen, notifications, debug logging) | Atomic write (temp file + rename). Corrupt file is kept as `settings.json.corrupt`. |
| `state\reminders.json` | Which reminders already fired: `eventHash\|startUtc\|reminderType` -> time fired | No meeting text. Entries older than 7 days are dropped; at most 2000 entries; files over 256 KB are quarantined as `reminders.json.corrupt`. |
| `logs\companyisland.log` (+ `.1.log` ... `.4.log`) | Diagnostic log (section 8) | Rotates at 1 MB, 5 files kept (about 5 MB maximum). |
| `EBWebView\` | WebView2 profile (cache, crash dumps) | The app points WebView2 here, so nothing is created under `%LOCALAPPDATA%\com.companyisland.app`. |

There is no on-disk calendar cache: events live in memory and are re-read from Outlook after a restart.

**Wipe** (per user; as that user, or from a logon script, with the app closed):

```bat
rmdir /s /q "%LOCALAPPDATA%\CompanyIsland"
```

To reset only the settings: delete `settings.json`. To make reminders re-fire for meetings still ahead: delete
`state\reminders.json`. To also remove the autostart opt-out: delete the `CompanyIsland` value under the
`StartupApproved\Run` key shown in section 5.

If the folder cannot be created (profile on a read-only or full volume), the app keeps running with
in-memory settings and without a log file (codes `APP-002`, `APP-010`).

## 8. Logs

| | |
|---|---|
| File | `%LOCALAPPDATA%\CompanyIsland\logs\companyisland.log` |
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
addresses, Outlook profile names (only a hash in debug mode).

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

To confirm in your environment, capture traffic for `CompanyIsland.exe` and its `msedgewebview2.exe` children
(parent = `CompanyIsland.exe`) during a 10-minute run; the expected result is no outbound connections.
(Automated coverage only pins the WebView2 command-line flags; a traffic capture has not been done in this
repository **(unverified)**.)

Outlook itself does its own network traffic (Exchange / Microsoft 365). CompanyIsland reads Outlook's local
object model and adds none.

## 10. Outlook requirements

| Requirement | Detail |
|---|---|
| Product | **Classic Outlook for Windows** (`OUTLOOK.EXE`): Microsoft 365 Apps (Click-to-Run) or Office 2016 / 2019 / 2021 (MSI or C2R). Version 2016+ is the supported baseline, 64-bit and 32-bit Office alike (out-of-process COM through the Running Object Table). Only the object-model calls are verified in code; each Office build still needs the QA run **(unverified)**. |
| Not supported | **New Outlook** (`olk.exe`, the "Outlook (new)" app) has no COM automation. If only `olk.exe` runs, the Calendar tab says "New Outlook isn't supported - Switch to classic Outlook to see your meetings" and diagnostics show `OUTLOOK-104`. |
| Running | Outlook must be running. CompanyIsland never starts Outlook, never picks a profile, never calls Logon. It attaches to the running instance through the Running Object Table. Until Outlook runs: "Waiting for Outlook" (`OUTLOOK-101`, informational). |
| Same user, session, integrity | It only considers `OUTLOOK.EXE` in the same Windows session, owned by the same user SID, at the same elevation. Elevated Outlook with a normal island (or the reverse) shows "Outlook runs with different permissions" (`OUTLOOK-103`). Do not run either elevated. |
| Calendar | The **default calendar of the default store** only, today to +48 hours, at most 50 events. Other mailboxes, shared and secondary calendars are not read in v1. |
| Prompts | None expected. Only these properties are read: EntryID (hashed immediately), Subject, Start, End, Location, Organizer (display name), AllDayEvent, IsRecurring, BusyStatus, ResponseStatus. Never body, attendees/recipients, e-mail addresses, attachments or user properties. No Trust Center "programmatic access" dialog is expected for these **(unverified against each Office build)**. If a policy or the object model guard blocks anyway, the code is `OUTLOOK-110` and the app retries with backoff. |
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
| AppLocker / WDAC | The exe is **unsigned** unless you sign it (section 14). Allow by path (`%ProgramFiles%\CompanyIsland\*`; AppLocker's default "everything in Program Files" rule already covers it), by file hash (changes every build) or by publisher once signed. Setup (`CompanyIsland_..._x64-setup.exe`) runs from a temp location and may need its own allow rule. | Code, doc |
| What is executed | One exe plus the uninstaller. **No DLL is installed next to the exe** (no side-loading surface). It loads only system DLLs and Microsoft's WebView2 runtime. | `installer.nsi` copies only `CompanyIsland.exe`; ENTERPRISE_DESIGN section 7 import-table audit |
| WebView2 child processes | The island hosts WebView2, so it starts `msedgewebview2.exe` (browser, renderer, GPU, utility, crashpad) from the runtime folder (typically `C:\Program Files (x86)\Microsoft\EdgeWebView\Application\<version>\`). These are Microsoft-signed and **must be allowed** by AppLocker/WDAC and EDR, otherwise the island never renders. | Doc |
| Defender ASR | "Block executable files from running unless they meet a prevalence, age, or trusted list criterion" can block a brand-new unsigned exe: add an exclusion or sign it. "Block process creations originating from PSExec and WMI" and the Office rules ("Office applications creating child processes", etc.) are not relevant: CompanyIsland is not started from Office and uses no WMI. | Doc |
| Child processes started | Only `explorer.exe` (absolute path from the Windows folder) for "Open logs", and the shell for activating the app behind a mirrored toast (only AUMIDs seen in toasts). The installer runs `taskkill`. | Code |
| Key-logger heuristics | Raw keyboard/mouse input registration is disabled (`DeviceEventFilter::Always`), there is no global mouse or keyboard hook, and no code injection: the fullscreen detector uses out-of-context `SetWinEventHook` only (foreground / minimize / location events). | Code |
| SmartScreen | A downloaded (mark-of-the-web) unsigned installer triggers "Windows protected your PC". Software-distribution tools do not add the mark. Signing fixes it for the publisher over time. | Doc |
| Privileges | `asInvoker`; no services, drivers, scheduled tasks, firewall rules, or HKLM writes at run time. | Code |
| Outlook programmatic access | See section 10 "Prompts". | Code, doc |

## 12. Multi-user, Fast User Switching, RDS / VDI

- **One instance per Windows session.** The single-instance mutex has no `Global\` prefix, so it is
  session-local: each interactive session (local user, Fast User Switching, RDP) runs its own island with its
  own data (`%LOCALAPPDATA%` of that user). A second launch in the same session toggles the running island.
- **Outlook discovery is filtered by session and SID**: another user's Outlook on the same machine is never
  attached, and does not affect the status ("Waiting").
- **Fast User Switching**: a disconnected session keeps its island and keeps polling for Outlook every 15 s in
  the background; the user's reminders are shown when the session is active again. (Behaviour on switch-back
  is covered by QA scenario 9, hardware needed.)
- **RDS / VDI**: autostart works through the HKLM `Run` key. In non-persistent VDI the profile is recreated, so
  `settings.json` and `reminders.json` are lost unless the profile solution (FSLogix, UPD) roams
  `%LOCALAPPDATA%\CompanyIsland`; roaming `EBWebView` is not needed and can be excluded. Many concurrent
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
- Installer is about 207 MiB because it carries the WebView2 runtime. Unsigned by default.
- Hebrew and English UI strings; other languages get English text with locale-correct dates.

## 14. Code signing recommendation

Sign **all three** artifacts so AppLocker/WDAC publisher rules, SmartScreen and EDR reputation work:
`CompanyIsland.exe`, `uninstall.exe` and the setup `.exe`. Tauri signs all three when a certificate is
configured under `bundle.windows` in `src-tauri/tauri.conf.json` (`certificateThumbprint` + `digestAlgorithm`
+ `timestampUrl`, or a `signCommand`); see [INSTALLER.md](INSTALLER.md) "Code signing" for the exact keys.
Nothing is signed or configured today, on purpose.

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
- [ ] Decide autostart default (HKLM `Run`, on) and whether to pre-set per-user opt-outs.
- [ ] Check `LetAppsAccessNotifications` policy if toast mirroring is wanted (value 2 turns it off).
- [ ] Confirm no policy forces Outlook to run elevated or as a different user than the desktop user.

Package and deploy

- [ ] Verify the setup file: `powershell -File scripts/verify-installer.ps1 <setup.exe>`; record SHA256.
- [ ] Intune / SCCM: install `CompanyIsland_1.0.0_x64-setup.exe /S`, uninstall `"%ProgramFiles%\CompanyIsland\uninstall.exe" /S`, run as System, no reboot.
- [ ] Detection: `C:\Program Files\CompanyIsland\CompanyIsland.exe` version >= 1.0.0 (+ HKLM `Run` value `CompanyIsland`).
- [ ] Pilot ring first: 10 to 20 users covering Windows 10 21H2 and Windows 11, at least one 125% / 150% DPI laptop and one dual-monitor desk.

After install (per pilot user, after next logon)

- [ ] Island visible at the top centre; shows `d/M` and the weekday; no UAC prompt; not elevated in Task Manager (Details > Elevated = No).
- [ ] With Classic Outlook running: Calendar tab shows meetings; Settings tab (diagnostics) shows Outlook "Running", mode "Classic Outlook".
- [ ] Meeting 30 minutes ahead produces one alert, once.
- [ ] No outbound traffic from `CompanyIsland.exe` / its WebView2 children.
- [ ] Logs exist in `%LOCALAPPDATA%\CompanyIsland\logs`; no meeting text in them.
- [ ] Help desk knows: Settings tab > "Copy diagnostics" for tickets; error codes in ARCHITECTURE.md section 8; per-user wipe command (section 7).

Operations

- [ ] Upgrade = run the new setup silently; users get the island back at next logon.
- [ ] Offboarding: uninstall command above, plus per-user data wipe via logon script if required.

## 16. Not verified

Everything marked **(unverified)** above, collected. Re-test on a real Windows 10 21H2 machine and a Windows 11
machine with Classic Outlook before broad rollout ([QA_MATRIX.md](QA_MATRIX.md)).

- The installer itself has not been executed end to end for this guide (no install exists on the dev PC):
  exit codes, shortcuts, the `Run` / `Uninstall` / `Software\CompanyIsland` keys and the 64-bit registry view
  are read from `installer-hooks.nsh` and the generated `installer.nsi`.
- Windows 10 N, LTSC and ARM64-with-emulation behaviour.
- The EdgeUpdate-based update of a too-old WebView2 runtime, and fixed-runtime operation
  (`BrowserExecutableFolder` / `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`) with CompanyIsland.
- `/R` when setup runs as SYSTEM; the `_?=` in-place uninstall form and what it leaves behind.
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
- The installer's own wizard text for Hebrew falls back to English (the Tauri build warns that the custom
  installer messages are not translated); the app itself is localised.
