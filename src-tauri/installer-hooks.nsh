; Yuval NSIS installer hooks (Tauri `bundle.windows.nsis.installerHooks`) and the upgrade / migration logic that
; src-tauri/nsis/installer.nsi (the custom template, `bundle.windows.nsis.template`) calls.
;
; The installer runs elevated (installMode perMachine) as the IT administrator, so it must only touch
; HKLM and Program Files: HKCU here would be the administrator's hive, not the end user's. The app itself
; never elevates and never writes HKLM. NOTHING in this file touches %LOCALAPPDATA%, %APPDATA% or any user
; profile: per-user data (settings, notes, logs) survives install, update, migration and uninstall.
;
; This file is included at the top level of the installer script, BEFORE its `!define PRODUCTNAME ...` lines, so:
;   - plain Functions here must not use ${PRODUCTNAME}, ${VERSION}, ${MAINBINARYNAME}, ${UNINSTKEY} ...
;   - those are used inside macros, which are expanded later (NSIS_HOOK_* by Tauri, YUVAL_* by the template).
; Hooks Tauri's template provides (each is optional): NSIS_HOOK_PREINSTALL (start of the Install section, before
; the running-app check and the file copy), NSIS_HOOK_POSTINSTALL, NSIS_HOOK_PREUNINSTALL (start of the Uninstall
; section) and NSIS_HOOK_POSTUNINSTALL. Used here: all four. Macros of OUR template: YUVAL_LANGSTRINGS and
; YUVAL_HOOK_ONINIT. This file is saved as UTF-8 with a BOM because it contains Hebrew text.
;
; Behaviour (docs/INSTALLER.md, "Upgrades and migration"):
;   - Yuval already installed (any version, any mode): update IN PLACE. The old uninstaller is never run.
;     A newer installed version is never replaced (exit code 1638, message box when interactive).
;   - CompanyIsland installed (the product Yuval replaces): its processes are closed, its program files, shortcuts,
;     autostart value and uninstall entry are removed WITHOUT running its uninstaller, and Yuval is installed.

!include LogicLib.nsh
!include x64.nsh
!include FileFunc.nsh

; The template refuses to compile without this (it calls the macros/variables below).
!define YUVAL_HOOKS_LOADED

; ---- shared state (declared here because the template includes this file before everything else) -------------
Var YuvalSameProduct        ; 1 = a Yuval install was found (update / repair); 0 = first install or migration
Var YuvalInstalledVersion   ; DisplayVersion of that install, "" if unknown
Var YuvalWasRunning         ; 1 = the app was running when setup closed it (it is started again afterwards)
Var YuvalMigrateDesktop     ; 1 = migration removed a CompanyIsland desktop shortcut; Yuval gets one in its place
Var YuvalProc               ; in:  image name for YuvalStopProcess
Var YuvalProcWasRunning     ; out: 1 = that image was running when YuvalStopProcess looked
Var YuvalOldFound           ; 1 = something of the old product was found and removed
Var YuvalOldDir             ; migration: folder being examined
Var YuvalOldTrusted         ; migration: 1 = the folder is the one the old Uninstall entry names (uninstall.exe counts as proof)
Var YuvalRegLocation        ; migration: what the old Uninstall entry / install-folder key say (see YuvalGatherOldDirs)
Var YuvalRegUninstall
Var YuvalRegPointer
Var YuvalOld1               ; migration: the candidate folders
Var YuvalOld2
Var YuvalOld3
Var YuvalOld4
Var YuvalLnk                ; migration: shortcut being examined
Var YuvalLnkTarget          ; migration: the target it must have to be ours
Var YuvalLnkRemoved         ; out: 1 = the shortcut was ours and was removed

; ---- constants ---------------------------------------------------------------------------------------------
!define YUVAL_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
; Windows 10 21H2 (19044) is the minimum supported build; Windows 11 builds are higher.
!define /ifndef YUVAL_MIN_BUILD 19044
; Exit codes of a silent install that did not install (documented in docs/INSTALLER.md).
!define YUVAL_EXIT_UNSUPPORTED_OS 1603     ; the usual "fatal error during installation"
!define YUVAL_EXIT_NEWER_INSTALLED 1638    ; ERROR_PRODUCT_VERSION: another (newer) version is already installed

; The product Yuval replaces. Everything about it is spelled out here, on purpose, and only used by the migration.
; (Its Run value was written by its installer hook; Tauri's bundler used publisher = product = "CompanyIsland".)
!define YUVAL_OLD_NAME "CompanyIsland"
!define YUVAL_OLD_MAIN_EXE "CompanyIsland.exe"
!define YUVAL_OLD_CENTER_EXE "CompanyIsland.Center.exe"
!define YUVAL_OLD_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\CompanyIsland"
!define YUVAL_OLD_MANUKEY "Software\CompanyIsland"
!define YUVAL_OLD_MANUPRODUCTKEY "Software\CompanyIsland\CompanyIsland"
; A path containing this is inside a user profile's AppData, i.e. per-user data (%LOCALAPPDATA%\CompanyIsland is
; both the app's data folder and where a per-user install would have put the program). The migration never removes
; anything below such a path. (Overridable only so scripts/test-installer.cjs can work inside a temp folder.)
!define /ifndef YUVAL_USERDATA_MARKER "\AppData\"

; ---- the installer's own messages (English first, Hebrew when the installer has the Hebrew language) ----------
; Expanded by the template after the language files, so ${PRODUCTNAME} and ${VERSION} exist by then. Two messages
; carry both languages (an administrator on an English Windows may still need to read the Hebrew, and the reverse);
; the rest of the setup log is in the installer's own language. $YuvalInstalledVersion and $YuvalOldDir are
; variables: they are filled in when the message is shown.
!macro YUVAL_LANGSTRINGS
  !define YUVAL_TXT_OS_EN "${PRODUCTNAME} requires 64-bit Windows 10 version 21H2 (build ${YUVAL_MIN_BUILD}) or Windows 11. Setup will now exit."
  !define YUVAL_TXT_OS_HE "${PRODUCTNAME} דורש Windows 10 בגרסה 21H2 (build ${YUVAL_MIN_BUILD}) ומעלה, או Windows 11, בגרסת 64 סיביות. ההתקנה תיסגר עכשיו."
  !define YUVAL_TXT_DOWNGRADE_EN "A newer version of ${PRODUCTNAME} ($YuvalInstalledVersion) is already installed on this computer, so this installer (version ${VERSION}) will not replace it.$\r$\n$\r$\nTo go back to an older version, uninstall ${PRODUCTNAME} first (your settings and notes are kept), then run this installer again."
  !define YUVAL_TXT_DOWNGRADE_HE "גרסה חדשה יותר של ${PRODUCTNAME} ($YuvalInstalledVersion) כבר מותקנת במחשב הזה, ולכן ההתקנה הזו (גרסה ${VERSION}) לא תחליף אותה.$\r$\n$\r$\nכדי לחזור לגרסה ישנה יותר יש להסיר קודם את ${PRODUCTNAME} (ההגדרות וההערות נשמרות) ורק אז להריץ שוב את ההתקנה."

  LangString yuvalOsRequirement ${LANG_ENGLISH} "${YUVAL_TXT_OS_EN}$\r$\n$\r$\n${YUVAL_TXT_OS_HE}"
  LangString yuvalDowngradeBlocked ${LANG_ENGLISH} "${YUVAL_TXT_DOWNGRADE_EN}$\r$\n$\r$\n${YUVAL_TXT_DOWNGRADE_HE}"
  LangString yuvalClosing ${LANG_ENGLISH} "Closing running ${PRODUCTNAME} programs..."
  LangString yuvalUpdating ${LANG_ENGLISH} "Updating ${PRODUCTNAME} $YuvalInstalledVersion to ${VERSION} in place. Settings, notes and shortcuts are kept."
  LangString yuvalRepairing ${LANG_ENGLISH} "${PRODUCTNAME} ${VERSION} is already installed; reinstalling it over the existing files (repair). Settings and notes are kept."
  LangString yuvalMigrateFound ${LANG_ENGLISH} "Found the previous product ${YUVAL_OLD_NAME} in $YuvalOldDir. Replacing it with ${PRODUCTNAME} (its uninstaller is not run)."
  LangString yuvalMigrateLeft ${LANG_ENGLISH} "Could not remove everything from $YuvalOldDir; what is left there was not changed."
  LangString yuvalMigrateHkcu ${LANG_ENGLISH} "A per-user ${YUVAL_OLD_NAME} installation was found in the current user's registry. It was not changed; remove it from Apps and features."
  LangString yuvalMigrateDone ${LANG_ENGLISH} "${YUVAL_OLD_NAME} was replaced: its program files, shortcuts, autostart entry and uninstall entry were removed. Per-user settings and notes were not changed."
  !ifdef LANG_HEBREW
    LangString yuvalOsRequirement ${LANG_HEBREW} "${YUVAL_TXT_OS_HE}$\r$\n$\r$\n${YUVAL_TXT_OS_EN}"
    LangString yuvalDowngradeBlocked ${LANG_HEBREW} "${YUVAL_TXT_DOWNGRADE_HE}$\r$\n$\r$\n${YUVAL_TXT_DOWNGRADE_EN}"
    LangString yuvalClosing ${LANG_HEBREW} "סוגר תוכנות ${PRODUCTNAME} פתוחות..."
    LangString yuvalUpdating ${LANG_HEBREW} "מעדכן את ${PRODUCTNAME} מגרסה $YuvalInstalledVersion לגרסה ${VERSION} באותה תיקייה. ההגדרות, ההערות והקיצורים נשמרים."
    LangString yuvalRepairing ${LANG_HEBREW} "${PRODUCTNAME} ${VERSION} כבר מותקן; מתקין אותו מחדש על הקבצים הקיימים (תיקון). ההגדרות וההערות נשמרות."
    LangString yuvalMigrateFound ${LANG_HEBREW} "נמצא המוצר הקודם ${YUVAL_OLD_NAME} בתיקייה $YuvalOldDir. הוא מוחלף ב-${PRODUCTNAME} (תוכנית ההסרה שלו לא מופעלת)."
    LangString yuvalMigrateLeft ${LANG_HEBREW} "לא ניתן היה להסיר הכול מהתיקייה $YuvalOldDir; מה שנותר בה לא שונה."
    LangString yuvalMigrateHkcu ${LANG_HEBREW} "נמצאה התקנה פר-משתמש של ${YUVAL_OLD_NAME} ברישום של המשתמש הנוכחי. היא לא שונתה; אפשר להסיר אותה דרך 'אפליקציות ותכונות'."
    LangString yuvalMigrateDone ${LANG_HEBREW} "${YUVAL_OLD_NAME} הוחלף: קבצי התוכנה, הקיצורים, ההפעלה האוטומטית והרשומה ב'אפליקציות ותכונות' הוסרו. ההגדרות וההערות של המשתמשים לא שונו."

    ; Tauri's own strings, which it ships in English only (the Hebrew installer used to show them in English).
    ; Names are Tauri's (tauri-bundler languages/English.nsh); the product name is written in directly.
    LangString createDesktop ${LANG_HEBREW} "יצירת קיצור דרך בשולחן העבודה"
    LangString webview2Downloading ${LANG_HEBREW} "מוריד את WebView2..."
    LangString webview2DownloadSuccess ${LANG_HEBREW} "WebView2 הורד בהצלחה"
    LangString webview2DownloadError ${LANG_HEBREW} "שגיאה: הורדת WebView2 נכשלה - $0"
    LangString webview2AbortError ${LANG_HEBREW} "התקנת WebView2 נכשלה! היישום לא יכול לפעול בלעדיו. נסו להפעיל את ההתקנה מחדש."
    LangString installingWebview2 ${LANG_HEBREW} "מתקין את WebView2..."
    LangString webview2InstallSuccess ${LANG_HEBREW} "WebView2 הותקן בהצלחה"
    LangString webview2InstallError ${LANG_HEBREW} "שגיאה: התקנת WebView2 נכשלה עם קוד יציאה $1"
    LangString appRunning ${LANG_HEBREW} "${PRODUCTNAME} פועל! יש לסגור אותו ולנסות שוב."
    LangString appRunningOkKill ${LANG_HEBREW} "${PRODUCTNAME} פועל!$\nלחצו אישור כדי לסגור אותו בכוח"
    LangString failedToKillApp ${LANG_HEBREW} "סגירת ${PRODUCTNAME} נכשלה. יש לסגור אותו ידנית ולנסות שוב"
    LangString deleteAppData ${LANG_HEBREW} "מחיקת נתוני היישום"
  !endif

  !undef YUVAL_TXT_OS_EN
  !undef YUVAL_TXT_OS_HE
  !undef YUVAL_TXT_DOWNGRADE_EN
  !undef YUVAL_TXT_DOWNGRADE_HE
!macroend

; ---- console output for silent installs --------------------------------------------------------------------
; Silent setup has no window; a message goes to the console that started it, if any (Tauri does the same).
; ANSI, so English only.
Function YuvalConsoleLine
  Exch $R0
  Push $R1
  System::Call 'kernel32::AttachConsole(i -1) i .R1'
  ${If} $R1 <> 0
    System::Call 'kernel32::GetStdHandle(i -11) i .R1'
    FileWrite $R1 "$R0$\r$\n"
  ${EndIf}
  Pop $R1
  Pop $R0
FunctionEnd

; ---- OS gate -----------------------------------------------------------------------------------------------
; Hidden section. Sections run in definition order and this file is included before Tauri's own sections,
; so an unsupported OS is rejected before the WebView2 runtime is installed or any process is stopped.
; (NSIS_HOOK_PREINSTALL would run only after the WebView2 section.)
Section "-YuvalOsGate"
  SetRegView 64
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ${IfNot} ${RunningX64}
  ${OrIf} $0 < ${YUVAL_MIN_BUILD}
    ; $0 is empty (0) when the build number is unreadable, which also fails the gate.
    ${If} ${Silent}
      Push "Setup cannot continue: 64-bit Windows 10 version 21H2 (build ${YUVAL_MIN_BUILD}) or Windows 11 is required (exit code ${YUVAL_EXIT_UNSUPPORTED_OS})."
      Call YuvalConsoleLine
      ; Distinct exit code for deployment tooling (1603 is the usual "fatal error during installation").
      SetErrorLevel ${YUVAL_EXIT_UNSUPPORTED_OS}
      Quit
    ${Else}
      ${If} $(^RTL) = 1
        MessageBox MB_OK|MB_ICONSTOP|MB_RIGHT|MB_RTLREADING "$(yuvalOsRequirement)"
      ${Else}
        MessageBox MB_OK|MB_ICONSTOP "$(yuvalOsRequirement)"
      ${EndIf}
      SetErrorLevel ${YUVAL_EXIT_UNSUPPORTED_OS}
      Quit
    ${EndIf}
  ${EndIf}
SectionEnd

; ---- .onInit: is Yuval already installed, and is this a downgrade? ----------------------------------------
; Expanded inside .onInit by the template, after $INSTDIR is known. .onInit is the one callback that also runs
; under /S (page callbacks do not), so the decision is made here for every mode.
; nsis_tauri_utils::SemverCompare "installer version" "installed version" returns 1 (installer newer), 0, or -1
; (installer older); an unreadable installed version compares as 1, i.e. it is updated.
!macro YUVAL_HOOK_ONINIT
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ReadRegStr $1 SHCTX "${UNINSTKEY}" "UninstallString"
  !insertmacro YUVAL_DECIDE_UPDATE
!macroend

; The decision itself, separate from the registry reads so that scripts/test-installer.cjs can feed it made-up
; versions. In: $0 = installed DisplayVersion, $1 = installed UninstallString ("" and "" = not installed).
; Out: $YuvalSameProduct, $YuvalInstalledVersion; quits with YUVAL_EXIT_NEWER_INSTALLED on a downgrade.
!macro YUVAL_DECIDE_UPDATE
  StrCpy $YuvalSameProduct 0
  StrCpy $YuvalInstalledVersion ""
  ${If} "$0$1" != ""
    StrCpy $YuvalSameProduct 1
    StrCpy $YuvalInstalledVersion $0
    ${If} $0 != ""
      nsis_tauri_utils::SemverCompare "${VERSION}" $0
      Pop $2
      ${If} $2 = -1
        ; Refuse. Unattended runs (/S, /P) get a console line and the exit code; nobody is there to click a box.
        ${If} ${Silent}
        ${OrIf} $PassiveMode = 1
          Push "${PRODUCTNAME} ${VERSION} setup stopped: the newer version $YuvalInstalledVersion is already installed and downgrades are refused (exit code ${YUVAL_EXIT_NEWER_INSTALLED}). Uninstall it first to go back."
          Call YuvalConsoleLine
        ${Else}
          ${If} $(^RTL) = 1
            MessageBox MB_OK|MB_ICONSTOP|MB_RIGHT|MB_RTLREADING "$(yuvalDowngradeBlocked)"
          ${Else}
            MessageBox MB_OK|MB_ICONSTOP "$(yuvalDowngradeBlocked)"
          ${EndIf}
        ${EndIf}
        SetErrorLevel ${YUVAL_EXIT_NEWER_INSTALLED}
        Quit
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; ---- stopping processes ------------------------------------------------------------------------------------
; The Island Center (WinUI 3, <install dir>\center\<name>.Center.exe) is a second process. Tauri's
; CheckIfAppIsRunning only watches the main exe, and an open Center would lock its own files (install: "error
; opening file for writing"; uninstall: files left behind and the center folder not removed). So both are stopped
; here: ask politely first (WM_CLOSE, no /F; both exit on close), wait up to ~5 s, then force.
; Elevated taskkill without /F only reaches windows of the session the installer runs in; /F reaches any session.
; In: $YuvalProc = image name. Out: $YuvalProcWasRunning. find exits 0 when the image name appears in tasklist's
; output, whatever the OS language.
; Defined twice: the installer's copy and the uninstaller's copy (un. prefix) are separate functions in NSIS.
!macro YUVAL_STOP_PROCESS_FUNCTION PREFIX
  Function ${PREFIX}YuvalStopProcess
    Push $0
    Push $1
    StrCpy $YuvalProcWasRunning 0
    nsExec::Exec 'cmd /c tasklist /NH /FI "IMAGENAME eq $YuvalProc" | find /I "$YuvalProc"'
    Pop $0
    ${If} $0 == 0
      StrCpy $YuvalProcWasRunning 1
      nsExec::Exec 'taskkill /IM "$YuvalProc"'
      Pop $0
      StrCpy $1 0
      ${Do}
        Sleep 500
        nsExec::Exec 'cmd /c tasklist /NH /FI "IMAGENAME eq $YuvalProc" | find /I "$YuvalProc"'
        Pop $0
        ${If} $0 != 0
          ${Break}
        ${EndIf}
        IntOp $1 $1 + 1
      ${LoopWhile} $1 < 10
      ; Still running after the grace period: force it (a hung program must not block an upgrade).
      ${If} $0 == 0
        nsExec::Exec 'taskkill /F /IM "$YuvalProc"'
        Pop $0
        Sleep 500
      ${EndIf}
    ${EndIf}
    Pop $1
    Pop $0
  FunctionEnd
!macroend
!insertmacro YUVAL_STOP_PROCESS_FUNCTION ""
!insertmacro YUVAL_STOP_PROCESS_FUNCTION "un."

; Closes one product: its Center first (it talks to the island), then the island. Sets $YuvalWasRunning when the
; island itself was running, so setup can start it again afterwards.
!macro YUVAL_STOP_PRODUCT PREFIX CENTER_EXE MAIN_EXE
  StrCpy $YuvalProc "${CENTER_EXE}"
  Call ${PREFIX}YuvalStopProcess
  StrCpy $YuvalProc "${MAIN_EXE}"
  Call ${PREFIX}YuvalStopProcess
  ${If} $YuvalProcWasRunning = 1
    StrCpy $YuvalWasRunning 1
  ${EndIf}
!macroend

; <folder>\center is owned entirely by the installer (the Center app, its runtime, the Tour page), and Tauri's
; uninstall list only knows the files of the installer that wrote uninstall.exe. Without this, an upgrade that
; skips the old uninstaller (every upgrade now does) leaves the old version's files (content-hashed
; center\web\assets\*, runtime DLLs the new publish dropped) behind, and uninstalling the newest version leaves
; center\... and the install folder behind. The folder is checked so an empty path can never make this a delete
; relative to the current directory. This is the ONLY recursive delete in this file.
!macro YUVAL_REMOVE_CENTER_DIR DIR
  ${If} ${DIR} != ""
    ${If} ${FileExists} "${DIR}\center\*.*"
      RMDir /r "${DIR}\center"
    ${EndIf}
  ${EndIf}
!macroend

; ---- migration from CompanyIsland ----------------------------------------------------------------------------
; The old uninstaller is never started: it would show its own UI, close the app, remove the shortcuts, and (with
; the "delete app data" box) touch the user's profile. Instead exactly the things its installer created are removed:
;   program folder: CompanyIsland.exe, uninstall.exe, center\ (then the folder itself if it is empty)
;   HKLM Run value "CompanyIsland", HKLM Uninstall\CompanyIsland, HKLM Software\CompanyIsland\CompanyIsland
;   shortcuts: all-users Start menu folder CompanyIsland, Start menu root, desktop, but only if they point at the
;   old exe
; Per-user data (%LOCALAPPDATA%\CompanyIsland) is not touched; the app migrates it itself on its first start.

; Strips surrounding quotes and trailing backslashes: the old installer wrote InstallLocation as "C:\dir".
; Stack in: path. Stack out: cleaned path.
Function YuvalCleanPath
  Exch $0
  Push $1
  StrCpy $1 $0 1
  ${If} $1 == '"'
    StrCpy $0 $0 "" 1
  ${EndIf}
  ${Do}
    StrCpy $1 $0 1 -1
    ${If} $1 == '"'
    ${OrIf} $1 == "\"
      StrCpy $0 $0 -1
    ${Else}
      ${Break}
    ${EndIf}
  ${Loop}
  Pop $1
  Exch $0
FunctionEnd

; In: $YuvalLnk (shortcut), $YuvalLnkTarget (the exe it must point at). Out: $YuvalLnkRemoved.
; A shortcut of the same name that points somewhere else is somebody else's and stays.
Function YuvalRemoveLnkIfTarget
  Push $0
  Push $1
  Push $2
  Push $3
  StrCpy $YuvalLnkRemoved 0
  ${If} ${FileExists} $YuvalLnk
    !insertmacro IsShortcutTarget "$YuvalLnk" "$YuvalLnkTarget"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$YuvalLnk"
      Delete $YuvalLnk
      StrCpy $YuvalLnkRemoved 1
    ${EndIf}
  ${EndIf}
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; In: $YuvalOldDir (an old program folder or ""). Removes the old shortcuts that point at its CompanyIsland.exe.
Function YuvalMigrateShortcutsForDir
  ${If} $YuvalOldDir != ""
    StrCpy $YuvalLnkTarget "$YuvalOldDir\${YUVAL_OLD_MAIN_EXE}"
    StrCpy $YuvalLnk "$SMPROGRAMS\${YUVAL_OLD_NAME}\${YUVAL_OLD_NAME}.lnk"
    Call YuvalRemoveLnkIfTarget
    ${If} $YuvalLnkRemoved = 1
      StrCpy $YuvalOldFound 1
    ${EndIf}
    ; Only when empty (RMDir without /r), so the folder of an unrelated shortcut is never touched.
    RMDir "$SMPROGRAMS\${YUVAL_OLD_NAME}"
    StrCpy $YuvalLnk "$SMPROGRAMS\${YUVAL_OLD_NAME}.lnk"
    Call YuvalRemoveLnkIfTarget
    ${If} $YuvalLnkRemoved = 1
      StrCpy $YuvalOldFound 1
    ${EndIf}
    StrCpy $YuvalLnk "$DESKTOP\${YUVAL_OLD_NAME}.lnk"
    Call YuvalRemoveLnkIfTarget
    ${If} $YuvalLnkRemoved = 1
      StrCpy $YuvalOldFound 1
      StrCpy $YuvalMigrateDesktop 1
    ${EndIf}
  ${EndIf}
FunctionEnd

; In: $YuvalOldDir (candidate folder), $YuvalOldTrusted. Removes the old program files from it, if it is one.
; A folder qualifies when it holds CompanyIsland.exe or center\CompanyIsland.Center.exe (or, for the folder the old
; Uninstall entry names, its uninstall.exe), and it is not inside a user profile's AppData (that is per-user data).
; The delete list is fixed and exact: a folder shared with other files keeps them (RMDir without /r at the end).
Function YuvalMigrateDir
  Push $0
  Push $1
  StrCpy $0 0
  ${If} $YuvalOldDir != ""
    ${StrLoc} $1 "$YuvalOldDir\" "${YUVAL_USERDATA_MARKER}" ">"
    ${If} $1 == ""
      ${If} ${FileExists} "$YuvalOldDir\${YUVAL_OLD_MAIN_EXE}"
      ${OrIf} ${FileExists} "$YuvalOldDir\center\${YUVAL_OLD_CENTER_EXE}"
        StrCpy $0 1
      ${ElseIf} $YuvalOldTrusted = 1
        ${If} ${FileExists} "$YuvalOldDir\uninstall.exe"
          StrCpy $0 1
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ${If} $0 = 1
    StrCpy $YuvalOldFound 1
    DetailPrint "$(yuvalMigrateFound)"
    Delete "$YuvalOldDir\${YUVAL_OLD_MAIN_EXE}"
    Delete "$YuvalOldDir\uninstall.exe"
    !insertmacro YUVAL_REMOVE_CENTER_DIR $YuvalOldDir
    ; The folder itself, when it is now empty. If the new product goes into the same folder it must stay.
    ${If} $YuvalOldDir != $INSTDIR
      RMDir "$YuvalOldDir"
    ${EndIf}
    ${If} ${FileExists} "$YuvalOldDir\${YUVAL_OLD_MAIN_EXE}"
    ${OrIf} ${FileExists} "$YuvalOldDir\center\*.*"
      DetailPrint "$(yuvalMigrateLeft)"
    ${EndIf}
  ${EndIf}
  Pop $1
  Pop $0
FunctionEnd

; Calls FUNC once for each folder the old product may have lived in (the four candidates found by
; YuvalMigrateOldProduct). $YuvalOldTrusted is 1 only for the folder of the old Uninstall entry's uninstall.exe.
!macro YUVAL_EACH_OLD_DIR FUNC
  StrCpy $YuvalOldTrusted 0
  StrCpy $YuvalOldDir $YuvalOld1
  Call ${FUNC}
  StrCpy $YuvalOldTrusted 1
  StrCpy $YuvalOldDir $YuvalOld2
  Call ${FUNC}
  StrCpy $YuvalOldTrusted 0
  StrCpy $YuvalOldDir $YuvalOld3
  Call ${FUNC}
  StrCpy $YuvalOldDir $YuvalOld4
  Call ${FUNC}
!macroend

; Where could the old product be? In: $YuvalRegLocation (Uninstall entry, InstallLocation, quoted), $YuvalRegUninstall
; (its UninstallString, quoted path of uninstall.exe), $YuvalRegPointer (the Software\<publisher>\<product> default
; value). Out: $YuvalOld1..3 from those, $YuvalOld4 = the default folder. Any of them may be "".
Function YuvalGatherOldDirs
  Push $0
  Push $YuvalRegLocation
  Call YuvalCleanPath
  Pop $YuvalOld1
  Push $YuvalRegUninstall
  Call YuvalCleanPath
  Pop $0
  ${GetParent} $0 $YuvalOld2
  Push $YuvalRegPointer
  Call YuvalCleanPath
  Pop $YuvalOld3
  StrCpy $YuvalOld4 "$PROGRAMFILES64\${YUVAL_OLD_NAME}"
  Pop $0
FunctionEnd

; The whole migration. Called by NSIS_HOOK_PREINSTALL after the old programs were closed.
Function YuvalMigrateOldProduct
  Push $0
  SetRegView 64
  StrCpy $YuvalOldFound 0

  ; What the old installer wrote (HKLM, 64-bit view), then the default folder.
  ReadRegStr $YuvalRegLocation HKLM "${YUVAL_OLD_UNINSTKEY}" "InstallLocation"
  ReadRegStr $YuvalRegUninstall HKLM "${YUVAL_OLD_UNINSTKEY}" "UninstallString"
  ReadRegStr $YuvalRegPointer HKLM "${YUVAL_OLD_MANUPRODUCTKEY}" ""
  Call YuvalGatherOldDirs

  !insertmacro YUVAL_EACH_OLD_DIR YuvalMigrateDir
  !insertmacro YUVAL_EACH_OLD_DIR YuvalMigrateShortcutsForDir

  ; Registry (machine-wide only): autostart value, Add/Remove Programs entry, install-folder pointer.
  ReadRegStr $0 HKLM "${YUVAL_RUN_KEY}" "${YUVAL_OLD_NAME}"
  ${If} $0 != ""
    StrCpy $YuvalOldFound 1
  ${EndIf}
  ReadRegStr $0 HKLM "${YUVAL_OLD_UNINSTKEY}" "DisplayName"
  ${If} $0 != ""
    StrCpy $YuvalOldFound 1
  ${EndIf}
  DeleteRegValue HKLM "${YUVAL_RUN_KEY}" "${YUVAL_OLD_NAME}"
  DeleteRegKey HKLM "${YUVAL_OLD_UNINSTKEY}"
  DeleteRegKey HKLM "${YUVAL_OLD_MANUPRODUCTKEY}"
  ; Only when empty: the publisher key may be shared with this product's own key.
  DeleteRegKey /ifempty HKLM "${YUVAL_OLD_MANUKEY}"

  ; A per-user install of the old product lives in the administrator's own hive and, for the standard per-user
  ; layout, in %LOCALAPPDATA%\CompanyIsland, which is also the app's data folder. Never touched; only reported.
  ReadRegStr $0 HKCU "${YUVAL_OLD_UNINSTKEY}" "UninstallString"
  ${If} $0 != ""
    DetailPrint "$(yuvalMigrateHkcu)"
  ${EndIf}

  ${If} $YuvalOldFound = 1
    DetailPrint "$(yuvalMigrateDone)"
  ${EndIf}
  Pop $0
FunctionEnd

; ---- Tauri hooks -------------------------------------------------------------------------------------------
!macro NSIS_HOOK_PREINSTALL
  SetRegView 64
  DetailPrint "$(yuvalClosing)"
  ; Install and update: the files below are overwritten, so nothing of ours may be running. The previous product
  ; (CompanyIsland) is closed too, in every session; the app is started again afterwards if it was running.
  !if "${PRODUCTNAME}" != "${YUVAL_OLD_NAME}"
    !insertmacro YUVAL_STOP_PRODUCT "" "${YUVAL_OLD_CENTER_EXE}" "${YUVAL_OLD_MAIN_EXE}"
  !endif
  !insertmacro YUVAL_STOP_PRODUCT "" "${MAINBINARYNAME}.Center.exe" "${MAINBINARYNAME}.exe"
  ; Replace the previous product without its uninstaller. (Not done when this build IS still named CompanyIsland.)
  !if "${PRODUCTNAME}" != "${YUVAL_OLD_NAME}"
    Call YuvalMigrateOldProduct
  !endif
  ; Drop this product's previous center folder; the files are copied again right after this hook.
  !insertmacro YUVAL_REMOVE_CENTER_DIR $INSTDIR
  ; The migration may have removed an (empty) old folder; if that was the install folder written differently
  ; (trailing backslash, case), make sure it exists for the files that follow.
  SetOutPath $INSTDIR
  ${If} $YuvalSameProduct = 1
    ${If} $YuvalInstalledVersion == "${VERSION}"
      DetailPrint "$(yuvalRepairing)"
    ${Else}
      DetailPrint "$(yuvalUpdating)"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Machine-wide autostart for every user. Each user can opt out in the app (Task Manager > Startup).
  ; Written on every install, so an update or repair also restores a deleted value. The name must equal
  ; RUN_VALUE_NAME in src/autostart.rs: the app toggles the per-user StartupApproved\Run entry that Windows pairs
  ; with this machine-wide Run value.
  SetRegView 64
  WriteRegStr HKLM "${YUVAL_RUN_KEY}" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe"'
  ; A CompanyIsland desktop shortcut was replaced: give the user the Yuval one in the same place.
  ${If} $YuvalMigrateDesktop = 1
    Call CreateOrUpdateDesktopShortcut
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Both programs are asked to close (WM_CLOSE, no /F), then forced after ~5 s, in any session.
  !insertmacro YUVAL_STOP_PRODUCT "un." "${MAINBINARYNAME}.Center.exe" "${MAINBINARYNAME}.exe"
  ; Not on /UPDATE: an update keeps the autostart value.
  ${If} $UpdateMode <> 1
    SetRegView 64
    DeleteRegValue HKLM "${YUVAL_RUN_KEY}" "${PRODUCTNAME}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Tauri has deleted the files of this version; whatever an older version left under center\ goes too, and
  ; then the install folder itself (RMDir without /r only removes it when empty, so user files are never touched).
  !insertmacro YUVAL_REMOVE_CENTER_DIR $INSTDIR
  ${If} $INSTDIR != ""
    RMDir "$INSTDIR"
  ${EndIf}
!macroend

; Per-user data under %LOCALAPPDATA%\Yuval (and, for a machine that ran CompanyIsland, %LOCALAPPDATA%\CompanyIsland,
; which the app moves to Yuval on its first start) is deliberately kept on uninstall (settings, reminder state,
; notes, logs). The installer cannot reach other users' profiles; see docs/INSTALLER.md for IT cleanup.
