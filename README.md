Kandel
======

[![CI](https://github.com/k1rana/kandel/actions/workflows/ci.yml/badge.svg)](https://github.com/k1rana/kandel/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows%2010%2B-0078D6.svg)](#)

Per-process supervision for the Windows interactive session: launch, monitor, and auto-restart the apps that the Service Control Manager cannot reach.

<img width="100%" alt="Banner Image" src="https://github.com/user-attachments/assets/63d5eb63-81c5-4e30-8e43-55d2edeb14cd" />

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
- [Viewing logs](#viewing-logs)
- [Troubleshooting](#troubleshooting)
- [Building from source](#building-from-source)
- [License](#license)

## Installation

Grab `kandel-<version>-windows-<arch>.zip` for your architecture (`x64` or `arm64`) from the Releases page, extract it anywhere (e.g. `%LOCALAPPDATA%\Programs\Kandel\`), and run `kandel.exe`. Both architectures are published for every release, so pick the one that matches your Windows install. To build it yourself, see [Building from source](#building-from-source).

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

Applications are managed through `config.toml`, which lives at `%USERPROFILE%\.config\kandel\config.toml` by default. Kandel creates the file with sample content on first launch. Override the path with the `-f <path>` CLI flag. The file is the single source of truth: both you and the UI write it, and the core reads it.

If you edit the file outside the UI, write it in one atomic step: save to a `.tmp` file, then rename it over the original. The UI already saves this way, and a partial or invalid file is ignored with an error dialog rather than applied.

Comments and formatting survive edits made through the UI. It rewrites only the entry you changed, so comments elsewhere in the file stay; a comment inside that entry's body is the one exception, since its fields are regenerated.

The file takes arguments as a string array (`args = ["-a", "-b"]`). The management UI accepts them as a single shell-like string (e.g. `-a -b "quoted string"`) and parses them for you.

The order of the tables is the order the UI lists them. Reorder entries by dragging a row's grip in the UI, or by moving the `[name]` tables in the file; both write the same order, and comments travel with their entry.

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
| `capture_output`      | Boolean      | `true`                          | If `true`, the process's stdout/stderr are captured into its UI log. The child is given a hidden console so console tools work. Set `false` to leave all standard handles untouched. See [Viewing logs](#viewing-logs). |
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

- **Rows** are one entry each: a drag grip, a state marker, name, command, state word, PID, and restart count. Double-click to edit; right-click to restart, pause, or resume. Hold the grip and drag to move an entry up or down: the file's table order is the list order, so a drop rewrites `config.toml`. Arrow keys move a focused grip for keyboard use.
- **View log** (right-click a row) opens a read-only panel of that app's captured output, with a timestamped divider per launch.
- **Add app** (the lime button) opens the editor. You can also drop an executable or script onto the window to start a new entry with its command filled in.
- **Theme**: the topbar toggle cycles **Auto, Light, Dark**. *Auto* follows your Windows setting and updates when it changes. The choice is remembered between launches.
- **Updates**: Kandel checks for a newer release when the management UI opens. A small ink dot on the wordmark means one is available; open **About** (click the wordmark) to install it. Installing runs the signed installer and restarts Kandel, so every supervised app stops and starts again. The portable ZIP does not update itself, because it was never installed.

The visual system is documented in [`DESIGN.md`](./DESIGN.md).

### Startup behavior

- Only one instance of Kandel may run **per Windows session**, not per machine. The single-instance guard uses a session-local named mutex, so different signed-in users can each run their own Kandel.
- On first launch, if the config file does not exist, Kandel creates it with sample content.
- If the config file is invalid at startup, Kandel shows an error dialog and exits.
- If the config file becomes invalid while Kandel is running, the current monitored state is kept and the change is ignored until the file is fixed.

## Behavior notes

- **Config changes are atomic or ignored.** The core watches the file through a debounced watcher. A partially written file fails to parse, so the previous state stays in effect until a valid version lands.
- **Environment changes are picked up live.** The core is a long-running process, so the environment it starts with would otherwise stay frozen. Before each config load and each launch it re-reads the current user's environment, so a `PATH` directory (or any other variable) added after Kandel started — by installing a tool, for example — resolves without restarting the tray.
- **Job Object cleanup is absolute.** Each child is bound to a job with kill-on-close, so quitting the core, or losing it, takes down every managed process and its descendants.
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
- `$$` produces a literal `$`. Any other `$` is also literal (only `${` starts a placeholder), so a regex anchor like `^(a|b)$` or a literal `$HOME` needs no escaping.

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

## Viewing logs

Right-click a row and choose **View log** to see everything the app has written to
stdout and stderr, including the diagnostics a stdio server prints when
something goes wrong (a failed HTTP call inside an MCP server, for example).

> [!IMPORTANT]
> Capture redirects the child's **stdout and stderr**. Two Windows details keep
> it broadly compatible: the child is given a **hidden console**
> (`CREATE_NO_WINDOW`) so console tools such as `timeout` run, and its **stdin is
> left as-is** (not piped) so a stdin reader is not torn down. The core is a GUI
> process with no console of its own, which is why the child is given one.
>
> Set `capture_output = false` to leave every standard handle exactly as Windows
> would give it.

The panel is read-only and shows the app's history **from the core's start**, not
just the current run:

- Each launch begins with a timestamped **process started** divider, so the
  history reads as a sequence of sessions. A restart appends; it does not clear
  what came before.
- Each line carries its time, an `out`/`err` tag, and its text; `stderr` is shown
  in the failure colour.
- **Follow** keeps the newest line in view. Scrolling up pauses it so you can
  read history; returning to the bottom resumes it.
- The buffer is bounded (the most recent 2000 events per app), so a chatty app
  cannot grow it without limit. It is in-memory only: it is a *view* of this
  session's output, not a durable log. To keep output permanently, have the app
  write its own file.

### What capture does and does not change

Capture is a **launch-handle** concern only. It redirects the child's standard
output into a pipe; `command`, `args`, `env`, and `cwd` are launched exactly as
configured, and the child still inherits the environment.

> [!NOTE]
> Because stdout is a pipe rather than a console, a program that checks for a TTY
> may change its behaviour: colours may be disabled, and output may be
> block-buffered (so lines appear only once a buffer fills or the process exits).
> Set `capture_output = false` if an entry needs its own console.

## Troubleshooting

- **An edit to `config.toml` did nothing.** Either the file was written non-atomically and the core read a partial file, or a key is misspelled (`restart_delay` for `restart_delay_sec`). Unknown keys are rejected rather than ignored, so both cases fail to parse and the core keeps the previous state. Write to a `.tmp` file and rename it into place, and check the key names.
- **The core exits right after launch and shows an error dialog.** `config.toml` failed to parse. Fix the TOML and start Kandel again.
- **A second Kandel will not start.** Only one instance runs per Windows session. Sign out and back in as a different user to run another, or quit the existing one from the tray.
- **The UI opens but shows no live status.** The UI reads status from the core over a named pipe and reconnects on its own. A window opened before Kandel starts attaches as soon as it does, and if the core is restarted the window attaches to the new core. While no core is attached, the state column reads `Unknown`, because nothing has reported on those entries. If every row still reads `Unknown` after a moment, no core is running in that session. Start Kandel from the tray or the Start menu.
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

`pnpm run release` builds both binaries and packages them for the host architecture into `build/kandel-<version>-windows-<arch>.zip`.

The binaries are architecture-specific. `pnpm run build` builds for the host, and `pnpm run build:arm64` cross-builds arm64, which needs `rustup target add aarch64-pc-windows-msvc`. Releases ship both `windows-x64` and `windows-arm64`. Passing the matching `--target` to `scripts/package.mjs` names the archive and installer for each.

For the dev setup and the test commands, see [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

Apache-2.0. See [LICENSE](./LICENSE) and [NOTICE](./NOTICE) for details.
