//! The `config.toml` model: parsing, defaulting, validation, resolution, and
//! atomic saves.
//!
//! `config.toml` is the single source of truth describing which applications
//! Kandel should supervise. It is intentionally human-friendly and
//! diff-friendly: [`save_config`] omits fields equal to their documented
//! defaults and writes the remaining fields in a fixed order so that saving a
//! just-loaded config is stable.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::args;
use crate::expand;

/// Default `stop_timeout_sec` when the field is omitted.
pub const DEFAULT_STOP_TIMEOUT_SEC: i64 = 5;
/// Default `max_retries` when the field is omitted (infinite retries).
pub const DEFAULT_MAX_RETRIES: i64 = -1;
/// Default `enabled` when the field is omitted.
pub const DEFAULT_ENABLED: bool = true;

/// Error produced while loading, validating, or saving a configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The config file could not be read.
    #[error("reading config file {path}: {source}")]
    Read {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The config file was not valid TOML.
    #[error("parsing TOML config: {0}")]
    Parse(String),
    /// A specific app entry failed validation.
    #[error("invalid config for app {app:?}: {message}")]
    Invalid {
        /// The offending app name.
        app: String,
        /// Human-readable reason.
        message: String,
    },
    /// The config or an entry was structurally invalid.
    #[error("{0}")]
    Other(String),
    /// A filesystem error occurred while saving.
    #[error("{context}: {source}")]
    Save {
        /// Description of the failing step.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

/// Configuration for a single managed application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    /// Identifier shown in the UI. Never serialized into the entry body; the
    /// TOML table name is used instead.
    #[serde(skip)]
    pub name: String,
    /// Whether Kandel launches and monitors this app.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Full path to the command or executable.
    pub command: String,
    /// Arguments passed to `command`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional shutdown executable.
    #[serde(default)]
    pub stop_command: String,
    /// Arguments passed to `stop_command`.
    #[serde(default)]
    pub stop_args: Vec<String>,
    /// Working directory when launching the process.
    #[serde(default)]
    pub cwd: String,
    /// Wait time (seconds) before restarting after a termination.
    #[serde(default)]
    pub restart_delay_sec: i64,
    /// Uptime (seconds) after which the restart counter resets.
    #[serde(default)]
    pub healthy_timeout_sec: i64,
    /// Whether to launch the process with a hidden window.
    #[serde(default)]
    pub hide_window: bool,
    /// Retry limit for a crashing app. Negative means infinite.
    #[serde(default = "default_max_retries")]
    pub max_retries: i64,
    /// How long to wait for graceful shutdown before forcing termination.
    #[serde(default = "default_stop_timeout_sec")]
    pub stop_timeout_sec: i64,
}

fn default_enabled() -> bool {
    DEFAULT_ENABLED
}
fn default_max_retries() -> i64 {
    DEFAULT_MAX_RETRIES
}
fn default_stop_timeout_sec() -> i64 {
    DEFAULT_STOP_TIMEOUT_SEC
}

impl Default for App {
    fn default() -> Self {
        App {
            name: String::new(),
            enabled: DEFAULT_ENABLED,
            command: String::new(),
            args: Vec::new(),
            stop_command: String::new(),
            stop_args: Vec::new(),
            cwd: String::new(),
            restart_delay_sec: 0,
            healthy_timeout_sec: 0,
            hide_window: false,
            max_retries: DEFAULT_MAX_RETRIES,
            stop_timeout_sec: DEFAULT_STOP_TIMEOUT_SEC,
        }
    }
}

impl App {
    /// Enforces mandatory fields and validates placeholder syntax.
    ///
    /// The user's original input is preserved (placeholders and relative paths
    /// are not rewritten); expansion and path resolution happen on demand via
    /// the `resolved_*` helpers.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.command.is_empty() {
            return Err(ConfigError::Other("command is mandatory".into()));
        }

        self.resolved_command()
            .map_err(|e| ConfigError::Other(format!("invalid command: {e}")))?;

        for (i, arg) in self.args.iter().enumerate() {
            expand::expand_env(arg)
                .map_err(|e| ConfigError::Other(format!("invalid args[{i}]: {e}")))?;
        }

        for (i, arg) in self.stop_args.iter().enumerate() {
            expand::validate_template(arg, true)
                .map_err(|e| ConfigError::Other(format!("invalid stop_args[{i}]: {e}")))?;
        }

        if !self.stop_command.is_empty() {
            self.resolved_stop_command()
                .map_err(|e| ConfigError::Other(format!("invalid stop_command: {e}")))?;
        }

        if !self.cwd.is_empty() {
            expand::expand_env(&self.cwd)
                .map_err(|e| ConfigError::Other(format!("invalid cwd: {e}")))?;
        }

        if self.stop_timeout_sec < 0 {
            return Err(ConfigError::Other("stop_timeout_sec must be >= 0".into()));
        }

        Ok(())
    }

    /// `command` with `${NAME}` placeholders expanded and the path resolved to
    /// an absolute path via the system `PATH` when necessary.
    pub fn resolved_command(&self) -> Result<PathBuf, ConfigError> {
        let expanded = expand::expand_env(&self.command)
            .map_err(|e| ConfigError::Other(format!("invalid command: {e}")))?;
        resolve_command_path(&expanded).map_err(|e| ConfigError::Other(e.to_string()))
    }

    /// `args` with `${NAME}` placeholders expanded in each element.
    pub fn resolved_args(&self) -> Result<Vec<String>, ConfigError> {
        self.args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                expand::expand_env(arg)
                    .map_err(|e| ConfigError::Other(format!("invalid args[{i}]: {e}")))
            })
            .collect()
    }

    /// `stop_command` with placeholders expanded and resolved via `PATH`.
    /// Returns `None` when `stop_command` is empty.
    pub fn resolved_stop_command(&self) -> Result<Option<PathBuf>, ConfigError> {
        if self.stop_command.is_empty() {
            return Ok(None);
        }
        let expanded = expand::expand_env(&self.stop_command)
            .map_err(|e| ConfigError::Other(format!("invalid stop_command: {e}")))?;
        let resolved = resolve_command_path(&expanded)
            .map_err(|e| ConfigError::Other(format!("invalid stop_command: {e}")))?;
        Ok(Some(resolved))
    }

    /// `cwd` with placeholders expanded. If empty, defaults to the directory
    /// containing the resolved `command`.
    pub fn resolved_cwd(&self) -> Result<PathBuf, ConfigError> {
        if !self.cwd.is_empty() {
            let expanded = expand::expand_env(&self.cwd)
                .map_err(|e| ConfigError::Other(format!("invalid cwd: {e}")))?;
            return Ok(PathBuf::from(expanded));
        }
        let resolved_command = self.resolved_command()?;
        Ok(resolved_command
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default())
    }
}

/// Reads and parses the config file at `path`.
///
/// Returns a map of app name to [`App`]. Every entry is validated; the first
/// invalid entry aborts the load with [`ConfigError::Invalid`].
pub fn load_config(path: &Path) -> Result<BTreeMap<String, App>, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_config(&text)
}

/// Parses config TOML from a string. See [`load_config`].
pub fn parse_config(text: &str) -> Result<BTreeMap<String, App>, ConfigError> {
    let table: toml::Table = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;

    let mut apps = BTreeMap::new();
    for (name, value) in table {
        if !value.is_table() {
            return Err(ConfigError::Invalid {
                app: name.clone(),
                message: "entry must be a table".into(),
            });
        }
        let mut app: App = value.try_into().map_err(|e| ConfigError::Invalid {
            app: name.clone(),
            message: format!("{e}"),
        })?;
        app.name = name.clone();
        app.validate().map_err(|e| ConfigError::Invalid {
            app: name.clone(),
            message: e.to_string(),
        })?;
        apps.insert(name, app);
    }
    Ok(apps)
}

/// The TOML representation of an app entry, with a fixed field order.
///
/// `Option` fields with `skip_serializing_if` omit values equal to their
/// documented defaults so the on-disk config stays minimal. `command` is always
/// written because it is mandatory.
#[derive(Serialize)]
struct AppToml<'a> {
    command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hide_window: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_command: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_args: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_timeout_sec: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    restart_delay_sec: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_retries: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    healthy_timeout_sec: Option<i64>,
}

impl<'a> From<&'a App> for AppToml<'a> {
    fn from(app: &'a App) -> Self {
        AppToml {
            command: &app.command,
            args: (!app.args.is_empty()).then_some(app.args.as_slice()),
            cwd: (!app.cwd.is_empty()).then_some(app.cwd.as_str()),
            // Default is true; persist only the explicit "temporarily off" case.
            enabled: (!app.enabled).then_some(false),
            hide_window: app.hide_window.then_some(true),
            stop_command: (!app.stop_command.is_empty()).then_some(app.stop_command.as_str()),
            stop_args: (!app.stop_args.is_empty()).then_some(app.stop_args.as_slice()),
            stop_timeout_sec: (app.stop_timeout_sec != DEFAULT_STOP_TIMEOUT_SEC)
                .then_some(app.stop_timeout_sec),
            restart_delay_sec: (app.restart_delay_sec != 0).then_some(app.restart_delay_sec),
            max_retries: (app.max_retries != DEFAULT_MAX_RETRIES).then_some(app.max_retries),
            healthy_timeout_sec: (app.healthy_timeout_sec != 0).then_some(app.healthy_timeout_sec),
        }
    }
}

/// Serializes apps to a TOML string. Table headers are sorted alphabetically.
pub fn apps_to_toml(apps: &BTreeMap<String, App>) -> Result<String, ConfigError> {
    let doc: BTreeMap<&str, AppToml> = apps
        .iter()
        .map(|(name, app)| (name.as_str(), AppToml::from(app)))
        .collect();
    toml::to_string(&doc).map_err(|e| ConfigError::Other(format!("marshaling config to TOML: {e}")))
}

/// Writes `apps` to `path` using an atomic write (temp file -> sync -> rename)
/// to prevent partial writes and spurious watcher events.
pub fn save_config(path: &Path, apps: &BTreeMap<String, App>) -> Result<(), ConfigError> {
    let body = apps_to_toml(apps)?;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(dir).map_err(|source| ConfigError::Save {
            context: format!("creating config directory {}", dir.display()),
            source,
        })?;
    }

    let mut tmp = tempfile::Builder::new()
        .prefix(".kandel-config-")
        .suffix(".toml")
        .tempfile_in(dir)
        .map_err(|source| ConfigError::Save {
            context: "creating temp file for atomic write".into(),
            source,
        })?;

    tmp.write_all(body.as_bytes())
        .map_err(|source| ConfigError::Save {
            context: "writing temp config file".into(),
            source,
        })?;
    tmp.as_file_mut()
        .sync_all()
        .map_err(|source| ConfigError::Save {
            context: "syncing temp config file".into(),
            source,
        })?;

    tmp.persist(path).map_err(|e| ConfigError::Save {
        context: format!("renaming temp config file to {}", path.display()),
        source: e.error,
    })?;

    Ok(())
}

/// Resolves `command` to an absolute path, using the process environment's
/// `PATH` and `PATHEXT`.
pub fn resolve_command_path(command: &str) -> Result<PathBuf, ConfigError> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").ok();
    resolve_command_path_in(command, &path_var, pathext.as_deref())
        .ok_or_else(|| ConfigError::Other(format!("command not found in PATH: {command}")))
}

/// Resolves `command` against an explicit `path_var` and `pathext`.
///
/// This is the testable core of [`resolve_command_path`]. `pathext` is a
/// `;`-separated list of extensions (e.g. `.COM;.EXE;.BAT`) applied when the
/// command has no extension of its own, mirroring `cmd.exe` lookup.
///
/// Semantics follow the reference implementation: an **absolute** path is
/// returned verbatim without an existence check (so a config referring to a
/// not-yet-installed tool still validates), while a relative path containing a
/// separator and a bare command name are checked against the filesystem.
pub fn resolve_command_path_in(
    command: &str,
    path_var: &OsStr,
    pathext: Option<&str>,
) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }

    let candidate = PathBuf::from(command);
    if candidate.is_absolute() {
        return Some(candidate);
    }

    // A relative path with a separator is tried directly, relative to the
    // current working directory.
    if command.contains('/') || command.contains('\\') {
        let absolute = absolute_path(&candidate);
        return absolute.is_file().then_some(absolute);
    }

    let extensions = command_extensions(command, pathext);

    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for ext in &extensions {
            let file_name = format!("{command}{ext}");
            let candidate = dir.join(file_name);
            if candidate.is_file() {
                return Some(absolute_path(&candidate));
            }
        }
    }
    None
}

/// Extensions to try for a bare command name: the command's own extension if
/// present, otherwise `pathext` entries first (so `cmd` resolves to `cmd.exe`),
/// falling back to the bare name last.
fn command_extensions(command: &str, pathext: Option<&str>) -> Vec<String> {
    let path = Path::new(command);
    if path.extension().is_some() {
        return vec![String::new()];
    }

    let mut exts = Vec::new();
    if let Some(raw) = pathext {
        for part in raw.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let ext = if part.starts_with('.') {
                part.to_string()
            } else {
                format!(".{part}")
            };
            exts.push(ext);
        }
    }
    // Bare-name fallback, last so PATHEXT matches take precedence.
    exts.push(String::new());
    exts
}

/// Best-effort absolute path without requiring the file to exist.
fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
    }
}

/// Parses a shell-like argument string. Re-exported for convenience.
pub fn parse_args(s: &str) -> Result<Vec<String>, args::ParseArgsError> {
    args::parse_args(s)
}

/// Formats arguments as a shell-like string. Re-exported for convenience.
pub fn format_args(args: &[String]) -> String {
    args::format_args(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn write_temp_config(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn load_applies_stop_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        let config = format!("[\"app\"]\ncommand = '{}'\nenabled = true\n", exe.display());
        let path = write_temp_config(dir.path(), &config);

        let apps = load_config(&path).unwrap();
        let app = &apps["app"];
        assert!(app.enabled);
        assert_eq!(app.stop_command, "");
        assert!(app.stop_args.is_empty());
        assert_eq!(app.stop_timeout_sec, 5);
    }

    #[test]
    fn max_retries_defaults_to_infinite() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        let config = format!("[\"app\"]\ncommand = '{}'\n", exe.display());
        let path = write_temp_config(dir.path(), &config);

        let apps = load_config(&path).unwrap();
        assert_eq!(apps["app"].max_retries, -1);
    }

    #[test]
    fn explicit_max_retries_zero_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        let config = format!(
            "[\"app\"]\ncommand = '{}'\nmax_retries = 0\n",
            exe.display()
        );
        let path = write_temp_config(dir.path(), &config);

        let apps = load_config(&path).unwrap();
        assert_eq!(apps["app"].max_retries, 0);
    }

    #[test]
    fn rejects_negative_stop_timeout() {
        let app = App {
            command: r"C:\Windows\System32\cmd.exe".into(),
            stop_timeout_sec: -1,
            ..App::default()
        };
        assert!(app.validate().is_err());
    }

    #[test]
    fn rejects_empty_command() {
        let app = App::default();
        let err = app.validate().unwrap_err();
        assert!(err.to_string().contains("command is mandatory"));
    }

    #[test]
    fn validate_preserves_stop_command_input() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        // The exact resolution target depends on the host PATH, so this test
        // only asserts the contract: validation succeeds and the user's input
        // is preserved verbatim (save_config would otherwise persist a resolved
        // absolute path).
        let app = App {
            command: exe.to_string_lossy().into_owned(),
            stop_command: "cmd".into(),
            stop_args: vec!["/PID".into(), "${PID}".into()],
            stop_timeout_sec: 5,
            ..App::default()
        };
        app.validate().unwrap();
        assert_eq!(app.stop_command, "cmd");
        assert!(app.resolved_stop_command().unwrap().is_some());
    }

    #[test]
    fn resolve_command_path_uses_pathext() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("myapp.exe");
        std::fs::write(&exe, b"").unwrap();

        let path_var = std::env::join_paths([dir.path()]).unwrap();
        // A bare name resolves to the PATHEXT-qualified file. Windows file
        // systems are case-insensitive, so compare paths case-insensitively.
        let resolved = resolve_command_path_in("myapp", &path_var, Some(".COM;.EXE"));
        assert_eq!(
            resolved.as_deref().map(normalize_case),
            Some(normalize_case(&exe))
        );

        // A name with an extension is matched as-is.
        let resolved = resolve_command_path_in("myapp.exe", &path_var, Some(".COM;.EXE"));
        assert_eq!(
            resolved.as_deref().map(normalize_case),
            Some(normalize_case(&exe))
        );

        // An unknown command resolves to nothing.
        assert!(resolve_command_path_in("nope", &path_var, Some(".COM;.EXE")).is_none());
    }

    #[cfg(windows)]
    fn normalize_case(p: &Path) -> String {
        p.to_string_lossy().to_ascii_lowercase()
    }

    #[cfg(not(windows))]
    fn normalize_case(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn resolve_command_path_returns_absolute_paths_verbatim() {
        // Absolute paths are not stat-checked, so a not-yet-installed tool still
        // validates in a config.
        let missing = if cfg!(windows) {
            PathBuf::from(r"C:\does\not\exist\app.exe")
        } else {
            PathBuf::from("/does/not/exist/app.exe")
        };
        let path_var = OsStr::new("");
        let resolved = resolve_command_path_in(missing.to_str().unwrap(), path_var, Some(".EXE"));
        assert_eq!(resolved.as_deref(), Some(missing.as_path()));
    }

    #[test]
    fn validate_preserves_relative_paths_and_dollar_escape() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        // Use an absolute command to avoid depending on the process PATH, and
        // only placeholder forms whose expansion does not require env mutation.
        let app = App {
            command: exe.to_string_lossy().into_owned(),
            args: vec!["--pid=$$".into(), "--path=${PATH}".into()],
            cwd: dir.path().to_string_lossy().into_owned(),
            stop_timeout_sec: 5,
            ..App::default()
        };
        app.validate().unwrap();

        // The user's original input is preserved verbatim.
        assert_eq!(app.args[0], "--pid=$$");
        assert_eq!(app.args[1], "--path=${PATH}");
        assert_eq!(app.cwd, dir.path().to_string_lossy());

        // Expansion is still available on demand; `$$` becomes a literal `$`.
        let resolved = app.resolved_args().unwrap();
        assert_eq!(resolved[0], "--pid=$");
    }

    #[test]
    fn rejects_pid_in_command() {
        let app = App {
            command: r"C:\bin\app.exe ${PID}".into(),
            ..App::default()
        };
        assert!(app.validate().is_err());
    }

    #[test]
    fn rejects_invalid_stop_args_template() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        let app = App {
            command: exe.to_string_lossy().into_owned(),
            stop_args: vec!["/PID".into(), "${unterminated".into()],
            ..App::default()
        };
        assert!(app.validate().is_err());
    }

    #[test]
    fn save_omits_default_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Use absolute paths in a temp dir so validation succeeds on any host.
        let minimal_exe = dir.path().join("app.exe");
        let explicit_exe = dir.path().join("other.exe");
        std::fs::write(&minimal_exe, b"").unwrap();
        std::fs::write(&explicit_exe, b"").unwrap();

        let mut apps = BTreeMap::new();
        apps.insert(
            "minimal".to_string(),
            App {
                command: minimal_exe.to_string_lossy().into_owned(),
                enabled: true,
                ..App::default()
            },
        );
        apps.insert(
            "explicit".to_string(),
            App {
                command: explicit_exe.to_string_lossy().into_owned(),
                enabled: false,
                args: vec!["--flag".into()],
                cwd: r"C:\work".into(),
                hide_window: true,
                restart_delay_sec: 3,
                healthy_timeout_sec: 60,
                max_retries: 0,
                stop_command: "taskkill".into(),
                stop_args: vec!["/PID".into(), "${PID}".into()],
                stop_timeout_sec: 10,
                ..App::default()
            },
        );

        save_config(&path, &apps).unwrap();
        let got = std::fs::read_to_string(&path).unwrap();

        // Default-valued keys must not appear in the [minimal] block.
        let minimal_start = got.find("[minimal]").expect("minimal table");
        let minimal_end = got[minimal_start..]
            .find("\n[")
            .map(|i| minimal_start + i)
            .unwrap_or(got.len());
        let minimal = &got[minimal_start..minimal_end];
        for key in [
            "args",
            "cwd",
            "enabled",
            "hide_window",
            "restart_delay_sec",
            "healthy_timeout_sec",
            "max_retries",
            "stop_command",
            "stop_args",
            "stop_timeout_sec",
        ] {
            let marker = format!("\n{key} =");
            assert!(
                !minimal.contains(&marker),
                "default key {key:?} was written in [minimal] block:\n{got}"
            );
        }

        // The explicit entry must round-trip through LoadConfig.
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded["minimal"].max_retries, -1);
        assert!(loaded["minimal"].enabled);
        assert_eq!(loaded["minimal"].stop_timeout_sec, 5);
        assert_eq!(loaded["explicit"].max_retries, 0);
        assert!(!loaded["explicit"].enabled);
        assert_eq!(loaded["explicit"].stop_timeout_sec, 10);
        assert_eq!(loaded["explicit"].args, ["--flag"]);
        assert_eq!(loaded["explicit"].cwd, r"C:\work");
        assert!(loaded["explicit"].hide_window);
        assert_eq!(loaded["explicit"].restart_delay_sec, 3);
        assert_eq!(loaded["explicit"].healthy_timeout_sec, 60);
        assert_eq!(loaded["explicit"].stop_command, "taskkill");
        assert_eq!(loaded["explicit"].stop_args, ["/PID", "${PID}"]);
    }

    #[test]
    fn save_writes_fields_in_fixed_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        let mut apps = BTreeMap::new();
        apps.insert(
            "app".to_string(),
            App {
                command: exe.to_string_lossy().into_owned(),
                enabled: false,
                args: vec!["--flag".into()],
                cwd: r"C:\work".into(),
                hide_window: true,
                restart_delay_sec: 3,
                healthy_timeout_sec: 60,
                max_retries: 5,
                stop_command: "taskkill".into(),
                stop_args: vec!["/PID".into(), "${PID}".into()],
                stop_timeout_sec: 10,
                ..App::default()
            },
        );

        save_config(&path, &apps).unwrap();
        let got = std::fs::read_to_string(&path).unwrap();

        let want_order = [
            "command =",
            "args =",
            "cwd =",
            "enabled =",
            "hide_window =",
            "stop_command =",
            "stop_args =",
            "stop_timeout_sec =",
            "restart_delay_sec =",
            "max_retries =",
            "healthy_timeout_sec =",
        ];
        let mut prev = -1isize;
        for key in want_order {
            let idx = got
                .find(key)
                .unwrap_or_else(|| panic!("missing {key:?} in:\n{got}"))
                as isize;
            assert!(idx > prev, "key {key:?} appears out of order:\n{got}");
            prev = idx;
        }
    }

    #[test]
    fn save_then_load_round_trips_explicit_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let exe = dir.path().join("svc.exe");
        std::fs::write(&exe, b"").unwrap();

        let original = App {
            name: "svc".into(),
            command: exe.to_string_lossy().into_owned(),
            args: vec!["--a".into(), "b c".into()],
            cwd: r"C:\bin".into(),
            enabled: false,
            hide_window: true,
            restart_delay_sec: 7,
            healthy_timeout_sec: 30,
            max_retries: 2,
            stop_command: "taskkill".into(),
            stop_args: vec!["/PID".into(), "${PID}".into()],
            stop_timeout_sec: 9,
        };
        let mut apps = BTreeMap::new();
        apps.insert("svc".into(), original.clone());

        save_config(&path, &apps).unwrap();
        let loaded = load_config(&path).unwrap();

        assert_eq!(loaded["svc"], original);
    }

    #[test]
    fn table_headers_are_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        let mut apps = BTreeMap::new();
        for name in ["zebra", "alpha", "mango"] {
            apps.insert(
                name.to_string(),
                App {
                    command: exe.to_string_lossy().into_owned(),
                    ..App::default()
                },
            );
        }
        let out = apps_to_toml(&apps).unwrap();
        let alpha = out.find("[alpha]").unwrap();
        let mango = out.find("[mango]").unwrap();
        let zebra = out.find("[zebra]").unwrap();
        assert!(alpha < mango && mango < zebra, "{out}");
    }

    #[test]
    fn load_rejects_invalid_toml() {
        assert!(matches!(
            parse_config("this is not = = toml"),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn load_rejects_non_table_entry() {
        let err = parse_config("app = 42").unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
    }
}
