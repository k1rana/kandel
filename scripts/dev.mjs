/**
 * One command to run the whole Kandel dev stack: `pnpm dev`.
 *
 * Why a script instead of a Tauri built-in:
 *
 * Tauri's only dev hook is `beforeDevCommand`, which runs the *frontend* dev
 * server. It cannot also manage a second native process, and Kandel genuinely
 * needs one: the UI reads live status from the core's named pipe (the core
 * publishes it and any UI window attaches), which in dev has to come from the
 * core the script starts — so the tray and the status stream both belong to
 * that process, not to `tauri dev`'s own window.
 *
 * So dev needs two long-running processes:
 *   1. Vite, serving the frontend with hot reload (the core's UI loads it via
 *      `devUrl`, so that window gets hot reload too), and
 *   2. the core with `-ui`, which owns the tray and opens that window.
 *
 * Starting them has two wrinkles a plain `cargo run &` does not handle, both
 * observed in practice: the core must not spawn its window before Vite is
 * answering (or the window renders "connection refused"), and the core is a
 * tray app that will not exit on its own, so Ctrl+C has to take it down.
 */
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const win = process.platform === "win32";
const PORT = 1420;
const URL = `http://localhost:${PORT}/`;
const exe = (name) => path.join(root, "target", "debug", win ? `${name}.exe` : name);

// Dev runs against a repo-local scratch config instead of the real
// %USERPROFILE%\.config\kandel\config.toml, so a contributor's own entries
// cannot break the dev loop (and dev edits cannot reach their real config).
// The file is created on first run and gitignored.
const DEV_CONFIG_DIR = path.join(root, ".dev");
const DEV_CONFIG = path.join(DEV_CONFIG_DIR, "config.toml");

/** Written only when `.dev/config.toml` is absent; edit it freely afterwards. */
const DEV_CONFIG_SEED = `# Local dev config created by scripts/dev.mjs.
# Gitignored. Your real config is %USERPROFILE%\\.config\\kandel\\config.toml, untouched.

["Welcome to Kandel"]
command = "cmd.exe"
args = ["/c", "echo Welcome to Kandel (dev)! & timeout /t 20"]
restart_delay_sec = 1
healthy_timeout_sec = 30
max_retries = 5
`;

/** Creates the repo-local dev config the first time dev runs. */
function ensureDevConfig() {
  if (existsSync(DEV_CONFIG)) return;
  mkdirSync(DEV_CONFIG_DIR, { recursive: true });
  writeFileSync(DEV_CONFIG, DEV_CONFIG_SEED);
  console.log(`[dev] created ${path.relative(root, DEV_CONFIG)}`);
}

/** Runs a command to completion, inheriting stdio. */
const run = (cmd, args) =>
  new Promise((resolve) => {
    const child = spawn(cmd, args, { cwd: root, stdio: "inherit", shell: win });
    child.on("exit", (code) => resolve(code ?? 0));
    child.on("error", () => resolve(1));
  });

/** Is the dev server already listening? */
const inUse = (port) =>
  new Promise((resolve) => {
    const socket = net.connect({ port, host: "127.0.0.1" });
    socket.once("connect", () => (socket.destroy(), resolve(true)));
    socket.once("error", () => resolve(false));
  });

/** Waits for the dev server to answer. */
async function waitForServer(timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      await fetch(URL, { method: "HEAD" });
      return true;
    } catch {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
  return false;
}

const children = [];
let stopping = false;

/** Stops every child, including the UI the core spawned. */
function stop() {
  if (stopping) return;
  stopping = true;
  for (const child of children) {
    if (child.exitCode !== null || child.pid === undefined) continue;
    if (win) spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    else {
      try {
        process.kill(-child.pid, "SIGTERM");
      } catch {
        /* already gone */
      }
    }
  }
}

for (const sig of ["SIGINT", "SIGTERM"]) process.on(sig, () => (stop(), process.exit(0)));

async function main() {
  // Build both binaries first: the core spawns the UI from beside itself, so
  // that binary has to exist before the core starts.
  console.log("[dev] building…");
  if ((await run("cargo", ["build", "-p", "kandel-core", "-p", "kandel-ui"])) !== 0) {
    console.error("[dev] build failed");
    return 1;
  }

  // 1. Frontend dev server.
  if (await inUse(PORT)) {
    console.log(`[dev] reusing the dev server already on :${PORT}`);
  } else {
    console.log("[dev] starting the frontend dev server…");
    const vite = spawn("pnpm", ["--dir", "crates/kandel-ui/frontend", "dev"], {
      cwd: root,
      stdio: "inherit",
      shell: win,
      detached: !win,
    });
    children.push(vite);
    if (!(await waitForServer())) {
      console.error(`[dev] dev server never came up at ${URL}`);
      return 1;
    }
  }

  // 2. Core, which opens the UI window itself (so it gets real statuses).
  if (!existsSync(exe("kandel")) || !existsSync(exe("kandel-ui"))) {
    console.error("[dev] expected binaries are missing after the build");
    return 1;
  }
  // Repo-local scratch config by default; KANDEL_CONFIG still overrides it.
  const configPath = process.env.KANDEL_CONFIG
    ? path.resolve(root, process.env.KANDEL_CONFIG)
    : DEV_CONFIG;
  if (configPath === DEV_CONFIG) ensureDevConfig();

  console.log(`[dev] config: ${path.relative(root, configPath)}`);
  console.log("[dev] starting the core + UI window (Ctrl+C stops everything)…");
  const core = spawn(exe("kandel"), ["-ui", "-f", configPath], {
    cwd: root,
    stdio: "inherit",
  });
  children.push(core);

  const code = await new Promise((resolve) => core.on("exit", (c) => resolve(c ?? 0)));
  if (!stopping) console.log(`[dev] core exited (${code})`);
  stop();
  return 0;
}

main().then((code) => process.exit(code));
