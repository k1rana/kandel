//! The `config.toml` model: parsing, defaulting, validation, resolution, and
//! atomic saves.
//!
//! `config.toml` is the single source of truth describing which applications
//! Kandel should supervise. It is intentionally human-friendly and
//! diff-friendly: [`save_config`] omits fields equal to their documented
//! defaults and writes the remaining fields in a fixed order so that saving a
//! just-loaded config is stable.

use std::collections::{BTreeMap, BTreeSet};
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
/// Default `capture_output` when the field is omitted.
///
/// On by default, so a managed process's stderr (and stdout) is visible in the
/// UI without the user having to opt in. Capture is safe for stdin-reading
/// programs because the child's stdin is a live pipe the core keeps open, not
/// `NUL`; a stdio server (MCP/LSP) therefore blocks on input instead of seeing
/// EOF and exiting.
pub const DEFAULT_CAPTURE_OUTPUT: bool = true;

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

/// One environment variable set for a managed process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    /// Variable name. Must not contain whitespace, `=`, or NUL, so it stays
    /// usable from a shell (`%NAME%` / `$NAME`).
    pub key: String,
    /// Variable value. May contain `${NAME}` placeholders; see
    /// [`App::resolved_env`].
    #[serde(default)]
    pub value: String,
}

/// Configuration for a single managed application.
///
/// Unknown keys are rejected rather than ignored: a misspelled field
/// (`restart_delay` for `restart_delay_sec`) would otherwise be silently dropped
/// on the next save, deleting what the user wrote without telling them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Environment variables set for the launched process.
    ///
    /// An ordered list of key/value pairs. Values may reference the process
    /// environment or a variable declared earlier in the list with `${NAME}`;
    /// see [`App::resolved_env`].
    #[serde(default)]
    pub env: Vec<EnvVar>,
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
    /// Whether to redirect the process's stdout/stderr into its UI log.
    ///
    /// Defaults to `true`, so a process's stderr — including the diagnostics a
    /// stdio server prints when something goes wrong (a failed HTTP call, say)
    /// — is visible in the UI without opting in.
    ///
    /// Capture redirects stdout, stderr, **and** stdin. The child's stdin is a
    /// live pipe whose write end the core holds open, so a program that reads
    /// stdin (an MCP/LSP server, an interactive tool) blocks waiting for input
    /// rather than seeing `NUL`/EOF and exiting. `command`, `args`, `env`, and
    /// `cwd` are still launched exactly as configured — only the standard
    /// handles change.
    #[serde(default = "default_capture_output")]
    pub capture_output: bool,
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
fn default_capture_output() -> bool {
    DEFAULT_CAPTURE_OUTPUT
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
            env: Vec::new(),
            stop_command: String::new(),
            stop_args: Vec::new(),
            cwd: String::new(),
            restart_delay_sec: 0,
            healthy_timeout_sec: 0,
            hide_window: false,
            capture_output: DEFAULT_CAPTURE_OUTPUT,
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
        match self.validate_all().into_iter().next() {
            Some(message) => Err(ConfigError::Other(message)),
            None => Ok(()),
        }
    }

    /// Collects every validation problem, so a caller can report them all at once
    /// instead of stopping at the first. Empty means valid.
    pub fn validate_all(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.command.is_empty() {
            errors.push("command is mandatory".into());
        } else if let Err(e) = self.resolved_command() {
            errors.push(format!("invalid command: {e}"));
        }

        for (i, arg) in self.args.iter().enumerate() {
            if let Err(e) = expand::expand_env(arg) {
                errors.push(format!("invalid args[{i}]: {e}"));
            }
        }

        for (i, var) in self.env.iter().enumerate() {
            if var.key.trim().is_empty() {
                errors.push(format!("env[{i}] key is empty"));
            } else if var
                .key
                .chars()
                .any(|c| c.is_whitespace() || c == '=' || c == '\0')
            {
                errors.push(format!(
                    "env[{i}] key must not contain whitespace, '=', or NUL"
                ));
            } else if var.value.contains('\0') {
                errors.push(format!("env[{i}] value must not contain NUL"));
            } else if let Err(e) = expand::validate_template(&var.value, false) {
                errors.push(format!("invalid env[{i}]: {e}"));
            }
        }

        for (i, arg) in self.stop_args.iter().enumerate() {
            if let Err(e) = expand::validate_template(arg, true) {
                errors.push(format!("invalid stop_args[{i}]: {e}"));
            }
        }

        if !self.stop_command.is_empty() {
            if let Err(e) = self.resolved_stop_command() {
                errors.push(format!("invalid stop_command: {e}"));
            }
        }

        if !self.cwd.is_empty() {
            if let Err(e) = expand::expand_env(&self.cwd) {
                errors.push(format!("invalid cwd: {e}"));
            }
        }

        if self.stop_timeout_sec < 0 {
            errors.push("stop_timeout_sec must be >= 0".into());
        }

        errors
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

    /// Resolves `env` into ordered `(name, value)` pairs.
    ///
    /// Top-level `${NAME}` placeholders in a value are expanded against the
    /// process environment and against variables declared earlier in the same
    /// list, so a variable can build on one above it (e.g. a `PATH` that
    /// extends a `PATH` set earlier in the list).
    pub fn resolved_env(&self) -> Result<Vec<(String, String)>, ConfigError> {
        let mut resolved: Vec<(String, String)> = Vec::with_capacity(self.env.len());
        for (i, var) in self.env.iter().enumerate() {
            let value = expand::expand(&var.value, |key| {
                if let Some((_, v)) = resolved.iter().rev().find(|(k, _)| k == key) {
                    return Some(v.clone());
                }
                std::env::var(key).ok()
            })
            .map_err(|e| ConfigError::Other(format!("invalid env[{i}]: {e}")))?;
            resolved.push((var.key.clone(), value));
        }
        Ok(resolved)
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

/// Why an entry was rejected, which decides how it is surfaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidKind {
    /// The entry's TOML is wrong: an unknown key, a wrong type, or a non-table
    /// entry. The entry cannot be read until the writing is fixed.
    Structural,
    /// The TOML reads fine but a value is unusable: an empty or unresolvable
    /// command, a bad placeholder, a negative timeout. The entry is shown and
    /// edited as-is, not repaired.
    Value,
}

/// A configuration entry that could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEntry {
    /// The entry's table name.
    pub name: String,
    /// Every reason the entry was rejected, so the UI can list them all.
    pub messages: Vec<String>,
    /// Best-effort reconstruction; unreadable fields fall back to defaults, so
    /// the entry can still be shown and edited.
    pub draft: App,
    /// Which kind of problem this is.
    pub kind: InvalidKind,
}

/// The result of a lenient load: entries that loaded and entries that did not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadedConfig {
    /// Entries that parsed and validated cleanly.
    pub apps: BTreeMap<String, App>,
    /// Entries that were rejected, keyed by table name.
    pub invalid: BTreeMap<String, InvalidEntry>,
}

/// Reads and parses the config file at `path` leniently.
///
/// Only a file that cannot be read, or that is not valid TOML, is an error: a
/// single bad entry never fails the load. It lands in [`LoadedConfig::invalid`]
/// with a reason and a repair [`draft`](InvalidEntry::draft) while every other
/// entry still loads.
pub fn load_config_lenient(path: &Path) -> Result<LoadedConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_config_lenient(&text)
}

/// Reads and parses the config file at `path`, failing on the first bad entry.
///
/// The strict counterpart to [`load_config_lenient`]; prefer it where a partial
/// config would be misleading (for example a test asserting rejection).
pub fn load_config(path: &Path) -> Result<BTreeMap<String, App>, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_config(&text)
}

/// Parses config TOML from a string leniently. See [`load_config_lenient`].
pub fn parse_config_lenient(text: &str) -> Result<LoadedConfig, ConfigError> {
    let table: toml::Table = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;

    let mut loaded = LoadedConfig::default();
    for (name, value) in table {
        match parse_entry(&name, value) {
            EntryOutcome::Valid(app) => {
                loaded.apps.insert(name, app);
            }
            EntryOutcome::Invalid {
                messages,
                draft,
                kind,
            } => {
                loaded.invalid.insert(
                    name.clone(),
                    InvalidEntry {
                        name,
                        messages,
                        draft,
                        kind,
                    },
                );
            }
        }
    }
    Ok(loaded)
}

/// Parses config TOML from a string, failing on the first bad entry.
///
/// The strict counterpart to [`parse_config_lenient`].
pub fn parse_config(text: &str) -> Result<BTreeMap<String, App>, ConfigError> {
    let loaded = parse_config_lenient(text)?;
    match loaded.invalid.into_iter().next() {
        Some((_, entry)) => Err(ConfigError::Invalid {
            app: entry.name,
            message: entry.messages.join("; "),
        }),
        None => Ok(loaded.apps),
    }
}

/// The outcome of parsing one entry.
enum EntryOutcome {
    /// Parsed and validated.
    Valid(App),
    /// Rejected, with every reason and a best-effort draft.
    Invalid {
        messages: Vec<String>,
        draft: App,
        kind: InvalidKind,
    },
}

/// Parses one table into an [`App`], never failing: a bad entry comes back as
/// [`EntryOutcome::Invalid`] so the caller can keep going.
///
/// A serde failure is a *structural* problem (the writing is wrong); a
/// validation failure is a *value* problem (the writing is fine).
fn parse_entry(name: &str, value: toml::Value) -> EntryOutcome {
    if !value.is_table() {
        return EntryOutcome::Invalid {
            kind: InvalidKind::Structural,
            messages: vec!["entry must be a table".into()],
            draft: App {
                name: name.to_string(),
                ..App::default()
            },
        };
    }

    // Prefer the serde model, so the happy path stays single-sourced.
    let parsed: Result<App, _> = value.clone().try_into();
    match parsed {
        Ok(mut app) => {
            app.name = name.to_string();
            let messages = app.validate_all();
            if messages.is_empty() {
                EntryOutcome::Valid(app)
            } else {
                // Validation never drops fields, so the parsed app is its own draft.
                EntryOutcome::Invalid {
                    kind: InvalidKind::Value,
                    messages,
                    draft: app,
                }
            }
        }
        Err(e) => EntryOutcome::Invalid {
            kind: InvalidKind::Structural,
            messages: vec![e.to_string()],
            draft: draft_from_value(name, &value),
        },
    }
}

/// Rebuilds an entry from its raw TOML, keeping every field that can be read and
/// defaulting the rest. Unknown keys are dropped so a repair writes a clean entry.
fn draft_from_value(name: &str, value: &toml::Value) -> App {
    let mut app = App {
        name: name.to_string(),
        ..App::default()
    };
    let Some(table) = value.as_table() else {
        return app;
    };

    if let Some(v) = table.get("command").and_then(toml::Value::as_str) {
        app.command = v.to_string();
    }
    app.args = string_array(table.get("args"));
    app.env = env_array(table.get("env"));
    if let Some(v) = table.get("cwd").and_then(toml::Value::as_str) {
        app.cwd = v.to_string();
    }
    if let Some(v) = table.get("enabled").and_then(toml::Value::as_bool) {
        app.enabled = v;
    }
    if let Some(v) = table.get("hide_window").and_then(toml::Value::as_bool) {
        app.hide_window = v;
    }
    if let Some(v) = table.get("capture_output").and_then(toml::Value::as_bool) {
        app.capture_output = v;
    }
    if let Some(v) = table.get("stop_command").and_then(toml::Value::as_str) {
        app.stop_command = v.to_string();
    }
    app.stop_args = string_array(table.get("stop_args"));
    if let Some(v) = table
        .get("stop_timeout_sec")
        .and_then(toml::Value::as_integer)
    {
        app.stop_timeout_sec = v;
    }
    if let Some(v) = table
        .get("restart_delay_sec")
        .and_then(toml::Value::as_integer)
    {
        app.restart_delay_sec = v;
    }
    if let Some(v) = table.get("max_retries").and_then(toml::Value::as_integer) {
        app.max_retries = v;
    }
    if let Some(v) = table
        .get("healthy_timeout_sec")
        .and_then(toml::Value::as_integer)
    {
        app.healthy_timeout_sec = v;
    }
    app
}

/// Reads a TOML array of strings, ignoring non-string elements.
fn string_array(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Reads a TOML array of `{ key, value }` tables, ignoring malformed elements.
fn env_array(value: Option<&toml::Value>) -> Vec<EnvVar> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_table)
                .filter_map(|t| {
                    let key = t.get("key").and_then(toml::Value::as_str)?;
                    let value = t
                        .get("value")
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default();
                    Some(EnvVar {
                        key: key.to_string(),
                        value: value.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
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
    env: Option<&'a [EnvVar]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hide_window: Option<bool>,
    // Default is true; persist only the explicit "do not capture" case.
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_output: Option<bool>,
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
            env: (!app.env.is_empty()).then_some(app.env.as_slice()),
            cwd: (!app.cwd.is_empty()).then_some(app.cwd.as_str()),
            // Default is true; persist only the explicit "temporarily off" case.
            enabled: (!app.enabled).then_some(false),
            hide_window: app.hide_window.then_some(true),
            capture_output: (!app.capture_output).then_some(false),
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
///
/// This regenerates the whole document from the model, so any comments or
/// hand-formatting in an existing file are discarded. Use
/// [`save_config_preserving`] to keep them.
pub fn save_config(path: &Path, apps: &BTreeMap<String, App>) -> Result<(), ConfigError> {
    let body = apps_to_toml(apps)?;
    write_atomic(path, &body)
}

/// Writes `apps` to `path`, preserving the existing file's comments and
/// formatting for every part that did not change.
///
/// An entry whose parsed value is identical to the one on disk is left byte for
/// byte, so editing one app never disturbs another's comments, key order, or
/// quoting. A changed entry gets its body rewritten in the canonical form, but
/// its table header comment (and any blank lines above it) is kept. A table no
/// longer in `apps` is deleted only when it parses cleanly; one that fails to
/// parse is left verbatim, so a save can never destroy a broken entry the user
/// can still repair.
///
/// Comments placed *inside* a changed entry's body are the one thing not kept:
/// writing a field means regenerating that entry's scalar fields. Comments above
/// the entry, on other entries, and at the top or bottom of the file all
/// survive. For the exact behavior, see the `save_preserving_*` tests.
///
/// When the existing file is absent or cannot be parsed *as a whole*, there is
/// nothing to preserve and this falls back to a canonical rewrite via
/// [`save_config`].
pub fn save_config_preserving(
    path: &Path,
    apps: &BTreeMap<String, App>,
) -> Result<(), ConfigError> {
    save_config_preserving_removing(path, apps, &BTreeSet::new())
}

/// Like [`save_config_preserving`], but also deletes the tables named in
/// `remove` even when they did not parse cleanly.
///
/// The delete path for the UI: a broken entry the user can see must be removable,
/// so removal is explicit rather than inferred from absence.
pub fn save_config_preserving_removing(
    path: &Path,
    apps: &BTreeMap<String, App>,
    remove: &BTreeSet<String>,
) -> Result<(), ConfigError> {
    let existing_text = std::fs::read_to_string(path).unwrap_or_default();
    let existing = parse_config_lenient(&existing_text).ok();
    let mut doc: toml_edit::DocumentMut = match existing_text.parse() {
        Ok(doc) => doc,
        Err(_) => return save_config(path, apps),
    };

    // Drop app tables no longer configured, but only those that parse cleanly
    // (a genuine removal) or are explicitly named in `remove`. Anything else is
    // left untouched so a broken entry the user still has is never deleted.
    let stale: Vec<String> = doc
        .iter()
        .filter(|(name, item)| item.is_table() && !apps.contains_key(*name))
        .map(|(name, _)| name.to_string())
        .filter(|name| {
            remove.contains(name)
                || existing
                    .as_ref()
                    .is_some_and(|loaded| loaded.apps.contains_key(name))
        })
        .collect();
    for name in stale {
        doc.remove(&name);
    }

    for (name, app) in apps {
        let unchanged = existing
            .as_ref()
            .and_then(|loaded| loaded.apps.get(name))
            .is_some_and(|current| current == app);
        if unchanged && doc.contains_key(name) {
            continue;
        }

        let table = entry_to_table(app)?;
        match doc.get_mut(name) {
            Some(item) if item.is_table() => {
                // Keep the comment and blank lines that precede the header.
                let decor = item.as_table().expect("checked is_table").decor().clone();
                *item = toml_edit::Item::Table(table);
                *item
                    .as_table_mut()
                    .expect("just assigned a table")
                    .decor_mut() = decor;
            }
            _ => {
                doc.insert(name, toml_edit::Item::Table(table));
            }
        }
    }

    write_atomic(path, &doc.to_string())
}

/// Serializes a single entry's body (no table header) as a `toml_edit` table.
fn entry_to_table(app: &App) -> Result<toml_edit::Table, ConfigError> {
    let body = toml::to_string(&AppToml::from(app))
        .map_err(|e| ConfigError::Other(format!("marshaling entry to TOML: {e}")))?;
    let doc: toml_edit::DocumentMut = body
        .parse()
        .map_err(|e| ConfigError::Other(format!("reparsing entry TOML: {e}")))?;
    Ok(doc.into_table())
}

/// Writes `body` to `path` atomically (temp file -> sync -> rename).
fn write_atomic(path: &Path, body: &str) -> Result<(), ConfigError> {
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
                env: vec![EnvVar {
                    key: "PORT".into(),
                    value: "3000".into(),
                }],
                cwd: r"C:\work".into(),
                hide_window: true,
                capture_output: false,
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
            "env",
            "cwd",
            "enabled",
            "hide_window",
            "capture_output",
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
        assert!(loaded["minimal"].capture_output, "capture must default on");
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
                env: vec![EnvVar {
                    key: "PORT".into(),
                    value: "3000".into(),
                }],
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

        // `env` is a TOML array of tables, so it lands after the scalar fields.
        assert!(got.contains("[[app.env]]"), "env table missing:\n{got}");
    }

    #[test]
    fn parse_env_accepts_inline_table_array() {
        // A hand-written config may use the compact inline form; both parse to
        // the same ordered pairs.
        let text = r#"
["app"]
command = "cmd.exe"
env = [
  { key = "NODE_ENV", value = "development" },
  { key = "PORT", value = "3000" },
]
"#;
        let apps = parse_config(text).unwrap();
        assert_eq!(
            apps["app"].env,
            vec![
                EnvVar {
                    key: "NODE_ENV".into(),
                    value: "development".into(),
                },
                EnvVar {
                    key: "PORT".into(),
                    value: "3000".into(),
                },
            ]
        );
    }

    #[test]
    fn capture_output_defaults_to_on_and_can_be_disabled() {
        // Capture is on by default so a process's stderr is visible in the UI.
        // It is safe for stdin-reading programs because the child's stdin is a
        // live pipe (see the core's spawn path), not `NUL`.
        let app: App = parse_config("[\"a\"]\ncommand = \"cmd.exe\"\n")
            .unwrap()
            .remove("a")
            .unwrap();
        assert!(app.capture_output);

        let off: App = parse_config("[\"a\"]\ncommand = \"cmd.exe\"\ncapture_output = false\n")
            .unwrap()
            .remove("a")
            .unwrap();
        assert!(!off.capture_output);
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
            env: vec![EnvVar {
                key: "PORT".into(),
                value: "8080".into(),
            }],
            cwd: r"C:\bin".into(),
            enabled: false,
            hide_window: true,
            restart_delay_sec: 7,
            healthy_timeout_sec: 30,
            max_retries: 2,
            stop_command: "taskkill".into(),
            stop_args: vec!["/PID".into(), "${PID}".into()],
            stop_timeout_sec: 9,
            capture_output: false,
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
    fn save_preserving_keeps_comments_and_untouched_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let a_exe = dir.path().join("a.exe");
        let b_exe = dir.path().join("b.exe");
        std::fs::write(&a_exe, b"").unwrap();
        std::fs::write(&b_exe, b"").unwrap();

        // A hand-written file: a file header, a per-entry comment, a trailing
        // comment, a non-default field, and a deliberately non-canonical order.
        let original = format!(
            "# Kandel configuration\n# keep me\n\n# web service\n[\"web\"]\ncommand = '{}'\n\n# worker\n[\"worker\"]\ncommand = '{}'\nrestart_delay_sec = 7 # wait a bit\n",
            a_exe.display(),
            b_exe.display(),
        );
        std::fs::write(&path, &original).unwrap();

        // Load, edit only `worker`, save preserving.
        let mut apps = load_config(&path).unwrap();
        apps.get_mut("worker").unwrap().max_retries = 3;
        save_config_preserving(&path, &apps).unwrap();

        let got = std::fs::read_to_string(&path).unwrap();

        // All comments outside the edited entry's body survive, including the
        // header comment of the entry that changed.
        assert!(
            got.contains("# Kandel configuration"),
            "file header lost:\n{got}"
        );
        assert!(got.contains("# keep me"), "second header line lost:\n{got}");
        assert!(
            got.contains("# web service"),
            "untouched entry comment lost:\n{got}"
        );
        assert!(
            got.contains("# worker"),
            "edited entry header comment lost:\n{got}"
        );

        // The edit landed and round-trips.
        let reloaded = load_config(&path).unwrap();
        assert_eq!(reloaded["worker"].max_retries, 3);
        assert_eq!(reloaded["worker"].restart_delay_sec, 7);
    }

    #[test]
    fn save_preserving_is_byte_stable_for_a_no_op_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        let original = format!(
            "# header\n[\"app\"]\ncommand = '{}'\nargs = [\"run\", \"dev\"]\nenabled = false\n",
            exe.display()
        );
        std::fs::write(&path, &original).unwrap();

        let apps = load_config(&path).unwrap();
        save_config_preserving(&path, &apps).unwrap();

        // Nothing changed, so the file must be untouched byte for byte (this is
        // what keeps the watcher from seeing a spurious change).
        let got = std::fs::read_to_string(&path).unwrap();
        assert_eq!(got, original);
    }

    #[test]
    fn save_preserving_drops_removed_entries_and_keeps_others() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let a_exe = dir.path().join("a.exe");
        std::fs::write(&a_exe, b"").unwrap();

        let original = format!(
            "# keep\n[\"keep\"]\ncommand = '{}'\n\n# gone\n[\"gone\"]\ncommand = '{}'\n",
            a_exe.display(),
            a_exe.display(),
        );
        std::fs::write(&path, &original).unwrap();

        let mut apps = load_config(&path).unwrap();
        apps.remove("gone");
        save_config_preserving(&path, &apps).unwrap();

        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got.contains("[\"keep\"]"), "kept entry lost:\n{got}");
        assert!(got.contains("# keep"), "kept comment lost:\n{got}");
        assert!(!got.contains("[\"gone\"]"), "removed entry lingers:\n{got}");
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

    #[test]
    fn load_rejects_unknown_field() {
        // A misspelled field must fail loudly: silently ignoring it would let
        // the next save drop the key the user wrote.
        let err =
            parse_config("[\"app\"]\ncommand = \"cmd.exe\"\nrestart_delay = 3\n").unwrap_err();
        match err {
            ConfigError::Invalid { app, message } => {
                assert_eq!(app, "app");
                assert!(
                    message.contains("restart_delay"),
                    "unhelpful message: {message}"
                );
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn save_omits_env_when_empty_and_writes_it_when_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let exe = dir.path().join("app.exe");
        std::fs::write(&exe, b"").unwrap();

        let mut apps = BTreeMap::new();
        apps.insert(
            "plain".to_string(),
            App {
                command: exe.to_string_lossy().into_owned(),
                ..App::default()
            },
        );
        apps.insert(
            "with-env".to_string(),
            App {
                command: exe.to_string_lossy().into_owned(),
                env: vec![EnvVar {
                    key: "PORT".into(),
                    value: "3000".into(),
                }],
                ..App::default()
            },
        );

        save_config(&path, &apps).unwrap();
        let got = std::fs::read_to_string(&path).unwrap();

        let plain = &got[got.find("[plain]").unwrap()..got.find("[with-env]").unwrap()];
        assert!(!plain.contains("env"), "env should be omitted:\n{got}");
        assert!(
            got.contains("key = \"PORT\"") && got.contains("value = \"3000\""),
            "env pair should be written:\n{got}"
        );

        let loaded = load_config(&path).unwrap();
        assert!(loaded["plain"].env.is_empty());
        assert_eq!(loaded["with-env"].env.len(), 1);
        assert_eq!(loaded["with-env"].env[0].key, "PORT");
        assert_eq!(loaded["with-env"].env[0].value, "3000");
    }

    #[test]
    fn env_round_trips_through_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let exe = dir.path().join("svc.exe");
        std::fs::write(&exe, b"").unwrap();
        let original = App {
            name: "svc".into(),
            command: exe.to_string_lossy().into_owned(),
            env: vec![
                EnvVar {
                    key: "PATH".into(),
                    value: "${PATH};C:\\tools".into(),
                },
                EnvVar {
                    key: "NODE_ENV".into(),
                    value: "development".into(),
                },
            ],
            ..App::default()
        };
        let mut apps = BTreeMap::new();
        apps.insert("svc".into(), original.clone());

        save_config(&path, &apps).unwrap();
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded["svc"], original);
    }

    #[test]
    fn env_accepts_keys_with_dots_and_dashes() {
        // The pair form exists so a name like `my.app-flag` survives; a TOML
        // table key would reject it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = App {
            name: "svc".into(),
            command: "cmd.exe".into(),
            env: vec![EnvVar {
                key: "my.app-flag".into(),
                value: "1".into(),
            }],
            ..App::default()
        };
        let mut apps = BTreeMap::new();
        apps.insert("svc".into(), original.clone());

        save_config(&path, &apps).unwrap();
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded["svc"], original);
    }

    #[test]
    fn validate_rejects_empty_env_key() {
        let app = App {
            command: "cmd.exe".into(),
            env: vec![EnvVar {
                key: "  ".into(),
                value: "x".into(),
            }],
            ..App::default()
        };
        assert!(app.validate().is_err());
    }

    #[test]
    fn validate_rejects_env_key_with_space() {
        let app = App {
            command: "cmd.exe".into(),
            env: vec![EnvVar {
                key: "MY VAR".into(),
                value: "x".into(),
            }],
            ..App::default()
        };
        assert!(app.validate().is_err());
    }

    #[test]
    fn validate_rejects_env_key_with_equals() {
        let app = App {
            command: "cmd.exe".into(),
            env: vec![EnvVar {
                key: "A=B".into(),
                value: "x".into(),
            }],
            ..App::default()
        };
        assert!(app.validate().is_err());
    }

    #[test]
    fn validate_rejects_env_value_with_pid_placeholder() {
        let app = App {
            command: "cmd.exe".into(),
            env: vec![EnvVar {
                key: "PORT".into(),
                value: "${PID}".into(),
            }],
            ..App::default()
        };
        assert!(app.validate().is_err());
    }

    #[test]
    fn resolved_env_expands_os_and_prior_names() {
        std::env::set_var("KANDEL_ENV_BASE", "root");
        let app = App {
            command: "cmd.exe".into(),
            env: vec![
                EnvVar {
                    key: "BASE".into(),
                    value: "${KANDEL_ENV_BASE}".into(),
                },
                EnvVar {
                    key: "DERIVED".into(),
                    value: "${BASE}/child".into(),
                },
            ],
            ..App::default()
        };
        let resolved = app.resolved_env().unwrap();
        std::env::remove_var("KANDEL_ENV_BASE");
        assert_eq!(
            resolved,
            [
                ("BASE".to_string(), "root".to_string()),
                ("DERIVED".to_string(), "root/child".to_string()),
            ]
        );
    }

    #[test]
    fn resolved_env_preserves_order() {
        let app = App {
            command: "cmd.exe".into(),
            env: vec![
                EnvVar {
                    key: "B".into(),
                    value: "2".into(),
                },
                EnvVar {
                    key: "A".into(),
                    value: "1".into(),
                },
            ],
            ..App::default()
        };
        let resolved = app.resolved_env().unwrap();
        assert_eq!(resolved[0].0, "B");
        assert_eq!(resolved[1].0, "A");
    }

    #[test]
    fn resolved_env_rejects_undefined_placeholder() {
        let app = App {
            command: "cmd.exe".into(),
            env: vec![EnvVar {
                key: "X".into(),
                value: "${KANDEL_DEFINITELY_UNSET}".into(),
            }],
            ..App::default()
        };
        assert!(app.resolved_env().is_err());
    }

    #[test]
    fn lenient_load_keeps_valid_entries_and_flags_the_bad_one() {
        // The core promise: one broken entry must not take the whole config down.
        let text = "[\"good\"]\ncommand = \"cmd.exe\"\n\n[\"broken\"]\ncommand = \"cmd.exe\"\nrestart_delay = 3\n";
        let loaded = parse_config_lenient(text).unwrap();

        assert!(loaded.apps.contains_key("good"));
        assert!(!loaded.apps.contains_key("broken"));
        let broken = &loaded.invalid["broken"];
        assert!(
            broken.messages.iter().any(|m| m.contains("restart_delay")),
            "{:?}",
            broken.messages
        );
        // A misspelled key is a TOML-writing problem, so repair may rewrite it.
        assert_eq!(broken.kind, InvalidKind::Structural);
    }

    #[test]
    fn lenient_flags_an_unresolvable_command_as_a_value_error() {
        // A command that is not on PATH is a *value* problem: the writing is fine,
        // so it is shown as an entry and edited, not repaired.
        let text = "[\"broken\"]\ncommand = \"kandel-no-such-tool-xyz\"\n";
        let loaded = parse_config_lenient(text).unwrap();
        let broken = &loaded.invalid["broken"];
        assert_eq!(broken.kind, InvalidKind::Value);
        // The draft keeps the command the user wrote.
        assert_eq!(broken.draft.command, "kandel-no-such-tool-xyz");
    }

    #[test]
    fn validate_all_reports_every_problem_not_just_the_first() {
        // The regression: a config with several bad fields used to surface only
        // the first, forcing one fix-and-retry per problem.
        let app = App {
            command: String::new(),
            stop_timeout_sec: -1,
            env: vec![
                EnvVar {
                    key: String::new(),
                    value: "x".into(),
                },
                EnvVar {
                    key: "BAD KEY".into(),
                    value: "y".into(),
                },
            ],
            ..App::default()
        };
        let errors = app.validate_all();
        assert!(errors.len() >= 3, "expected several errors, got {errors:?}");
        assert!(errors.iter().any(|e| e.contains("command is mandatory")));
        assert!(errors.iter().any(|e| e.contains("stop_timeout_sec")));
        assert!(errors.iter().any(|e| e.contains("env[0]")));
    }

    #[test]
    fn a_single_value_problem_is_reported_as_one_message() {
        let text = "[\"broken\"]\ncommand = \"kandel-no-such-tool-xyz\"\n";
        let loaded = parse_config_lenient(text).unwrap();
        assert_eq!(loaded.invalid["broken"].messages.len(), 1);
    }

    #[test]
    fn lenient_flags_an_empty_command_as_a_value_error() {
        let loaded = parse_config_lenient("[\"broken\"]\ncommand = \"\"\n").unwrap();
        assert_eq!(loaded.invalid["broken"].kind, InvalidKind::Value);
    }

    #[test]
    fn lenient_draft_recovers_known_fields() {
        // A draft must carry what the user wrote so the entry can be repaired in
        // the editor rather than lost.
        let text = "[\"broken\"]\ncommand = \"myapp.exe\"\nargs = [\"--x\"]\nrestart_delay = 3\n";
        let loaded = parse_config_lenient(text).unwrap();
        let draft = &loaded.invalid["broken"].draft;
        assert_eq!(draft.name, "broken");
        assert_eq!(draft.command, "myapp.exe");
        assert_eq!(draft.args, ["--x"]);
    }

    #[test]
    fn lenient_load_still_errors_on_broken_toml() {
        // A whole-file syntax error is still fatal; only per-entry problems are
        // tolerated.
        assert!(matches!(
            parse_config_lenient("this is not = = toml"),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn lenient_load_of_valid_config_has_no_invalid() {
        let loaded = parse_config_lenient("[\"a\"]\ncommand = \"cmd.exe\"\n").unwrap();
        assert_eq!(loaded.apps.len(), 1);
        assert!(loaded.invalid.is_empty());
    }

    #[test]
    fn save_preserving_leaves_a_broken_entry_verbatim() {
        // A save that only touches a valid entry must not delete a broken one the
        // user can still repair.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "[\"good\"]\ncommand = \"cmd.exe\"\n\n[\"broken\"]\ncommand = \"cmd.exe\"\nrestart_delay = 3\n";
        std::fs::write(&path, original).unwrap();

        let loaded = parse_config_lenient(original).unwrap();
        save_config_preserving(&path, &loaded.apps).unwrap();

        let got = std::fs::read_to_string(&path).unwrap();
        assert!(
            got.contains("[\"broken\"]"),
            "broken entry was deleted:\n{got}"
        );
        assert!(
            got.contains("restart_delay = 3"),
            "broken entry lost data:\n{got}"
        );
    }

    #[test]
    fn save_preserving_removing_deletes_a_broken_entry_when_named() {
        // The delete path: a broken entry the user can see must be removable.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "[\"good\"]\ncommand = \"cmd.exe\"\n\n[\"broken\"]\ncommand = \"cmd.exe\"\nrestart_delay = 3\n";
        std::fs::write(&path, original).unwrap();

        let loaded = parse_config_lenient(original).unwrap();
        let mut remove = BTreeSet::new();
        remove.insert("broken".to_string());
        save_config_preserving_removing(&path, &loaded.apps, &remove).unwrap();

        let got = std::fs::read_to_string(&path).unwrap();
        assert!(
            !got.contains("[\"broken\"]"),
            "broken entry lingers:\n{got}"
        );
        assert!(got.contains("[\"good\"]"), "good entry was lost:\n{got}");
    }
}
