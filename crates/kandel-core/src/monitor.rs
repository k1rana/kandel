//! Per-process supervision.
//!
//! Each [`Monitor`] owns a background thread that starts, watches, restarts,
//! and stops a single managed process. The process tree is bound to a Job
//! Object so it is reliably reaped.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use kandel_config::{expand_env_with_pid, App};

use crate::logs::LogStore;
use crate::notify::Notifier;
use crate::reconciler as decision;
use crate::status::{AppState, MonitorStatus};
use crate::win::process::{self, ChildProcess, LaunchSpec, LineSink};
use crate::win::{console, window};

/// Callback invoked whenever a monitor's state changes.
pub type StateCallback = Arc<dyn Fn(MonitorStatus) + Send + Sync>;

/// Shared state between a [`Monitor`] handle and its thread.
struct MonitorInner {
    /// The app's config-table key: the identity used for its supervised state.
    name: String,
    config: Mutex<App>,
    state: Mutex<MonitorStatus>,
    stop: AtomicBool,
    on_state: StateCallback,
    /// Used to tell the user when an app exhausts its retries.
    notifier: Arc<Notifier>,
    /// Restart-surviving output logs, owned by the supervisor. Recording a
    /// line here also forwards it to the UI (via the store's sink).
    logs: Arc<LogStore>,
}

impl MonitorInner {
    fn snapshot_config(&self) -> App {
        self.config.lock().unwrap().clone()
    }

    fn emit(&self, state: AppState, pid: u32, restart_count: i64) {
        let cfg = self.config.lock().unwrap();
        let status = MonitorStatus {
            name: cfg.name.clone(),
            state,
            pid,
            enabled: cfg.enabled,
            command: cfg.command.clone(),
            args: cfg.args.clone(),
            restart_count,
        };
        drop(cfg);
        *self.state.lock().unwrap() = status.clone();
        (self.on_state)(status);
    }
}

/// Manages the lifecycle of a single monitored process.
pub struct Monitor {
    inner: Arc<MonitorInner>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Monitor {
    /// Creates a monitor for `cfg` that reports state changes via `on_state`.
    ///
    /// `name` is the config-table key (the identity used for logs and state);
    /// `logs` is the restart-surviving per-app log store, whose sink forwards
    /// captured output to the UI.
    pub fn new(
        name: String,
        cfg: App,
        on_state: StateCallback,
        notifier: Arc<Notifier>,
        logs: Arc<LogStore>,
    ) -> Self {
        let status = MonitorStatus {
            name: cfg.name.clone(),
            state: AppState::Stopped,
            pid: 0,
            enabled: cfg.enabled,
            command: cfg.command.clone(),
            args: cfg.args.clone(),
            restart_count: 0,
        };
        Monitor {
            inner: Arc::new(MonitorInner {
                name,
                config: Mutex::new(cfg),
                state: Mutex::new(status),
                stop: AtomicBool::new(false),
                on_state,
                notifier,
                logs,
            }),
            join: Mutex::new(None),
        }
    }

    /// Launches the monitoring thread. A no-op if already running.
    pub fn start(&self) {
        let mut join = self.join.lock().unwrap();
        if join.is_some() {
            return;
        }
        self.inner.stop.store(false, Ordering::SeqCst);
        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("kandel-monitor".into())
            .spawn(move || monitor_loop(inner))
            .expect("spawning monitor thread");
        *join = Some(handle);
    }

    /// Signals the monitor to stop and waits for the thread to finish.
    ///
    /// The thread performs a graceful stop (per config) before returning.
    pub fn stop(&self) {
        let handle = self.join.lock().unwrap().take();
        if let Some(handle) = handle {
            self.inner.stop.store(true, Ordering::SeqCst);
            let _ = handle.join();
        }
    }

    /// Replaces the monitor's config (used before stopping so that state
    /// notifications during shutdown reflect the new config).
    pub fn set_config(&self, cfg: App) {
        *self.inner.config.lock().unwrap() = cfg;
    }

    /// Hot-reloads the monitoring parameters without restarting the process.
    pub fn update_monitoring_params(&self, cfg: App) {
        let mut current = self.inner.config.lock().unwrap();
        current.restart_delay_sec = cfg.restart_delay_sec;
        current.healthy_timeout_sec = cfg.healthy_timeout_sec;
        current.max_retries = cfg.max_retries;
        current.stop_command = cfg.stop_command;
        current.stop_args = cfg.stop_args;
        current.stop_timeout_sec = cfg.stop_timeout_sec;
    }

    /// Returns a snapshot of the monitor's current config.
    pub fn config(&self) -> App {
        self.inner.snapshot_config()
    }

    /// Returns a snapshot of the monitor's current status.
    pub fn status(&self) -> MonitorStatus {
        self.inner.state.lock().unwrap().clone()
    }
}

/// The monitor thread body: a restart loop around a single process.
fn monitor_loop(shared: Arc<MonitorInner>) {
    let mut restart_count: i64 = 0;

    loop {
        if shared.stop.load(Ordering::SeqCst) {
            shared.emit(AppState::Stopped, 0, restart_count);
            return;
        }

        let cfg = shared.snapshot_config();
        // Capture (and therefore logging) is per-entry. When off, the process is
        // launched with its handles untouched and nothing is logged.
        let capture = cfg.capture_output;
        shared.emit(AppState::Starting, 0, restart_count);
        let start_time = Instant::now();

        let spec = match LaunchSpec::from_app(&cfg) {
            Ok(spec) => spec,
            Err(e) => {
                if capture {
                    shared.logs.record_started(&shared.name);
                    shared.logs.record_line(
                        &shared.name,
                        crate::logs::STREAM_STDERR,
                        e.to_string(),
                    );
                }
                if handle_start_failure(&shared, &cfg, &mut restart_count, &e) {
                    return;
                }
                continue;
            }
        };

        // Begin a new log session, preserving everything captured before. The
        // marker opens each restart's section, and history is never cleared.
        if capture {
            shared.logs.record_started(&shared.name);
        }
        let name = shared.name.clone();
        let logs = Arc::clone(&shared.logs);
        let sink: LineSink = Arc::new(move |stream_name: &str, text: &str| {
            logs.record_line(&name, stream_name, text.to_string());
        });

        let mut child = match process::spawn_with_job_object(&spec, Some(sink)) {
            Ok(child) => child,
            Err(e) => {
                if capture {
                    shared.logs.record_line(
                        &shared.name,
                        crate::logs::STREAM_STDERR,
                        e.to_string(),
                    );
                }
                if handle_start_failure(&shared, &cfg, &mut restart_count, &e) {
                    return;
                }
                continue;
            }
        };

        tracing::info!(
            component = "monitor",
            app = %cfg.name,
            pid = child.pid,
            "process started"
        );
        shared.emit(AppState::Running, child.pid, restart_count);

        let exited_naturally = process::wait_for_exit_or_stop(&child, &shared.stop);

        if !exited_naturally {
            // Stop was requested.
            let live_cfg = shared.snapshot_config();
            if let Err(e) = stop_process(&live_cfg, &child) {
                tracing::warn!(
                    component = "monitor",
                    app = %live_cfg.name,
                    error = %e,
                    "stop strategy failed, continuing cleanup"
                );
            }
            child.join_capture();
            shared.emit(AppState::Stopped, 0, restart_count);
            return;
        }

        // Drained before any restart, so this session's output is complete and
        // ordered ahead of the next start marker.
        child.join_capture();

        // Process exited on its own.
        let duration = start_time.elapsed();

        if cfg.healthy_timeout_sec > 0
            && duration >= Duration::from_secs(cfg.healthy_timeout_sec as u64)
        {
            restart_count = 0;
        } else {
            restart_count += 1;
        }

        if cfg.max_retries >= 0 && restart_count > cfg.max_retries {
            let retries = restart_count - 1;
            tracing::error!(
                component = "monitor",
                app = %cfg.name,
                retries,
                max_retries = cfg.max_retries,
                "app marked as Failed: restart retries exhausted"
            );
            shared.emit(AppState::Failed, 0, restart_count);
            shared.notifier.app_failed(
                &cfg.name,
                &format!(
                    "Exited repeatedly and has been marked as Failed after {retries} restarts."
                ),
            );
            return;
        }

        shared.emit(AppState::Retrying, 0, restart_count);
        let delay = Duration::from_secs(cfg.restart_delay_sec.max(0) as u64);
        if sleep_interruptible(&shared.stop, delay) {
            shared.emit(AppState::Stopped, 0, restart_count);
            return;
        }
    }
}

/// Handles a failed start: retries or marks the app Failed.
///
/// Returns `true` when the monitor thread should exit.
fn handle_start_failure(
    shared: &MonitorInner,
    cfg: &App,
    restart_count: &mut i64,
    err: &anyhow::Error,
) -> bool {
    tracing::error!(
        component = "monitor",
        app = %cfg.name,
        error = %err,
        "failed to start process"
    );

    *restart_count += 1;
    if cfg.max_retries >= 0 && *restart_count > cfg.max_retries {
        let retries = *restart_count - 1;
        shared.emit(AppState::Failed, 0, *restart_count);
        shared.notifier.app_failed(
            &cfg.name,
            &format!(
                "Failed to start after {retries} retries and has been marked as Failed.\nLast error: {err}"
            ),
        );
        return true;
    }

    shared.emit(AppState::Retrying, 0, *restart_count);
    let delay = Duration::from_secs(cfg.restart_delay_sec.max(0) as u64);
    sleep_interruptible(&shared.stop, delay)
}

/// Sleeps for `duration`, returning `true` if a stop was requested first.
fn sleep_interruptible(stop: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    loop {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let remaining = (deadline - now).min(Duration::from_millis(50));
        thread::sleep(remaining);
    }
}

/// Stops `child` according to `cfg`: an explicit stop command, an automatic
/// graceful stop, then a forceful fallback.
pub fn stop_process(cfg: &App, child: &ChildProcess) -> Result<()> {
    let timeout = Duration::from_secs(cfg.stop_timeout_sec.max(0) as u64);

    if !cfg.stop_command.is_empty() {
        return match launch_stop_command(cfg, child.pid) {
            Ok(()) => wait_or_terminate(child, timeout),
            Err(e) => {
                tracing::warn!(
                    component = "monitor",
                    app = %cfg.name,
                    error = %e,
                    "stop_command failed to start, falling back to TerminateProcess"
                );
                child.terminate()
            }
        };
    }

    if window::post_close_to_process_windows(child.pid) > 0 {
        return wait_or_terminate(child, timeout);
    }

    match console::send_ctrl_break(child.pid) {
        Ok(guard) => {
            let result = wait_or_terminate(child, timeout);
            // The guard detaches from the console only now, after the child has
            // exited or the timeout elapsed.
            drop(guard);
            result
        }
        Err(e) => {
            tracing::warn!(
                component = "monitor",
                app = %cfg.name,
                error = %e,
                "automatic console stop failed, falling back to TerminateProcess"
            );
            child.terminate()
        }
    }
}

fn wait_or_terminate(child: &ChildProcess, timeout: Duration) -> Result<()> {
    if child.wait_for_exit(Some(timeout)) {
        Ok(())
    } else {
        child.terminate()
    }
}

/// Launches the configured `stop_command` for the monitored `pid`.
fn launch_stop_command(cfg: &App, pid: u32) -> Result<()> {
    let program = cfg
        .resolved_stop_command()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .ok_or_else(|| anyhow::anyhow!("stop_command is empty"))?;
    let args = expand_stop_args(&cfg.stop_args, pid)?;

    let mut command = std::process::Command::new(&program);
    command.args(&args);
    if let Ok(cwd) = cfg.resolved_cwd() {
        if !cwd.as_os_str().is_empty() {
            command.current_dir(cwd);
        }
    }
    // Hide the stop command's window.
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);

    command
        .spawn()
        .with_context(|| format!("starting stop_command {}", program.display()))?;
    Ok(())
}

/// Expands `${PID}` and environment placeholders in `stop_args`.
pub fn expand_stop_args(stop_args: &[String], pid: u32) -> Result<Vec<String>> {
    stop_args
        .iter()
        .enumerate()
        .map(|(i, arg)| {
            expand_env_with_pid(arg, pid).map_err(|e| anyhow::anyhow!("stop_args[{i}]: {e}"))
        })
        .collect()
}

// Re-export the decision helper so the stateful reconciler can reuse it.
pub use decision::Action;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_stop_args_expands_pid() {
        let got = expand_stop_args(
            &[
                "/PID".to_string(),
                "${PID}".to_string(),
                "/FI".to_string(),
                "pid eq ${PID}".to_string(),
            ],
            4242,
        )
        .unwrap();
        assert_eq!(got, ["/PID", "4242", "/FI", "pid eq 4242"]);
    }

    #[test]
    fn expand_stop_args_rejects_undefined_var() {
        let got = expand_stop_args(&["${KANDEL_NOT_DEFINED}".to_string()], 1);
        assert!(got.is_err());
    }

    #[test]
    fn sleep_interruptible_returns_immediately_when_stopped() {
        let stop = AtomicBool::new(true);
        let start = Instant::now();
        assert!(sleep_interruptible(&stop, Duration::from_secs(5)));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn sleep_interruptible_sleeps_full_duration() {
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        assert!(!sleep_interruptible(&stop, Duration::from_millis(150)));
        assert!(start.elapsed() >= Duration::from_millis(140));
    }

    #[test]
    fn monitor_starts_and_stops_long_running_process() {
        use std::sync::mpsc::channel;

        let (tx, rx) = channel();
        let on_state: StateCallback = Arc::new(move |status| {
            let _ = tx.send(status);
        });

        let cfg = App {
            name: "sleeper".into(),
            command: "cmd.exe".into(),
            args: vec!["/c".into(), "ping -n 60 127.0.0.1 > nul".into()],
            hide_window: true,
            ..App::default()
        };

        let monitor = Monitor::new(
            "sleeper".into(),
            cfg,
            on_state,
            Arc::new(Notifier::new()),
            Arc::new(LogStore::default()),
        );
        monitor.start();

        // Wait until the process reports Running.
        let mut saw_running = false;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_secs(3)) {
                Ok(status) if status.state == AppState::Running => {
                    saw_running = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_running, "expected to observe a Running state");

        // Stopping must terminate the process and report Stopped.
        monitor.stop();
        assert_eq!(monitor.status().state, AppState::Stopped);
        assert_eq!(monitor.status().pid, 0);
    }

    #[test]
    fn monitor_marks_crashing_app_failed_after_retries() {
        use std::sync::mpsc::channel;

        let (tx, rx) = channel();
        let on_state: StateCallback = Arc::new(move |status| {
            let _ = tx.send(status);
        });

        let cfg = App {
            name: "crashy".into(),
            command: "cmd.exe".into(),
            args: vec!["/c".into(), "exit 1".into()],
            hide_window: true,
            restart_delay_sec: 0,
            max_retries: 0,
            ..App::default()
        };

        let monitor = Monitor::new(
            "crashy".into(),
            cfg,
            on_state,
            Arc::new(Notifier::new()),
            Arc::new(LogStore::default()),
        );
        monitor.start();

        let mut saw_failed = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_secs(3)) {
                Ok(status) if status.state == AppState::Failed => {
                    saw_failed = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_failed, "expected the app to be marked Failed");
        // A terminal monitor has nothing to stop; stop() is a no-op.
        monitor.stop();
    }

    #[test]
    fn monitor_log_survives_restarts() {
        use std::sync::mpsc::channel;

        let (tx, rx) = channel();
        let on_state: StateCallback = Arc::new(move |status| {
            let _ = tx.send(status);
        });

        // A process that prints one line then exits: the restart loop runs it
        // twice, and the second session must append to the first.
        let cfg = App {
            name: "talky".into(),
            command: "cmd.exe".into(),
            args: vec!["/c".into(), "echo hello-session".into()],
            hide_window: true,
            restart_delay_sec: 0,
            max_retries: 1,
            capture_output: true,
            ..App::default()
        };

        let logs = Arc::new(LogStore::default());
        let monitor = Monitor::new(
            "talky".into(),
            cfg,
            on_state,
            Arc::new(Notifier::new()),
            Arc::clone(&logs),
        );
        monitor.start();

        // Wait until the app gives up (max_retries = 1 means two launches).
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_secs(3)) {
                Ok(status) if status.state == AppState::Failed => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }

        let events = logs.events("talky");
        let starts = events
            .iter()
            .filter(|e| matches!(e, crate::logs::LogEvent::Started { .. }))
            .count();
        let lines = events
            .iter()
            .filter(|e| matches!(e, crate::logs::LogEvent::Line { .. }))
            .count();
        assert_eq!(starts, 2, "expected two session markers: {events:?}");
        assert!(lines >= 2, "expected output from both sessions: {events:?}");
        monitor.stop();
    }
}
