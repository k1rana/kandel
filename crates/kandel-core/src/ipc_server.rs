//! The core's side of the UI channel: publishing the status stream and
//! launching/stopping the UI process.
//!
//! These are deliberately separate concerns:
//!
//! * [`StatusServer`] publishes the core's named pipe and forwards events to
//!   whichever UI is attached. It does not care *which* process is on the other
//!   end, which is what makes the stream reconnectable (see [`crate::win::pipe`]).
//! * [`UiLauncher`] starts and stops the UI executable, so a tray click opens a
//!   window. The window then attaches to the pipe on its own.
//!
//! Framing is newline-delimited JSON, one [`CoreEvent`] per line — unchanged
//! from the previous stdin transport, so the frontend needs no changes.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use kandel_config::{LogFormat, RuntimeFlags};

use crate::ipc::CoreEvent;
use crate::logs::LogEvent;
use crate::status::MonitorStatus;
use crate::win::pipe::PipeServer;

/// Builds the event stream a freshly attached UI receives, oldest first.
///
/// Statuses come first so the ledger is populated before its log panels fill in.
pub fn snapshot_events(
    statuses: &[MonitorStatus],
    logs: &[(String, Vec<LogEvent>)],
) -> Vec<CoreEvent> {
    let mut events = Vec::with_capacity(statuses.len() + logs.len());
    events.extend(statuses.iter().cloned().map(CoreEvent::Status));
    for (name, events_for_app) in logs {
        for event in events_for_app {
            events.push(CoreEvent::Logs {
                name: name.clone(),
                event: event.clone(),
            });
        }
    }
    events
}

/// Publishes the core's status pipe and serves whichever UI attaches.
///
/// `snapshot` is invoked on every attach — initial connect *and* reconnect — so
/// a late or returning client is never left with an empty ledger.
pub fn serve(snapshot: impl Fn() -> Vec<CoreEvent> + Send + Sync + 'static) -> StatusServer {
    let name = crate::win::pipe::local_pipe_name();
    let server = PipeServer::spawn(name.clone(), Arc::new(snapshot));
    tracing::info!(component = "ipc", pipe = %name, "status pipe published");
    StatusServer { server }
}

/// A handle to the running status server.
///
/// Cheap to clone; every clone addresses the same stream.
#[derive(Clone)]
pub struct StatusServer {
    server: PipeServer,
}

impl StatusServer {
    /// Sends a status update to the attached UI, if any.
    pub fn send(&self, status: &MonitorStatus) {
        self.server.send(&CoreEvent::Status(status.clone()));
    }

    /// Sends a captured log event for `name` to the attached UI, if any.
    pub fn send_log(&self, name: &str, event: &LogEvent) {
        self.server.send(&CoreEvent::Logs {
            name: name.to_string(),
            event: event.clone(),
        });
    }
}

/// Launches and tracks the single UI process.
///
/// The UI is not given a status pipe: it attaches to the core's named pipe
/// itself. That is why the same executable works whether the core launched it or
/// the user opened it directly.
#[derive(Clone, Default)]
pub struct UiLauncher {
    child: Arc<Mutex<Option<Child>>>,
}

impl UiLauncher {
    /// Creates an empty launcher.
    pub fn new() -> Self {
        Self::default()
    }

    /// Launches the UI if it is not already running.
    ///
    /// `flags` supplies the config path and log options, so both processes agree
    /// on which file they are working with.
    pub fn show(&self, flags: &RuntimeFlags) -> Result<()> {
        let mut guard = self.child.lock().unwrap();

        // Reap a UI that has already exited.
        if let Some(child) = guard.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                *guard = None;
            }
        }

        if guard.is_none() {
            let ui_exe = ui_executable_path();
            let mut command = Command::new(&ui_exe);
            command
                .arg("-f")
                .arg(&flags.config_path)
                // The UI is an independent process now: it must not inherit the
                // core's standard handles or it would keep a console alive.
                .stdin(Stdio::null())
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

            let child = command
                .spawn()
                .with_context(|| format!("launching UI process {}", ui_exe.display()))?;
            *guard = Some(child);
        }

        Ok(())
    }

    /// Returns true when a UI process is currently being tracked.
    pub fn is_running(&self) -> bool {
        let mut guard = self.child.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                *guard = None;
                return false;
            }
            return true;
        }
        false
    }

    /// Terminates the tracked UI process, if any.
    pub fn shutdown(&self) {
        let mut guard = self.child.lock().unwrap();
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
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
    use crate::logs::STREAM_STDOUT;
    use crate::status::AppState;

    fn status(name: &str) -> MonitorStatus {
        MonitorStatus {
            name: name.into(),
            state: AppState::Running,
            pid: 1,
            enabled: true,
            command: "cmd.exe".into(),
            args: vec![],
            restart_count: 0,
        }
    }

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
    fn snapshot_puts_statuses_before_logs() {
        let statuses = vec![status("api")];
        let logs = vec![(
            "api".to_string(),
            vec![LogEvent::Line {
                ts: 1,
                stream: STREAM_STDOUT.into(),
                text: "boot".into(),
            }],
        )];

        let events = snapshot_events(&statuses, &logs);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], CoreEvent::Status(_)));
        assert!(matches!(events[1], CoreEvent::Logs { .. }));
    }

    #[test]
    fn snapshot_of_nothing_is_empty() {
        assert!(snapshot_events(&[], &[]).is_empty());
    }

    #[test]
    fn shutdown_without_ui_is_a_noop() {
        UiLauncher::new().shutdown();
        assert!(!UiLauncher::new().is_running());
    }
}
