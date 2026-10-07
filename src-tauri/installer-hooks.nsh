; CompanyIsland NSIS installer hooks (Tauri `bundle.windows.nsis.installerHooks`).
;
; The installer runs elevated (installMode perMachine) as the IT administrator, so it must only touch
; HKLM and Program Files: HKCU here would be the administrator's hive, not the end user's. The app itself
; never elevates and never writes HKLM.
;
; This file is included at the top level of Tauri's installer.nsi, before its own sections.
; Hooks Tauri's template provides (each is optional): NSIS_HOOK_PREINSTALL (start of the Install section, before
; the running-app check and the file copy), NSIS_HOOK_POSTINSTALL, NSIS_HOOK_PREUNINSTALL (start of the Uninstall
; section) and NSIS_HOOK_POSTUNINSTALL. Used here: PREINSTALL, POSTINSTALL, PREUNINSTALL and POSTUNINSTALL.

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

; The Island Center (WinUI 3, <install dir>\center\CompanyIsland.Center.exe) is a second process. Tauri's
; CheckIfAppIsRunning only watches the main exe, and an open Center would lock its own files (install: "error
; opening file for writing"; uninstall: files left behind and the center folder not removed). So it is stopped
; here: ask politely first (WM_CLOSE, no /F; the Center exits on close), wait up to ~5 s, then force.
; Elevated taskkill without /F only reaches windows of the session the installer runs in; /F reaches any session.
!define COMPANYISLAND_CENTER_EXE "CompanyIsland.Center.exe"
; find exits 0 when the image name appears in tasklist's output, whatever the OS language.
!define COMPANYISLAND_CENTER_RUNNING 'cmd /c tasklist /NH /FI "IMAGENAME eq ${COMPANYISLAND_CENTER_EXE}" | find /I "${COMPANYISLAND_CENTER_EXE}"'

; Defined twice: the installer's copy and the uninstaller's copy (un. prefix) are separate functions in NSIS.
!macro COMPANYISLAND_STOP_CENTER_FUNCTION PREFIX
  Function ${PREFIX}CompanyIslandStopCenter
    Push $0
    Push $1
    nsExec::Exec '${COMPANYISLAND_CENTER_RUNNING}'
    Pop $0
    ${If} $0 == 0
      nsExec::Exec 'taskkill /IM "${COMPANYISLAND_CENTER_EXE}"'
      Pop $0
      StrCpy $1 0
      ${Do}
        Sleep 500
        nsExec::Exec '${COMPANYISLAND_CENTER_RUNNING}'
        Pop $0
        ${If} $0 != 0
          ${Break}
        ${EndIf}
        IntOp $1 $1 + 1
      ${LoopWhile} $1 < 10
      ; Still running after the grace period: force it (a hung Center must not block an upgrade).
      ${If} $0 == 0
        nsExec::Exec 'taskkill /F /IM "${COMPANYISLAND_CENTER_EXE}"'
        Pop $0
        Sleep 500
      ${EndIf}
    ${EndIf}
    Pop $1
    Pop $0
  FunctionEnd
!macroend
!insertmacro COMPANYISLAND_STOP_CENTER_FUNCTION ""
!insertmacro COMPANYISLAND_STOP_CENTER_FUNCTION "un."

; <install dir>\center is owned entirely by this installer (the Center app, its runtime, the Tour page), and
; Tauri's uninstall list only knows the files of the installer that wrote uninstall.exe. Without this, an upgrade
; that skips the old uninstaller (/S, /UPDATE, passive, or "do not uninstall") leaves the old version's files
; (content-hashed center\web\assets\*, runtime DLLs the new publish dropped) behind, and uninstalling the newest
; version leaves center\... and the install folder behind. $INSTDIR is checked so an empty path can never make this
; a delete relative to the current directory.
!macro COMPANYISLAND_REMOVE_CENTER_DIR
  ${If} $INSTDIR != ""
    ${If} ${FileExists} "$INSTDIR\center\*.*"
      RMDir /r "$INSTDIR\center"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; Install and upgrade: the Center's files are overwritten below, so it must not be running.
  Call CompanyIslandStopCenter
  ; Then drop the previous version's center folder; the files are copied again right after this hook.
  !insertmacro COMPANYISLAND_REMOVE_CENTER_DIR
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Machine-wide autostart for every user. Each user can opt out in the app (Task Manager > Startup).
  SetRegView 64
  WriteRegStr HKLM "${COMPANYISLAND_RUN_KEY}" "${COMPANYISLAND_RUN_VALUE}" '"$INSTDIR\${MAINBINARYNAME}.exe"'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Call un.CompanyIslandStopCenter
  ; Ask running instances to close (WM_CLOSE, no /F). Elevated taskkill can only close windows in the
  ; session the uninstaller runs in; Tauri's CheckIfAppIsRunning then force-kills what is left, in any
  ; session, right after this hook.
  nsExec::Exec 'taskkill /IM "${MAINBINARYNAME}.exe"'
  Pop $0
  Sleep 1000
  SetRegView 64
  DeleteRegValue HKLM "${COMPANYISLAND_RUN_KEY}" "${COMPANYISLAND_RUN_VALUE}"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Tauri has deleted the files of this version; whatever an older version left under center\ goes too, and
  ; then the install folder itself (RMDir without /r only removes it when empty, so user files are never touched).
  !insertmacro COMPANYISLAND_REMOVE_CENTER_DIR
  ${If} $INSTDIR != ""
    RMDir "$INSTDIR"
  ${EndIf}
!macroend

; Per-user data under %LOCALAPPDATA%\CompanyIsland is deliberately kept on uninstall (settings, reminder
; state, logs). The installer cannot reach other users' profiles; see docs/INSTALLER.md for IT cleanup.
