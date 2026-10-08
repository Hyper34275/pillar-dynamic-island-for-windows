/**
 * Fails when the app version differs between package.json, src-tauri/Cargo.toml, src-tauri/tauri.conf.json
 * and center/Directory.Build.props. Runs as `prebuild:installer`, so a mismatched installer is never built.
 */

const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");
const read = (...parts) => fs.readFileSync(path.join(root, ...parts), "utf8");

// Only the [package] table: dependency tables also have `version = "..."` entries.
const cargoPackage = (read("src-tauri", "Cargo.toml").split(/^\[package\]\s*$/m)[1] || "").split(/^\[/m)[0];
const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargoPackage);
// The Yuval Center (WinUI 3) is versioned with the island: <Version> in its shared MSBuild props.
const centerVersion = /<Version>\s*([^<\s]+)\s*<\/Version>/.exec(read("center", "Directory.Build.props"));
const versions = {
  "package.json": JSON.parse(read("package.json")).version,
  "src-tauri/Cargo.toml": cargoVersion && cargoVersion[1],
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri", "tauri.conf.json")).version,
  "center/Directory.Build.props": centerVersion && centerVersion[1],
};

for (const [file, version] of Object.entries(versions)) console.log(`${file}: ${version}`);

if (Object.values(versions).some((v) => !v) || new Set(Object.values(versions)).size !== 1) {
  console.error("Version mismatch: set the same version in all four files.");
  process.exit(1);
}
