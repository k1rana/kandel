//! Kandel management UI (Tauri).
//!
//! The UI is a disposable process launched on demand by the core. It reads and
//! writes `config.toml` directly (the single source of truth) and receives
//! real-time monitor status from the core over stdin as line-delimited JSON.

pub mod chrome;
pub mod commands;
pub mod ipc;

use std::path::PathBuf;

use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

/// Runs the Tauri application. `config_path` is the path to `config.toml`.
///
/// `ipc_mode` enables the stdin status listener and the "exit on core exit"
/// behavior. It is set by the core when it launches the UI as a child process,
/// and unset for standalone runs such as `tauri dev`.
pub fn run(config_path: PathBuf, ipc_mode: bool) {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Persist and restore the window size/position/maximized state between
        // launches. `VISIBLE` lets the plugin show the (initially hidden)
        // window only after the restored geometry is applied, so there is no
        // visible jump or flash on startup.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    StateFlags::SIZE
                        | StateFlags::POSITION
                        | StateFlags::MAXIMIZED
                        | StateFlags::VISIBLE,
                )
                .build(),
        )
        .manage(commands::UiState::new(config_path))
        .manage(ipc::StatusCache::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_about_info,
            commands::open_repository,
            commands::get_full_config,
            commands::update_app_config,
            commands::delete_app_config,
            commands::get_command_extensions,
            commands::select_command_path,
            ipc::get_initial_statuses,
            ipc::get_app_logs,
        ])
        .setup(move |app| {
            // Tweak the DWM chrome (remove the Windows 11 accent border, opt
            // into rounded corners) and make sure a restored state file cannot
            // produce an unusable window. The window state plugin restores
            // geometry and shows the window later, in `on_window_ready`.
            if let Some(window) = app.get_webview_window("main") {
                chrome::apply_window_chrome(&window);
                chrome::sanitize_window_geometry(&window);

                // The window-state plugin restores geometry in its own
                // `on_window_ready`, which runs after this setup hook. Re-run
                // the guard on resize so a corrupt or off-monitor restore is
                // corrected. The clamp is a fixpoint, so this cannot loop.
                let guarded = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Resized(_) = event {
                        chrome::sanitize_window_geometry(&guarded);
                    }
                });
            }

            // In IPC mode, forward core status updates (stdin) to the frontend
            // as events. Outside IPC mode stdin is not a pipe from the core, so
            // reading it would see EOF immediately and close the window.
            if ipc_mode {
                ipc::spawn_stdin_listener(app.handle().clone());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Kandel UI");
}
