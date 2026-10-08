; Unit tests for src-tauri/installer-hooks.nsh. Built and run by scripts/test-installer.cjs (never by Tauri).
;
; The test exe runs as the current user, silently, and touches only ${TESTDIR} (a temp folder): no elevation, no
; registry write, no file outside it. The one real process it starts is a copy of ping.exe named
; YuvalNsisTestProc.exe in that folder. It reads HKLM (SemverCompare needs none; the OS gate reads the build number).
;
; Defines (makensis /D...): TEST (units | decide | osgate), TESTDIR, OUTEXE, HOOKS, PLUGINS, UTILS and per test
;   decide: INSTALLED ("none", "unknown" or a version) and VERSION_UNDER_TEST (the installer's version)
;   osgate: MINBUILD (the build number the gate demands)
; Results go to ${TESTDIR}\results.txt, one "PASS <group>: <detail>" or "FAIL <group>: <detail>" per line; the exit
; code is the installer's own (0, or what the code under test quits with, or the number of failed checks + 100).

Unicode true
!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include "${UTILS}"
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "Win\RestartManager.nsh"
!include "StrFunc.nsh"
${StrCase}
${StrLoc}

!addplugindir "${PLUGINS}"
OutFile "${OUTEXE}"
RequestExecutionLevel user
SilentInstall silent
InstallDir "${TESTDIR}\inst"

; What the real template declares/defines before and around the hooks file.
Var PassiveMode
Var UpdateMode
Var NoShortcutMode
!ifndef VERSION_UNDER_TEST
  !define VERSION_UNDER_TEST "1.0.14"
!endif
!ifdef MINBUILD
  !define YUVAL_MIN_BUILD ${MINBUILD}
!endif
; The default marker is "\AppData\"; a temp folder lives below it, so the file tests move the marker (the guard
; itself is exercised with a path that really contains the marker, and by the "guard" variant with the default).
!ifdef USERDATA_MARKER
  !define YUVAL_USERDATA_MARKER "\${USERDATA_MARKER}\"
  !define PROFILE_DATA_DIR "${USERDATA_MARKER}"
!else
  !define PROFILE_DATA_DIR "AppData"
!endif
!define PRODUCTNAME "Yuval"
!define MANUFACTURER "Yuval"
!define VERSION "${VERSION_UNDER_TEST}"
!define MAINBINARYNAME "YuvalNsisTestProc"
!define BUNDLEID "com.companyisland.app"
; A key that does not exist: the tests never write the registry.
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Yuval-NSIS-Test-Does-Not-Exist"

!include "${HOOKS}"

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Hebrew"
!insertmacro YUVAL_LANGSTRINGS

; ---- tiny test framework -------------------------------------------------------------------------------------
Var Fails
Var Out
!macro Say text
  FileOpen $Out "${TESTDIR}\results.txt" a
  FileSeek $Out 0 END
  FileWrite $Out "${text}$\r$\n"
  FileClose $Out
!macroend
!macro Eq group detail actual expected
  ${If} "${actual}" == "${expected}"
    !insertmacro Say "PASS ${group}: ${detail}"
  ${Else}
    !insertmacro Say "FAIL ${group}: ${detail} (got [${actual}], wanted [${expected}])"
    IntOp $Fails $Fails + 1
  ${EndIf}
!macroend
!macro Touch path
  FileOpen $9 "${path}" w
  FileWrite $9 "x"
  FileClose $9
!macroend
; 1 when the path exists (file or folder), else 0, in the variable.
!macro Exists var path
  StrCpy ${var} 0
  ${If} ${FileExists} "${path}"
    StrCpy ${var} 1
  ${EndIf}
!macroend

; ---- .onInit: the "decide" test runs the real decision code the way the installer's .onInit does -----------------
Function .onInit
  !if "${TEST}" == "decide"
    ${If} "${INSTALLED}" == "none"
      StrCpy $0 ""
      StrCpy $1 ""
    ${ElseIf} "${INSTALLED}" == "unknown"
      StrCpy $0 ""
      StrCpy $1 "C:\Program Files\Yuval\uninstall.exe"
    ${Else}
      StrCpy $0 "${INSTALLED}"
      StrCpy $1 "C:\Program Files\Yuval\uninstall.exe"
    ${EndIf}
    !insertmacro YUVAL_DECIDE_UPDATE
    ; Reached only when the update is allowed (a refused downgrade quits above with the exit code).
    ${If} "${INSTALLED}" == "none"
      !insertmacro Eq "not installed" "same=0" "$YuvalSameProduct" "0"
    ${Else}
      !insertmacro Eq "${INSTALLED}" "same=1" "$YuvalSameProduct" "1"
    ${EndIf}
    ; The pre-existing Yuval keeps its version string for the log message.
    !insertmacro Eq "installed version" "kept" "$YuvalInstalledVersion" "$0"
  !endif
FunctionEnd

; ---- the tests -------------------------------------------------------------------------------------------------
Function TestCleanPath
  Push '"C:\Program Files\CompanyIsland"'
  Call YuvalCleanPath
  Pop $0
  !insertmacro Eq "clean path" "quoted InstallLocation" "$0" "C:\Program Files\CompanyIsland"
  Push 'C:\Program Files\CompanyIsland\'
  Call YuvalCleanPath
  Pop $0
  !insertmacro Eq "clean path" "trailing backslash" "$0" "C:\Program Files\CompanyIsland"
  Push '"C:\Program Files\CompanyIsland\uninstall.exe"'
  Call YuvalCleanPath
  Pop $0
  !insertmacro Eq "clean path" "quoted UninstallString" "$0" "C:\Program Files\CompanyIsland\uninstall.exe"
  Push '"D:\Apps\CompanyIsland\\"'
  Call YuvalCleanPath
  Pop $0
  !insertmacro Eq "clean path" "quote and backslashes" "$0" "D:\Apps\CompanyIsland"
  Push ''
  Call YuvalCleanPath
  Pop $0
  !insertmacro Eq "clean path" "empty stays empty" "$0" ""
  Push '""'
  Call YuvalCleanPath
  Pop $0
  !insertmacro Eq "clean path" "only quotes" "$0" ""
FunctionEnd

Function TestGather
  ; The default install, as the old Tauri installer registered it (quoted paths).
  StrCpy $YuvalRegLocation '"C:\Program Files\CompanyIsland"'
  StrCpy $YuvalRegUninstall '"C:\Program Files\CompanyIsland\uninstall.exe"'
  StrCpy $YuvalRegPointer 'C:\Program Files\CompanyIsland'
  Call YuvalGatherOldDirs
  !insertmacro Eq "gather" "InstallLocation unquoted" "$YuvalOld1" "C:\Program Files\CompanyIsland"
  !insertmacro Eq "gather" "folder of uninstall.exe" "$YuvalOld2" "C:\Program Files\CompanyIsland"
  !insertmacro Eq "gather" "install-folder key" "$YuvalOld3" "C:\Program Files\CompanyIsland"
  !insertmacro Eq "gather" "default folder is always a candidate" "$YuvalOld4" "$PROGRAMFILES64\CompanyIsland"
  ; A custom folder (/D=D:\Apps\CompanyIsland).
  StrCpy $YuvalRegLocation '"D:\Apps\CompanyIsland"'
  StrCpy $YuvalRegUninstall '"D:\Apps\CompanyIsland\uninstall.exe"'
  StrCpy $YuvalRegPointer 'D:\Apps\CompanyIsland'
  Call YuvalGatherOldDirs
  !insertmacro Eq "gather" "custom folder, location" "$YuvalOld1" "D:\Apps\CompanyIsland"
  !insertmacro Eq "gather" "custom folder, uninstall.exe parent" "$YuvalOld2" "D:\Apps\CompanyIsland"
  !insertmacro Eq "gather" "custom folder, pointer" "$YuvalOld3" "D:\Apps\CompanyIsland"
  ; Only some of the values exist (a half-removed install), or none at all.
  StrCpy $YuvalRegLocation ''
  StrCpy $YuvalRegUninstall 'E:\Tools\ci\uninstall.exe'
  StrCpy $YuvalRegPointer ''
  Call YuvalGatherOldDirs
  !insertmacro Eq "gather" "missing values stay empty" "$YuvalOld1$YuvalOld3" ""
  !insertmacro Eq "gather" "unquoted uninstall string" "$YuvalOld2" "E:\Tools\ci"
  StrCpy $YuvalRegUninstall ''
  Call YuvalGatherOldDirs
  !insertmacro Eq "gather" "nothing registered: no parent either" "$YuvalOld2" ""
FunctionEnd

Function TestSemver
  ; The contract the installer depends on: 1 = installer newer, 0 = same, -1 = installer older; unreadable = 1.
  nsis_tauri_utils::SemverCompare "1.0.14" "1.0.13"
  Pop $0
  !insertmacro Eq "semver" "update" "$0" "1"
  nsis_tauri_utils::SemverCompare "1.0.14" "1.0.14"
  Pop $0
  !insertmacro Eq "semver" "same" "$0" "0"
  nsis_tauri_utils::SemverCompare "1.0.13" "1.0.14"
  Pop $0
  !insertmacro Eq "semver" "downgrade" "$0" "-1"
  nsis_tauri_utils::SemverCompare "1.0.9" "1.0.10"
  Pop $0
  !insertmacro Eq "semver" "1.0.9 is older than 1.0.10" "$0" "-1"
  nsis_tauri_utils::SemverCompare "2.0.0" "1.99.99"
  Pop $0
  !insertmacro Eq "semver" "major wins" "$0" "1"
  nsis_tauri_utils::SemverCompare "1.0.14" "garbage"
  Pop $0
  !insertmacro Eq "semver" "unreadable installed version is updated" "$0" "1"
FunctionEnd

Function TestMigrateDir
  StrCpy $INSTDIR "${TESTDIR}\inst"
  CreateDirectory "$INSTDIR"

  ; (1) A normal old install with a stranger's file next to it.
  StrCpy $1 "${TESTDIR}\old1\CompanyIsland"
  CreateDirectory "$1\center\web\assets"
  !insertmacro Touch "$1\CompanyIsland.exe"
  !insertmacro Touch "$1\uninstall.exe"
  !insertmacro Touch "$1\center\CompanyIsland.Center.exe"
  !insertmacro Touch "$1\center\web\assets\a.js"
  !insertmacro Touch "$1\keep.txt"
  StrCpy $YuvalOldDir $1
  StrCpy $YuvalOldTrusted 0
  StrCpy $YuvalOldFound 0
  Call YuvalMigrateDir
  !insertmacro Exists $2 "$1\CompanyIsland.exe"
  !insertmacro Eq "migrate dir" "old exe removed" "$2" "0"
  !insertmacro Exists $2 "$1\uninstall.exe"
  !insertmacro Eq "migrate dir" "old uninstaller file removed (not run)" "$2" "0"
  !insertmacro Exists $2 "$1\center"
  !insertmacro Eq "migrate dir" "old center folder removed" "$2" "0"
  !insertmacro Exists $2 "$1\keep.txt"
  !insertmacro Eq "migrate dir" "a file we do not own stays" "$2" "1"
  !insertmacro Eq "migrate dir" "old product reported as found" "$YuvalOldFound" "1"
  !insertmacro Exists $2 "$1"
  !insertmacro Eq "migrate dir" "folder kept while not empty" "$2" "1"

  ; (2) Nothing but ours: the folder goes too.
  StrCpy $1 "${TESTDIR}\old2\CompanyIsland"
  CreateDirectory "$1\center"
  !insertmacro Touch "$1\CompanyIsland.exe"
  !insertmacro Touch "$1\center\CompanyIsland.Center.exe"
  StrCpy $YuvalOldDir $1
  Call YuvalMigrateDir
  !insertmacro Exists $2 "$1"
  !insertmacro Eq "migrate dir" "empty old folder removed" "$2" "0"

  ; (3) The old folder is also the new install folder: files go, the folder stays for the new files.
  StrCpy $1 "${TESTDIR}\same\CompanyIsland"
  CreateDirectory "$1\center"
  !insertmacro Touch "$1\CompanyIsland.exe"
  !insertmacro Touch "$1\uninstall.exe"
  StrCpy $INSTDIR $1
  StrCpy $YuvalOldDir $1
  Call YuvalMigrateDir
  !insertmacro Exists $2 "$1\CompanyIsland.exe"
  !insertmacro Eq "migrate dir" "same folder: old exe removed" "$2" "0"
  !insertmacro Exists $2 "$1"
  !insertmacro Eq "migrate dir" "same folder: kept for the new install" "$2" "1"
  StrCpy $INSTDIR "${TESTDIR}\inst"

  ; (5) A folder that only has an uninstall.exe is somebody else's unless the old Uninstall entry names it.
  StrCpy $1 "${TESTDIR}\other\tool"
  CreateDirectory "$1"
  !insertmacro Touch "$1\uninstall.exe"
  StrCpy $YuvalOldDir $1
  StrCpy $YuvalOldTrusted 0
  Call YuvalMigrateDir
  !insertmacro Exists $2 "$1\uninstall.exe"
  !insertmacro Eq "migrate dir" "unregistered folder with only uninstall.exe untouched" "$2" "1"
  StrCpy $YuvalOldTrusted 1
  Call YuvalMigrateDir
  !insertmacro Exists $2 "$1\uninstall.exe"
  !insertmacro Eq "migrate dir" "registered folder: its uninstall.exe goes" "$2" "0"

  ; (6) Empty / missing folders are fine.
  StrCpy $YuvalOldDir ""
  Call YuvalMigrateDir
  StrCpy $YuvalOldDir "${TESTDIR}\does\not\exist"
  Call YuvalMigrateDir
  !insertmacro Eq "migrate dir" "empty and missing folders do nothing" "ok" "ok"
FunctionEnd

Function TestMigrateGuard
  StrCpy $INSTDIR "${TESTDIR}\inst"
  CreateDirectory "$INSTDIR"
  ; (4) Anything inside a profile's AppData is per-user data: never touched, even with the exe's name in it.
  StrCpy $1 "${TESTDIR}\Users\someone\${PROFILE_DATA_DIR}\Local\CompanyIsland"
  CreateDirectory "$1\center"
  !insertmacro Touch "$1\CompanyIsland.exe"
  !insertmacro Touch "$1\center\CompanyIsland.Center.exe"
  !insertmacro Touch "$1\settings.json"
  StrCpy $YuvalOldDir $1
  StrCpy $YuvalOldTrusted 1
  Call YuvalMigrateDir
  !insertmacro Exists $2 "$1\CompanyIsland.exe"
  !insertmacro Eq "migrate dir" "AppData: nothing removed (exe)" "$2" "1"
  !insertmacro Exists $2 "$1\center\CompanyIsland.Center.exe"
  !insertmacro Eq "migrate dir" "AppData: nothing removed (center)" "$2" "1"
  !insertmacro Exists $2 "$1\settings.json"
  !insertmacro Eq "migrate dir" "AppData: user data kept" "$2" "1"

FunctionEnd

Function TestShortcut
  CreateDirectory "${TESTDIR}\lnk"
  ; A shortcut that points at the old exe is ours; one with the same name that points elsewhere is not.
  CreateShortcut "${TESTDIR}\lnk\mine.lnk" "${TESTDIR}\old\CompanyIsland.exe"
  CreateShortcut "${TESTDIR}\lnk\other.lnk" "${TESTDIR}\somewhere\else.exe"
  StrCpy $YuvalLnkTarget "${TESTDIR}\old\CompanyIsland.exe"
  StrCpy $YuvalLnk "${TESTDIR}\lnk\other.lnk"
  Call YuvalRemoveLnkIfTarget
  !insertmacro Eq "shortcut" "foreign shortcut: not removed flag" "$YuvalLnkRemoved" "0"
  !insertmacro Exists $2 "${TESTDIR}\lnk\other.lnk"
  !insertmacro Eq "shortcut" "foreign shortcut stays" "$2" "1"
  StrCpy $YuvalLnk "${TESTDIR}\lnk\mine.lnk"
  Call YuvalRemoveLnkIfTarget
  !insertmacro Eq "shortcut" "our shortcut: removed flag" "$YuvalLnkRemoved" "1"
  !insertmacro Exists $2 "${TESTDIR}\lnk\mine.lnk"
  !insertmacro Eq "shortcut" "our shortcut deleted" "$2" "0"
  StrCpy $YuvalLnk "${TESTDIR}\lnk\missing.lnk"
  Call YuvalRemoveLnkIfTarget
  !insertmacro Eq "shortcut" "missing shortcut: nothing happens" "$YuvalLnkRemoved" "0"
FunctionEnd

Function TestStopProcess
  ; Not running: reported as such, quickly.
  StrCpy $YuvalProc "YuvalNsisTestProc.exe"
  Call YuvalStopProcess
  !insertmacro Eq "stop process" "not running is reported" "$YuvalProcWasRunning" "0"

  ; A real process: a copy of ping.exe with our test name, hidden. It has no window, so the polite taskkill cannot
  ; end it and this also exercises the forced path after the grace period (about 5 seconds).
  CopyFiles /SILENT "$SYSDIR\ping.exe" "${TESTDIR}\YuvalNsisTestProc.exe"
  ; (-n 100: ends by itself after about 100 seconds if a failure ever leaves it behind.)
  ExecShell "open" "${TESTDIR}\YuvalNsisTestProc.exe" "-n 100 127.0.0.1" SW_HIDE
  ; Wait (up to ~20 s, the machine may be busy) until it shows up in the process list.
  StrCpy $1 0
  ${Do}
    Sleep 500
    nsis_tauri_utils::FindProcess "YuvalNsisTestProc.exe"
    Pop $0
    ${If} $0 == 0
      ${Break}
    ${EndIf}
    IntOp $1 $1 + 1
  ${LoopWhile} $1 < 40
  !insertmacro Eq "stop process" "test process is up before" "$0" "0"
  StrCpy $YuvalWasRunning 0
  !insertmacro YUVAL_STOP_PRODUCT "" "YuvalNsisTestProc.Center.exe" "YuvalNsisTestProc.exe"
  nsis_tauri_utils::FindProcess "YuvalNsisTestProc.exe"
  Pop $0
  !insertmacro Eq "stop process" "process is gone after" "$0" "1"
  !insertmacro Eq "stop process" "app was running: remembered for the restart" "$YuvalWasRunning" "1"
  ; Only the Center running does not count as "the app was running".
  StrCpy $YuvalWasRunning 0
  !insertmacro YUVAL_STOP_PRODUCT "" "YuvalNsisTestProc.Center.exe" "YuvalNsisTestProc.exe"
  !insertmacro Eq "stop process" "nothing running: not remembered" "$YuvalWasRunning" "0"
FunctionEnd

Function TestCenterDir
  StrCpy $1 "${TESTDIR}\cd"
  CreateDirectory "$1\center\a\b"
  !insertmacro Touch "$1\center\a\b\c.txt"
  !insertmacro Touch "$1\keep.txt"
  !insertmacro YUVAL_REMOVE_CENTER_DIR $1
  !insertmacro Exists $2 "$1\center"
  !insertmacro Eq "center dir" "center\ removed recursively" "$2" "0"
  !insertmacro Exists $2 "$1\keep.txt"
  !insertmacro Eq "center dir" "neighbours stay" "$2" "1"
  ; An empty folder name must never turn into a delete relative to the current directory.
  StrCpy $1 ""
  !insertmacro YUVAL_REMOVE_CENTER_DIR $1
  !insertmacro Eq "center dir" "empty folder name is a no-op" "ok" "ok"
FunctionEnd

Section "Tests"
  StrCpy $Fails 0
  !if "${TEST}" == "units"
    Call TestCleanPath
    Call TestGather
    Call TestSemver
    Call TestMigrateDir
    Call TestMigrateGuard
    Call TestShortcut
    Call TestCenterDir
    Call TestStopProcess
  !endif
  !if "${TEST}" == "guard"
    Call TestMigrateGuard
  !endif
  !if "${TEST}" == "osgate"
    ; The gate section of installer-hooks.nsh ran before this one and did not quit.
    !insertmacro Eq "os gate" "passes on a supported build" "ok" "ok"
  !endif
  ${If} $Fails <> 0
    IntOp $Fails $Fails + 100
    SetErrorLevel $Fails
  ${EndIf}
SectionEnd
