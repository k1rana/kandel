//! `alert_on_wm_close` blocks in a GUI message loop until it receives
//! `WM_CLOSE`, then reports by exiting.
//!
//! Two ways to use it, and the mode is chosen by argument:
//!
//! * **Manual** (default) — run it under Kandel and trigger a stop; an alert
//!   dialog appears, proving a `WM_CLOSE` was posted rather than the process
//!   being killed.
//! * **Automated** (`--auto`) — no dialog; the window stays hidden and the
//!   process exits with code 0 on `WM_CLOSE`. `tests/stop_behavior.rs` uses
//!   this to assert the graceful path without a human in the loop.
//!
//! Exit codes: 0 when `WM_CLOSE` was received. A forced stop records 1, and
//! anything else means the message never arrived, so the two paths stay
//! distinguishable from the outside.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MessageBoxW,
        PostQuitMessage, RegisterClassExW, ShowWindow, TranslateMessage, CW_USEDEFAULT,
        MB_ICONINFORMATION, MB_OK, MSG, SW_SHOW, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY,
        WNDCLASSEXW, WS_CAPTION, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
    };

    const CLASS_NAME: windows::core::PCWSTR = w!("KandelAlertOnWMCloseWindow");

    // In automated mode the window must exist (so `WM_CLOSE` has a target) but
    // never be shown, and closing must not wait on a human.
    let auto = std::env::args().any(|a| a == "--auto");

    // Read by the window procedure; the message loop runs on this thread.
    thread_local! {
        static AUTO: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    AUTO.with(|cell| cell.set(auto));

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CLOSE => {
                if AUTO.with(|cell| cell.get()) {
                    // Headless: report success through the exit code and quit.
                    unsafe { PostQuitMessage(0) };
                } else {
                    unsafe {
                        // When launched with a hidden window, show it so the
                        // dialog is reliably surfaced for manual verification.
                        let _ = ShowWindow(hwnd, SW_SHOW);
                        let _ = MessageBoxW(
                            Some(hwnd),
                            w!("WM_CLOSE received. Close this app?"),
                            w!("Kandel alert_on_wm_close"),
                            MB_OK | MB_ICONINFORMATION,
                        );
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    let instance = unsafe { GetModuleHandleW(None) }.expect("GetModuleHandleW");

    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    assert_ne!(atom, 0, "RegisterClassExW failed");

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let create_style = if auto { style } else { style | WS_VISIBLE };

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            w!("Kandel alert_on_wm_close"),
            create_style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            480,
            160,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .expect("CreateWindowExW");

    unsafe {
        if !auto {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This helper only supports Windows.");
}
