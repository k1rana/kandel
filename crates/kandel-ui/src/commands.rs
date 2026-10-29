//! Tauri commands exposed to the frontend.
//!
//! These provide CRUD access to `config.toml`. The UI writes the file directly;
//! the core detects the change via its file watcher and reconciles. The UI never
//! sends commands to the core.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use kandel_config::{
    format_args, parse_args, save_config_preserving, save_config_preserving_removing, App,
    ConfigError, EnvVar, InvalidEntry, InvalidKind, LoadedConfig, DEFAULT_CONFIG,
};

/// Shared UI state: the resolved config path.
pub struct UiState {
    config_path: Mutex<PathBuf>,
}

impl UiState {
    /// Creates state bound to `config_path`.
    pub fn new(config_path: PathBuf) -> Self {
        UiState {
            config_path: Mutex::new(config_path),
        }
    }

    /// Returns a clone of the configured path.
    pub fn config_path(&self) -> PathBuf {
        self.config_path.lock().unwrap().clone()
    }
}

/// The DTO exposed to the frontend. Mirrors [`App`] but represents `args` and
/// `stop_args` as a single shell-like string for editing in one text box.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// App name (config table key).
    pub name: String,
    /// Whether the entry is enabled.
    pub enabled: bool,
    /// The command to launch.
    pub command: String,
    /// Shell-formatted arguments string.
    pub args: String,
    /// Environment variables as ordered key/value pairs.
    pub env: Vec<EnvVar>,
    /// Optional stop command.
    pub stop_command: String,
    /// Shell-formatted stop arguments string.
    pub stop_args: String,
    /// Working directory.
    pub cwd: String,
    /// Restart delay in seconds.
    pub restart_delay_sec: i64,
    /// Healthy timeout in seconds.
    pub healthy_timeout_sec: i64,
    /// Whether to hide the launched window.
    pub hide_window: bool,
    /// Max retries (-1 means infinite).
    pub max_retries: i64,
    /// Graceful stop timeout in seconds.
    pub stop_timeout_sec: i64,
    /// Whether the process's stdout/stderr are captured into its log.
    pub capture_output: bool,
    /// Why the entry failed to load, or empty when it loaded cleanly.
    ///
    /// A non-empty list means the entry is on disk but invalid, and the other
    /// fields hold a best-effort draft so it can be shown and edited.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    /// Which kind of problem `errors` describes.
    ///
    /// `structural` is a TOML-writing problem the repair flow can rewrite;
    /// `value` is a bad value shown as an entry in an error state and edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<AppErrorKind>,
}

/// The kind of problem an entry's `error` describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppErrorKind {
    /// A TOML-writing problem: unknown key, wrong type, non-table entry.
    Structural,
    /// A bad value: an empty or unresolvable command, a bad placeholder.
    Value,
}

impl AppConfig {
    /// Converts an [`App`] into a DTO, formatting argv as shell strings.
    pub fn from_app(app: &App) -> Self {
        AppConfig {
            name: app.name.clone(),
            enabled: app.enabled,
            command: app.command.clone(),
            args: format_args(&app.args),
            env: app.env.clone(),
            stop_command: app.stop_command.clone(),
            stop_args: format_args(&app.stop_args),
            cwd: app.cwd.clone(),
            restart_delay_sec: app.restart_delay_sec,
            healthy_timeout_sec: app.healthy_timeout_sec,
            hide_window: app.hide_window,
            max_retries: app.max_retries,
            stop_timeout_sec: app.stop_timeout_sec,
            capture_output: app.capture_output,
            errors: Vec::new(),
            error_kind: None,
        }
    }

    /// Converts a rejected entry into a DTO carrying its draft and the reasons.
    pub fn from_invalid(entry: InvalidEntry) -> Self {
        let mut dto = Self::from_app(&entry.draft);
        dto.name = entry.name;
        dto.errors = entry.messages;
        dto.error_kind = Some(match entry.kind {
            InvalidKind::Structural => AppErrorKind::Structural,
            InvalidKind::Value => AppErrorKind::Value,
        });
        dto
    }

    /// Converts the DTO into an [`App`], parsing the shell-formatted args.
    pub fn to_app(&self) -> Result<App, ConfigError> {
        let args =
            parse_args(&self.args).map_err(|e| ConfigError::Other(format!("parsing args: {e}")))?;
        let stop_args = parse_args(&self.stop_args)
            .map_err(|e| ConfigError::Other(format!("parsing stop args: {e}")))?;
        Ok(App {
            name: self.name.clone(),
            enabled: self.enabled,
            command: self.command.clone(),
            args,
            env: self.env.clone(),
            stop_command: self.stop_command.clone(),
            stop_args,
            cwd: self.cwd.clone(),
            restart_delay_sec: self.restart_delay_sec,
            healthy_timeout_sec: self.healthy_timeout_sec,
            hide_window: self.hide_window,
            capture_output: self.capture_output,
            max_retries: self.max_retries,
            stop_timeout_sec: self.stop_timeout_sec,
        })
    }
}

/// Loads the config leniently, bootstrapping the sample config if the file is
/// missing. Only a whole-file failure is an error; a bad entry never fails the
/// load.
fn load_lenient(path: &Path) -> Result<LoadedConfig, ConfigError> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, DEFAULT_CONFIG);
    }
    kandel_config::load_config_lenient(path)
}

/// Information shown in the About dialog.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    /// Application name.
    pub name: String,
    /// Build version, from the Tauri config.
    pub version: String,
    /// Absolute path to the `config.toml` this UI is editing.
    pub config_path: String,
    /// Repository URL.
    pub repository: String,
}

/// Returns the metadata shown in the About dialog.
#[tauri::command]
pub fn get_about_info(app: AppHandle, state: State<'_, UiState>) -> AboutInfo {
    let package = app.package_info();
    AboutInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        config_path: state.config_path().to_string_lossy().into_owned(),
        repository: env!("CARGO_PKG_REPOSITORY").to_string(),
    }
}

/// Opens the project repository in the user's default browser.
///
/// Takes no URL argument on purpose: the only link the About dialog offers is
/// the project's own repository, so accepting a URL from the frontend would add
/// a way to launch arbitrary handlers for no benefit.
#[tauri::command]
pub fn open_repository() -> Result<(), String> {
    open_external(env!("CARGO_PKG_REPOSITORY"))
        .map_err(|e| format!("could not open the browser: {e}"))
}

/// Hands `url` to the OS default handler without opening a console window.
#[cfg(windows)]
fn open_external(url: &str) -> std::io::Result<()> {
    // `FileProtocolHandler` routes a URL to whatever is registered for its
    // scheme, which is the shell's own behaviour, with no console flash.
    Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| ())
}

/// Hand-off for non-Windows hosts, which only affects development builds.
#[cfg(not(windows))]
fn open_external(url: &str) -> std::io::Result<()> {
    Command::new("xdg-open").arg(url).spawn().map(|_| ())
}

/// Reads `config.toml` and returns every entry as an [`AppConfig`].
///
/// Invalid entries are included with `errors` set, so the UI can list and repair
/// them rather than losing an entry it cannot see.
#[tauri::command]
pub fn get_full_config(state: State<'_, UiState>) -> Result<BTreeMap<String, AppConfig>, String> {
    let path = state.config_path();
    let loaded = load_lenient(&path).map_err(|e| format!("loading config: {e}"))?;
    let LoadedConfig { apps, invalid } = loaded;
    let mut out: BTreeMap<String, AppConfig> = apps
        .into_iter()
        .map(|(name, app)| (name, AppConfig::from_app(&app)))
        .collect();
    for (name, entry) in invalid {
        out.insert(name, AppConfig::from_invalid(entry));
    }
    Ok(out)
}

/// Saves `cfg` to `config.toml`. When `old_name != cfg.name` the entry is
/// renamed (the old key is removed).
///
/// Saving never deletes an entry that failed to parse, so it cannot destroy a
/// broken entry the user can still repair.
#[tauri::command]
pub fn update_app_config(
    state: State<'_, UiState>,
    old_name: String,
    cfg: AppConfig,
) -> Result<(), String> {
    let path = state.config_path();
    let loaded = load_lenient(&path).map_err(|e| format!("loading current config: {e}"))?;
    let LoadedConfig { mut apps, .. } = loaded;

    // A rename replaces the old key, even a broken one: the user is replacing it.
    let renaming = old_name != cfg.name;
    if renaming {
        apps.remove(&old_name);
    }

    // No validation here: a value the loader rejects (an unresolvable command,
    // say) shows up as that one entry's issue after the reload. `to_app` already
    // copies the name, so it is not set again.
    let app = cfg.to_app().map_err(|e| format!("{e}"))?;
    apps.insert(cfg.name, app);

    // A rename removes the old key explicitly: it may replace a broken entry,
    // which the preserving save would otherwise keep.
    let remove = if renaming {
        BTreeSet::from([old_name])
    } else {
        BTreeSet::new()
    };

    save_config_preserving_removing(&path, &apps, &remove)
        .map_err(|e| format!("saving config: {e}"))
}

/// Removes the entry named `name` from `config.toml`.
///
/// The name is passed to the removing save explicitly: a broken entry the user
/// can see must be removable even though it never loaded.
#[tauri::command]
pub fn delete_app_config(state: State<'_, UiState>, name: String) -> Result<(), String> {
    let path = state.config_path();
    let loaded = load_lenient(&path).map_err(|e| format!("loading current config: {e}"))?;
    let LoadedConfig { mut apps, invalid } = loaded;

    let known = apps.remove(&name).is_some() || invalid.contains_key(&name);
    if !known {
        return Err(format!("app {name:?} not found in config"));
    }

    let remove = BTreeSet::from([name]);
    save_config_preserving_removing(&path, &apps, &remove)
        .map_err(|e| format!("saving config: {e}"))
}

/// Best-effort repair of the entries whose TOML is malformed.
///
/// Only *structural* problems are rewritten from the draft. A *value* problem
/// is left alone: the entry is already editable, and rewriting it would drop the
/// value the user meant to keep.
#[tauri::command]
pub fn repair_config(state: State<'_, UiState>) -> Result<RepairReport, String> {
    let path = state.config_path();
    let loaded = load_lenient(&path).map_err(|e| format!("loading config: {e}"))?;
    let LoadedConfig { mut apps, invalid } = loaded;

    let mut repaired = Vec::new();
    let mut skipped = Vec::new();
    for (name, entry) in invalid {
        // Value problems are surfaced as entries, not rewritten here.
        if entry.kind != InvalidKind::Structural {
            continue;
        }
        let mut draft = entry.draft;
        draft.name = name.clone();
        // A draft can still be invalid (for example a missing command, which
        // cannot be invented). Only write back entries that now validate.
        match draft.validate() {
            Ok(()) => {
                repaired.push(name.clone());
                apps.insert(name, draft);
            }
            Err(e) => skipped.push(RepairSkip {
                name,
                reason: e.to_string(),
            }),
        }
    }

    if repaired.is_empty() {
        return Ok(RepairReport { repaired, skipped });
    }

    // The fixed entries are overwritten by the write; the value errors stay on
    // disk, untouched, because the preserving save skips them.
    save_config_preserving(&path, &apps).map_err(|e| format!("saving config: {e}"))?;
    Ok(RepairReport { repaired, skipped })
}

/// Summary of a [`repair_config`] run.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairReport {
    /// Entries that were rewritten and now load.
    pub repaired: Vec<String>,
    /// Entries that could not be repaired automatically.
    pub skipped: Vec<RepairSkip>,
}

/// An entry that could not be repaired automatically.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairSkip {
    /// The entry's name.
    pub name: String,
    /// Why it could not be repaired.
    pub reason: String,
}

/// Returns the executable/script extensions accepted by this Windows environment.
#[tauri::command]
pub fn get_command_extensions() -> Vec<String> {
    command_extensions()
}

/// Opens a native file dialog to select a command path.
#[tauri::command]
pub fn select_command_path(app: AppHandle, current: String) -> Result<Option<String>, String> {
    let patterns = command_extensions_patterns();
    let pattern_refs: Vec<&str> = patterns.iter().map(String::as_str).collect();

    let mut builder = app
        .dialog()
        .file()
        .add_filter("Command Files", &pattern_refs)
        .add_filter("All Files", &["*"]);
    if !current.trim().is_empty() {
        builder = builder.set_file_name(current.trim());
    }

    match builder.blocking_pick_file() {
        Some(path) => Ok(Some(path.to_string())),
        None => Ok(None),
    }
}

const DEFAULT_COMMAND_EXTENSIONS: &[&str] = &[".exe", ".cmd", ".bat", ".com", ".ps1", ".vbs"];

/// Executable/script extensions derived from `PATHEXT`, falling back to a
/// sensible default and normalized to lowercase with a leading dot.
pub fn command_extensions() -> Vec<String> {
    let raw = std::env::var("PATHEXT").unwrap_or_default();
    derive_extensions(&raw)
}

fn derive_extensions(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return DEFAULT_COMMAND_EXTENSIONS
            .iter()
            .map(|s| s.to_string())
            .collect();
    }

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for part in raw.split(';') {
        let trimmed = part.trim().to_lowercase();
        if trimmed.is_empty() {
            continue;
        }
        let ext = if trimmed.starts_with('.') {
            trimmed
        } else {
            format!(".{trimmed}")
        };
        if seen.insert(ext.clone()) {
            result.push(ext);
        }
    }

    if result.is_empty() {
        DEFAULT_COMMAND_EXTENSIONS
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        result
    }
}

fn command_extensions_patterns() -> Vec<String> {
    command_extensions()
        .iter()
        .map(|e| format!("*{e}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_round_trips_args() {
        let dto = AppConfig {
            name: "app".into(),
            enabled: true,
            command: r"C:\bin\app.exe".into(),
            args: r#"-c "hello world" --debug"#.into(),
            env: vec![
                EnvVar {
                    key: "NODE_ENV".into(),
                    value: "development".into(),
                },
                EnvVar {
                    key: "PORT".into(),
                    value: "3000".into(),
                },
            ],
            stop_command: "taskkill".into(),
            stop_args: "/PID ${PID} /T".into(),
            cwd: r"C:\work".into(),
            restart_delay_sec: 3,
            healthy_timeout_sec: 60,
            hide_window: true,
            max_retries: 5,
            stop_timeout_sec: 10,
            capture_output: true,
            errors: Vec::new(),
            error_kind: None,
        };

        let app = dto.to_app().unwrap();
        assert_eq!(app.args, ["-c", "hello world", "--debug"]);
        assert_eq!(app.stop_args, ["/PID", "${PID}", "/T"]);

        // `$` is shell-significant, so `${PID}` is re-quoted on format. The
        // round-trip is therefore argv-stable rather than string-identical.
        let back = AppConfig::from_app(&app);
        assert_eq!(back.stop_args, r#"/PID "${PID}" /T"#);
        assert_eq!(back.to_app().unwrap().stop_args, app.stop_args);

        // Fields without shell-significant characters round-trip verbatim.
        assert_eq!(back.args, dto.args);
        assert_eq!(back.env, dto.env);
        assert_eq!(back.name, dto.name);
        assert_eq!(back.command, dto.command);
        assert_eq!(back.cwd, dto.cwd);
        assert_eq!(back.max_retries, dto.max_retries);
        assert_eq!(back.stop_timeout_sec, dto.stop_timeout_sec);
    }

    #[test]
    fn app_config_rejects_unterminated_quote() {
        let dto = AppConfig {
            name: "app".into(),
            enabled: true,
            command: "app.exe".into(),
            args: r#""unterminated"#.into(),
            env: Vec::new(),
            stop_command: String::new(),
            stop_args: String::new(),
            cwd: String::new(),
            restart_delay_sec: 0,
            healthy_timeout_sec: 0,
            hide_window: false,
            max_retries: -1,
            stop_timeout_sec: 5,
            capture_output: true,
            errors: Vec::new(),
            error_kind: None,
        };
        assert!(dto.to_app().is_err());
    }

    #[test]
    fn derive_extensions_from_pathext() {
        let exts = derive_extensions(".COM;.EXE;.BAT;.CMD");
        assert_eq!(exts, [".com", ".exe", ".bat", ".cmd"]);
    }

    #[test]
    fn derive_extensions_normalizes_missing_dot() {
        let exts = derive_extensions("COM;EXE");
        assert_eq!(exts, [".com", ".exe"]);
    }

    #[test]
    fn derive_extensions_dedupes() {
        let exts = derive_extensions(".EXE;.exe;.BAT");
        assert_eq!(exts, [".exe", ".bat"]);
    }

    #[test]
    fn derive_extensions_falls_back_when_empty() {
        assert_eq!(derive_extensions(""), command_extensions_default());
        assert_eq!(derive_extensions("  "), command_extensions_default());
    }

    fn command_extensions_default() -> Vec<String> {
        DEFAULT_COMMAND_EXTENSIONS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}
