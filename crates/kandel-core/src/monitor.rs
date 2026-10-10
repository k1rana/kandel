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

use crate::notify::Notifier;
use crate::reconciler as decision;
use crate::status::{AppState, MonitorStatus};
use crate::win::process::{self, ChildProcess, LaunchSpec};
use crate::win::{console, window};

/// Callback invoked whenever a monitor's state changes.
pub type StateCallback = Arc<dyn Fn(MonitorStatus) + Send + Sync>;

/// Shared state between a [`Monitor`] handle and its thread.
struct MonitorShared {
    config: Mutex<App>,
    state: Mutex<MonitorStatus>,
    stop: AtomicBool,
    on_state: StateCallback,
    /// Used to tell the user when an app exhausts its retries.
    notifier: Arc<Notifier>,
}

impl MonitorShared {
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
    shared: Arc<MonitorShared>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Monitor {
    /// Creates a monitor for `cfg` that reports state changes via `on_state`.
    pub fn new(cfg: App, on_state: StateCallback, notifier: Arc<Notifier>) -> Self {
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
            shared: Arc::new(MonitorShared {
                config: Mutex::new(cfg),
                state: Mutex::new(status),
                stop: AtomicBool::new(false),
                on_state,
                notifier,
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
        self.shared.stop.store(false, Ordering::SeqCst);
        let shared = Arc::clone(&self.shared);
        let handle = thread::Builder::new()
            .name("kandel-monitor".into())
            .spawn(move || monitor_loop(shared))
            .expect("spawning monitor thread");
        *join = Some(handle);
    }

    /// Signals the monitor to stop and waits for the thread to finish.
    ///
    /// The thread performs a graceful stop (per config) before returning.
    pub fn stop(&self) {
        let handle = self.join.lock().unwrap().take();
        if let Some(handle) = handle {
            self.shared.stop.store(true, Ordering::SeqCst);
            let _ = handle.join();
        }
    }

    /// Replaces the monitor's config (used before stopping so that state
    /// notifications during shutdown reflect the new config).
    pub fn set_config(&self, cfg: App) {
        *self.shared.config.lock().unwrap() = cfg;
    }

    /// Hot-reloads the monitoring parameters without restarting the process.
    pub fn update_monitoring_params(&self, cfg: App) {
        let mut current = self.shared.config.lock().unwrap();
        current.restart_delay_sec = cfg.restart_delay_sec;
        current.healthy_timeout_sec = cfg.healthy_timeout_sec;
        current.max_retries = cfg.max_retries;
        current.stop_command = cfg.stop_command;
        current.stop_args = cfg.stop_args;
        current.stop_timeout_sec = cfg.stop_timeout_sec;
    }

    /// Returns a snapshot of the monitor's current config.
    pub fn config(&self) -> App {
        self.shared.snapshot_config()
    }

    /// Returns a snapshot of the monitor's current status.
    pub fn status(&self) -> MonitorStatus {
        self.shared.state.lock().unwrap().clone()
    }
}

/// The monitor thread body: a restart loop around a single process.
fn monitor_loop(shared: Arc<MonitorShared>) {
    let mut restart_count: i64 = 0;

    loop {
        if shared.stop.load(Ordering::SeqCst) {
            shared.emit(AppState::Stopped, 0, restart_count);
            return;
        }

        let cfg = shared.snapshot_config();
        shared.emit(AppState::Starting, 0, restart_count);
        let start_time = Instant::now();

        let spec = match LaunchSpec::from_app(&cfg) {
            Ok(spec) => spec,
            Err(e) => {
                if handle_start_failure(&shared, &cfg, &mut restart_count, &e) {
                    return;
                }
                continue;
            }
        };

        let child = match process::spawn_with_job_object(&spec) {
            Ok(child) => child,
            Err(e) => {
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
            shared.emit(AppState::Stopped, 0, restart_count);
            return;
        }

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
    shared: &MonitorShared,
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

        let monitor = Monitor::new(cfg, on_state, Arc::new(Notifier::new()));
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

        let monitor = Monitor::new(cfg, on_state, Arc::new(Notifier::new()));
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
}
