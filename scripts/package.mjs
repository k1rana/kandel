/**
 * Packages the release build into a distributable archive.
 *
 * Kandel is two executables — the core (`kandel.exe`, the resident supervisor
 * and tray) and the UI (`kandel-ui.exe`, launched by the core on demand) — and
 * the core locates the UI *beside itself*. Both therefore have to ship together
 * in one folder; a bundle containing only one of them is non-functional.
 *
 * Tauri's own NSIS bundler is not used for this: it knows only about the Tauri
 * app (the UI), so its installer ships `kandel-ui.exe` alone and silently omits
 * the core. This script produces the portable archive the README documents.
 *
 * Output: `build/kandel-<version>-windows-amd64.zip`, containing both binaries,
 * the README, the LICENSE, and the NOTICE.
 */
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(readFileSync(path.join(root, "package.json"), "utf8"));
const version = pkg.version;
const isWindows = process.platform === "win32";

const buildDir = path.join(root, "build");
const name = `kandel-${version}-windows-amd64`;
const stageDir = path.join(buildDir, name);
const archive = path.join(buildDir, `${name}.zip`);

/** Bails out with a clear message. */
function fail(message) {
  console.error(`[package] ${message}`);
  process.exit(1);
}

// ── 1. Verify the build inputs exist ───────────────────────────────────────
const required = ["kandel.exe", "kandel-ui.exe"];
for (const file of required) {
  const full = path.join(buildDir, file);
  if (!existsSync(full)) {
    fail(`${file} is missing from build/ — run "pnpm run build" first`);
  }
}

// ── 2. Stage both binaries together ────────────────────────────────────────
mkdirSync(stageDir, { recursive: true });
for (const file of required) {
  copyFileSync(path.join(buildDir, file), path.join(stageDir, file));
}
for (const extra of ["README.md", "LICENSE", "NOTICE"]) {
  const src = path.join(root, extra);
  if (existsSync(src)) copyFileSync(src, path.join(stageDir, extra));
}

// ── 3. Zip the staged folder ───────────────────────────────────────────────
// Prefer `7z` when it is on PATH, then the usual install locations, then
// PowerShell's Compress-Archive (which ships with Windows). Nothing here is
// machine-specific: each candidate is probed before use.
function firstExisting(paths) {
  return paths.find((p) => existsSync(p));
}

function trySpawn(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  return result.status === 0;
}

function zipWith7z() {
  const programFiles = process.env["ProgramFiles"] ?? "C:/Program Files";
  const programFilesX86 = process.env["ProgramFiles(x86)"] ?? "C:/Program Files (x86)";
  const exe = firstExisting([
    path.join(programFiles, "7-Zip", "7z.exe"),
    path.join(programFilesX86, "7-Zip", "7z.exe"),
  ]);
  // `7z` may already be on PATH even when neither default location exists.
  return trySpawn(exe ?? "7z", ["a", "-tzip", "-mx=9", archive, stageDir]);
}

function zipWithPowerShell() {
  // A literal single quote in a path would break the embedded command; the
  // build paths here never contain one, but guard rather than produce a
  // confusing failure.
  if (stageDir.includes("'") || archive.includes("'")) return false;
  return trySpawn("powershell.exe", [
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    `Compress-Archive -Path '${stageDir}' -DestinationPath '${archive}' -Force`,
  ]);
}

let ok = false;
if (isWindows) {
  if (existsSync(archive)) rmSync(archive);
  ok = zipWith7z() || zipWithPowerShell();
} else {
  // A non-Windows host is only packaging, not running, the app.
  ok = trySpawn("zip", ["-r", archive, name]);
}

if (!ok) fail("could not create the archive (no working zip tool found)");

// ── 4. Report what shipped ─────────────────────────────────────────────────
const size = statSync(archive).size;
console.log(`[package] archive: ${path.relative(root, archive)} (${(size / 1024).toFixed(0)} KiB)`);
console.log("[package] contents:");
for (const entry of readdirSync(stageDir)) {
  console.log(`  ${entry}`);
}
