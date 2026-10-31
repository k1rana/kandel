/**
 * Packages the release build into a distributable archive.
 *
 * Kandel is two executables, the core (`kandel.exe`, the resident supervisor and
 * tray) and the UI (`kandel-ui.exe`, launched by the core on demand), and the
 * core locates the UI beside itself. Both therefore have to ship together in one
 * folder; a bundle containing only one of them is non-functional.
 *
 * The NSIS installer Tauri produces already contains the core, because
 * `stage-core.mjs` stages it as a sidecar before `tauri build` runs. This script
 * additionally produces the portable ZIP the README documents: a folder the user
 * can extract and run without installing.
 *
 * It packages one architecture per run, given as `--target <rust-triple>` (or
 * `--arch <x64|arm64>`). With no argument the host architecture is used.
 *
 * Outputs, both under `build/`:
 *   * `kandel-<version>-windows-<arch>.zip` — portable archive containing both
 *     binaries, the README, the LICENSE, and the NOTICE.
 *   * `kandel-<version>-windows-<arch>-setup.exe` — the NSIS installer, renamed
 *     from Tauri's hard-coded `<productName>_<version>_<arch>-setup.exe` shape
 *     so every release asset shares one lowercase scheme.
 */
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const isWindows = process.platform === "win32";

/**
 * Rust target triples Kandel ships, mapped to the WinGet architecture token
 * used in the asset names. Add a row here to build a further architecture.
 */
const ARCHES = {
  "x86_64-pc-windows-msvc": "x64",
  "aarch64-pc-windows-msvc": "arm64",
};

/** Reads `--target <triple>` / `--arch <token>` from the CLI, if either is given. */
function argArch() {
  const args = process.argv.slice(2);
  const valueOf = (flag) => {
    const i = args.indexOf(flag);
    return i === -1 ? undefined : args[i + 1];
  };
  const target = valueOf("--target");
  const arch = valueOf("--arch");
  if (!target && !arch) return undefined;
  if (target && !ARCHES[target]) {
    fail(`unknown target ${target} — expected one of ${Object.keys(ARCHES).join(", ")}`);
  }
  if (arch && !Object.values(ARCHES).includes(arch)) {
    fail(`unknown arch ${arch} — expected one of ${Object.values(ARCHES).join(", ")}`);
  }
  const fromTarget = target && ARCHES[target];
  if (fromTarget && arch && fromTarget !== arch) {
    fail(`--target ${target} is ${fromTarget}, which does not match --arch ${arch}`);
  }
  return { triple: target, arch: fromTarget ?? arch };
}

/** The host triple, whose WinGet token stands in when no target was given. */
function hostTriple() {
  const result = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
  const match = result.error ? null : /host:\s*(\S+)/.exec(result.stdout ?? "");
  if (!match) {
    fail("could not determine the host target triple; pass --target or --arch explicitly");
  }
  return match[1];
}

/** Bails out with a clear message. */
function fail(message) {
  console.error(`[package] ${message}`);
  process.exit(1);
}

// The version is single-sourced from `[workspace.package]` in the root
// Cargo.toml — the one place Cargo and Tauri both read it from. Nothing else
// declares it, so this is the only locator to keep in step with a bump.
function workspaceVersion() {
  const toml = readFileSync(path.join(root, "Cargo.toml"), "utf8");
  const section = toml
    .split(/^\[/m)
    .find((part) => part.startsWith("workspace.package]"));
  const match = section && /^version\s*=\s*"([^"]+)"/m.exec(section);
  if (!match) fail("cannot read the version from [workspace.package] in Cargo.toml");
  return match[1];
}

const version = workspaceVersion();

// Resolve the architecture: an explicit flag wins, otherwise the host. `triple`
// is kept for the Tauri bundle path; `arch` is the token in every asset name.
const requested = argArch();
const triple = requested?.triple ?? hostTriple();
const arch = requested?.arch ?? ARCHES[triple];
if (!arch) {
  fail(`unsupported architecture for ${triple} — add it to ARCHES in this script`);
}

const buildDir = path.join(root, "build");
const name = `kandel-${version}-windows-${arch}`;
const stageDir = path.join(buildDir, name);
const archive = path.join(buildDir, `${name}.zip`);

// Tauri names the NSIS installer `<productName>_<version>_<arch>-setup.exe`
// (e.g. `Kandel_1.1.0_x64-setup.exe`); `productName` is capitalized for display
// and cannot be changed per-bundle, so the file is renamed here instead.
const installer = path.join(buildDir, `kandel-${version}-windows-${arch}-setup.exe`);

// ── 1. Verify the build inputs exist ───────────────────────────────────────
const required = ["kandel.exe", "kandel-ui.exe"];
for (const file of required) {
  const full = path.join(buildDir, file);
  if (!existsSync(full)) {
    fail(
      `${file} is missing from build/ — run "node scripts/build.mjs${requested?.triple ? ` --target ${requested.triple}` : ""}" first`,
    );
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

// ── 3b. Normalize the NSIS installer name ──────────────────────────────────
// Tauri leaves it at `<productName>_<version>_<arch>-setup.exe`, with `x64` or
// `arm64` as the token. Copy it to the lowercase scheme above so the release
// assets are consistent. Match on the version, not just the suffix: a stale
// installer from a previous build would otherwise be renamed to the current
// version and shipped wrong.
//
// A `--target` build lands under `target/<triple>/release` and a host build
// under plain `target/release`. This script cannot tell which produced the
// binaries in build/, so it tries the targeted path first, then the host path.
const nsisDirs = requested?.triple
  ? [
      path.join(root, "target", triple, "release", "bundle", "nsis"),
      path.join(root, "target", "release", "bundle", "nsis"),
    ]
  : [path.join(root, "target", "release", "bundle", "nsis")];

const nsisDir = nsisDirs.find((dir) => existsSync(dir));
const setup = nsisDir
  ? readdirSync(nsisDir).find((f) => f.endsWith(`_${version}_${arch}-setup.exe`))
  : undefined;
let installerCopied = false;
if (setup) {
  copyFileSync(path.join(nsisDir, setup), installer);
  installerCopied = true;
} else {
  console.warn(
    `[package] no NSIS installer for ${version} in ${path.relative(root, nsisDir)} — ` +
      "run \"node scripts/build.mjs\" (which runs `tauri build`) to produce one",
  );
}

// ── 4. Report what shipped ─────────────────────────────────────────────────
console.log(`[package] arch: ${arch} (${triple})`);
const size = statSync(archive).size;
console.log(`[package] archive: ${path.relative(root, archive)} (${(size / 1024).toFixed(0)} KiB)`);
if (installerCopied) {
  const installerSize = statSync(installer).size;
  console.log(`[package] installer: ${path.relative(root, installer)} (${(installerSize / 1024).toFixed(0)} KiB)`);
}
console.log("[package] contents:");
for (const entry of readdirSync(stageDir)) {
  console.log(`  ${entry}`);
}
