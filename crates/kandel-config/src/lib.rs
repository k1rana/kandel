//! Portable core of Kandel: the `config.toml` model and its helpers.
//!
//! This crate is platform-independent so it can be unit-tested on any host. The
//! Windows-specific process supervision lives in `kandel-core`.
//!
//! # Layout
//!
//! * [`config`] — the [`App`] model, TOML load/save, validation, resolution.
//! * [`expand`] — `${NAME}` / `${PID}` / `$$` placeholder expansion.
//! * [`args`] — shell-like argument string parsing and formatting.
//! * [`flags`] — shared CLI flags and default config-path resolution.

pub mod args;
pub mod config;
pub mod expand;
pub mod flags;

pub use args::{format_args, parse_args, ParseArgsError};
pub use config::{
    apps_to_toml, load_config, parse_config, resolve_command_path, resolve_command_path_in,
    save_config, App, ConfigError, DEFAULT_ENABLED, DEFAULT_MAX_RETRIES, DEFAULT_STOP_TIMEOUT_SEC,
};
pub use expand::{expand, expand_env, expand_env_with_pid, validate_template, ExpandError};
pub use flags::{default_config_path, LogFormat, RuntimeFlags};

/// Sample configuration written on first launch when the config file is absent.
pub const DEFAULT_CONFIG: &str = r#"["Welcome to Kandel"]
command = "cmd.exe"
args = ["/c", "echo Welcome to Kandel! & echo This application monitors and restarts processes. & echo Right-click the system tray icon to configure settings! & timeout /t 20"]
restart_delay_sec = 1
healthy_timeout_sec = 30
max_retries = 5
"#;

/// The identity both binaries present to the Windows shell.
///
/// Windows groups taskbar buttons and resolves per-app notification settings by
/// AppUserModelID. Without an explicit id it derives one from the executable's
/// **file name**, which makes the app read as "kandel.exe". (The tray balloon's
/// header line is a separate matter: the shell takes that from the executable's
/// VERSIONINFO `FileDescription`, embedded by `crates/kandel-core/build.rs`.)
///
/// Kept here, rather than in either binary, so the core and the UI cannot drift
/// apart: they must present the *same* identity for the shell to treat them as
/// one application. The value matches `identifier` in `tauri.conf.json`.
pub const APP_USER_MODEL_ID: &str = "dev.tojoo.kandel";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn default_config_is_valid() {
        // The bootstrap sample must parse cleanly on Windows, where `cmd.exe`
        // resolves via PATH.
        let apps = parse_config(DEFAULT_CONFIG).unwrap();
        assert!(apps.contains_key("Welcome to Kandel"));
    }

    #[test]
    fn persisted_default_config_is_stable() {
        // Loading the sample and saving it again must be byte-stable, so the
        // file watcher does not see spurious changes from a no-op save.
        let apps: BTreeMap<String, App> = parse_config(DEFAULT_CONFIG).unwrap();
        let first = apps_to_toml(&apps).unwrap();
        let reloaded: BTreeMap<String, App> = parse_config(&first).unwrap();
        let second = apps_to_toml(&reloaded).unwrap();
        assert_eq!(first, second);
    }
}
