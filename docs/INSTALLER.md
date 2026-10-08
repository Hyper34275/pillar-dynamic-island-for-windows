# Yuval installer

One NSIS installer, per-machine, x64. IT installs it elevated; the app itself always runs as the
signed-in standard user, never elevated, and never writes HKLM.

The product was called CompanyIsland until 1.0.13. This installer **updates an installed Yuval in place** and
**replaces an installed CompanyIsland** without ever running an old uninstaller and without touching anyone's
per-user data. How and why is in "Upgrades and migration" below.

## Building

Requirements: Windows x64, Node.js 18+, Rust (MSVC toolchain), and since 1.0.4 the .NET 10 SDK (`dotnet`) for the
Island Center. The build machine needs internet **once**: Tauri downloads the NSIS toolchain and the WebView2
Evergreen Standalone x64 installer (`webviewInstallMode: offlineInstaller`), which is then embedded in the
installer so target PCs need no internet, and `dotnet publish` restores the Center's NuGet packages
(`Microsoft.WindowsAppSDK.WinUI` 1.8 and `Microsoft.WindowsAppSDK.Runtime` 1.8). Build from a short folder path:
a deep one breaks the MSIX content extraction of the Windows App SDK packages.

```bash
npm install
npm run build:installer      # checks the four versions, then `tauri build --config src-tauri/tauri.installer.conf.json`
```

`src-tauri/tauri.installer.conf.json` is merged into `tauri.conf.json` only by this script. It

- sets `build.beforeBuildCommand` to `npm run build && npm run build:center` (the web build, which also produces
  `dist/tour.html`, then `scripts/build-center.cjs`: `dotnet publish` of the Center project under `center/` in
  Release, self-contained and trimmed, into `center/publish/`, with `onnxruntime.dll`, `DirectML.dll` and the
  `.pdb` files removed and `dist/` copied to `center/publish/web/`);
- maps `bundle.resources` `../center/publish` to `center` under the install folder;
- sets `bundle.windows.nsis.template` to `nsis/installer.nsi`, **our copy of Tauri's installer script**
  (see "Upgrades and migration", the part about the template). `src-tauri/installer-hooks.nsh`
  (`installerHooks`, in `tauri.conf.json`) holds the logic.

A plain `cargo test` or `tauri dev` never needs the publish folder. `build-center.cjs` fails early when
`dist/tour.html` is missing or a file name contains `$`, a double quote or a backtick (they would break the NSIS
`File` lines).

Output: `src-tauri/target/release/bundle/nsis/Yuval_<version>_x64-setup.exe`.

```powershell
powershell -File scripts/verify-installer.ps1 <path-to-setup.exe>   # size, SHA256, PE type, file name, Island Center in the payload, our template
node scripts/test-installer.cjs                                      # the installer script compiles; logic tests (no install, see below)
```

`verify-installer.ps1` fails an installer whose generated `installer.nsi` is not made from our template (a build
that skipped `tauri.installer.conf.json` would show Tauri's "uninstall before installing" page).

The version must be identical in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` and
`center/Directory.Build.props` (`<Version>`); `scripts/check-versions.cjs` (the `prebuild:installer` hook) enforces it.
The NSIS stub itself is a 32-bit PE even though the payload is x64; that is normal for Tauri.

`@tauri-apps/api`, `@tauri-apps/cli` and the `tauri` crate must share the same major.minor (currently
2.12); `tauri build` refuses to run otherwise. A cold release build takes about 5 minutes.

## Installing and removing

| Task | Command (elevated) |
|---|---|
| Silent install or update | `Yuval_1.0.14_x64-setup.exe /S` |
| Silent install, custom folder | `Yuval_1.0.14_x64-setup.exe /S /D=D:\Apps\Yuval` (`/D=` must be last, no quotes) |
| Passive (progress bar, no questions) | `Yuval_1.0.14_x64-setup.exe /P` |
| Start the app afterwards (silent or passive) | add `/R` (and `/ARGS <arguments>`) |
| No Start menu / desktop shortcuts | add `/NS` |
| Silent uninstall | `"%ProgramFiles%\Yuval\uninstall.exe" /S` |

Default folder: `%ProgramFiles%\Yuval`. Installer language follows the Windows UI language (English or Hebrew,
no selector; Tauri's own texts are in Hebrew too since 1.0.14, see `YUVAL_LANGSTRINGS` in
`installer-hooks.nsh`).

### Exit codes

| Code | Meaning | Installed anything? |
|---|---|---|
| `0` | Installed, updated or repaired | yes |
| `1603` | The OS gate failed: not 64-bit Windows 10 build 19044+ or Windows 11 | no |
| `1638` | A **newer** Yuval is already installed; downgrades are refused (Windows Installer's `ERROR_PRODUCT_VERSION`) | no |
| `1` | The user cancelled the wizard (NSIS) | no |
| `2` | Aborted by the script (NSIS `Abort`): a program could not be closed, WebView2 could not be installed, a file could not be written | partly |

A silent or passive setup that refuses prints one line to the console that started it (if there is one) in
English; an interactive one shows a message box in Hebrew and English (the language of the installer first).

## Upgrades and migration

### What Tauri's stock NSIS installer does (2.12.1, the template inside `@tauri-apps/cli`)

Read from the template (`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`, extracted from the installed
CLI and diffed with `node scripts/test-installer.cjs diff`) and, where noted, observed with a test installer.

- **Registry** (perMachine: HKLM, 64-bit view). `Software\Microsoft\Windows\CurrentVersion\Uninstall\<product>`:
  `DisplayName`, `DisplayIcon`, `DisplayVersion`, `Publisher`, `InstallLocation` (quoted), `UninstallString` (quoted),
  `NoModify=1`, `NoRepair=1`, `EstimatedSize`, `MainBinaryName`. `Software\<publisher>\<product>`: default value =
  install folder, read by the next setup to pre-fill the folder. `HKCU\Software\<publisher>\<product>\Installer
  Language` only if the language selector is on (it is off here).
- **`PageReinstall`**, a custom page after Welcome, shown (interactive only) when the Uninstall entry exists. It
  compares the installer's version with `DisplayVersion` (`nsis_tauri_utils::SemverCompare`). Same version:
  "Add/Reinstall" (default) or "Uninstall". Installed version older: "**Uninstall before installing**" (the
  **default**) or "Do not uninstall". Installed version newer: the same two choices (with `allowDowngrades=false`
  the second one is greyed out). Choosing to uninstall runs the old `uninstall.exe` (`"<UninstallString>" [/UPDATE]
  [/P] _?=<folder>`, so it runs in place and waits), hides the wizard meanwhile, and returns to the page if the
  uninstaller fails or `<product>.exe` is still there. It also uninstalls an old WiX/MSI install of the same product
  unconditionally.
- **`/S` (silent): the page is never shown.** NSIS does not call page callbacks at all under `/S` (checked with a
  test installer: only `.onInit`, the sections and `.onInstSuccess` run). So a silent run installs **over** the old
  version and never uninstalls it. The stock "downgrades are disabled" guard for silent mode reads a variable that
  only `PageReinstall` sets, so it never fires.
- **`/P` (passive)**: the page's code runs but is not shown; by reading the template, an older installed version is
  installed over, and **the same version runs the old uninstaller first** (the unread radio button counts as
  "uninstall"). Not run here.
- **`/UPDATE`** (Tauri's in-app updater): never uninstalls, creates no shortcuts, skips the WebView2 install; the
  uninstaller run with `/UPDATE` keeps shortcuts and `Run` values.
- **What the old uninstaller deletes**: `NSIS_HOOK_PREUNINSTALL`, the running app (Restart Manager, any session),
  `<product>.exe`, **the resource files listed in the installer that wrote it** (so an older version's list, not the
  current one), `uninstall.exe`, the empty folders, the Start menu and desktop shortcuts if they point at the exe
  (not with `/UPDATE`), the jump list, the `Uninstall\<product>` key, `HKCU\...\Run\<product>` (not with
  `/UPDATE`), and with the interactive "delete app data" box `Software\<publisher>\<product>` plus the
  **administrator's** `%APPDATA%` and `%LOCALAPPDATA%` folders named after the bundle identifier. Our hooks
  additionally remove the HKLM `Run` value and `center\`. It never touches `%LOCALAPPDATA%\<product>` of anyone.
- **Restarting the app**: `/R` (silent or passive only) starts `<product>.exe` through `nsis_tauri_utils::RunAsUser`;
  the finish page of the wizard has a "Run" checkbox (checked by default) that does the same.

### What the Yuval installer does instead

| Situation | Behaviour |
|---|---|
| Nothing installed | Normal install: folder and Start menu pages (interactive), files, `Run` value, shortcuts. |
| Yuval installed, same or older version, any mode (interactive, `/P`, `/S`) | **Update in place.** No reinstall page, no old uninstaller. The folder and Start menu pages are skipped (the update goes into the registered folder; `/D=` still wins). `Yuval.Center.exe` and `Yuval.exe` are closed first (all sessions), the files are replaced (`center\` is removed and re-copied so no file of the old version stays), `DisplayVersion`, `InstallLocation` and the rest of the Add/Remove Programs entry are rewritten, the HKLM `Run` value is rewritten (restored if it had been deleted), the Start menu shortcut is refreshed, and a silent or passive update refreshes the desktop shortcut **only if it still exists** (one the user deleted is not recreated; the finish page's "Create desktop shortcut" box still works). Same version = repair, the same steps. |
| Yuval installed, **newer** version | **Refused.** Exit code `1638` (silent and passive: one console line; interactive: a Hebrew/English message box). Nothing is stopped or changed. To go back, uninstall Yuval first (settings and notes are kept) and run the older setup. |
| CompanyIsland installed | **Migration** (below), then a normal install of Yuval. |
| Unsupported OS | Exit code `1603` before anything else happens. |

The decision is made in `.onInit` (`YUVAL_HOOK_ONINIT`), the one callback that also runs under `/S`.

### Migration from CompanyIsland

CompanyIsland is detected through what its installer wrote (`HKLM\...\Uninstall\CompanyIsland`
`InstallLocation` and `UninstallString`, `HKLM\Software\CompanyIsland\CompanyIsland`) and its default folder
`%ProgramFiles%\CompanyIsland`; a folder counts only if it really holds `CompanyIsland.exe` or
`center\CompanyIsland.Center.exe` (or is the folder the old Uninstall entry names and holds its `uninstall.exe`).
Then, in this order, all in `NSIS_HOOK_PREINSTALL`:

1. `CompanyIsland.Center.exe`, `CompanyIsland.exe`, `Yuval.Center.exe` and `Yuval.exe` are closed in every
   session: a polite `taskkill` (no `/F`), a wait of up to about 5 s, then `taskkill /F`.
2. The old program files are removed: `CompanyIsland.exe`, `uninstall.exe` (deleted, **never started**), the
   `center\` folder, then the folder itself **only if it is empty** (a folder that holds anything else keeps it, and
   the setup log says so). If the old folder is the same as the new install folder (`/D=` to the old custom folder),
   the files go and the folder stays for the new ones.
3. Machine-wide registry only: the HKLM `Run` value `CompanyIsland`, `Uninstall\CompanyIsland`,
   `Software\CompanyIsland\CompanyIsland` (and `Software\CompanyIsland` if that leaves it empty).
4. The all-users shortcuts `Start menu\CompanyIsland\CompanyIsland.lnk`, `Start menu\CompanyIsland.lnk` and
   `Desktop\CompanyIsland.lnk` **if they point at the old exe** (unpinned first). If the desktop one existed, Yuval
   gets a desktop shortcut in its place (not with `/NS`).
5. Yuval is installed in `%ProgramFiles%\Yuval` (or `/D=`).

The setup log (interactive: the install page) says what was found and removed, in the installer's language.

What migration **never** does: run the old uninstaller; touch `%LOCALAPPDATA%`, `%APPDATA%` or any profile (a folder
whose path contains `\AppData\` is refused even if it holds `CompanyIsland.exe`: that is where a per-user install
and the app's own data would live); write or delete anything in HKCU. A per-user CompanyIsland install of the
installing administrator (not something any CompanyIsland setup produced) is only reported in the log.

The app itself moves `%LOCALAPPDATA%\CompanyIsland` to `%LOCALAPPDATA%\Yuval` on its first start. Its settings,
notes and reminder state therefore survive the product change. Two consequences for IT: the per-user
`StartupApproved\Run` opt-out is stored under the value name `Yuval` from now on (the app is expected to carry the
old `CompanyIsland` opt-out over; a user who had switched autostart off may otherwise be started again), and any
logon script that wiped `%LOCALAPPDATA%\CompanyIsland` must wipe `%LOCALAPPDATA%\Yuval`.

PILLAR, the open-source app CompanyIsland descends from, is not migrated: it was only ever a per-user install
(`%LOCALAPPDATA%\PILLAR`), which is a path this installer must not touch.

### After the install: is the app started again?

- Interactive: the finish page has "Run Yuval" (checked by default); it uses `RunAsUser`.
- Silent or passive: the app is started again when **it was running when setup closed it** (including a running
  CompanyIsland, for a migration), or when `/R` is given. It is started through `nsis_tauri_utils::RunAsUser`,
  which launches the program with **Explorer's token of the interactive user** (the plugin imports
  `GetShellWindow`, `OpenProcessToken`, `DuplicateTokenEx` and `CreateProcessWithTokenW`; read from the DLL, not run
  here), i.e. not elevated and not as the administrator that ran setup (also when a standard
  user entered an administrator's credentials at the UAC prompt). With no Explorer (setup run as SYSTEM by Intune or
  SCCM, nobody logged on, a different session) it starts nothing and fails silently; the HKLM `Run` value starts the app
  at the next logon. Other users' sessions are never touched: with several users logged on (RDS) only the console
  user's Explorer is used, the rest get the app at their next logon. The Island Center is not started again; the
  island opens it on demand.

### The template (`src-tauri/nsis/installer.nsi`)

Hooks alone cannot skip `PageReinstall`, so the build uses a copy of Tauri's template with a few changes, each marked
`YUVAL-CHANGE`: the reinstall page and `Section EarlyChecks` are gone, the folder and Start menu pages skip on an
update, `.onInit` calls the decision macro, `.onInstSuccess` restarts the app, the silent desktop shortcut respects a
deleted one. Everything else is Tauri's text, byte for byte.

- A build without the template (a plain `tauri build`, which ignores `tauri.installer.conf.json`) still compiles
  with the hooks, but then Tauri's reinstall page is back and the messages above are empty; it also has no Island
  Center. It is not a supported build, and `scripts/verify-installer.ps1` fails it.
- After a Tauri upgrade: `node scripts/test-installer.cjs diff` prints our changes against the template of the
  installed CLI; merge the upstream differences by hand, update the "Based on" line at the top of the template.
  `node scripts/test-installer.cjs diff --check` fails if a change lacks a `YUVAL-CHANGE` comment.
- `node scripts/test-installer.cjs` (also run by `npx vitest run`, `src/lib/installerUpgrade.test.ts`) renders the
  template the way the bundler does (small Handlebars subset), compiles it with the NSIS that Tauri downloaded
  (`%LOCALAPPDATA%\tauri\NSIS`) for product names `Yuval` and `CompanyIsland`, checks that every LangString has a
  Hebrew text, and runs `src-tauri/nsis/tests/logic.nsi` silently as the current user inside a temp folder: path
  cleaning, the migration's file removal (including the `\AppData\` refusal and the same-folder case), shortcut
  removal, closing a real process (a copy of `ping.exe`, polite then forced), the version decision for 8 installed
  / installer version pairs with the exit codes `1638` and `1603`. No registry write, no file outside the temp folder,
  no elevation. Without the NSIS toolchain it prints `SKIPPED`.
- Not covered by tests (needs elevation and a real machine): the registry cleanup of the migration, the real
  `Run` value, the RunAsUser restart, the WebView2 section, the finish page. See "Manual test plan".

### Manual test plan (needs UAC; not run yet)

Prepare: a VM or spare PC with CompanyIsland 1.0.13 installed by its own setup, running, with some notes and settings
in `%LOCALAPPDATA%\CompanyIsland` (note the file list and hashes), and the Yuval installer. Snapshot the machine
before each case.

1. **Migration, interactive.** Run `Yuval_1.0.14_x64-setup.exe`: Welcome, folder (default `C:\Program Files\Yuval`),
   Start menu folder, install. The log shows "Closing running Yuval programs", "Found the previous product
   CompanyIsland in C:\Program Files\CompanyIsland", "CompanyIsland was replaced ...". Finish with "Run Yuval" checked.
   Check: `C:\Program Files\CompanyIsland` is gone; `HKLM\...\Run` has `Yuval` and no `CompanyIsland`;
   `Uninstall\CompanyIsland` and `Software\CompanyIsland\CompanyIsland` are gone, `Uninstall\Yuval` has
   `DisplayVersion 1.0.14`; Settings > Apps lists only Yuval; the old Start menu and desktop shortcuts are gone, the
   new ones exist; `%LOCALAPPDATA%\CompanyIsland` is byte-identical to before until Yuval's first start, which then
   moves it to `%LOCALAPPDATA%\Yuval` with notes intact; Yuval.exe is **not** elevated (Task Manager > Details >
   Elevated).
2. **Migration, silent.** Restore the snapshot. `start /wait Yuval_1.0.14_x64-setup.exe /S` then `echo %errorlevel%`
   (0). Same checks; the island appears on the screen of the logged-on user without logoff (it was running before).
3. **Update in place.** With Yuval 1.0.14 installed (running), build 1.0.15 and run it interactively: no
   reinstall page, no folder page, the log says "Updating Yuval 1.0.14 to 1.0.15 in place"; `DisplayVersion` is 1.0.15;
   the `Run` value is unchanged; shortcuts unchanged; the old uninstaller was not run (its `uninstall.exe` timestamp
   is replaced by the new `WriteUninstaller`, no uninstall UI flashed). Repeat with `/S` and with `/P`.
4. **Shortcut respect.** Delete the desktop shortcut, run the 1.0.15 setup `/S` again (same version, repair): the
   desktop shortcut is not recreated, the Start menu one is.
5. **Repair.** Delete `center\` and the `Run` value, run the same version `/S`: both are back.
6. **Downgrade.** With 1.0.15 installed run the 1.0.14 setup: `/S` prints one line and exits `1638`
   (`start /wait` + `%errorlevel%`), interactive shows the Hebrew/English box; `DisplayVersion` is still 1.0.15, the
   island was not closed.
7. **Custom folder.** CompanyIsland installed in `D:\Apps\CompanyIsland`: run `Yuval ... /S /D=D:\Apps\CompanyIsland`:
   the old files are gone, Yuval is in that folder, one Uninstall entry.
8. **A stranger's file.** Put `keep.txt` into the old CompanyIsland folder first: it stays, the log says what was left.
9. **SYSTEM.** `psexec -s` the silent setup with nobody logged on: exit 0, nothing started, the `Run` value is there.
10. **Uninstall.** `"C:\Program Files\Yuval\uninstall.exe" /S`: folder, `Run` value, Uninstall entry gone;
    `%LOCALAPPDATA%\Yuval` kept.

## What the installer does (all modes)

- Gates on 64-bit Windows with build >= 19044 **before** installing WebView2 or touching processes
  (`installer-hooks.nsh`, hidden section `YuvalOsGate`). Interactive installs show a message
  box (Hebrew and English); silent installs print a line and exit `1603`. Windows 10 22H2 is build 19045: it passes.
- Installs WebView2 Evergreen (embedded, silent) only if the machine has none or one older than
  `111.0.1661.41` (the frontend needs Chromium 111+).
- Writes `HKLM\Software\Microsoft\Windows\CurrentVersion\Run\Yuval` (64-bit view) pointing at
  `Yuval.exe`: autostart for every user. The value name is the product name and must equal `RUN_VALUE_NAME` in
  `src-tauri/src/autostart.rs`.
- Never writes HKCU (it would be the administrator's hive). Each user can opt out of autostart in the
  app or Task Manager; that only writes the user's own `StartupApproved\Run` value.
- Installs the Island Center into `<install folder>\center\` (about 347 files, 86 MiB on disk: the
  self-contained WinUI 3 app and its runtime, `center\web\` for the tour). Tauri's own running-app check only
  watches `Yuval.exe`, so `NSIS_HOOK_PREINSTALL` and `NSIS_HOOK_PREUNINSTALL` close both programs themselves
  (`YuvalStopProcess`, and an `un.` copy for the uninstaller): `taskkill /IM` (no `/F`), a poll of `tasklist` every
  0.5 s for up to 5 s, then `taskkill /F`. Every program (`cmd`, `tasklist`, `find`, `taskkill`) is started by its
  full path under the system folder, never through `PATH` or the setup's own folder (the setup is elevated). An open
  Center would otherwise lock its files during an upgrade or uninstall.
- Uninstall: closes the Center and the island (above, any session), then Tauri removes files (the `center\` files
  are deleted one by one from the install script's resource list, then our hook removes `center\` completely and the
  folder when empty), shortcuts and the `Run` value (not on `/UPDATE`). A file that is not in the list and not under
  `center\` stays behind with its folder.

### Per-user data is kept

Install, update, migration and uninstall deliberately leave user data. The installer runs as the admin and cannot
see other profiles. To wipe it, run per user (logon script or as that user):

```bat
rmdir /s /q "%LOCALAPPDATA%\Yuval"
rmdir /s /q "%LOCALAPPDATA%\CompanyIsland"
```

(the second only exists on a PC that has not started Yuval yet since the migration.) That folder holds settings,
reminder state, the user's notes (`state\notes.json`, since 1.0.4), logs and the WebView2 profiles (`EBWebView`, and
`EBWebView-Center` for the Center's tour; the app points WebView2 there, so nothing is created under
`%LOCALAPPDATA%\com.companyisland.app`). The uninstaller's "delete app data" checkbox only cleans the
administrator's own `%LOCALAPPDATA%\com.companyisland.app` (the bundle identifier is unchanged), which is empty.

## Application manifest

`src-tauri/windows-app.manifest` is embedded by `src-tauri/build.rs`: `asInvoker` (never
`requireAdministrator`), Windows 10/11 (plus 8.1/8/7) compatibility GUIDs, PerMonitorV2 DPI, long path
awareness and Common-Controls v6 (needed by `SetWindowSubclass`). Inspect a built exe with
`mt.exe -inputresource:Yuval.exe;#1 -out:manifest.xml`.

## Code signing (not configured)

Nothing is signed; no certificate settings exist in `tauri.conf.json` on purpose. To sign, add under
`bundle.windows` either `certificateThumbprint` (+ `digestAlgorithm: "sha256"`, `timestampUrl`) or a
`signCommand` (for example `{ "cmd": "signtool", "args": ["sign", "/fd", "sha256", "/a", "%1"] }`).
Tauri then signs `Yuval.exe`, the uninstaller and the installer. Unsigned installers trigger
SmartScreen and may be blocked by AppLocker/WDAC policies. The Island Center's files travel as bundle
resources, which the Tauri bundler copies as they are **(unverified: not tried with a certificate)**: to have
`center\Yuval.Center.exe` (and `Yuval.Center.dll`, `Yuval.Center.Core.dll`) signed, sign
them in `center/publish/` after `scripts/build-center.cjs` and before the bundler runs. Nothing does that today.

The Center has its own manifest (`app.manifest` in its project under `center/`): `asInvoker`, Windows 10/11
compatibility, PerMonitorV2 DPI.

## Placeholders to change when the real company name is known

`publisher`, `copyright`, `startMenuFolder` and the descriptions in `src-tauri/tauri.conf.json`; the
product/identifier names listed in `docs/ENTERPRISE_DESIGN.md` section 0; `RUN_VALUE_NAME` in
`src-tauri/src/autostart.rs` (the installer takes the Run value name from the product name, so they move together);
the legacy-product constants `YUVAL_OLD_*` in `src-tauri/installer-hooks.nsh` stay as they are (they name what
shipped before); the icon and the installer images are generated from `assets/brand/*.svg` by `node scripts/make-brand-assets.cjs` (see `assets/brand/LOGO_NOTES.md`).
