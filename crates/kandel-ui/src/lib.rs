//! Kandel management UI (Tauri).
//!
//! The UI is a disposable process: it reads and writes `config.toml` directly
//! (the single source of truth) and receives real-time monitor status from the
//! core by attaching to the core's named pipe. Attaching (rather than being
//! handed a stream by whoever launched it) is what lets the UI be opened
//! directly and reconnect across a core restart.

pub mod chrome;
pub mod commands;
pub mod ipc;
pub mod pipe;

use std::path::PathBuf;

use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

/// Runs the Tauri application. `config_path` is the path to `config.toml`.
///
/// The UI always connects to the core's status pipe, whether the core launched
/// it or the user opened it directly: the pipe is reconnectable, so both paths
/// give identical behaviour. See [`ipc::spawn_reader`].
pub fn run(config_path: PathBuf) {
    // Shared with the reader thread, and readable by the frontend, so a window
    // that mounted after the core attached still learns it is connected. Managed
    // by value: Tauri resolves state by exact type, so `State<'_, ConnectionState>`
    // only resolves if this exact type was registered. Cloning is cheap (the flag
    // is behind an `Arc` inside).
    let connection = ipc::ConnectionState::default();
    let connection_for_reader = connection.clone();

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
        .manage(connection)
        .invoke_handler(tauri::generate_handler![
            commands::get_about_info,
            commands::open_repository,
            commands::get_full_config,
            commands::update_app_config,
            commands::delete_app_config,
            commands::repair_config,
            commands::get_command_extensions,
            commands::select_command_path,
            ipc::get_initial_statuses,
            ipc::get_app_logs,
            ipc::get_connection_state,
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

            // Stream status from the core over its named pipe. The reader
            // reconnects on its own, so a UI opened before the core is ready —
            // or one that outlives a core restart — still converges to live
            // state instead of freezing on stale rows.
            ipc::spawn_reader(app.handle().clone(), connection_for_reader.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Kandel UI");
}
