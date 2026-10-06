; CompanyIsland NSIS installer hooks (Tauri `bundle.windows.nsis.installerHooks`).
;
; The installer runs elevated (installMode perMachine) as the IT administrator, so it must only touch
; HKLM and Program Files: HKCU here would be the administrator's hive, not the end user's. The app itself
; never elevates and never writes HKLM.
;
; This file is included at the top level of Tauri's installer.nsi, before its own sections.

!include LogicLib.nsh
!include x64.nsh

; Must equal RUN_VALUE_NAME in src/autostart.rs: the app toggles the per-user StartupApproved\Run entry
; that Windows pairs with this machine-wide Run value.
!define COMPANYISLAND_RUN_VALUE "CompanyIsland"
!define COMPANYISLAND_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
; Windows 10 21H2 (19044) is the minimum supported build; Windows 11 builds are higher.
!define COMPANYISLAND_MIN_BUILD 19044

; Hidden section. Sections run in definition order and this file is included before Tauri's own sections,
; so an unsupported OS is rejected before the WebView2 runtime is installed or any process is stopped.
; (NSIS_HOOK_PREINSTALL would run only after the WebView2 section.)
Section "-CompanyIslandOsGate"
  SetRegView 64
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ${IfNot} ${RunningX64}
  ${OrIf} $0 < ${COMPANYISLAND_MIN_BUILD}
    ; $0 is empty (0) when the build number is unreadable, which also fails the gate.
    ${If} ${Silent}
      ; Distinct exit code for deployment tooling (1603 is the usual "fatal error during installation").
      SetErrorLevel 1603
      Quit
    ${Else}
      MessageBox MB_OK|MB_ICONSTOP "CompanyIsland requires 64-bit Windows 10 version 21H2 (build ${COMPANYISLAND_MIN_BUILD}) or Windows 11. Setup will now exit."
      SetErrorLevel 1603
      Quit
    ${EndIf}
  ${EndIf}
SectionEnd

!macro NSIS_HOOK_POSTINSTALL
  ; Machine-wide autostart for every user. Each user can opt out in the app (Task Manager > Startup).
  SetRegView 64
  WriteRegStr HKLM "${COMPANYISLAND_RUN_KEY}" "${COMPANYISLAND_RUN_VALUE}" '"$INSTDIR\${MAINBINARYNAME}.exe"'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Ask running instances to close (WM_CLOSE, no /F). Elevated taskkill can only close windows in the
  ; session the uninstaller runs in; Tauri's CheckIfAppIsRunning then force-kills what is left, in any
  ; session, right after this hook.
  nsExec::Exec 'taskkill /IM "${MAINBINARYNAME}.exe"'
  Pop $0
  Sleep 1000
  SetRegView 64
  DeleteRegValue HKLM "${COMPANYISLAND_RUN_KEY}" "${COMPANYISLAND_RUN_VALUE}"
!macroend

; Per-user data under %LOCALAPPDATA%\CompanyIsland is deliberately kept on uninstall (settings, reminder
; state, logs). The installer cannot reach other users' profiles; see docs/INSTALLER.md for IT cleanup.
