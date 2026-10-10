//! Native Windows message boxes for surfacing errors to the user.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

use crate::win::to_wide_null;

/// Whether error dialogs are shown. Disabled by tests and non-interactive hosts
/// so a modal box cannot block a supervisor thread.
///
/// Defaults to off in test builds and when `KANDEL_NO_DIALOGS` is set.
static DIALOGS_ENABLED: AtomicBool = AtomicBool::new(!cfg!(test));

/// Enables or disables error dialogs process-wide.
///
/// Interactive builds leave this enabled; tests and headless/service contexts
/// disable it so a failure does not block on an unattended dialog.
pub fn set_dialogs_enabled(enabled: bool) {
    DIALOGS_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Returns whether dialogs are currently enabled.
pub fn dialogs_enabled() -> bool {
    DIALOGS_ENABLED.load(Ordering::SeqCst) && std::env::var_os("KANDEL_NO_DIALOGS").is_none()
}

/// Shows a modal error dialog with the given title and message.
///
/// When dialogs are disabled the message is logged instead.
pub fn show_error_dialog(title: &str, message: &str) {
    if !dialogs_enabled() {
        tracing::error!(
            component = "dialog",
            title,
            message,
            "error dialog suppressed"
        );
        return;
    }

    let title_wide = to_wide_null(title);
    let message_wide = to_wide_null(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message_wide.as_ptr()),
            PCWSTR(title_wide.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_wide_null_terminates_with_zero() {
        let wide = to_wide_null("hi");
        assert_eq!(wide, vec![b'h' as u16, b'i' as u16, 0]);
    }

    #[test]
    fn to_wide_null_handles_empty() {
        assert_eq!(to_wide_null(""), vec![0]);
    }

    #[test]
    fn dialogs_can_be_disabled() {
        // Save and restore the previous value so parallel tests are unaffected.
        let previous = DIALOGS_ENABLED.load(Ordering::SeqCst);
        set_dialogs_enabled(false);
        assert!(!dialogs_enabled());
        // Suppressed dialogs must return immediately.
        show_error_dialog("test", "should not block");
        set_dialogs_enabled(previous);
    }

    #[test]
    fn dialogs_default_off_in_test_builds() {
        // Guards against a future change re-enabling modal dialogs in tests,
        // which would hang the suite on an unattended message box.
        assert!(!dialogs_enabled());
    }
}
