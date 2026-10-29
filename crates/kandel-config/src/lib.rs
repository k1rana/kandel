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
    apps_to_toml, load_config, load_config_lenient, parse_config, parse_config_lenient,
    resolve_command_path, resolve_command_path_in, save_config, save_config_preserving,
    save_config_preserving_removing, App, ConfigError, EnvVar, InvalidEntry, InvalidKind,
    LoadedConfig, DEFAULT_CAPTURE_OUTPUT, DEFAULT_ENABLED, DEFAULT_MAX_RETRIES,
    DEFAULT_STOP_TIMEOUT_SEC,
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

/// Stable identity suffix for every named kernel object the two binaries use to
/// find each other: the core's single-instance mutex and its status pipe.
///
/// Kept in one place so the names cannot drift apart. Debug (dev) builds get a
/// distinct suffix so a development core and an installed release build can run
/// side by side without colliding.
const fn identity_suffix() -> &'static str {
    if cfg!(debug_assertions) {
        "dev-6d3fe7b9-a7fc-48e8-bc7a-66d205c03b0e"
    } else {
        "6d3fe7b9-a7fc-48e8-bc7a-66d205c03b0e"
    }
}

/// Name of the session-local mutex a running core holds for its lifetime.
///
/// The core creates it as its single-instance guard; the UI opens the same name
/// to tell whether a core is already running before deciding to start one. The
/// `Local\` prefix scopes it to the current Windows session, so different
/// signed-in users each get their own Kandel.
pub fn instance_mutex_name() -> String {
    format!(r"Local\Kandel-core-{}", identity_suffix())
}

/// Name of the core's status pipe for `session_id`.
///
/// The pipe namespace (`\\.\pipe\`) is machine-wide, so the session id is part
/// of the name; that keeps different signed-in users from attaching to each
/// other's core, matching the session-local mutex above.
pub fn pipe_name(session_id: u32) -> String {
    format!(r"\\.\pipe\kandel-core-{}-{}", identity_suffix(), session_id)
}

/// Stable identity of the tray icon, so the shell keys its position and
/// "always show" state on the app rather than on a process.
///
/// Like the mutex and pipe names, this differs between dev and release. A tray
/// GUID is machine-wide, and a dev and a release core can legitimately run at
/// the same time (which is why the identity split exists at all), so sharing one
/// GUID makes the second core override the first icon. The release value is
/// fixed: changing it would reset every user's tray preference.
pub const fn tray_guid() -> u128 {
    if cfg!(debug_assertions) {
        0x6f1c8b2e_7a4d_4f3a_9c1e_2b8d5a7f0c32
    } else {
        0x6f1c8b2e_7a4d_4f3a_9c1e_2b8d5a7f0c31
    }
}

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

    #[test]
    fn instance_mutex_name_is_session_local() {
        // The `Local\` prefix is what keeps separate Windows sessions (and thus
        // separate users) from colliding on the guard.
        assert!(instance_mutex_name().starts_with(r"Local\Kandel-core"));
    }

    #[test]
    fn pipe_name_is_scoped_by_session() {
        let a = pipe_name(1);
        let b = pipe_name(2);
        assert!(a.starts_with(r"\\.\pipe\kandel-core"));
        // Different sessions must resolve to different pipes, or two users would
        // share one core's stream.
        assert_ne!(a, b);
    }

    #[test]
    fn mutex_and_pipe_share_one_identity() {
        // They must never drift: both are built from the same suffix.
        let suffix = identity_suffix();
        assert!(instance_mutex_name().contains(suffix));
        assert!(pipe_name(7).contains(suffix));
    }

    #[test]
    fn tray_identity_is_stable_and_profile_scoped() {
        // The release GUID is fixed forever (users' tray preferences key on it),
        // and a dev core must not share it, since both can run at once.
        let release = 0x6f1c8b2e_7a4d_4f3a_9c1e_2b8d5a7f0c31;
        if cfg!(debug_assertions) {
            assert_ne!(
                tray_guid(),
                release,
                "a dev core must not claim the release tray icon"
            );
        } else {
            assert_eq!(tray_guid(), release);
        }
    }
}
