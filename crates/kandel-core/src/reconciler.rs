//! Pure reconciliation logic: deciding what to do when config changes.
//!
//! The stateful reconciler that actually owns monitors lives in the Windows
//! `monitor` module; this module holds the platform-independent comparison and
//! decision helpers so they can be tested on any host.

use kandel_config::App;

/// The action the reconciler should take for a single entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Start the app: a new enabled entry appeared.
    Start,
    /// Track the entry but do not run it: a new disabled entry appeared.
    TrackDisabled,
    /// Stop the running process and stop monitoring: entry disabled or removed.
    Stop,
    /// Kill and relaunch: process-identity fields changed.
    Restart,
    /// Update monitoring parameters in place, without restarting the process.
    HotReload,
    /// Nothing to do: already converged.
    NoOp,
}

/// Decides the action for an existing entry given its current and desired config.
///
/// This does not cover entries that are newly added or removed; see
/// [`Action::Start`], [`Action::TrackDisabled`], and [`Action::Stop`] for those
/// cases, which the caller detects from map membership.
pub fn decide(current: &App, desired: &App) -> Action {
    if current.enabled && !desired.enabled {
        return Action::Stop;
    }
    if !current.enabled && desired.enabled {
        return Action::Start;
    }
    if !desired.enabled {
        // Still disabled: refresh the stored config but take no process action.
        return Action::HotReload;
    }
    if has_identity_changed(current, desired) {
        return Action::Restart;
    }
    if has_monitoring_params_changed(current, desired) {
        return Action::HotReload;
    }
    Action::NoOp
}

/// Returns true when any process-identity field changed, which requires a full
/// process restart.
///
/// Process-identity fields: `command`, `args`, `env`, `cwd`, `hide_window`,
/// `capture_output`.
pub fn has_identity_changed(current: &App, desired: &App) -> bool {
    current.command != desired.command
        || current.cwd != desired.cwd
        || current.hide_window != desired.hide_window
        || current.capture_output != desired.capture_output
        || current.args != desired.args
        || current.env != desired.env
}

/// Returns true when only monitoring parameters changed, which can be
/// hot-reloaded without restarting the process.
///
/// Monitoring-parameter fields: `restart_delay_sec`, `healthy_timeout_sec`,
/// `max_retries`, `stop_command`, `stop_args`, `stop_timeout_sec`.
pub fn has_monitoring_params_changed(current: &App, desired: &App) -> bool {
    current.restart_delay_sec != desired.restart_delay_sec
        || current.healthy_timeout_sec != desired.healthy_timeout_sec
        || current.max_retries != desired.max_retries
        || current.stop_timeout_sec != desired.stop_timeout_sec
        || current.stop_command != desired.stop_command
        || current.stop_args != desired.stop_args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App {
            command: r"C:\Windows\System32\cmd.exe".into(),
            cwd: r"C:\Windows\System32".into(),
            ..App::default()
        }
    }

    #[test]
    fn identity_change_ignores_stop_settings() {
        let current = app();
        let mut desired = current.clone();
        desired.stop_command = r"C:\Windows\System32\taskkill.exe".into();
        desired.stop_args = vec!["/PID".into(), "${PID}".into()];
        desired.stop_timeout_sec = 10;

        assert!(!has_identity_changed(&current, &desired));
        assert!(has_monitoring_params_changed(&current, &desired));
    }

    #[test]
    fn identity_change_on_args() {
        let current = app();
        let mut desired = current.clone();
        desired.args = vec!["/c".into(), "echo hi".into()];
        assert!(has_identity_changed(&current, &desired));
    }

    #[test]
    fn identity_change_on_cwd_and_hide_window() {
        let current = app();

        let mut cwd_changed = current.clone();
        cwd_changed.cwd = r"C:\other".into();
        assert!(has_identity_changed(&current, &cwd_changed));

        let mut hidden = current.clone();
        hidden.hide_window = true;
        assert!(has_identity_changed(&current, &hidden));
    }

    #[test]
    fn identity_change_on_env() {
        let current = app();
        let mut desired = current.clone();
        desired.env = vec![kandel_config::EnvVar {
            key: "PORT".into(),
            value: "3000".into(),
        }];
        assert!(has_identity_changed(&current, &desired));
    }

    #[test]
    fn identity_change_on_capture_output() {
        // Turning capture on or off only takes effect at launch, so it must
        // restart the process like any other identity field.
        let current = app();
        let mut desired = current.clone();
        desired.capture_output = !current.capture_output;
        assert!(has_identity_changed(&current, &desired));
    }

    #[test]
    fn monitoring_params_include_stop_settings() {
        let current = App {
            stop_timeout_sec: 5,
            ..App::default()
        };
        let mut desired = current.clone();
        desired.stop_timeout_sec = 10;
        assert!(has_monitoring_params_changed(&current, &desired));

        let mut desired = current.clone();
        desired.stop_command = "taskkill".into();
        desired.stop_args = vec!["/PID".into(), "${PID}".into()];
        assert!(has_monitoring_params_changed(&current, &desired));
    }

    #[test]
    fn decide_handles_enabled_transitions() {
        let enabled = App {
            enabled: true,
            ..app()
        };
        let disabled = App {
            enabled: false,
            ..app()
        };

        assert_eq!(decide(&enabled, &disabled), Action::Stop);
        assert_eq!(decide(&disabled, &enabled), Action::Start);
    }

    #[test]
    fn decide_restart_beats_hot_reload() {
        let current = app();
        let mut desired = current.clone();
        desired.command = r"C:\Windows\System32\notepad.exe".into();
        desired.restart_delay_sec = 5;
        assert_eq!(decide(&current, &desired), Action::Restart);
    }

    #[test]
    fn decide_hot_reload_for_params_only() {
        let current = app();
        let mut desired = current.clone();
        desired.restart_delay_sec = 5;
        assert_eq!(decide(&current, &desired), Action::HotReload);
    }

    #[test]
    fn decide_noop_when_unchanged() {
        let current = app();
        assert_eq!(decide(&current, &current.clone()), Action::NoOp);
    }

    #[test]
    fn decide_refreshes_disabled_entries() {
        let disabled = App {
            enabled: false,
            ..app()
        };
        let mut desired = disabled.clone();
        desired.healthy_timeout_sec = 30;
        assert_eq!(decide(&disabled, &desired), Action::HotReload);
    }
}
