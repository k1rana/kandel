//! UI entry point. Released builds use the Windows GUI subsystem.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use kandel_config::RuntimeFlags;

fn main() {
    // Must run before Tauri creates any window so the shell associates the
    // window, taskbar button, and notifications with the product name rather
    // than the executable's file name (`kandel-ui.exe`).
    kandel_ui_lib::chrome::set_app_user_model_id();

    let flags = RuntimeFlags::parse(std::env::args().skip(1)).unwrap_or_else(|e| {
        eprintln!("invalid arguments: {e}");
        std::process::exit(2);
    });

    kandel_ui_lib::run(flags.config_path);
}
