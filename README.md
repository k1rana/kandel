Kandel
======

[![CI](https://github.com/k1rana/kandel/actions/workflows/ci.yml/badge.svg)](https://github.com/k1rana/kandel/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows%2010%2B-0078D6.svg)](#)

Per-process supervision for the Windows interactive session: launch, monitor, and auto-restart the apps that the Service Control Manager cannot reach.

## Why use it

- **Per-process identity, not per-executable.** Each monitored app runs as a child bound to a Windows Job Object, so two `node.exe` instances for two different projects are tracked separately. Closing the job reaps the whole process tree, so nothing is orphaned.
- **Live config reload.** Edits to `config.toml` apply instantly, whether they come from the built-in UI or an external editor.
- **Minimal footprint.** The resident core is pure Rust with a small dependency surface, and it is event-driven, so idle CPU stays near zero.

On Linux, `systemd` supervises processes out of the box. On Windows the equivalent, the Service Control Manager, only covers Windows services, which run in an isolated session and cannot interact with the desktop. Apps that need to live in your interactive session have few supervision options, and the ones that exist watch by executable name. That breaks the moment two apps share an executable: a name-based watcher cannot tell two `node.exe` processes apart. Kandel watches the process instead.

## Contents

- [Installation](#installation)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [Usage](#usage)
- [Behavior notes](#behavior-notes)
- [Advanced usage](#advanced-usage)
- [Troubleshooting](#troubleshooting)
- [Building from source](#building-from-source)
- [Development](#development)
- [Tests](#tests)
- [License](#license)

## Installation

Grab the latest `kandel-<version>-windows-amd64.zip` from the Releases page, extract it anywhere (e.g. `%LOCALAPPDATA%\Programs\Kandel\`), and run `kandel.exe`. To build it yourself, see [Building from source](#building-from-source).

Kandel is two binaries, and both have to sit in the same folder:

| Binary          | Role                                                                             |
| --------------- | -------------------------------------------------------------------------------- |
| `kandel.exe`    | Resident core: tray icon, config watcher, and process supervisor.                |
| `kandel-ui.exe` | Disposable management UI (Tauri + Svelte). Launched on demand, exits when closed. |

The core locates the UI beside itself, so a folder containing only one of the two is non-functional.

## Quick start

1. Run `kandel.exe`. An icon appears in the system tray.
2. Right-click the tray icon and select **Open Settings** to launch the UI.
3. Add the apps you want monitored and save. Kandel starts watching them immediately.

## Configuration

Applications are managed through `config.toml`, which lives at `%USERPROFILE%\.config\kandel\config.toml` by default. Kandel creates the file with sample content on first launch. Override the path with the `-f <path>` CLI flag.

`config.toml` is the single source of truth. Both the user and the UI write it; the core reads it and never writes it after first-run bootstrap. The two sides exchange only:

1. **Status updates (core to UI).** The core launches the UI as a child and writes a line-delimited JSON stream of `MonitorStatus` objects to its stdin.
2. **Config changes (UI to core).** The UI writes `config.toml` atomically; the core picks the change up through a debounced file watcher.

> [!WARNING]
> Kandel watches this file for real-time changes, so **atomic writes are required** when modifying it with external tools. The built-in UI handles this automatically. From a script, write to a `.tmp` file and perform an atomic rename.

> [!TIP]
> The TOML file takes arguments as a string array (`args = ["-a", "-b"]`). The management UI accepts them as a single shell-like string (e.g. `-a -b "quoted string"`) and parses them for you.

### Example

```toml
# Kandel Configuration

["PowerToys Awake"]
command = 'C:\Program Files\PowerToys\modules\Awake\PowerToys.Awake.exe'
args = ["--use-pt-config"]
cwd = 'C:\Program Files\PowerToys\modules\Awake'
hide_window = true
restart_delay_sec = 3
max_retries = 5
healthy_timeout_sec = 60

["My Svelte Dev Server"]
command = 'npm'
args = ["run", "dev"]
cwd = 'C:\Users\user\projects\my-svelte-app'
enabled = false
```

### Field reference

Each top-level TOML table (`[name]`) defines one monitored app. `name` is the identifier shown in the UI.

| Field                 | Type         | Default                         | Description                                                                       |
| --------------------- | ------------ | ------------------------------- | --------------------------------------------------------------------------------- |
| `command`             | String       | —                               | **Required.** Path to the command. Relative paths and bare names resolve via `PATH`. |
| `args`                | String array | `[]`                            | Arguments passed to `command`.                                                    |
| `env`                 | Table array  | `[]`                            | Environment variables added for this app. Each entry is a table with a `key` and a `value`. See [Environment variables](#environment-variables). |
| `cwd`                 | String       | Directory of resolved `command` | Working directory when launching the process.                                     |
| `enabled`             | Boolean      | `true`                          | Whether Kandel launches and monitors this app.                                    |
| `hide_window`         | Boolean      | `false`                         | If `true`, launches the process with a hidden window.                             |
| `restart_delay_sec`   | Integer      | `0`                             | Wait time (seconds) before attempting a restart after detecting termination.      |
| `max_retries`         | Integer      | `-1` (infinite)                 | Retry limit for a crashing app. `0` disables retry; negative means infinite.      |
| `healthy_timeout_sec` | Integer      | `0`                             | If the process runs this long before exiting, the restart counter resets. `0` disables the reset. |
| `stop_command`        | String       | `""`                            | Optional shutdown executable. Resolved via `PATH` if relative.                    |
| `stop_args`           | String array | `[]`                            | Arguments for `stop_command`. Supports `${PID}` / `${NAME}`.                      |
| `stop_timeout_sec`    | Integer      | `5`                             | How long to wait for graceful shutdown before falling back to `TerminateProcess`. |

## Usage

### Tray icon

- **Double-click** the tray icon to open the management UI, or right-click and choose **Open Settings**.
- **Right-click** for the menu:
  - **Kandel v\<version\>**: read-only header showing the running build version.
  - **Open Settings**: launches the management UI. If the UI is already open, Kandel does not launch a second copy.
  - **Open config with...**: opens `config.toml` in an editor of your choice.
  - **Auto-start Kandel**: toggles Windows sign-in auto-start for the current user by updating `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
  - **Quit**: stops all monitored processes and exits Kandel.

### Management UI

- **Rows** are one entry each: a state marker, name, command, state word, PID, and restart count. Double-click to edit; right-click to restart, pause, or resume.
- **Add app** (the lime button) opens the editor. You can also drop an executable or script onto the window to start a new entry with its command filled in.
- **Theme**: the topbar toggle cycles **Auto, Light, Dark**. *Auto* follows your Windows setting and updates when it changes. The choice is remembered between launches.

The visual system is documented in [`DESIGN.md`](./DESIGN.md).

### Startup behavior

- Only one instance of Kandel may run **per Windows session**, not per machine. The single-instance guard uses a session-local named mutex, so different signed-in users can each run their own Kandel.
- On first launch, if the config file does not exist, Kandel creates it with sample content.
- If the config file is invalid at startup, Kandel shows an error dialog and exits.
- If the config file becomes invalid while Kandel is running, the current monitored state is kept and the change is ignored until the file is fixed.

## Behavior notes

- **Config changes are atomic or ignored.** The core watches the file through a debounced watcher. A partially written file fails to parse, so the previous state stays in effect until a valid version lands.
- **Job Object cleanup is absolute.** Each child is bound to a job with kill-on-close, so quitting the core, or losing it, takes down every managed process and its descendants.
- **Restart counters respect `healthy_timeout_sec`.** A process that stays up past the timeout resets its retry counter, so a long-running app that crashes once is not treated like a crash loop.
- **`max_retries` distinguishes the three cases.** `-1` retries forever, `0` never retries, and a positive number caps the attempts.
- **The UI holds no process state.** Every change the UI makes goes through `config.toml`; the core is the only writer of process lifecycle.

## Advanced usage

### CLI options

| Flag                       | Description                                                                        |
| -------------------------- | ---------------------------------------------------------------------------------- |
| `-f <path>`                | Custom path to `config.toml`. Default: `%USERPROFILE%\.config\kandel\config.toml`. |
| `-log-file <path>`         | Write logs to the given file in append mode. Default: `stderr`.                    |
| `-log-format <text\|json>` | Log output format. Default: `text`.                                                |
| `-ui`                      | Open the management UI at startup instead of waiting for a tray click.             |

### Placeholders and environment variables

`command`, `args`, `cwd`, `stop_command`, `stop_args`, and every `env` value support placeholder expansion:

- `${NAME}` expands to the value of the environment variable `NAME`. An undefined `NAME` is an error.
- `${PID}` expands to the monitored process PID. **Only valid inside `stop_args`.**
- `$$` produces a literal `$`.

On Windows, prefer TOML literal strings (single-quoted) so backslashes don't need escaping:

```toml
["My App"]
command = '${USERPROFILE}\bin\myapp.exe'
args = ['--config', '${APPDATA}\myapp\config.json']
stop_command = 'taskkill'
stop_args = ['/PID', '${PID}', '/T']
```

Expansion is single-pass, so the content of an environment variable is always treated literally.

### Environment variables

The `env` field adds environment variables to a monitored process. Each entry is a
key/value table, so the name may contain characters (such as `.` or `-`) that a
bare TOML key cannot:

```toml
["My Dev Server"]
command = 'npm'
args = ['run', 'dev']
env = [
  { key = "NODE_ENV", value = "development" },
  { key = "PORT", value = "3000" },
  { key = "PATH", value = '${PATH};C:\tools\bin' },
]
```

- The variables are **added to** the process's inherited environment; they do not
  replace it, so `PATH`, `SystemRoot`, and the rest remain available.
- A name must not contain whitespace, `=` or NUL, so it stays usable from a shell
  (`%NAME%` / `$NAME`). Names like `my.app-flag` are fine. In the editor, a space
  typed into a name is folded to `_` as you type; a stray `=` is rejected on save.
- A name that already exists in the environment is overridden (case-insensitively,
  since Windows variable names are not case-sensitive).
- An `env` value may reference the process environment or a variable declared
  earlier in the same list, so one variable can build on another.
- Changing `env` restarts the process, the same as changing `command` or `args`.

The management UI edits the same variables as key/value rows in the editor.

### Stop behavior

If `stop_command` is set, Kandel runs it first and waits up to `stop_timeout_sec` for the monitored process to exit. Otherwise it picks a best-effort graceful stop:

1. If the target PID owns a non-console top-level window, post `WM_CLOSE`.
2. Otherwise, send `CTRL_BREAK_EVENT` to the child's process group.
3. If the process is still alive after `stop_timeout_sec`, call `TerminateProcess`.

## Troubleshooting

- **The core exits right after launch and shows an error dialog.** `config.toml` failed to parse. Fix the TOML and start Kandel again.
- **An edit to `config.toml` did nothing.** The file was probably written non-atomically and failed to parse, so the core kept the previous state. Write to a `.tmp` file and rename it into place.
- **A second Kandel will not start.** Only one instance runs per Windows session. Sign out and back in as a different user to run another, or quit the existing one from the tray.
- **The UI opens but shows no live status.** The UI only receives status when the core launches it with the `--ipc` flag. A UI opened outside the core (for example under `tauri dev`) has no status pipe and runs without live state.
- **Auto-start did not take effect.** The toggle writes to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` for the current user. Check that key if the entry is missing.

## Building from source

### Prerequisites

> **Supported OS:** Windows 10 version 1809 (October 2018 Update) or later, and Windows 11.

- Rust (stable, MSVC toolchain) from <https://rustup.rs>
- Visual Studio Build Tools with the "Desktop development with C++" workload
- Node.js 22+ (LTS recommended) and pnpm
- WebView2 Runtime (pre-installed on Windows 11; required for the UI)

### Build

From the repository root:

```bash
pnpm install
pnpm run build
```

This produces:

- `build/kandel.exe` for the background supervisor
- `build/kandel-ui.exe` for the management UI

`pnpm run release` builds both and packages them into `build/kandel-<version>-windows-amd64.zip`.

## Development

```bash
pnpm install   # first time only: Tauri CLI and frontend dependencies
pnpm dev       # build core + UI, start Vite, then launch the core with -ui
```

`pnpm dev` runs the whole stack. It builds both binaries, waits for the Vite dev server, then starts the core with the `-ui` flag, which owns the tray and opens the UI window. That window gets hot reload from Vite, and it also gets live status, because the core is the process that spawned it. Ctrl+C stops everything. Set `KANDEL_CONFIG` to point the core at a scratch config.

> [!NOTE]
> `tauri dev` alone is not enough here. It opens its own window with no status pipe, so the UI renders without live state. `pnpm dev` exists to avoid that.

## Tests

```bash
# Rust: portable config logic, reconciliation, Windows supervision
cargo test --workspace

# Frontend: pure helper functions
pnpm run test:ui

# Everything, plus lints
pnpm test
pnpm run check
```

The end-to-end smoke test starts the core against a scratch config, checks that it spawns the managed child, then confirms that exiting the core reaps the process tree through the Job Object:

```bash
bash scripts/smoke_core.sh
```

Integration tests in the `kandel-testkit` crate cover both graceful stop paths
by spawning real processes:

```bash
cargo test -p kandel-testkit
```

`stop_gui` drives the `WM_CLOSE` path and `stop_console` the `CTRL_BREAK_EVENT`
path. Each asserts the target exited with code 0, which a force-terminated
process never records (that path always yields 1), so the tests fail if a
graceful stop silently regresses to a kill. They need an interactive Windows
session, which the CI runner provides.

## License

Apache-2.0. See [LICENSE](./LICENSE) and [NOTICE](./NOTICE) for details.
