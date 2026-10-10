//! Windows shell identity: AppUserModelID registration.
//!
//! An AppUserModelID is what lets the shell treat a process as a named
//! application rather than as just an executable: it groups the taskbar button,
//! keys the per-app notification settings, and is what a pinned shortcut points
//! at. Without an explicit id Windows derives one from the executable's **file
//! name**, so the app shows up as "kandel.exe".
//!
//! Registering the id alone is not enough for the shell to know what to *call*
//! the app: it also needs a display name for that id. A packaged (MSIX) app gets
//! it from its manifest and an installed desktop app from its Start Menu
//! shortcut; Kandel is neither, being a portable desktop binary. The documented
//! per-user location for that metadata on unpackaged apps is
//! `HKCU\Software\Classes\AppUserModelId\<id>`, which needs no elevation.
//!
//! Note that the tray balloon's *header line* is a separate matter: the shell
//! takes that from the executable's VERSIONINFO `FileDescription`, which
//! `crates/kandel-core/build.rs` embeds. This module covers the AUMID side.
//!
//! Both operations are best-effort: if either fails the shell falls back to the
//! executable name, which is cosmetic and not worth failing startup over.

use anyhow::{Context, Result};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use kandel_config::APP_USER_MODEL_ID;

use crate::win::to_wide_null;

/// Registry root (relative to `HKEY_CURRENT_USER`) holding per-AUMID metadata.
pub const AUMID_KEY_ROOT: &str = r"Software\Classes\AppUserModelId";

/// The name the shell shows for this app's notifications and taskbar entry.
pub const DISPLAY_NAME: &str = "Kandel";

/// Returns the full per-user registry subkey path for `aumid`.
///
/// Pure, so the layout can be asserted without touching the registry.
pub fn aumid_key_path(aumid: &str) -> String {
    format!(r"{AUMID_KEY_ROOT}\{aumid}")
}

/// Registers [`APP_USER_MODEL_ID`] as this process's AppUserModelID.
///
/// Must run before any window or tray icon exists, so the shell keys those on
/// the id rather than on the image path.
pub fn set_app_user_model_id() {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let id = to_wide_null(APP_USER_MODEL_ID);
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr())) };
    if let Err(e) = result {
        tracing::warn!(
            component = "shell",
            app_id = APP_USER_MODEL_ID,
            error = %e,
            "could not set the AppUserModelID; notifications may show the executable name"
        );
    }
}

/// Writes the display name for [`APP_USER_MODEL_ID`] to the per-user registry.
///
/// Idempotent and cheap, so it runs on every startup: the value can be missing
/// after a fresh profile, a registry clean-up, or a portable install moved to a
/// new machine. Returns an error so the caller can log it; the shell falls back
/// to the executable name if the write fails.
pub fn register_app_identity() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(aumid_key_path(APP_USER_MODEL_ID))
        .context("creating the AppUserModelId registry key")?;
    key.set_value("DisplayName", &DISPLAY_NAME)
        .context("writing the AppUserModelId DisplayName")?;
    Ok(())
}

/// Sets the process id and registers its display name in one step.
///
/// Best-effort wrapper around [`set_app_user_model_id`] and
/// [`register_app_identity`]: a registry failure is logged, never fatal.
pub fn init() {
    set_app_user_model_id();
    if let Err(e) = register_app_identity() {
        tracing::warn!(
            component = "shell",
            app_id = APP_USER_MODEL_ID,
            error = %e,
            "could not register the app display name; notifications may show the executable name"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_path_joins_the_root_and_id() {
        assert_eq!(
            aumid_key_path("dev.tojoo.kandel"),
            r"Software\Classes\AppUserModelId\dev.tojoo.kandel"
        );
    }

    #[test]
    fn key_path_uses_the_shared_app_id() {
        // The registered path must track the constant, so a re-brand changes
        // both the process id and the registry key together.
        assert!(aumid_key_path(APP_USER_MODEL_ID).ends_with(APP_USER_MODEL_ID));
    }

    #[test]
    fn display_name_is_the_product_name() {
        assert_eq!(DISPLAY_NAME, "Kandel");
    }
}
