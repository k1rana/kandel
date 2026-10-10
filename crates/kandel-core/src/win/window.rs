//! Top-level window enumeration and `WM_CLOSE` delivery.
//!
//! The automatic stop path posts `WM_CLOSE` to a monitored process's non-console
//! top-level windows. Console hosts (`ConsoleWindowClass`, `PseudoConsoleWindow`)
//! are deliberately excluded so closing the target does not terminate a console
//! host unexpectedly.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

/// The classic console host window class.
pub const CONSOLE_WINDOW_CLASS: &str = "ConsoleWindowClass";
/// The ConPTY pseudo-console window class (Windows 10 1809+).
pub const PSEUDO_CONSOLE_WINDOW_CLASS: &str = "PseudoConsoleWindow";

/// Returns true when a window class is a valid target for a graceful
/// `WM_CLOSE` stop. Console host classes are excluded.
pub fn is_window_stop_target_class(class_name: &str) -> bool {
    !matches!(
        class_name,
        CONSOLE_WINDOW_CLASS | PSEUDO_CONSOLE_WINDOW_CLASS
    )
}

/// Mutable state threaded through `EnumWindows` via its `lParam`.
///
/// Carrying the state through `lParam` (rather than a `thread_local!`) keeps the
/// enumeration self-contained: the accumulator cannot be confused with another
/// declaration of the same name, which is a bug this code previously had.
struct EnumState {
    pid: u32,
    found: Vec<HWND>,
}

/// Returns the top-level window handles owned by `pid` that are valid
/// `WM_CLOSE` targets.
pub fn top_level_windows_for_pid(pid: u32) -> Vec<HWND> {
    let mut state = EnumState {
        pid,
        found: Vec::new(),
    };

    unsafe {
        // `EnumWindows` is synchronous and calls back on this thread, so `state`
        // is alive for the whole enumeration.
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut state as *mut EnumState as isize),
        );
    }

    state.found
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the pointer to the `EnumState` created by
    // `top_level_windows_for_pid`. That borrow outlives this synchronous call.
    let state = unsafe { &mut *(lparam.0 as *mut EnumState) };

    let mut window_pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
    }

    if window_pid != state.pid {
        return BOOL(1);
    }

    // A console host with an unreadable class is never a valid target, so only
    // record windows whose class we can read and that are not console hosts.
    if let Ok(class_name) = window_class_name(hwnd) {
        if is_window_stop_target_class(&class_name) {
            state.found.push(hwnd);
        }
    }

    BOOL(1)
}

fn window_class_name(hwnd: HWND) -> Result<String, windows::core::Error> {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(String::from_utf16_lossy(&buf[..len as usize]))
}

/// Posts `WM_CLOSE` to every eligible top-level window owned by `pid`.
///
/// Returns the number of windows the message was posted to.
pub fn post_close_to_process_windows(pid: u32) -> usize {
    let windows = top_level_windows_for_pid(pid);
    let mut posted = 0;
    for hwnd in &windows {
        let ok = unsafe { PostMessageW(Some(*hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        if ok.is_ok() {
            posted += 1;
        }
    }
    posted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_window_classes_are_excluded() {
        assert!(!is_window_stop_target_class(CONSOLE_WINDOW_CLASS));
        assert!(!is_window_stop_target_class(PSEUDO_CONSOLE_WINDOW_CLASS));
    }

    #[test]
    fn application_window_class_is_included() {
        assert!(is_window_stop_target_class("AlertOnWMCloseWindow"));
        assert!(is_window_stop_target_class("Notepad"));
    }

    #[test]
    fn unknown_pid_has_no_windows() {
        // PID 0xFFFF_FFFF is not a real process; enumeration should find nothing.
        assert!(top_level_windows_for_pid(u32::MAX).is_empty());
    }

    #[test]
    fn finds_a_window_owned_by_a_live_process() {
        // Regression guard: the accumulator used to be a second `thread_local!`
        // shadowing the outer one, so this always returned an empty vec and the
        // whole graceful `WM_CLOSE` path was dead. Build a real window in this
        // process and assert it is found.
        let hwnd = create_probe_window();
        let pid = std::process::id();

        let found = top_level_windows_for_pid(pid);
        assert!(
            found.contains(&hwnd),
            "the process's own top-level window should be enumerated"
        );

        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
        }
    }

    /// Creates a hidden top-level window owned by this process for the test
    /// above. Returns its handle; the caller destroys it.
    fn create_probe_window() -> HWND {
        use windows::core::w;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassExW, WINDOW_EX_STYLE, WNDCLASSEXW,
            WS_OVERLAPPED,
        };

        const CLASS: windows::core::PCWSTR = w!("KandelTopLevelWindowTest");

        unsafe extern "system" fn proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> windows::Win32::Foundation::LRESULT {
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        let instance = unsafe { GetModuleHandleW(None) }.expect("GetModuleHandleW");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        unsafe {
            let _ = RegisterClassExW(&class);
        }

        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                w!("probe"),
                WS_OVERLAPPED,
                0,
                0,
                100,
                100,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .expect("CreateWindowExW")
    }
}
