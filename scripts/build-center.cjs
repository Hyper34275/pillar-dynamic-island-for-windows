/**
 * Builds the Yuval Center (WinUI 3) into center/publish/ for the installer: `npm run build:center`.
 *
 * Run after `npm run build`: the Tour page (dist/tour.html) is copied next to the exe as center/publish/web/.
 * src-tauri/tauri.installer.conf.json maps center/publish to <install dir>\center\ (bundle.resources).
 *
 * The same file also guards against an installer built WITHOUT the Center (plain `tauri build`, which does not
 * pass tauri.installer.conf.json, so the Center is neither built nor bundled yet a normal-looking setup.exe comes
 * out). Tauri writes every bundled file into the generated installer.nsi, so that script is the proof of what the
 * setup contains (no 7-Zip needed):
 *
 *   node scripts/build-center.cjs --check-installer-script [--nsi <installer.nsi>]
 *       strict: exit 1 unless installer.nsi lists every REQUIRED_PAYLOAD file (postbuild:installer, verify-installer.ps1)
 *   node scripts/build-center.cjs --after-tauri
 *       `posttauri` hook: when `npm run tauri` just produced a setup for the current version, run the strict check
 *       on it; any other tauri command (dev, icon, info) does nothing.
 */

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const root = path.join(__dirname, "..");
const dist = path.join(root, "dist");
const publish = path.join(root, "center", "publish");
const project = path.join(root, "center", "CompanyIsland.Center", "CompanyIsland.Center.csproj");

// Not needed at run time: the Center uses no on-device AI. The WinAppSDK runtime packages ship them anyway
// (verified by running the published app without them).
const UNNEEDED_FILES = ["onnxruntime.dll", "DirectML.dll"];

// Files the installed Center cannot work without, relative to center/publish (= <install dir>\center\).
// The exe is the Center, the .pri its packaged resources (WinUI fails to start without it), tour.html the Welcome page.
const REQUIRED_PAYLOAD = ["Yuval.Center.exe", "Yuval.Center.pri", "web/tour.html"];
// The folder the resources are installed to (the value in tauri.installer.conf.json bundle.resources).
const PAYLOAD_DEST = "center";

const NSI_DEFAULT = path.join(root, "src-tauri", "target", "release", "nsis", "x64", "installer.nsi");
const SETUP_DIR = path.join(root, "src-tauri", "target", "release", "bundle", "nsis");
// A setup newer than this was produced by the `tauri` command that just finished.
const FRESH_SETUP_MS = 30 * 60 * 1000;

function fail(message) {
  console.error(`build-center: ${message}`);
  process.exit(1);
}

/** Every file below `dir`, as paths relative to it. */
function listFiles(dir, prefix = "") {
  const files = [];
  for (const entry of fs.readdirSync(path.join(dir, prefix), { withFileTypes: true })) {
    const rel = path.join(prefix, entry.name);
    if (entry.isDirectory()) files.push(...listFiles(dir, rel));
    else files.push(rel);
  }
  return files;
}

/** Files installed by a generated installer.nsi (`File /a "/oname=<dest>" "<source>"`), lower-case, backslashes. */
function installedFiles(nsiText) {
  const found = new Set();
  for (const match of nsiText.replace(/\\\\/g, "\\").matchAll(/\/oname=([^"\r\n]+)"/g)) {
    found.add(match[1].replace(/\//g, "\\").toLowerCase());
  }
  return found;
}

/** The REQUIRED_PAYLOAD files (as `center\...`) a generated installer.nsi does not install. Empty when complete. */
function missingPayload(nsiText) {
  const installed = installedFiles(nsiText);
  return REQUIRED_PAYLOAD.map((rel) => `${PAYLOAD_DEST}\\${rel.replace(/\//g, "\\")}`).filter(
    (target) => !installed.has(target.toLowerCase()),
  );
}

/** Checks one installer.nsi; returns an error message, or null when the setup built from it carries the Center. */
function checkInstallerScript(nsiPath) {
  if (!fs.existsSync(nsiPath)) {
    return `${nsiPath} not found: the installer was not built by Tauri's NSIS bundler, so its contents cannot be checked.`;
  }
  const missing = missingPayload(fs.readFileSync(nsiPath, "utf8"));
  if (missing.length === 0) return null;
  return (
    `the installer does NOT contain: ${missing.join(", ")}.\n` +
    "  It was built without the Yuval Center (a plain `tauri build` ignores tauri.installer.conf.json).\n" +
    "  Do not ship it. Rebuild with `npm run build:installer`."
  );
}

function argValue(args, flag) {
  const i = args.indexOf(flag);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : null;
}

/** `posttauri`: only a `tauri build` leaves a fresh setup for the current version behind. */
function afterTauri(nsiPath) {
  const version = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8")).version;
  const setup = path.join(SETUP_DIR, `Yuval_${version}_x64-setup.exe`);
  if (!fs.existsSync(setup) || Date.now() - fs.statSync(setup).mtimeMs > FRESH_SETUP_MS) return;
  const problem = checkInstallerScript(nsiPath);
  if (problem) fail(`${path.basename(setup)}: ${problem}`);
  console.log(`build-center: ${path.basename(setup)} includes the Yuval Center.`);
}

function build() {
  // Fail before touching anything when the web build is missing (the Tour would be an empty page).
  if (!fs.existsSync(path.join(dist, "tour.html"))) {
    fail("dist/tour.html not found. Run `npm run build` first (it builds the island and the Tour with Vite).");
  }

  fs.rmSync(publish, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });

  console.log("build-center: dotnet publish (Release, win-x64, self-contained) ...");
  const result = spawnSync("dotnet", ["publish", project, "-c", "Release", "-o", publish, "--nologo"], {
    cwd: root,
    stdio: "inherit",
  });
  if (result.error) fail(`could not run dotnet (${result.error.message}). Is the .NET SDK installed and on PATH?`);
  if (result.status !== 0) fail(`dotnet publish failed (exit code ${result.status}).`);
  if (!fs.existsSync(path.join(publish, "Yuval.Center.exe"))) {
    fail("dotnet publish succeeded but center/publish/Yuval.Center.exe is missing.");
  }

  // Trim the publish folder: AI/ML leftovers and debug symbols (never loaded, not worth shipping).
  for (const rel of listFiles(publish)) {
    const name = path.basename(rel).toLowerCase();
    if (UNNEEDED_FILES.some((f) => f.toLowerCase() === name) || name.endsWith(".pdb")) {
      fs.rmSync(path.join(publish, rel), { force: true });
    }
  }

  fs.cpSync(dist, path.join(publish, "web"), { recursive: true });
  if (!fs.existsSync(path.join(publish, "web", "tour.html"))) fail("copying dist/ to center/publish/web/ failed.");

  // Everything the installer is checked for must exist here, or the check on installer.nsi could never pass.
  const absent = REQUIRED_PAYLOAD.filter((rel) => !fs.existsSync(path.join(publish, ...rel.split("/"))));
  if (absent.length > 0) fail(`center/publish is missing required files: ${absent.join(", ")}`);

  // The installer script quotes these paths: characters NSIS would interpret must not occur in them.
  const files = listFiles(publish);
  const risky = files.filter((f) => /[$"`]/.test(f));
  if (risky.length > 0) fail(`file names the NSIS installer cannot take: ${risky.join(", ")}`);

  const bytes = files.reduce((sum, f) => sum + fs.statSync(path.join(publish, f)).size, 0);
  console.log(`build-center: ${files.length} files, ${(bytes / 1024 / 1024).toFixed(1)} MiB in center/publish`);
}

module.exports = { REQUIRED_PAYLOAD, installedFiles, missingPayload, checkInstallerScript };

if (require.main === module) {
  const args = process.argv.slice(2);
  const nsiPath = path.resolve(argValue(args, "--nsi") || NSI_DEFAULT);
  if (args.includes("--check-installer-script")) {
    const problem = checkInstallerScript(nsiPath);
    if (problem) fail(problem);
    console.log(`build-center: installer script lists ${REQUIRED_PAYLOAD.length} required Center files.`);
  } else if (args.includes("--after-tauri")) {
    afterTauri(nsiPath);
  } else {
    build();
  }
}
