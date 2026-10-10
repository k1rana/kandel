//! `wait_for_console_event` blocks until it receives an OS interrupt/break
//! event, then reports and exits.
//!
//! Use it to verify Kandel's automatic `CTRL_BREAK_EVENT` graceful stop: run
//! this program under Kandel and trigger a stop; it should print the event
//! rather than being forcefully terminated.
//!
//! Pass `--ready-file <path>` to have it create that file once its control
//! handler is installed. An automated test polls for the file instead of
//! sleeping, because an event delivered before the handler is armed would kill
//! the process by default and make the test race.
//!
//! Exit codes: 0 after handling the event. A forced stop records 1, so the two
//! paths stay distinguishable from the outside.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "console")]

fn main() {
    println!("Waiting for a console control event (Ctrl+Break / CTRL_BREAK_EVENT)...");
    wait_for_break();
    println!("Received a console control event; exiting.");
}

/// Sends the control-event code from the handler thread to the main thread.
///
/// A `static` rather than a `thread_local!`: Windows invokes the control
/// handler on its *own* thread, so a thread-local set up by `main` would be
/// empty there and the event would never be forwarded.
#[cfg(windows)]
static CTRL_SENDER: std::sync::Mutex<Option<std::sync::mpsc::Sender<u32>>> =
    std::sync::Mutex::new(None);

#[cfg(windows)]
fn wait_for_break() {
    use windows::Win32::System::Console::SetConsoleCtrlHandler;

    // Our handler runs on a separate thread; signal the main thread through a
    // channel and report which event arrived.
    let (tx, rx) = std::sync::mpsc::channel::<u32>();

    unsafe extern "system" fn handler(ctrl_type: u32) -> windows::core::BOOL {
        // 0 = CTRL_C, 1 = CTRL_BREAK. Return TRUE to signal we handled it.
        if let Ok(slot) = CTRL_SENDER.lock() {
            if let Some(tx) = slot.as_ref() {
                let _ = tx.send(ctrl_type);
            }
        }
        windows::core::BOOL(1)
    }

    *CTRL_SENDER
        .lock()
        .expect("the handler mutex is never poisoned here") = Some(tx);

    unsafe {
        let _ = SetConsoleCtrlHandler(Some(handler), true);
    }

    // Signal readiness only after the handler is armed, so a caller that waits
    // on this file cannot deliver the event too early.
    if let Some(path) = ready_file_arg() {
        let _ = std::fs::write(&path, b"ready");
    }

    match rx.recv() {
        Ok(ctrl_type) => println!("control event code: {ctrl_type}"),
        Err(_) => println!("no event received"),
    }
}

/// Returns the `--ready-file <path>` value, if the flag was supplied.
#[cfg(windows)]
fn ready_file_arg() -> Option<std::path::PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--ready-file" {
            return args.next().map(std::path::PathBuf::from);
        }
    }
    None
}

#[cfg(not(windows))]
fn wait_for_break() {
    eprintln!("This helper only supports Windows.");
}
