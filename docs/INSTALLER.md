# CompanyIsland installer

One NSIS installer, per-machine, x64. IT installs it elevated; the app itself always runs as the
signed-in standard user, never elevated, and never writes HKLM.

## Building

Requirements: Windows x64, Node.js 18+, Rust (MSVC toolchain). The build machine needs internet
**once**: Tauri downloads the NSIS toolchain and the WebView2 Evergreen Standalone x64 installer
(`webviewInstallMode: offlineInstaller`), which is then embedded in the installer so target PCs need
no internet.

```bash
npm install
npm run build:installer      # checks versions, then `tauri build`
```

Output: `src-tauri/target/release/bundle/nsis/CompanyIsland_<version>_x64-setup.exe`.

```powershell
powershell -File scripts/verify-installer.ps1 <path-to-setup.exe>   # size, SHA256, PE type, file name
```

The version must be identical in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`; `scripts/check-versions.cjs` (the `prebuild:installer` hook) enforces it.
The NSIS stub itself is a 32-bit PE even though the payload is x64; that is normal for Tauri.

`@tauri-apps/api`, `@tauri-apps/cli` and the `tauri` crate must share the same major.minor (currently
2.12); `tauri build` refuses to run otherwise. A cold release build takes about 5 minutes.

## Installing and removing

| Task | Command (elevated) |
|---|---|
| Silent install | `CompanyIsland_1.0.0_x64-setup.exe /S` |
| Silent install, custom folder | `CompanyIsland_1.0.0_x64-setup.exe /S /D=D:\Apps\CompanyIsland` (`/D=` must be last, no quotes) |
| Silent uninstall | `"%ProgramFiles%\CompanyIsland\uninstall.exe" /S` |

Default folder: `%ProgramFiles%\CompanyIsland`. Installer language follows the Windows UI language
(English or Hebrew, no selector). Upgrades install over the previous version; running instances are
closed first.

Exit code `1603` means the OS gate failed (not 64-bit Windows 10 build 19044+/Windows 11); nothing was
installed. Other non-zero codes are standard NSIS codes (`2` = aborted).

### What the installer does

- Gates on 64-bit Windows with build >= 19044 **before** installing WebView2 or touching processes
  (`installer-hooks.nsh`, hidden section `CompanyIslandOsGate`). Interactive installs show a message
  box; silent installs exit `1603`.
- Installs WebView2 Evergreen (embedded, silent) only if the machine has none or one older than
  `111.0.1661.41` (the frontend needs Chromium 111+).
- Writes `HKLM\Software\Microsoft\Windows\CurrentVersion\Run\CompanyIsland` (64-bit view) pointing at
  `CompanyIsland.exe`: autostart for every user. The value name must equal `RUN_VALUE_NAME` in
  `src-tauri/src/autostart.rs`.
- Never writes HKCU (it would be the administrator's hive). Each user can opt out of autostart in the
  app or Task Manager; that only writes the user's own `StartupApproved\Run` value.
- Uninstall: asks `CompanyIsland.exe` to close (`taskkill`, no `/F`; elevated, this only reaches the
  uninstaller's own session), then Tauri force-closes any remaining instance in any session, removes
  files, shortcuts and the `Run` value.

### Per-user data is kept

Uninstall deliberately leaves user data. The installer runs as the admin and cannot see other
profiles. To wipe it, run per user (logon script or as that user):

```bat
rmdir /s /q "%LOCALAPPDATA%\CompanyIsland"
```

That folder holds settings, reminder state, logs and the WebView2 profile (`EBWebView`; the app points
WebView2 there, so nothing is created under `%LOCALAPPDATA%\com.companyisland.app`). The uninstaller's
"delete app data" checkbox only cleans the administrator's own `%LOCALAPPDATA%\com.companyisland.app`,
which is empty.

## Application manifest

`src-tauri/windows-app.manifest` is embedded by `src-tauri/build.rs`: `asInvoker` (never
`requireAdministrator`), Windows 10/11 (plus 8.1/8/7) compatibility GUIDs, PerMonitorV2 DPI, long path
awareness and Common-Controls v6 (needed by `SetWindowSubclass`). Inspect a built exe with
`mt.exe -inputresource:CompanyIsland.exe;#1 -out:manifest.xml`.

## Code signing (not configured)

Nothing is signed; no certificate settings exist in `tauri.conf.json` on purpose. To sign, add under
`bundle.windows` either `certificateThumbprint` (+ `digestAlgorithm: "sha256"`, `timestampUrl`) or a
`signCommand` (for example `{ "cmd": "signtool", "args": ["sign", "/fd", "sha256", "/a", "%1"] }`).
Tauri then signs `CompanyIsland.exe`, the uninstaller and the installer. Unsigned installers trigger
SmartScreen and may be blocked by AppLocker/WDAC policies.

## Placeholders to change when the real company name is known

`publisher`, `copyright`, `startMenuFolder` and the descriptions in `src-tauri/tauri.conf.json`; the
product/identifier names listed in `docs/ENTERPRISE_DESIGN.md` section 0; `COMPANYISLAND_RUN_VALUE`
in `src-tauri/installer-hooks.nsh` together with `RUN_VALUE_NAME` in `src-tauri/src/autostart.rs`; the
expected file name in `scripts/verify-installer.ps1`; the icon via `scripts/make-icon.ps1` then
`npx tauri icon app-icon.png`.
