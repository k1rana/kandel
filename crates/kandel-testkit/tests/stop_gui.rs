//! End-to-end check of the graceful `WM_CLOSE` stop path.
//!
//! `kandel_core::monitor::stop_process` has three stop strategies; two of them
//! (post `WM_CLOSE` to a GUI app, send `CTRL_BREAK_EVENT` to a console app) had
//! no automated coverage: the unit tests only exercise the window-class filter
//! and the datapath around them, and `scripts/smoke_core.sh` deliberately hard
//! kills the core so it never reaches a graceful stop.
//!
//! This test drives the real code path against a real Windows process:
//!
//! 1. spawn the fixture, which owns a top-level window and exits 0 on `WM_CLOSE`
//! 2. wait until the window exists (a stop before that would find no target)
//! 3. call `stop_process` and assert the fixture exited **0**, not the **1**
//!    that `TerminateProcess` records
//!
//! Runs in its own test binary (its own process) so the console manipulation in
//! the sibling `stop_console.rs` cannot interfere with it.

#![cfg(windows)]

use std::time::{Duration, Instant};

use kandel_config::App;
use kandel_core::monitor::stop_process;
use kandel_core::win::process::{spawn_with_job_object, LaunchSpec};
use kandel_core::win::window::top_level_windows_for_pid;

/// The fixture that owns a GUI window and exits 0 on `WM_CLOSE`.
///
/// `CARGO_BIN_EXE_*` is set by Cargo for integration tests and guarantees the
/// binary was built before this test runs.
const FIXTURE: &str = env!("CARGO_BIN_EXE_alert_on_wm_close");

/// Polls until `pid` owns a top-level window, or `timeout` elapses.
///
/// Polling rather than sleeping keeps the test fast when the fixture is quick
/// and still tolerant on a loaded CI machine.
fn wait_for_window(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !top_level_windows_for_pid(pid).is_empty() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn wm_close_stops_a_gui_process_gracefully() {
    let spec = LaunchSpec {
        program: FIXTURE.into(),
        args: vec!["--auto".into()],
        cwd: None,
        hide_window: true,
        env: Vec::new(),
        capture_output: false,
    };
    let child = spawn_with_job_object(&spec, None).expect("spawning the GUI fixture");

    assert!(
        wait_for_window(child.pid, Duration::from_secs(10)),
        "the fixture never created a top-level window"
    );

    // Empty `stop_command` selects the automatic path, which posts WM_CLOSE.
    let cfg = App {
        name: "gui-fixture".into(),
        ..App::default()
    };
    stop_process(&cfg, &child).expect("stop_process");

    // 0 means the fixture handled WM_CLOSE. 1 would mean it was force-killed,
    // which is exactly the failure this test exists to catch.
    assert_eq!(
        child.exit_code().expect("reading the exit code"),
        Some(0),
        "the process should have stopped gracefully on WM_CLOSE, not been killed"
    );
}
