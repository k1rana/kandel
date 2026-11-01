//! Runtime status types shared by the supervisor and the UI IPC channel.

use serde::{Deserialize, Serialize};

/// Lifecycle state of a monitored process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AppState {
    /// Not running and not being monitored.
    Stopped,
    /// A launch is in progress.
    Starting,
    /// Running and healthy.
    Running,
    /// The process is being stopped so it can be relaunched (a config change
    /// with a new process identity).
    Restarting,
    /// Waiting to retry after an unexpected exit.
    Retrying,
    /// Retries exhausted; no further attempts will be made.
    Failed,
    /// A one-shot entry (`max_retries = 0`) exited cleanly and is done.
    Finished,
    /// Entry was removed from the configuration.
    Removed,
}

impl AppState {
    /// Returns the display name, matching the on-the-wire JSON representation.
    pub fn as_str(self) -> &'static str {
        match self {
            AppState::Stopped => "Stopped",
            AppState::Starting => "Starting",
            AppState::Running => "Running",
            AppState::Restarting => "Restarting",
            AppState::Retrying => "Retrying",
            AppState::Failed => "Failed",
            AppState::Finished => "Finished",
            AppState::Removed => "Removed",
        }
    }
}

impl std::fmt::Display for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A snapshot of a monitor's current state, sent to the UI via IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatus {
    /// App name (config table key).
    pub name: String,
    /// Current lifecycle state.
    pub state: AppState,
    /// PID of the current root process, or 0 when not running.
    pub pid: u32,
    /// Whether the entry is enabled in the configuration.
    pub enabled: bool,
    /// The configured command, unexpanded.
    pub command: String,
    /// The configured arguments, unexpanded.
    pub args: Vec<String>,
    /// Number of restart attempts since the last healthy run.
    pub restart_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_serializes_pascal_case() {
        let json = serde_json::to_string(&AppState::Running).unwrap();
        assert_eq!(json, "\"Running\"");
        // The UI matches on this exact string, so keep the spelling stable.
        let json = serde_json::to_string(&AppState::Restarting).unwrap();
        assert_eq!(json, "\"Restarting\"");
        let json = serde_json::to_string(&AppState::Finished).unwrap();
        assert_eq!(json, "\"Finished\"");
    }

    #[test]
    fn status_serializes_camel_case() {
        let status = MonitorStatus {
            name: "app".into(),
            state: AppState::Retrying,
            pid: 1234,
            enabled: true,
            command: "cmd.exe".into(),
            args: vec!["/c".into()],
            restart_count: 3,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"restartCount\":3"), "{json}");
        assert!(json.contains("\"state\":\"Retrying\""), "{json}");
        assert!(json.contains("\"pid\":1234"), "{json}");
    }

    #[test]
    fn status_round_trips() {
        let status = MonitorStatus {
            name: "app".into(),
            state: AppState::Stopped,
            pid: 0,
            enabled: false,
            command: "app.exe".into(),
            args: Vec::new(),
            restart_count: -1,
        };
        let json = serde_json::to_string(&status).unwrap();
        let back: MonitorStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }
}
