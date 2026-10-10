//! Embeds the Windows icon and version resource into `kandel.exe` (the core).
//!
//! Why this exists: the shell labels a tray balloon's header, the taskbar
//! button, and any Explorer tooltip with the executable's VERSIONINFO
//! **`FileDescription`**, falling back to the file name when the resource is
//! absent. Tauri embeds a version resource into `kandel-ui.exe`, but the core
//! is a plain Cargo binary and would otherwise show "kandel.exe". Setting the
//! field here — together with the AppUserModelID registered at startup in
//! `src/shell.rs` — is what makes notifications read "Kandel".
//!
//! The icon is emitted from the same geometry as every other Kandel icon by
//! `scripts/make_icons.py`, so the exe, tray, and in-app mark cannot drift.

fn main() {
    // `build.rs` runs on the host, so `#[cfg(target_os)]` would test the host
    // OS rather than the target. `CARGO_CFG_TARGET_OS` is the target's.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico");

    // The name the shell shows for notifications and in Explorer. Kept in step
    // with `shell::DISPLAY_NAME`.
    res.set("FileDescription", "Kandel");
    res.set("ProductName", "Kandel");
    res.set("OriginalFilename", "kandel.exe");

    if let Err(e) = res.compile() {
        // A missing resource toolchain (rc.exe / windres) is not worth failing
        // a debug build over; the binaries still run, only the shell label
        // falls back to the file name.
        println!("cargo:warning=could not compile the Windows resource: {e}");
    }

    // Rebuild when the icon or this script changes, so the embedded resource
    // never goes stale.
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
}
