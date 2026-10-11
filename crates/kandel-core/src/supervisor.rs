//! The stateful supervisor: owns the set of [`Monitor`]s and reconciles them
//! against the desired state from `config.toml`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use kandel_config::App;

use crate::logs::LogStore;
use crate::monitor::{Monitor, StateCallback};
use crate::notify::Notifier;
use crate::reconciler::{decide, Action};
use crate::status::{AppState, MonitorStatus};

/// Owns all monitors and applies reconciliation between desired and actual state.
pub struct Supervisor {
    monitors: Mutex<BTreeMap<String, Monitor>>,
    on_state: StateCallback,
    notifier: Arc<Notifier>,
    /// Restart-surviving output logs, shared with every monitor.
    logs: Arc<LogStore>,
}

impl Supervisor {
    /// Creates an empty supervisor reporting state through `on_state`.
    pub fn new(on_state: StateCallback, notifier: Arc<Notifier>, logs: Arc<LogStore>) -> Self {
        Supervisor {
            monitors: Mutex::new(BTreeMap::new()),
            on_state,
            notifier,
            logs,
        }
    }

    /// Builds a monitor for `name`, wired to the shared log store.
    fn make_monitor(&self, name: &str, cfg: &App) -> Monitor {
        Monitor::new(
            name.to_string(),
            cfg.clone(),
            Arc::clone(&self.on_state),
            Arc::clone(&self.notifier),
            Arc::clone(&self.logs),
        )
    }

    /// Reconciles the running monitors to match `desired`.
    ///
    /// Implements the algorithm from the design doc: start new entries, stop
    /// removed or disabled ones, restart on identity changes, hot-reload on
    /// monitoring-parameter changes, and no-op when converged. Removed and
    /// disabled entries have their processes terminated so actual state
    /// converges to what the config declares.
    pub fn reconcile(&self, desired: BTreeMap<String, App>) {
        let mut monitors = self.monitors.lock().unwrap();

        // Process desired entries.
        for (name, desired_cfg) in &desired {
            match monitors.get(name) {
                None => {
                    self.add_entry(&mut monitors, name, desired_cfg);
                }
                Some(existing) => {
                    let current_cfg = existing.config();
                    match decide(&current_cfg, desired_cfg) {
                        Action::Stop => {
                            tracing::info!(component = "reconciler", action = "STOP", app = %name, reason = "disabled");
                            existing.set_config(desired_cfg.clone());
                            existing.stop();
                        }
                        Action::Start => {
                            existing.stop();
                            let monitor = self.make_monitor(name, desired_cfg);
                            monitor.start();
                            monitors.insert(name.clone(), monitor);
                        }
                        Action::Restart => {
                            tracing::info!(component = "reconciler", action = "RESTART", app = %name, reason = "identity changed");
                            existing.set_config(desired_cfg.clone());
                            existing.stop();
                            let monitor = self.make_monitor(name, desired_cfg);
                            monitor.start();
                            monitors.insert(name.clone(), monitor);
                        }
                        Action::HotReload => {
                            existing.update_monitoring_params(desired_cfg.clone());
                        }
                        Action::NoOp | Action::TrackDisabled => {}
                    }
                }
            }
        }

        // Removed entries: present in current but not desired.
        let removed: Vec<String> = monitors
            .keys()
            .filter(|name| !desired.contains_key(*name))
            .cloned()
            .collect();
        for name in removed {
            if let Some(monitor) = monitors.remove(&name) {
                tracing::info!(component = "reconciler", action = "STOP", app = %name, reason = "removed from config");
                monitor.stop();
                self.logs.remove(&name);
                let mut status = monitor.status();
                status.state = AppState::Removed;
                (self.on_state)(status);
            }
        }
    }

    fn add_entry(&self, monitors: &mut BTreeMap<String, Monitor>, name: &str, cfg: &App) {
        if cfg.enabled {
            tracing::info!(component = "reconciler", action = "START", app = %name, reason = "new entry");
            let monitor = self.make_monitor(name, cfg);
            monitor.start();
            monitors.insert(name.to_string(), monitor);
        } else {
            tracing::info!(component = "reconciler", action = "TRACK", app = %name, reason = "new entry, disabled");
            let monitor = self.make_monitor(name, cfg);
            (self.on_state)(monitor.status());
            monitors.insert(name.to_string(), monitor);
        }
    }

    /// Stops every monitor. Called when the core process is exiting.
    pub fn stop_all(&self) {
        let monitors: Vec<Monitor> = {
            let mut guard = self.monitors.lock().unwrap();
            std::mem::take(&mut *guard).into_values().collect()
        };

        let mut handles = Vec::with_capacity(monitors.len());
        for monitor in monitors {
            handles.push(std::thread::spawn(move || {
                monitor.stop();
            }));
        }
        for handle in handles {
            let _ = handle.join();
        }
        tracing::info!(component = "reconciler", "all monitors stopped");
    }

    /// Returns all monitor statuses sorted by name.
    pub fn all_statuses(&self) -> Vec<MonitorStatus> {
        let monitors = self.monitors.lock().unwrap();
        monitors.values().map(Monitor::status).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};
    use std::time::{Duration, Instant};

    fn collecting_callback() -> (StateCallback, Receiver<MonitorStatus>) {
        let (tx, rx) = channel();
        let cb: StateCallback = Arc::new(move |status| {
            let _ = tx.send(status);
        });
        (cb, rx)
    }

    fn disabled_app(name: &str) -> App {
        App {
            name: name.into(),
            command: "cmd.exe".into(),
            enabled: false,
            ..App::default()
        }
    }

    #[test]
    fn adding_disabled_entry_tracks_without_running() {
        let (cb, rx) = collecting_callback();
        let supervisor =
            Supervisor::new(cb, Arc::new(Notifier::new()), Arc::new(LogStore::default()));

        let mut desired = BTreeMap::new();
        desired.insert("paused".to_string(), disabled_app("paused"));
        supervisor.reconcile(desired);

        let status = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(status.name, "paused");
        assert_eq!(status.state, AppState::Stopped);
        assert_eq!(supervisor.all_statuses().len(), 1);
    }

    #[test]
    fn removing_entry_emits_removed_state() {
        let (cb, rx) = collecting_callback();
        let supervisor =
            Supervisor::new(cb, Arc::new(Notifier::new()), Arc::new(LogStore::default()));

        let mut desired = BTreeMap::new();
        desired.insert("paused".to_string(), disabled_app("paused"));
        supervisor.reconcile(desired);
        let _ = rx.recv_timeout(Duration::from_secs(2));

        supervisor.reconcile(BTreeMap::new());

        let mut saw_removed = false;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(status) = rx.recv_timeout(Duration::from_millis(500)) {
                if status.state == AppState::Removed {
                    saw_removed = true;
                    break;
                }
            } else {
                break;
            }
        }
        assert!(saw_removed, "expected a Removed status");
        assert!(supervisor.all_statuses().is_empty());
    }

    #[test]
    fn stop_all_clears_monitors() {
        let (cb, _rx) = collecting_callback();
        let supervisor =
            Supervisor::new(cb, Arc::new(Notifier::new()), Arc::new(LogStore::default()));
        let mut desired = BTreeMap::new();
        desired.insert("paused".to_string(), disabled_app("paused"));
        supervisor.reconcile(desired);
        supervisor.stop_all();
        assert!(supervisor.all_statuses().is_empty());
    }
}
