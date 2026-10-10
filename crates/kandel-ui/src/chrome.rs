//! Windows 11 window chrome adjustments.
//!
//! Windows 11 (notably 24H2) draws a thin accent border around *every* window,
//! including ones created with `decorations: false`. For a fully custom,
//! rounded window this border is unwanted. It can be suppressed with
//! `DwmSetWindowAttribute` + `DWMWA_BORDER_COLOR` = `DWMWA_COLOR_NONE`, and the
//! corner rounding can be opted into explicitly.
//!
//! Both calls are best-effort: on older Windows versions the attributes are
//! unsupported and return an error, which is harmless and ignored.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::WebviewWindow;

/// The smallest usable window, matching `tauri.conf.json`'s `minInnerSize`.
const MIN_W: u32 = 520;
const MIN_H: u32 = 360;

/// Registers the app's `AppUserModelID` for this process (Windows only).
///
/// Tauri does not set an explicit AppUserModelID, so without this the shell
/// labels notifications, the taskbar button, and pinned shortcuts with the
/// executable file name (`kandel-ui.exe`) instead of the product name. Setting
/// it process-wide *before* any window is created fixes that.
///
/// Best-effort: if it fails the shell simply falls back to the exe name, which
/// is not worth failing startup over.
#[cfg(windows)]
pub fn set_app_user_model_id() {
    use kandel_config::APP_USER_MODEL_ID;
    use windows::core::PCWSTR;

    let id: Vec<u16> = APP_USER_MODEL_ID
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr()))
    };
    if let Err(e) = result {
        eprintln!(
            "kandel: could not set the AppUserModelID ({APP_USER_MODEL_ID}): {e}; \
             notifications may show the executable name"
        );
    }
}

/// Registers the app's `AppUserModelID` (no-op on non-Windows targets).
#[cfg(not(windows))]
pub fn set_app_user_model_id() {}

/// Clamps a restored window size into a usable range for a monitor of
/// `screen` pixels.
///
/// Extracted so the policy is testable without a live window.
pub fn clamp_size(size: (u32, u32), screen: (u32, u32)) -> (u32, u32) {
    let max_w = screen.0.max(MIN_W);
    let max_h = screen.1.max(MIN_H);
    (size.0.clamp(MIN_W, max_w), size.1.clamp(MIN_H, max_h))
}

/// Keeps a restored window usable.
///
/// The window-state plugin restores whatever geometry is on disk. A missing or
/// corrupt state file (or a saved size from a monitor that is no longer
/// connected) can otherwise produce a window taller or wider than any monitor,
/// or smaller than the minimum. This clamps the size into a sane range and
/// re-centres the window when the restored position is off-screen.
///
/// Idempotent: a size already in range is left untouched, so it is safe to call
/// from a resize handler (it reaches a fixpoint after one adjustment).
pub fn sanitize_window_geometry(window: &WebviewWindow) {
    let Ok(monitor) = window.current_monitor() else {
        return;
    };
    let Some(monitor) = monitor else {
        return;
    };
    let screen = monitor.size();

    let Ok(size) = window.inner_size() else {
        return;
    };

    let (w, h) = clamp_size((size.width, size.height), (screen.width, screen.height));

    if w != size.width || h != size.height {
        let _ = window.set_size(tauri::PhysicalSize::new(w, h));
    }

    // If the window ended up off-screen, re-centre it.
    if let (Ok(pos), Ok(outer)) = (window.outer_position(), window.outer_size()) {
        let origin = monitor.position();
        let right = origin.x + screen.width as i32;
        let bottom = origin.y + screen.height as i32;
        let outside = pos.x + outer.width as i32 <= origin.x
            || pos.y + outer.height as i32 <= origin.y
            || pos.x >= right
            || pos.y >= bottom;
        if outside {
            let _ = window.center();
        }
    }
}

/// Applies DWM chrome tweaks to `window` (Windows only).
///
/// On non-Windows targets this is a no-op.
#[cfg(windows)]
pub fn apply_window_chrome(window: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);

    unsafe {
        // Remove the Windows 11 accent border around the window.
        let border = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&border) as u32,
        );

        // Opt into rounded corners so the DWM shadow follows the rounded shape.
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&corner) as u32,
        );
    }
}

/// Applies DWM chrome tweaks to `window`. No-op on non-Windows targets.
#[cfg(not(windows))]
pub fn apply_window_chrome(_window: &WebviewWindow) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_oversized_geometry_to_the_screen() {
        // The corrupt case observed in practice: a saved height far past the
        // monitor, which previously produced an unusable window.
        assert_eq!(clamp_size((1180, 65526), (1366, 768)), (1180, 768));
    }

    #[test]
    fn raises_too_small_geometry_to_the_minimum() {
        assert_eq!(clamp_size((10, 10), (1920, 1080)), (MIN_W, MIN_H));
    }

    #[test]
    fn leaves_a_usable_size_untouched() {
        assert_eq!(clamp_size((900, 640), (1920, 1080)), (900, 640));
    }

    #[test]
    fn never_exceeds_a_tiny_screen() {
        // A screen smaller than the minimum still yields a usable window.
        assert_eq!(clamp_size((2000, 2000), (400, 300)), (MIN_W, MIN_H));
    }
}
