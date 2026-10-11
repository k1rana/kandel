//! Kandel core entry point: a resident tray process that supervises managed
//! applications. Released builds use the Windows GUI subsystem so no console
//! window is shown.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    use kandel_config::RuntimeFlags;
    use kandel_core::{app, dialog, logging, shell};

    // Must run before any window or tray icon exists, so the shell associates
    // them with the app's name instead of the executable's file name. This also
    // registers the display name the notification balloon's header shows.
    shell::init();

    let flags = match RuntimeFlags::parse(std::env::args().skip(1)) {
        Ok(flags) => flags,
        Err(e) => {
            dialog::show_error_dialog("Kandel - Error", &format!("Invalid arguments:\n\n{e}"));
            std::process::exit(2);
        }
    };

    if let Err(e) = logging::configure(&flags) {
        // Fall back to a default logger so the failure is recorded somewhere.
        let _ = logging::configure(&RuntimeFlags::with_config_path(flags.config_path.clone()));
        tracing::error!(component = "main", error = %e, "failed to configure logger");
    }

    tracing::info!(
        component = "main",
        binary = "core",
        version = VERSION,
        pid = std::process::id(),
        "starting Kandel"
    );

    if let Err(e) = app::run(flags, VERSION) {
        tracing::error!(component = "main", error = %e, "core exited with error");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Kandel core only supports Windows.");
    std::process::exit(1);
}

/// Build version, single-sourced from the workspace version in `Cargo.toml`.
/// `KANDEL_VERSION` can still override it at build time (e.g. from a release
/// tag), but no longer falls back to a hard-coded `dev`.
pub const VERSION: &str = match option_env!("KANDEL_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
