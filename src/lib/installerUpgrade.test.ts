// @ts-nocheck
// Build-tooling test (runs in Node): the repo has no @types/node, which `tsc` would need for the node: imports.
//
// The installer must UPDATE an installed Yuval in place and MIGRATE the old CompanyIsland product, never run an
// old uninstaller and never touch per-user data. Static checks on src-tauri/nsis/installer.nsi (our copy of Tauri's
// template) and src-tauri/installer-hooks.nsh, plus (when the NSIS toolchain Tauri downloads is on this machine)
// the real thing: scripts/test-installer.cjs compiles the rendered template with makensis and runs the logic tests
// in src-tauri/nsis/tests/logic.nsi silently inside a temp folder. See docs/INSTALLER.md.
import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(__dirname, "..", "..");
const read = (...p: string[]) => readFileSync(join(root, ...p), "utf8").replace(/^﻿/, "").replace(/\r\n/g, "\n");
const template = read("src-tauri", "nsis", "installer.nsi");
const hooks = read("src-tauri", "installer-hooks.nsh");
/** The script lines that are code, not comments (our comments name the things we removed). */
const code = (text: string) =>
  text
    .split("\n")
    .filter((l) => !l.trim().startsWith(";"))
    .join("\n");
const script = join(root, "scripts", "test-installer.cjs");

/** The text of one `!macro NAME ... !macroend` / `Function NAME ... FunctionEnd` block. */
const block = (text: string, open: string, close: string, name: string) => {
  const start = text.indexOf(`${open} ${name}`);
  expect(start, `${open} ${name}`).toBeGreaterThanOrEqual(0);
  return text.slice(start, text.indexOf(close, start));
};
const macro = (name: string) => block(hooks, "!macro", "!macroend", name);
const fn = (text: string, name: string) => block(text, "Function", "FunctionEnd", name);

describe("the installer updates in place (nsis/installer.nsi)", () => {
  it("is wired in by the installer config, next to the hooks", () => {
    const conf = JSON.parse(read("src-tauri", "tauri.installer.conf.json"));
    expect(conf.bundle.windows.nsis.template).toBe("nsis/installer.nsi");
    expect(existsSync(join(root, "src-tauri", conf.bundle.windows.nsis.template))).toBe(true);
    const main = JSON.parse(read("src-tauri", "tauri.conf.json"));
    expect(main.bundle.windows.nsis.installerHooks).toBe("installer-hooks.nsh");
    // The template only works together with the hooks (macros, variables, functions).
    expect(template).toContain("!ifndef YUVAL_HOOKS_LOADED");
    expect(hooks).toContain("!define YUVAL_HOOKS_LOADED");
  });

  it("has no \"uninstall before installing\" page and never starts an uninstaller", () => {
    const tplCode = code(template);
    expect(tplCode).not.toContain("Page custom PageReinstall");
    expect(tplCode).not.toContain("PageLeaveReinstall");
    expect(tplCode).not.toContain("reinst_uninstall");
    expect(tplCode).not.toContain("_?=");
    // The hooks start processes only through nsExec::Exec, and every program by its full path under $SYSDIR: nothing
    // is resolved through PATH or the setup's own folder (the setup is elevated; a GNU find.exe early in PATH made
    // "is it running" answer no).
    const execs = code(hooks).split("\n").filter((l) => /\bExec(Wait|Shell|ShellWait)?\b|\bnsExec::/.test(l));
    expect(execs.length).toBeGreaterThan(0);
    for (const line of execs) expect(line).toMatch(/nsExec::Exec '("\$SYSDIR\\taskkill\.exe" |\$\{YUVAL_CMD_IS_RUNNING\}')/);
    const running = code(hooks).match(/^!define YUVAL_CMD_IS_RUNNING .*$/m)[0];
    expect(running).toContain('"$SYSDIR\\cmd.exe" /c ""$SYSDIR\\tasklist.exe" /NH /FI');
    expect(running).toContain('| "$SYSDIR\\find.exe" /I');
    expect(code(hooks).replace(running, "")).not.toMatch(/ExecWait|ExecShell|ExecDos|\bcmd\b|\btasklist\b|\bfind\b/);
  });

  it("decides in .onInit (page callbacks do not run under /S) and skips the folder pages on an update", () => {
    const onInit = fn(template, ".onInit");
    expect(onInit).toContain("!insertmacro YUVAL_HOOK_ONINIT");
    expect(onInit.indexOf("RestorePreviousInstallLocation")).toBeLessThan(onInit.indexOf("YUVAL_HOOK_ONINIT"));
    // Both pages that choose a place use the same skip function, which also skips for passive mode.
    expect(template.match(/MUI_PAGE_CUSTOMFUNCTION_PRE YuvalSkipIfPassiveOrUpdate/g)).toHaveLength(2);
    const skip = fn(template, "YuvalSkipIfPassiveOrUpdate");
    expect(skip).toContain("$PassiveMode = 1");
    expect(skip).toContain("$YuvalSameProduct = 1");
  });

  it("refuses a downgrade with a distinct exit code in every mode, and says why", () => {
    expect(hooks).toContain("!define YUVAL_EXIT_NEWER_INSTALLED 1638");
    expect(hooks).toContain("!define YUVAL_EXIT_UNSUPPORTED_OS 1603");
    const decide = macro("YUVAL_DECIDE_UPDATE");
    expect(decide).toContain('nsis_tauri_utils::SemverCompare "${VERSION}"');
    expect(decide).toContain("$2 = -1");
    expect(decide).toContain("SetErrorLevel ${YUVAL_EXIT_NEWER_INSTALLED}");
    expect(decide.indexOf("SetErrorLevel")).toBeLessThan(decide.indexOf("Quit"));
    // Unattended runs get a console line, attended ones a message box in Hebrew and English.
    expect(decide).toContain("YuvalConsoleLine");
    expect(decide).toContain("$(yuvalDowngradeBlocked)");
    const langstrings = macro("YUVAL_LANGSTRINGS");
    const message = langstrings.match(/yuvalDowngradeBlocked \$\{LANG_ENGLISH\} "([^"]*)"/)[1];
    expect(message).toContain("YUVAL_TXT_DOWNGRADE_EN");
    expect(message).toContain("YUVAL_TXT_DOWNGRADE_HE");
    expect(langstrings).toMatch(/[֐-׿]/); // Hebrew text is there (and the file needs its BOM, below)
  });

  it("keeps the autostart value on an update and writes it on every install", () => {
    const post = macro("NSIS_HOOK_POSTINSTALL");
    expect(post).toContain('WriteRegStr HKLM "${YUVAL_RUN_KEY}" "${PRODUCTNAME}"');
    const preUn = macro("NSIS_HOOK_PREUNINSTALL");
    expect(preUn).toContain("$UpdateMode <> 1");
    expect(preUn).toContain('DeleteRegValue HKLM "${YUVAL_RUN_KEY}" "${PRODUCTNAME}"');
  });

  it("starts the app again only through RunAsUser (never elevated), and only if it was running or /R was given", () => {
    const onSuccess = fn(template, ".onInstSuccess");
    expect(onSuccess).toContain("$YuvalWasRunning = 1");
    expect(onSuccess.match(/nsis_tauri_utils::RunAsUser/g)).toHaveLength(2);
    expect(onSuccess).not.toMatch(/\bExec/);
    // $YuvalWasRunning is set by the island's stop (not by the Center's), in the hooks.
    expect(macro("YUVAL_STOP_PRODUCT")).toContain("StrCpy $YuvalWasRunning 1");
  });

  it("does not recreate a desktop shortcut the user removed when it updates in place", () => {
    expect(template).toContain('${If} ${FileExists} "$DESKTOP\\${PRODUCTNAME}.lnk"');
  });
});

describe("migration from CompanyIsland (installer-hooks.nsh)", () => {
  it("names the old product exactly, and is skipped by a build that is still called CompanyIsland", () => {
    expect(hooks).toContain('!define YUVAL_OLD_NAME "CompanyIsland"');
    expect(hooks).toContain('!define YUVAL_OLD_MAIN_EXE "CompanyIsland.exe"');
    expect(hooks).toContain('!define YUVAL_OLD_CENTER_EXE "CompanyIsland.Center.exe"');
    expect(hooks).toContain('!define YUVAL_OLD_UNINSTKEY "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CompanyIsland"');
    expect(hooks).toContain('!define YUVAL_OLD_MANUPRODUCTKEY "Software\\CompanyIsland\\CompanyIsland"');
    const pre = macro("NSIS_HOOK_PREINSTALL");
    expect(pre).toContain('!if "${PRODUCTNAME}" != "${YUVAL_OLD_NAME}"');
    // Closed in this order: old programs, new programs, then the old files go, then this product's center\ folder.
    expect(pre.indexOf("YUVAL_OLD_CENTER_EXE")).toBeLessThan(pre.indexOf('"${MAINBINARYNAME}.Center.exe"'));
    expect(pre.indexOf('"${MAINBINARYNAME}.Center.exe"')).toBeLessThan(pre.indexOf("Call YuvalMigrateOldProduct"));
    expect(pre.indexOf("Call YuvalMigrateOldProduct")).toBeLessThan(pre.indexOf("YUVAL_REMOVE_CENTER_DIR"));
  });

  it("removes only what the old installer created, with exact names", () => {
    const dir = fn(hooks, "YuvalMigrateDir");
    expect(dir).toContain('Delete "$YuvalOldDir\\${YUVAL_OLD_MAIN_EXE}"');
    expect(dir).toContain('Delete "$YuvalOldDir\\uninstall.exe"');
    expect(dir).toContain("YUVAL_REMOVE_CENTER_DIR $YuvalOldDir");
    expect(dir).toContain('RMDir "$YuvalOldDir"'); // without /r: a folder with other files stays
    const all = fn(hooks, "YuvalMigrateOldProduct");
    expect(all).toContain('DeleteRegValue HKLM "${YUVAL_RUN_KEY}" "${YUVAL_OLD_NAME}"');
    expect(all).toContain('DeleteRegKey HKLM "${YUVAL_OLD_UNINSTKEY}"');
    expect(all).toContain('DeleteRegKey HKLM "${YUVAL_OLD_MANUPRODUCTKEY}"');
    expect(all).toContain('DeleteRegKey /ifempty HKLM "${YUVAL_OLD_MANUKEY}"');
    const lnk = fn(hooks, "YuvalMigrateShortcutsForDir");
    expect(lnk).toContain("$SMPROGRAMS\\${YUVAL_OLD_NAME}\\${YUVAL_OLD_NAME}.lnk");
    expect(lnk).toContain("$DESKTOP\\${YUVAL_OLD_NAME}.lnk");
  });

  it("never touches per-user data: no profile paths, no HKCU writes, AppData folders are refused", () => {
    expect(hooks).not.toMatch(/\$(LOCALAPPDATA|APPDATA|PROFILE|DOCUMENTS|TEMP|SMSTARTUP)\b/);
    expect(hooks).not.toMatch(/SetShellVarContext\s+current/);
    const code = hooks.split("\n").filter((l) => !l.trim().startsWith(";"));
    expect(code.filter((l) => /\b(WriteReg\w*|DeleteReg\w*)\b.*\bHKCU\b|\b(WriteReg\w*|DeleteReg\w*)\s+\S+\s+HKCU/.test(l))).toEqual([]);
    expect(hooks).toContain('!define /ifndef YUVAL_USERDATA_MARKER "\\AppData\\"');
    expect(fn(hooks, "YuvalMigrateDir")).toContain('${StrLoc} $1 "$YuvalOldDir\\" "${YUVAL_USERDATA_MARKER}" ">"');
  });

  it("does not delete a shortcut that is not ours, and does not remove the old data key if shared", () => {
    expect(fn(hooks, "YuvalRemoveLnkIfTarget")).toContain('IsShortcutTarget "$YuvalLnk" "$YuvalLnkTarget"');
    expect(hooks).toContain('RMDir "$SMPROGRAMS\\${YUVAL_OLD_NAME}"'); // without /r
  });

  it("closes programs politely, then forces them, for both products", () => {
    const stop = macro("YUVAL_STOP_PROCESS_FUNCTION");
    expect(stop).toContain('taskkill.exe" /IM "$YuvalProc"');
    expect(stop).toContain('taskkill.exe" /F /IM "$YuvalProc"');
    expect(stop.indexOf('taskkill.exe" /IM')).toBeLessThan(stop.indexOf('taskkill.exe" /F'));
    expect(stop.match(/nsExec::Exec '\$\{YUVAL_CMD_IS_RUNNING\}'/g)).toHaveLength(2);
    expect(hooks).toContain('!insertmacro YUVAL_STOP_PROCESS_FUNCTION "un."');
  });

  it("ships Hebrew texts, so the file has a UTF-8 BOM", () => {
    const bytes = readFileSync(join(root, "src-tauri", "installer-hooks.nsh"));
    expect([...bytes.subarray(0, 3)]).toEqual([0xef, 0xbb, 0xbf]);
  });
});

const makensis = join(process.env.LOCALAPPDATA || "", "tauri", "NSIS", "makensis.exe");
const toolchain = existsSync(makensis) && existsSync(join(root, "node_modules", "@tauri-apps", "cli-win32-x64-msvc"));

describe("the real NSIS toolchain (skipped when Tauri never downloaded it on this machine)", () => {
  it.skipIf(!toolchain)(
    "only marked changes differ from Tauri's stock template",
    () => {
      const r = spawnSync(process.execPath, [script, "diff", "--check"], { encoding: "utf8" });
      expect(r.stdout + r.stderr).toMatch(/0 without a YUVAL-CHANGE comment/);
      expect(r.status).toBe(0);
    },
    60000,
  );

  it.skipIf(!toolchain)(
    "the template compiles (English and Hebrew) and the logic tests pass",
    () => {
      const r = spawnSync(process.execPath, [script, "check"], { encoding: "utf8", timeout: 280000 });
      const text = r.stdout + r.stderr;
      expect(text, text).toContain("RESULT: OK");
      expect(r.status).toBe(0);
    },
    290000,
  );
});
