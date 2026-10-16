//! Console attach/detach and `CTRL_BREAK_EVENT` delivery.
//!
//! Delivering `CTRL_BREAK_EVENT` to a specific child without side effects on
//! the core or other children requires three coordinated Windows behaviors:
//!
//! 1. The child was spawned with `CREATE_NEW_PROCESS_GROUP`, so its process
//!    group ID equals its own PID.
//! 2. `GenerateConsoleCtrlEvent` is called with `dwProcessGroupId = child PID`
//!    (not 0), confining the event to the child and its descendants.
//! 3. Before sending the event, the core calls `FreeConsole` then
//!    `AttachConsole(child)` — `GenerateConsoleCtrlEvent` requires the caller to
//!    share a console with the target.

use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use windows::Win32::System::Console::{
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler,
};

/// `CTRL_BREAK_EVENT` (value 1). Defined locally to avoid depending on the
/// exact typing of the constant across `windows` crate versions.
const CTRL_BREAK_EVENT: u32 = 1;

/// Serializes console attach/detach across the process.
///
/// A process can only be attached to one console at a time, so concurrent
/// `send_ctrl_break` calls would otherwise race for the shared per-process
/// console state.
static CONSOLE_ATTACH_MU: Mutex<()> = Mutex::new(());

/// Attaches to the target child's console and posts `CTRL_BREAK_EVENT` to the
/// child's process group.
///
/// Returns a cleanup guard. Drop it as soon as the event has been posted: the
/// event is already queued in the child's process group, and the guard holds a
/// process-wide console lock, so keeping it across a wait serializes every
/// other console stop behind this one.
pub fn send_ctrl_break(pid: u32) -> Result<ConsoleGuard> {
    let guard = CONSOLE_ATTACH_MU
        .lock()
        .map_err(|_| anyhow!("console attach mutex poisoned"))?;

    // A process can only be attached to one console at a time. The core runs
    // under `windows_subsystem = "windows"`, so it normally has no console;
    // ignore a failure here.
    unsafe {
        let _ = FreeConsole();
    }

    unsafe { AttachConsole(pid) }.context("AttachConsole")?;

    if let Err(e) = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) } {
        unsafe {
            let _ = FreeConsole();
        }
        drop(guard);
        return Err(anyhow!("GenerateConsoleCtrlEvent: {e}"));
    }

    Ok(ConsoleGuard { _guard: guard })
}

/// Guard that detaches from the console and releases the console lock on drop.
pub struct ConsoleGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeConsole();
        }
    }
}

/// Best-effort installation of a no-op `Ctrl` handler so the core itself is not
/// terminated by a stray console control event it may receive while attached.
///
/// `SetConsoleCtrlHandler(NULL, TRUE)` ignores Ctrl+C for the calling process.
/// The core normally has no console, so a failure here is harmless and ignored.
pub fn ignore_ctrl_events() {
    unsafe {
        let _ = SetConsoleCtrlHandler(None, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_break_constant_is_one() {
        assert_eq!(CTRL_BREAK_EVENT, 1);
    }
}
