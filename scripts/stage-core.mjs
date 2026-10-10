/**
 * Builds the core and stages it as a Tauri sidecar.
 *
 * The NSIS installer Tauri produces contains only the Tauri app (`kandel-ui.exe`),
 * so an installer built without this step ships a UI with no supervisor. Tauri's
 * `bundle.externalBin` is the supported way to include a second binary: it looks
 * for `<name>-<target-triple>.exe` and installs it beside the main executable —
 * which is exactly where the core expects to find the UI.
 *
 * Runs from `beforeBuildCommand`, so `tauri build` alone produces a complete
 * installer.
 */
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// Tauri resolves external binaries by target triple; detect it from rustc so
// the name stays correct if the toolchain or architecture changes.
// `cargo`/`rustc` are real executables, so no shell is needed (and passing args
// with a shell would risk quoting problems on Windows).
function spawnDirect(command, args, options = {}) {
  return spawnSync(command, args, { cwd: root, ...options });
}

function hostTriple() {
  const result = spawnDirect("rustc", ["-vV"], { encoding: "utf8" });
  if (result.error) {
    console.error(`[sidecar] could not run rustc: ${result.error.message}`);
    process.exit(1);
  }
  const match = /host:\s*(\S+)/.exec(result.stdout ?? "");
  if (!match) {
    console.error("[sidecar] could not determine the host target triple from `rustc -vV`");
    process.exit(1);
  }
  return match[1];
}

const triple = hostTriple();
const ext = process.platform === "win32" ? ".exe" : "";

console.log("[sidecar] building the core…");
const build = spawnDirect("cargo", ["build", "--release", "-p", "kandel-core"], {
  stdio: "inherit",
});
if (build.status !== 0) {
  console.error("[sidecar] core build failed");
  process.exit(build.status ?? 1);
}

const source = path.join(root, "target", "release", `kandel${ext}`);
if (!existsSync(source)) {
  console.error(`[sidecar] expected the core at ${source}`);
  process.exit(1);
}

// Tauri requires the sidecar directory to exist, and the file to carry the
// target-triple suffix. `binaries/` is gitignored: it is build output.
const dir = path.join(root, "crates", "kandel-ui", "binaries");
mkdirSync(dir, { recursive: true });

const dest = path.join(dir, `kandel-${triple}${ext}`);
copyFileSync(source, dest);
console.log(`[sidecar] staged ${path.relative(root, dest)} from the release build`);
