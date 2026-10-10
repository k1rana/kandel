//! Manages the single UI child process and pushes status updates to it.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use kandel_config::{LogFormat, RuntimeFlags};

use crate::ipc::StatusWriter;
use crate::status::MonitorStatus;

/// A running UI process and its status writer.
struct UiProcess {
    child: Child,
    writer: StatusWriter<std::process::ChildStdin>,
}

/// Launches and tracks the single UI process.
#[derive(Clone)]
pub struct UiManager {
    inner: Arc<Mutex<Option<UiProcess>>>,
}

impl Default for UiManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UiManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        UiManager {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Launches the UI if it is not already running, then sends `initial`.
    ///
    /// The UI executable is expected next to the core binary, named
    /// `kandel-ui.exe`. Passing `-f`, `-log-file`, and `-log-format` keeps the
    /// two processes consistent.
    pub fn show(&self, flags: &RuntimeFlags, initial: &[MonitorStatus]) -> Result<()> {
        let mut guard = self.inner.lock().unwrap();

        // Reap a UI that has already exited.
        if let Some(ui) = guard.as_mut() {
            if matches!(ui.child.try_wait(), Ok(Some(_))) {
                *guard = None;
            }
        }

        if guard.is_none() {
            let ui_exe = ui_executable_path();
            let mut command = Command::new(&ui_exe);
            // `--ipc` tells the UI it is being launched by the core with a
            // piped stdin, so it should listen for status updates and exit
            // when the pipe closes.
            command
                .arg("--ipc")
                .arg("-f")
                .arg(&flags.config_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(log_file) = &flags.log_file {
                command.arg("-log-file").arg(log_file);
            }
            let format = match flags.log_format {
                LogFormat::Json => "json",
                LogFormat::Text => "text",
            };
            command.arg("-log-format").arg(format);

            let mut child = command
                .spawn()
                .with_context(|| format!("launching UI process {}", ui_exe.display()))?;
            let stdin = child.stdin.take().context("taking UI stdin")?;
            *guard = Some(UiProcess {
                child,
                writer: StatusWriter::new(stdin),
            });
        }

        if let Some(ui) = guard.as_mut() {
            for status in initial {
                let _ = ui.writer.send(status);
            }
        }

        Ok(())
    }

    /// Sends a status update to the UI, ignoring the case where no UI is running.
    pub fn send(&self, status: &MonitorStatus) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(ui) = guard.as_mut() {
            if ui.writer.send(status).is_err() {
                // Writing to a closed pipe means the UI exited; drop it.
                *guard = None;
            }
        }
    }

    /// Returns true when a UI process is currently being tracked.
    pub fn is_running(&self) -> bool {
        let mut guard = self.inner.lock().unwrap();
        if let Some(ui) = guard.as_mut() {
            if matches!(ui.child.try_wait(), Ok(Some(_))) {
                *guard = None;
                return false;
            }
            return true;
        }
        false
    }

    /// Terminates the tracked UI process, if any.
    pub fn shutdown(&self) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(mut ui) = guard.take() {
            let _ = ui.child.kill();
            let _ = ui.child.wait();
        }
    }
}

/// Resolves the UI executable path next to the current executable.
pub fn ui_executable_path() -> PathBuf {
    let current = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("kandel"));
    let dir = current.parent().map(PathBuf::from).unwrap_or_default();
    let name = if cfg!(windows) {
        "kandel-ui.exe"
    } else {
        "kandel-ui"
    };
    dir.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::AppState;

    #[test]
    fn ui_executable_sits_next_to_core() {
        let path = ui_executable_path();
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("kandel-ui"));
    }

    #[test]
    fn send_without_ui_is_a_noop() {
        let manager = UiManager::new();
        manager.send(&MonitorStatus {
            name: "a".into(),
            state: AppState::Running,
            pid: 1,
            enabled: true,
            command: "cmd.exe".into(),
            args: vec![],
            restart_count: 0,
        });
        assert!(!manager.is_running());
    }

    #[test]
    fn shutdown_without_ui_is_a_noop() {
        UiManager::new().shutdown();
    }
}
