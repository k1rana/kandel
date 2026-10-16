//! Core application wiring: bootstrap, single-instance guard, reconciliation,
//! config watching, and the tray loop.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

use kandel_config::{
    instance_mutex_name, load_config, save_config, App as AppConfig, RuntimeFlags, DEFAULT_CONFIG,
};

use crate::autostart::AutoStartManager;
use crate::dialog::show_error_dialog;
use crate::ipc_server::{self, StatusServer, UiLauncher};
use crate::logs::LogStore;
use crate::notify::Notifier;
use crate::openwith::open_with_dialog;
use crate::status::MonitorStatus;
use crate::supervisor::Supervisor;
use crate::tray::{self, TrayCallbacks};
use crate::watcher;
use crate::win::to_wide_null;

/// Runs the core process. Blocks until the user quits.
pub fn run(flags: RuntimeFlags, version: &str) -> Result<()> {
    // Single-instance guard: a second launch in the same session exits early.
    let _mutex = acquire_single_instance()?;

    let config_path = bootstrap_config(&flags.config_path)?;

    let auto_start = AutoStartManager::new(flags.clone()).ok();

    // Initial (strict) load: an invalid config at startup is fatal.
    let apps = match load_config(&config_path) {
        Ok(apps) => apps,
        Err(e) => {
            show_error_dialog(
                "Kandel - Configuration Error",
                &format!("Failed to load config.toml:\n\n{e}"),
            );
            anyhow::bail!("failed to load config: {e}");
        }
    };

    let logs = Arc::new(LogStore::new(crate::logs::DEFAULT_LOG_CAP));

    // The status server and the supervisor need each other: the supervisor
    // forwards events, and the server's snapshot reads the supervisor. Create a
    // shared slot first, fill it once both exist, and let early events drop
    // harmlessly until then (the store still retains them, and the first client
    // gets them in the snapshot).
    let server_slot: Arc<Mutex<Option<StatusServer>>> = Arc::new(Mutex::new(None));
    let slot_for_state = Arc::clone(&server_slot);
    let on_state = Arc::new(move |status: MonitorStatus| {
        if let Some(server) = slot_for_state.lock().unwrap().as_ref() {
            server.send(&status);
        }
    });

    // Notices raised before the tray exists are queued here and flushed once it
    // does (reconciliation runs first, so an app that fails immediately would
    // otherwise have nowhere to notify).
    let notifier = Arc::new(Notifier::new());

    let supervisor = Arc::new(Supervisor::new(
        on_state,
        Arc::clone(&notifier),
        Arc::clone(&logs),
    ));
    supervisor.reconcile(apps);
    tracing::info!(
        component = "main",
        entries = supervisor.all_statuses().len(),
        "initial reconciliation complete"
    );

    // Publish the status pipe. Every attach (including a reconnect) replays the
    // current statuses and log backlog, so a UI that opens late is never empty.
    let supervisor_for_snapshot = Arc::clone(&supervisor);
    let logs_for_snapshot = Arc::clone(&logs);
    let server = ipc_server::serve(move || {
        ipc_server::snapshot_events(
            &supervisor_for_snapshot.all_statuses(),
            &logs_for_snapshot.snapshot(),
        )
    });
    *server_slot.lock().unwrap() = Some(server.clone());

    // Captured output is retained in the store regardless; the sink forwards it
    // to whichever UI is attached.
    let server_for_logs = server.clone();
    logs.set_sink(Box::new(move |name, event| {
        server_for_logs.send_log(name, event);
    }));

    let ui = UiLauncher::new();

    // Watch config.toml for changes and reconcile.
    spawn_watcher(config_path.clone(), Arc::clone(&supervisor));

    // Open the UI at startup when `-ui` was passed, so development does not
    // depend on reaching the tray icon.
    if flags.open_ui {
        if let Err(e) = ui.show(&flags) {
            tracing::error!(component = "main", error = %e, "failed to open the UI at startup");
        }
    }

    // Tray loop (blocks until quit).
    let supervisor_for_quit = Arc::clone(&supervisor);
    let ui_for_open = ui.clone();
    let ui_for_quit = ui.clone();
    let flags_for_open = flags.clone();
    let config_path_for_config = config_path.clone();

    let result = tray::run(
        version,
        auto_start.as_ref(),
        notifier,
        TrayCallbacks {
            on_open_ui: Box::new(move || {
                // The window attaches to the status pipe on its own and receives
                // the snapshot there, so opening is just launching the process.
                if let Err(e) = ui_for_open.show(&flags_for_open) {
                    tracing::error!(component = "main", error = %e, "failed to show UI");
                }
            }),
            on_open_config_with: Box::new(move || {
                if let Err(e) = open_with_dialog(&config_path_for_config) {
                    tracing::error!(component = "main", path = %config_path_for_config.display(), error = %e, "failed to open config with dialog");
                    show_error_dialog(
                        "Kandel - Error",
                        &format!("Failed to open the 'Open with' dialog for config.toml:\n\n{e}"),
                    );
                }
            }),
            on_quit: Box::new(move || {
                tracing::info!(component = "main", "quit requested, stopping all monitors");
                // Close the window first: stopping monitors can take seconds per
                // app (graceful stop, then a forced fallback), and a hung window
                // during that wait reads as a frozen quit. It is also killed
                // again, harmlessly, by the shutdown below the tray loop.
                ui_for_quit.shutdown();
                supervisor_for_quit.stop_all();
            }),
        },
    );

    ui.shutdown();
    result
}

/// Acquires the session-local single-instance mutex.
///
/// Returns the handle (kept alive for the process lifetime) or an error if
/// another instance already holds it. The handle is leaked intentionally so the
/// mutex lives as long as the process.
fn acquire_single_instance() -> Result<HANDLE> {
    let name = to_wide_null(&instance_mutex_name());
    let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) };
    match handle {
        Ok(h) => {
            // CreateMutexW succeeds even when the mutex already exists; the
            // distinguishing signal is ERROR_ALREADY_EXISTS from GetLastError.
            let last = unsafe { windows::Win32::Foundation::GetLastError() };
            if last == ERROR_ALREADY_EXISTS {
                unsafe {
                    let _ = CloseHandle(h);
                }
                show_error_dialog(
                    "Kandel",
                    "Another copy of Kandel is already running.\n\nOnly one core instance can run at a time.",
                );
                anyhow::bail!("another instance is already running");
            }
            // HANDLE has no Drop, so simply returning it keeps the mutex owned
            // until the caller drops it at process exit.
            Ok(h)
        }
        Err(e) => {
            show_error_dialog(
                "Kandel - Error",
                &format!("Failed to create single-instance lock:\n\n{e}"),
            );
            Err(anyhow::anyhow!("{e}"))
        }
    }
}

/// Ensures the config directory exists, writing the sample config on first run.
fn bootstrap_config(config_path: &Path) -> Result<PathBuf> {
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating config directory {}", dir.display()))?;
    }

    if !config_path.exists() {
        std::fs::write(config_path, DEFAULT_CONFIG)
            .with_context(|| format!("writing default config {}", config_path.display()))?;
        tracing::info!(component = "main", path = %config_path.display(), "created default config.toml");
    }

    Ok(config_path.to_path_buf())
}

/// Spawns the config watcher thread that reloads and reconciles on change.
fn spawn_watcher(config_path: PathBuf, supervisor: Arc<Supervisor>) {
    let path_for_watch = config_path.clone();
    let result = watcher::spawn_config_watcher(config_path, move || {
        reload_and_reconcile(&path_for_watch, &supervisor);
    });
    if let Err(e) = result {
        tracing::error!(component = "watcher", error = %e, "failed to start config watcher");
    }
}

/// Reloads the config, keeping current state on failure.
fn reload_and_reconcile(config_path: &Path, supervisor: &Supervisor) {
    match watcher::load_with_retry(config_path) {
        Ok(apps) => {
            tracing::info!(
                component = "reload",
                entries = apps.len(),
                "config reloaded, reconciling"
            );
            supervisor.reconcile(apps);
        }
        Err(e) => {
            tracing::error!(component = "reload", path = %config_path.display(), error = %e, "config reload failed after retries, keeping current state");
            show_error_dialog(
                "Kandel - Configuration Error",
                &format!(
                    "Failed to reload config.toml. The previous configuration remains active.\n\n{e}"
                ),
            );
        }
    }
}

/// Saves an app config, used by tests and future embedding. Exposed for parity
/// with the UI bridge.
pub fn save_apps(path: &Path, apps: &BTreeMap<String, AppConfig>) -> Result<()> {
    save_config(path, apps).map_err(|e| anyhow::anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_creates_default_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        let out = bootstrap_config(&path).unwrap();
        assert_eq!(out, path);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("Welcome to Kandel"));
    }

    #[test]
    fn bootstrap_preserves_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[\"keep\"]\ncommand = \"cmd.exe\"\n").unwrap();
        bootstrap_config(&path).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("keep"));
    }
}
