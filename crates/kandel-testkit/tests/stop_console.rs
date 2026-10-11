//! End-to-end check of the graceful `CTRL_BREAK_EVENT` stop path.
//!
//! The sibling coverage gap to `stop_gui.rs`: a console app has no `WM_CLOSE`
//! target (its console host window class is deliberately excluded), so
//! `stop_process` falls through to `console::send_ctrl_break`. That path is
//! only otherwise covered by a constant-equality unit test.
//!
//! The test drives the real code path:
//!
//! 1. spawn the console fixture, which installs a control handler and only then
//!    writes a ready-file
//! 2. wait for that file, so the event cannot arrive before the handler is
//!    armed (which would kill the process by default and make the test race)
//! 3. call `stop_process` and assert the fixture exited **0**, not the **1**
//!    that `TerminateProcess` records
//!
//! This lives in its own test binary because `send_ctrl_break` calls
//! `FreeConsole` on the calling process; sharing a process with the GUI test
//! would detach its console underneath it.

#![cfg(windows)]

use std::time::{Duration, Instant};

use kandel_config::App;
use kandel_core::monitor::stop_process;
use kandel_core::win::process::{spawn_with_job_object, LaunchSpec};

/// The fixture that blocks until a control event and exits 0 once it arrives.
const FIXTURE: &str = env!("CARGO_BIN_EXE_wait_for_console_event");

/// Polls for `path` to exist, or `timeout` elapses.
fn wait_for_file(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.is_file() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn ctrl_break_stops_a_console_process_gracefully() {
    // A temp dir whose lifetime covers the whole test, so the ready-file path
    // stays valid until the assertion below.
    let dir = tempfile::tempdir().expect("creating a temp dir");
    let ready = dir.path().join("ready");

    let spec = LaunchSpec {
        program: FIXTURE.into(),
        args: vec!["--ready-file".into(), ready.to_string_lossy().into_owned()],
        cwd: None,
        hide_window: true,
        env: Vec::new(),
    };
    let child = spawn_with_job_object(&spec).expect("spawning the console fixture");

    assert!(
        wait_for_file(&ready, Duration::from_secs(10)),
        "the fixture never signalled that its control handler was installed"
    );

    // Empty `stop_command` and no eligible window selects the console path,
    // which sends CTRL_BREAK_EVENT to the child's process group.
    let cfg = App {
        name: "console-fixture".into(),
        ..App::default()
    };
    stop_process(&cfg, &child).expect("stop_process");

    // 0 means the fixture's handler ran. 1 would mean it was force-killed.
    assert_eq!(
        child.exit_code().expect("reading the exit code"),
        Some(0),
        "the process should have stopped gracefully on CTRL_BREAK, not been killed"
    );
}
