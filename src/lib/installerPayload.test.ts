// @ts-nocheck
// Build-tooling test (runs in Node): the repo has no @types/node, which `tsc` would need for the node: imports.
import { describe, expect, it } from "vitest";
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(__dirname, "..", "..");
const script = join(root, "scripts", "build-center.cjs");
const { REQUIRED_PAYLOAD, missingPayload, installedFiles } = createRequire(import.meta.url)(script);
const read = (...p: string[]) => readFileSync(join(root, ...p), "utf8");

/** The lines Tauri's NSIS template writes for bundle.resources: one `File /a "/oname=..."` per file. */
const nsiWith = (targets: string[]) =>
  [
    "Section Install",
    '  File "${MAINBINARYSRCPATH}"',
    ...targets.map((t) => `  File /a "/oname=${t}" "C:\\build\\center\\publish\\${t.split("\\").pop()}"`),
    "SectionEnd",
  ].join("\r\n");

const ALL = ["center\\CompanyIsland.Center.exe", "center\\CompanyIsland.Center.pri", "center\\web\\tour.html"];

describe("installer payload check (build-center.cjs)", () => {
  it("requires the Center exe, its .pri and the Tour page", () => {
    expect(REQUIRED_PAYLOAD).toEqual(["CompanyIsland.Center.exe", "CompanyIsland.Center.pri", "web/tour.html"]);
  });

  it("passes an installer script that lists every required file", () => {
    expect(missingPayload(nsiWith([...ALL, "center\\web\\assets\\index-abc.js"]))).toEqual([]);
  });

  it("reports every file of an installer built without the Center (plain `tauri build`)", () => {
    expect(missingPayload(nsiWith([]))).toEqual(ALL);
  });

  it("reports only what is missing", () => {
    expect(missingPayload(nsiWith(ALL.filter((t) => !t.endsWith("tour.html"))))).toEqual(["center\\web\\tour.html"]);
  });

  it("ignores case, forward slashes and doubled backslashes", () => {
    const nsi = ALL.map((t) => `File /a "/oname=${t.toUpperCase().replace(/\\/g, "\\\\")}" "x"`).join("\n");
    expect(missingPayload(nsi)).toEqual([]);
    expect(installedFiles('File /a "/oname=center/web/tour.html" "x"').has("center\\web\\tour.html")).toBe(true);
  });

  it("does not count a file that is only mentioned (a Delete line or a similarly named file)", () => {
    const nsi = `Delete "$INSTDIR\\center\\CompanyIsland.Center.exe"\nFile /a "/oname=center\\CompanyIsland.Center.exe.bak" "x"`;
    expect(missingPayload(nsi)).toContain("center\\CompanyIsland.Center.exe");
  });

  it("CLI: exits 1 on an installer without the Center and 0 with it", () => {
    const dir = mkdtempSync(join(tmpdir(), "ci-payload-"));
    try {
      const bad = join(dir, "bad.nsi");
      const good = join(dir, "good.nsi");
      writeFileSync(bad, nsiWith([]));
      writeFileSync(good, nsiWith(ALL));
      const run = (nsi: string) =>
        spawnSync(process.execPath, [script, "--check-installer-script", "--nsi", nsi], { encoding: "utf8" });
      const failed = run(bad);
      expect(failed.status).toBe(1);
      expect(failed.stderr).toContain("center\\CompanyIsland.Center.exe");
      expect(failed.stderr).toContain("build:installer");
      expect(run(good).status).toBe(0);
      expect(run(join(dir, "missing.nsi")).status).toBe(1);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("package.json checks every installer build and any fresh `tauri build`", () => {
    const scripts = JSON.parse(read("package.json")).scripts;
    expect(scripts["postbuild:installer"]).toContain("--check-installer-script");
    expect(scripts["check:installer"]).toContain("--check-installer-script");
    expect(scripts["posttauri"]).toContain("--after-tauri");
  });

  it("the installer config builds the Center and maps it to <install dir>\\center", () => {
    const conf = JSON.parse(read("src-tauri", "tauri.installer.conf.json"));
    expect(conf.build.beforeBuildCommand).toContain("build:center");
    expect(conf.bundle.resources["../center/publish"]).toBe("center");
  });
});

describe("installer hooks keep center\\ clean (installer-hooks.nsh)", () => {
  const hooks = read("src-tauri", "installer-hooks.nsh").replace(/\r\n/g, "\n");
  const macro = (name: string) => {
    const start = hooks.indexOf(`!macro ${name}`);
    expect(start).toBeGreaterThanOrEqual(0);
    return hooks.slice(start, hooks.indexOf("!macroend", start));
  };

  it("wipes the old center folder on install, after the programs are stopped", () => {
    const pre = macro("NSIS_HOOK_PREINSTALL");
    expect(pre).toContain("YUVAL_STOP_PRODUCT");
    expect(pre).toContain("YUVAL_REMOVE_CENTER_DIR");
    expect(pre.indexOf("YUVAL_STOP_PRODUCT")).toBeLessThan(pre.indexOf("YUVAL_REMOVE_CENTER_DIR"));
  });

  it("wipes center\\ and then the install folder on uninstall", () => {
    const post = macro("NSIS_HOOK_POSTUNINSTALL");
    expect(post).toContain("YUVAL_REMOVE_CENTER_DIR");
    expect(post).toContain('RMDir "$INSTDIR"');
    expect(post).not.toContain("RMDir /r");
  });

  it("only ever deletes recursively below a non-empty <folder>\\center", () => {
    // One macro does it for the install folder and for the folder of the old (CompanyIsland) product.
    const remove = macro("YUVAL_REMOVE_CENTER_DIR");
    expect(remove).toContain('${DIR} != ""');
    expect(remove).toContain('RMDir /r "${DIR}\\center"');
    const recursive = hooks.match(/RMDir \/r .*/g) ?? [];
    expect(recursive).toEqual(['RMDir /r "${DIR}\\center"']);
  });
});
