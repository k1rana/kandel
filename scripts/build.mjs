/**
 * Builds both Kandel binaries for one Rust target and stages them in `build/`.
 *
 * The core locates the UI beside itself, so both are always built and copied to
 * one folder. `tauri build` runs `stage-core.mjs`, which stages the core for the
 * same `--target`, so both binaries match.
 *
 *   node scripts/build.mjs                                # host target
 *   node scripts/build.mjs --target aarch64-pc-windows-msvc
 *
 * A cross target needs `rustup target add <triple>` and the matching MSVC linker.
 */
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const buildDir = path.join(root, "build");

/** Reads `--target <triple>` from the CLI, if given. */
function argTarget() {
  const i = process.argv.indexOf("--target");
  if (i === -1) return undefined;
  const value = process.argv[i + 1];
  if (!value || value.startsWith("--")) {
    console.error("[build] --target needs a Rust target triple");
    process.exit(1);
  }
  return value;
}

/** Runs a command to completion, inheriting stdio, and exits on failure. */
function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    // `pnpm` is a `.cmd` shim on Windows, so it needs a shell there.
    shell: process.platform === "win32",
  });
  if (result.error) {
    console.error(`[build] could not run ${command}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

/** The host triple, so a native build reads the plain `target/release` path. */
function hostTriple() {
  const result = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
  const match = result.error ? null : /host:\s*(\S+)/.exec(result.stdout ?? "");
  if (!match) {
    console.error("[build] could not determine the host target triple from `rustc -vV`");
    process.exit(1);
  }
  return match[1];
}

const target = argTarget();
const triple = target ?? hostTriple();
const ext = triple.includes("windows") ? ".exe" : "";

// Cargo writes any `--target` build under `target/<triple>/release`, even when
// that target is the host; only a build with no `--target` uses `target/release`.
const releaseDir = path.join(root, "target", target ? triple : "", "release");

console.log(`[build] target ${triple}${target ? "" : " (host)"}`);
mkdirSync(buildDir, { recursive: true });

console.log("[build] building the core…");
run("cargo", [
  "build",
  "--release",
  "-p",
  "kandel-core",
  ...(target ? ["--target", triple] : []),
]);

// `tauri build` runs `stage-core.mjs` (via `beforeBuildCommand`), which builds
// and stages the sidecar, then builds the frontend and bundles the UI.
console.log("[build] building the UI…");
run("pnpm", [
  "exec",
  "tauri",
  "build",
  "--config",
  "crates/kandel-ui/tauri.conf.json",
  ...(target ? ["--target", triple] : []),
]);

for (const name of ["kandel", "kandel-ui"]) {
  const from = path.join(releaseDir, `${name}${ext}`);
  if (!existsSync(from)) {
    console.error(`[build] expected ${name}${ext} at ${path.relative(root, from)}`);
    process.exit(1);
  }
  copyFileSync(from, path.join(buildDir, `${name}${ext}`));
  console.log(`[build] staged build/${name}${ext}`);
}
