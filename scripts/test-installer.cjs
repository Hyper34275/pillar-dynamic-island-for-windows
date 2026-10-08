#!/usr/bin/env node
/**
 * Tests for the installer script without installing anything: `node scripts/test-installer.cjs [check|diff|render <dir>]`.
 *
 *   check   (default) 1. renders src-tauri/nsis/installer.nsi the way Tauri's bundler would (a small Handlebars
 *                        subset, enough for this template) with dummy data, for the product name "Yuval" and for
 *                        the legacy name "CompanyIsland", and compiles both with makensis (syntax, includes, every
 *                        LangString, the conditional migration code);
 *                     2. compiles src-tauri/nsis/tests/logic.nsi in several variants and runs them silently, as the
 *                        current user, inside a temp folder: path cleaning, the migration's file removal, shortcut
 *                        removal, closing a real (copied) process, the downgrade refusal and its exit code, the OS
 *                        gate and its exit code. No registry write, no file outside the temp folder, no elevation.
 *   diff               prints the difference between the stock template embedded in the installed @tauri-apps/cli
 *                      and our copy (only the YUVAL-CHANGE hunks should show). Use it after a Tauri upgrade.
 *   render <dir>       writes the rendered installer.nsi and its helper files into <dir> for inspection.
 *
 * Needs the NSIS toolchain that `tauri build` downloads (%LOCALAPPDATA%\tauri\NSIS, or --makensis <path>) and
 * node_modules (the Tauri CLI carries the template and the helper .nsh files). Without them it prints SKIPPED and
 * exits 0, so a machine that never built the installer is not blocked.
 */
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");

const root = path.resolve(__dirname, "..");
const TEMPLATE = path.join(root, "src-tauri", "nsis", "installer.nsi");
const HOOKS = path.join(root, "src-tauri", "installer-hooks.nsh");
const LOGIC = path.join(root, "src-tauri", "nsis", "tests", "logic.nsi");
const CLI_NODE = path.join(root, "node_modules", "@tauri-apps", "cli-win32-x64-msvc", "cli.win32-x64-msvc.node");

// ---------------------------------------------------------------------------------------------------------------
// Tool discovery

function findMakensis() {
  const i = process.argv.indexOf("--makensis");
  const candidates = [];
  if (i >= 0 && process.argv[i + 1]) candidates.push(process.argv[i + 1]);
  if (process.env.LOCALAPPDATA) candidates.push(path.join(process.env.LOCALAPPDATA, "tauri", "NSIS", "makensis.exe"));
  return candidates.find((c) => fs.existsSync(c)) || null;
}

function pluginDir(makensis) {
  // tauri-utils plugin (SemverCompare, RunAsUser, StrReplace) sits next to the NSIS install.
  return path.join(path.dirname(makensis), "Plugins", "x86-unicode", "additional");
}

// ---------------------------------------------------------------------------------------------------------------
// Files Tauri embeds in the CLI (the native module holds them as plain text)

function readCli() {
  if (!fs.existsSync(CLI_NODE)) return null;
  return fs.readFileSync(CLI_NODE);
}

/** The text from `start` up to and including the first `end` after it (both byte strings), or null. */
function sliceBetween(buf, start, end) {
  const s = buf.indexOf(start);
  if (s < 0) return null;
  const e = buf.indexOf(end, s);
  if (e < 0) return null;
  return buf.slice(s, e + Buffer.byteLength(end)).toString("utf8");
}

function extractStock(buf) {
  return {
    template: sliceBetween(
      buf,
      "Unicode true\r\nManifestDPIAware true\r\n",
      '!insertmacro SetLnkAppUserModelId "$DESKTOP\\${PRODUCTNAME}.lnk"\r\nFunctionEnd\r\n',
    ),
    utils: sliceBetween(buf, "; Change shell and registry context based on running", "  Push $3\r\n!macroend\r\n"),
    fileAssociation: sliceBetween(
      buf,
      "; from https://gist.github.com/nikku/",
      'SHChangeNotify(i,i,i,i) (${SHCNE_ASSOCCHANGED}, ${SHCNF_FLUSH}, 0, 0)"\r\n!macroend\r\n',
    ),
    english: (() => {
      const s = buf.indexOf("LangString addOrReinstall ${LANG_ENGLISH}");
      if (s < 0) return null;
      const lines = [];
      for (const line of buf.slice(s, s + 8000).toString("utf8").split("\r\n")) {
        if (!/^LangString \w+ \$\{LANG_ENGLISH\} /.test(line)) break;
        lines.push(line);
      }
      return lines.join("\r\n") + "\r\n";
    })(),
  };
}

// ---------------------------------------------------------------------------------------------------------------
// A tiny Handlebars: the constructs installer.nsi uses, nothing else (anything else throws, loudly).

function parseTemplate(src) {
  const tokens = [];
  const re = /\{\{(~?)([\s\S]*?)(~?)\}\}/g;
  let last = 0;
  let m;
  while ((m = re.exec(src))) {
    if (m.index > last) {
      let text = src.slice(last, m.index);
      // Handlebars: two backslashes right before a tag are one literal backslash, then the tag is evaluated.
      if (text.endsWith("\\\\")) text = text.slice(0, -1);
      else if (text.endsWith("\\")) throw new Error("escaped tag (single backslash before double braces) is not supported");
      tokens.push({ t: "text", v: text });
    }
    tokens.push({ t: "tag", v: m[2].trim() });
    last = re.lastIndex;
  }
  if (last < src.length) tokens.push({ t: "text", v: src.slice(last) });

  let pos = 0;
  function parseNodes(closing) {
    const nodes = [];
    while (pos < tokens.length) {
      const tok = tokens[pos++];
      if (tok.t === "text") {
        nodes.push(tok);
      } else if (tok.v.startsWith("/")) {
        if (tok.v !== "/" + closing) throw new Error(`unexpected {{${tok.v}}}`);
        return nodes;
      } else if (tok.v.startsWith("#")) {
        const [kind, ...rest] = tok.v.slice(1).split(/\s+/);
        if (kind !== "if" && kind !== "each") throw new Error(`unsupported block {{${tok.v}}}`);
        nodes.push({ t: "block", kind, expr: rest[0], children: parseNodes(kind) });
      } else {
        nodes.push({ t: "var", v: tok.v });
      }
    }
    if (closing) throw new Error(`unclosed {{#${closing}}}`);
    return nodes;
  }
  return parseNodes(null);
}

function renderNodes(nodes, ctx, stack) {
  let out = "";
  for (const n of nodes) {
    if (n.t === "text") out += n.v;
    else if (n.t === "var") out += evalVar(n.v, ctx, stack);
    else if (n.kind === "if") {
      const v = lookup(n.expr, ctx, stack);
      if (v && !(Array.isArray(v) && v.length === 0)) out += renderNodes(n.children, ctx, stack);
    } else {
      const v = lookup(n.expr, ctx, stack);
      const entries = Array.isArray(v) ? v.map((x, i) => [i, x]) : Object.entries(v || {});
      for (const [key, value] of entries) out += renderNodes(n.children, ctx, [...stack, { this: value, key }]);
    }
  }
  return out;
}

function lookup(name, ctx, stack) {
  const top = stack[stack.length - 1];
  if (name === "this") return top.this;
  if (name === "@key") return top.key;
  const idx = /^this\.\[(\d+)\]$/.exec(name);
  if (idx) return top.this[Number(idx[1])];
  if (!(name in ctx)) throw new Error(`template variable not provided: ${name}`);
  return ctx[name];
}

function evalVar(expr, ctx, stack) {
  const parts = expr.split(/\s+/);
  if (parts[0] === "no-escape") return String(lookup(parts[1], ctx, stack));
  if (parts.length > 1) throw new Error(`unsupported helper {{${expr}}}`);
  return String(lookup(expr, ctx, stack));
}

function renderTemplate(src, ctx) {
  return renderNodes(parseTemplate(src), ctx, [{ this: ctx, key: "" }]);
}

// ---------------------------------------------------------------------------------------------------------------
// Rendering our template with dummy data

function renderOurs(dir, makensis, cli, productName) {
  const stock = extractStock(cli);
  for (const [k, v] of Object.entries(stock)) {
    if (!v) throw new Error(`could not find ${k} inside the Tauri CLI; the CLI version changed, update scripts/test-installer.cjs`);
  }
  const out = path.join(dir, "x64");
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, "utils.nsh"), stock.utils);
  fs.writeFileSync(path.join(out, "FileAssociation.nsh"), stock.fileAssociation);
  fs.writeFileSync(path.join(out, "English.nsh"), stock.english);

  // A payload shaped like the real one: the main exe, and center\ as bundle resources.
  const payload = path.join(dir, "payload");
  fs.mkdirSync(path.join(payload, "center", "web"), { recursive: true });
  const mainExe = path.join(payload, `${productName}.exe`);
  fs.writeFileSync(mainExe, "MZ");
  const resources = {};
  for (const rel of [`center\\${productName}.Center.exe`, "center\\web\\tour.html"]) {
    const src = path.join(payload, ...rel.split("\\"));
    fs.writeFileSync(src, "x");
    resources[src] = [src, rel];
  }

  const data = {
    compression: "none",
    signed_plugins_path: "",
    installer_hooks: HOOKS,
    manufacturer: productName,
    product_name: productName,
    version: "1.0.14",
    version_with_build: "1.0.14.0",
    homepage: "",
    install_mode: "perMachine",
    license: "",
    installer_icon: "",
    sidebar_image: "",
    header_image: "",
    uninstaller_icon: "",
    uninstaller_header_image: "",
    main_binary_name: productName,
    main_binary_path: mainExe,
    bundle_id: "com.companyisland.app",
    copyright: "test",
    out_file: path.join(dir, "setup-test.exe"),
    arch: "x64",
    additional_plugins_path: pluginDir(makensis),
    allow_downgrades: "true",
    display_language_selector: "false",
    install_webview2_mode: "downloadBootstrapper",
    webview2_installer_args: "/silent",
    webview2_bootstrapper_path: "",
    webview2_installer_path: "",
    minimum_webview2_version: "111.0.1661.41",
    uninstaller_sign_cmd: "",
    estimated_size: "1000",
    start_menu_folder: productName,
    languages: ["English", "Hebrew"],
    language_files: [path.join(out, "English.nsh")],
    resources_dirs: ["center", "center\\web"],
    resources,
    binaries: [],
    file_associations: [],
    deep_link_protocols: [],
    resources_ancestors: ["center\\web", "center"],
  };
  const nsi = renderTemplate(fs.readFileSync(TEMPLATE, "utf8"), data);
  const nsiPath = path.join(out, "installer.nsi");
  fs.writeFileSync(nsiPath, nsi);
  return { nsiPath, stock };
}

// ---------------------------------------------------------------------------------------------------------------

// A check is [passed, label, detail?]. Jobs return arrays of checks so that parallel jobs still print in order.
function spawnP(cmd, args, cwd, timeout = 180000) {
  return new Promise((resolve) => {
    let out = "";
    let done = false;
    const finish = (status) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      resolve({ status, out });
    };
    const child = spawn(cmd, args, { cwd, windowsHide: true });
    const timer = setTimeout(() => {
      out += "\n(timed out)";
      child.kill();
    }, timeout);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (out += d));
    child.on("close", (status) => finish(status));
    child.on("error", (e) => {
      out += String(e);
      finish(-1);
    });
  });
}

const makensisP = (makensis, args, cwd) => spawnP(makensis, ["/V2", ...args], cwd);

/** Runs fn over items with at most `limit` in flight; results keep the item order. */
async function mapLimit(items, limit, fn) {
  const results = new Array(items.length);
  let next = 0;
  async function worker() {
    while (next < items.length) {
      const i = next++;
      results[i] = await fn(items[i], i);
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return results;
}

async function checkCompile(makensis, cli, tmp, productName) {
  const dir = path.join(tmp, `render-${productName}`);
  fs.mkdirSync(dir, { recursive: true });
  const { nsiPath } = renderOurs(dir, makensis, cli, productName);
  const r = await makensisP(makensis, [nsiPath], path.dirname(nsiPath));
  const checks = [[r.status === 0, `template renders and compiles with makensis (product "${productName}")`, r.out.split("\n").slice(-25).join("\n")]];
  const warnings = r.out.split("\n").filter((l) => /warning/i.test(l));
  // 6040 = a LangString without a Hebrew text: the Hebrew installer would show that line in English. installer-hooks.nsh
  // supplies Hebrew for Tauri's strings; a Tauri upgrade that adds a string shows up here.
  const missingHebrew = warnings.filter((l) => /6040/.test(l));
  checks.push([missingHebrew.length === 0, `every LangString has a Hebrew text (product "${productName}")`, [...new Set(missingHebrew)].join("\n")]);
  // (A build still named CompanyIsland skips the migration, so its functions are legitimately "not referenced".)
  if (productName === "Yuval") {
    const ours = warnings.filter((l) => /yuval/i.test(l));
    checks.push([ours.length === 0, `no makensis warning about the Yuval strings/functions (product "${productName}")`, ours.join("\n")]);
  }
  return checks;
}

// One variant of logic.nsi: compile with /D defines, run silently in its own temp folder.
async function logicVariant(makensis, stock, tmp, v) {
  const { name, defines, expectExit, expectLines } = v;
  const dir = path.join(tmp, `logic-${name}`);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "utils.nsh"), stock.utils);
  const exe = path.join(dir, "logic.exe");
  const defs = Object.entries({ ...defines, TESTDIR: dir, OUTEXE: exe, HOOKS, PLUGINS: pluginDir(makensis), UTILS: path.join(dir, "utils.nsh") }).map(
    ([k, val]) => `/D${k}=${val}`,
  );
  const c = await makensisP(makensis, [...defs, LOGIC], dir);
  if (c.status !== 0) return [[false, `logic.nsi variant ${name} compiles`, c.out.split("\n").slice(-25).join("\n")]];
  const r = await spawnP(exe, ["/S"], dir);
  const results = fs.existsSync(path.join(dir, "results.txt")) ? fs.readFileSync(path.join(dir, "results.txt"), "utf8") : "";
  const checks = [[r.status === expectExit, `variant ${name}: exit code ${expectExit}`, `got ${r.status}\n${results}`]];
  for (const line of results.split(/\r?\n/).filter(Boolean)) {
    if (line.startsWith("FAIL")) checks.push([false, `variant ${name}: ${line}`]);
  }
  for (const want of expectLines || []) checks.push([results.includes(want), `variant ${name}: ${want}`, results]);
  return checks;
}

function printChecks(checks) {
  let failed = 0;
  for (const [passed, label, detail] of checks) {
    if (passed) {
      console.log(`  ok    ${label}`);
    } else {
      failed += 1;
      console.log(`  FAIL  ${label}${detail ? `\n        ${String(detail).split("\n").join("\n        ")}` : ""}`);
    }
  }
  return failed;
}

// [installed version, installer version, expected exit code, line the run must write]
const DECIDE_MATRIX = [
  ["none", "1.0.14", 0, "PASS not installed: same=0"],
  ["1.0.13", "1.0.14", 0, "PASS 1.0.13: same=1"],
  ["1.0.14", "1.0.14", 0, "PASS 1.0.14: same=1"],
  ["1.0.9", "1.0.10", 0, "PASS 1.0.9: same=1"],
  ["unknown", "1.0.14", 0, "PASS unknown: same=1"],
  ["1.0.15", "1.0.14", 1638, null],
  ["1.0.10", "1.0.9", 1638, null],
  ["2.0.0", "1.99.99", 1638, null],
];

async function check() {
  const makensis = findMakensis();
  const cli = readCli();
  if (!makensis || !cli) {
    console.log(`SKIPPED: ${!makensis ? "NSIS toolchain not found (run a tauri build once, or pass --makensis)" : "Tauri CLI not found in node_modules"}.`);
    return 0;
  }
  const stock = extractStock(cli);
  for (const [k, v] of Object.entries(stock)) {
    if (!v) throw new Error(`could not find ${k} inside the Tauri CLI; the CLI version changed, update scripts/test-installer.cjs`);
  }
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "yuval-nsis-"));
  let failed = 0;
  try {
    console.log(`makensis: ${makensis}\ntemp: ${tmp}`);
    const variants = [
      // The temp folder is below ...\AppData\..., which the migration (rightly) refuses to touch, so the file tests
      // move the "user data" marker; the real marker is checked by the "guard" variant.
      {
        name: "units",
        defines: { TEST: "units", USERDATA_MARKER: "Yuval-NSIS-Test-Profile" },
        expectExit: 0,
        expectLines: ["PASS clean path", "PASS gather", "PASS migrate dir", "PASS shortcut", "PASS stop process", "PASS center dir", "PASS semver"],
      },
      { name: "guard", defines: { TEST: "guard" }, expectExit: 0, expectLines: ["PASS migrate dir: AppData: user data kept"] },
      ...DECIDE_MATRIX.map(([installed, version, exit, line]) => ({
        name: `decide-${installed}-to-${version}`,
        defines: { TEST: "decide", INSTALLED: installed, VERSION_UNDER_TEST: version },
        expectExit: exit,
        expectLines: line ? [line] : [],
      })),
      { name: "os-gate-ok", defines: { TEST: "osgate", MINBUILD: "10240" }, expectExit: 0, expectLines: [] },
      { name: "os-gate-too-new-required", defines: { TEST: "osgate", MINBUILD: "99999" }, expectExit: 1603, expectLines: [] },
    ];
    // Everything runs in parallel (a handful at a time): each job has its own folder. The one that closes a real
    // process, "units", names it uniquely.
    const jobs = [
      ...["Yuval", "CompanyIsland"].map((p) => () => checkCompile(makensis, cli, tmp, p)),
      ...variants.map((v) => () => logicVariant(makensis, stock, tmp, v)),
    ];
    const groups = await mapLimit(jobs, 6, (job) => job());
    console.log("template compile");
    failed += printChecks(groups.slice(0, 2).flat());
    console.log("installer logic (silent, current user, temp folder only)");
    failed += printChecks(groups.slice(2).flat());
  } finally {
    if (!process.argv.includes("--keep")) {
      try {
        fs.rmSync(tmp, { recursive: true, force: true });
      } catch (e) {
        console.log(`(could not remove ${tmp}: ${e.message})`);
      }
    } else {
      console.log(`kept ${tmp}`);
    }
  }
  console.log(failed === 0 ? "RESULT: OK" : `RESULT: FAILED (${failed})`);
  return failed === 0 ? 0 : 1;
}

/**
 * `diff`: prints how our template differs from the stock one embedded in the installed CLI.
 * `diff --check`: exits 1 if a changed region carries no YUVAL-CHANGE comment (a silent edit of Tauri's text).
 */
function diff() {
  const check = process.argv.includes("--check");
  const cli = readCli();
  if (!cli) {
    console.log(check ? "SKIPPED: Tauri CLI not found in node_modules." : "Tauri CLI not found in node_modules.");
    return check ? 0 : 1;
  }
  const stock = extractStock(cli).template;
  if (!stock) {
    console.log("Could not find the stock template in the CLI.");
    return 1;
  }
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "yuval-nsis-diff-"));
  try {
    fs.writeFileSync(path.join(tmp, "stock.nsi"), stock.replace(/\r\n/g, "\n"));
    fs.writeFileSync(path.join(tmp, "ours.nsi"), fs.readFileSync(TEMPLATE, "utf8").replace(/\r\n/g, "\n"));
    const gitDiff = (extra) =>
      spawnSync("git", ["diff", "--no-index", "--no-color", ...extra, "stock.nsi", "ours.nsi"], { cwd: tmp, encoding: "utf8" }).stdout;
    if (check) {
      // -U3: changes closer than 7 lines are one hunk (git splits a big removal where stock lines happen to repeat).
      const hunks = gitDiff(["-U3"]).split(/^@@.*$/m).slice(1);
      const unmarked = hunks.filter((h) => !h.split("\n").some((l) => l.startsWith("+") && l.includes("YUVAL-CHANGE")));
      console.log(`template differs from the stock text in ${hunks.length} places; ${unmarked.length} without a YUVAL-CHANGE comment`);
      for (const h of unmarked) console.log(`${h.trim().split("\n").slice(0, 8).join("\n")}\n---`);
      return unmarked.length === 0 ? 0 : 1;
    }
    console.log(gitDiff(["--stat"]));
    console.log(gitDiff(["-U2"]));
    return 0;
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function render(dir) {
  const makensis = findMakensis();
  const cli = readCli();
  if (!makensis || !cli || !dir) {
    console.log("usage: node scripts/test-installer.cjs render <dir>  (needs the NSIS toolchain and node_modules)");
    return 1;
  }
  fs.mkdirSync(dir, { recursive: true });
  const { nsiPath } = renderOurs(path.resolve(dir), makensis, cli, "Yuval");
  console.log(nsiPath);
  return 0;
}

if (require.main === module) {
  const cmd = process.argv[2] && !process.argv[2].startsWith("--") ? process.argv[2] : "check";
  try {
    if (cmd === "diff") process.exit(diff());
    else if (cmd === "render") process.exit(render(process.argv[3]));
    else check().then((code) => process.exit(code), (e) => { console.error(e.stack || String(e)); process.exit(1); });
  } catch (e) {
    console.error(e.stack || String(e));
    process.exit(1);
  }
}

module.exports = { renderTemplate, extractStock };
