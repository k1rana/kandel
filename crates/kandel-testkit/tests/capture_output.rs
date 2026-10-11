//! End-to-end check of the output-capture pipeline.
//!
//! This drives the whole path a real entry takes, with no fakes in the middle:
//!
//! 1. start from an `App` config (as `config.toml` would produce);
//! 2. hand it to the real `Supervisor`, which builds a `Monitor`;
//! 3. the monitor resolves a `LaunchSpec`, spawns the process, and captures its
//!    stdout/stderr through the real pipe plumbing;
//! 4. each captured line lands in the per-app `LogStore` **and** is forwarded
//!    through the store's sink (the same channel the UI subscribes to).
//!
//! The unit tests cover the individual pieces; this covers them wired together,
//! which is where an "it compiles but nothing shows up" bug would live.
//!
//! Each fixture prints and exits on its own, with `max_retries = 0`, so the
//! monitor converges without a stop. That deliberately avoids the graceful-stop
//! path (console attach), which calls `FreeConsole` and therefore must not run
//! in a binary that other tests share.

#![cfg(windows)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kandel_config::App;
use kandel_core::logs::{LogEvent, LogStore, STREAM_STDERR};
use kandel_core::notify::Notifier;
use kandel_core::supervisor::Supervisor;

/// Polls `check` until it returns true or `timeout` elapses.
fn eventually(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Collects captured events for one app, keyed by the app name.
#[derive(Clone, Default)]
struct SinkLog(Arc<Mutex<Vec<(String, LogEvent)>>>);

impl SinkLog {
    fn record(&self, name: &str, event: &LogEvent) {
        self.0
            .lock()
            .unwrap()
            .push((name.to_string(), event.clone()));
    }

    fn lines_for(&self, name: &str) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| n == name)
            .filter_map(|(_, e)| match e {
                LogEvent::Line { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn captured_output_reaches_the_store_and_the_sink() {
    // Prints one line to stdout and one to stderr, then exits.
    let cfg = App {
        name: "e2e-capture".into(),
        command: "cmd.exe".into(),
        args: vec![
            "/c".into(),
            "echo kandel-e2e-stdout & echo kandel-e2e-stderr 1>&2".into(),
        ],
        hide_window: true,
        restart_delay_sec: 0,
        max_retries: 0,
        capture_output: true,
        ..App::default()
    };

    let logs = Arc::new(LogStore::default());
    let sink = SinkLog::default();
    let sink_for_store = sink.clone();
    logs.set_sink(Box::new(move |name, event| {
        sink_for_store.record(name, event);
    }));

    let supervisor = Supervisor::new(
        Arc::new(|_| {}),
        Arc::new(Notifier::new()),
        Arc::clone(&logs),
    );
    let mut desired = BTreeMap::new();
    desired.insert("e2e-capture".to_string(), cfg);
    supervisor.reconcile(desired);

    // Wait for both lines to arrive through the sink (not just the store), since
    // the sink is what the UI actually receives.
    let saw_both = eventually(Duration::from_secs(15), || {
        let lines = sink.lines_for("e2e-capture");
        lines.iter().any(|l| l.contains("kandel-e2e-stdout"))
            && lines.iter().any(|l| l.contains("kandel-e2e-stderr"))
    });

    assert!(
        saw_both,
        "expected both captured lines to reach the sink; got {:?}",
        sink.lines_for("e2e-capture")
    );

    // The store retains the same history for a later UI panel to fetch.
    let events = logs.events("e2e-capture");
    assert!(
        events.iter().any(
            |e| matches!(e, LogEvent::Line { text, .. } if text.contains("kandel-e2e-stdout"))
        ),
        "store missing stdout line: {events:?}"
    );
    // stderr was tagged as stderr, not folded into stdout.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, LogEvent::Line { stream, .. } if stream == STREAM_STDERR)),
        "no stderr-tagged line: {events:?}"
    );
    // A session marker opens the history, so the panel can draw a divider.
    assert!(
        events.iter().any(|e| matches!(e, LogEvent::Started { .. })),
        "no session marker: {events:?}"
    );
}

#[test]
fn restart_appends_a_new_session_marker() {
    // A short-lived process forces the restart loop to run it more than once.
    let cfg = App {
        name: "e2e-restart".into(),
        command: "cmd.exe".into(),
        args: vec!["/c".into(), "echo session-line".into()],
        hide_window: true,
        restart_delay_sec: 0,
        max_retries: 1,
        capture_output: true,
        ..App::default()
    };

    let logs = Arc::new(LogStore::default());
    let supervisor = Supervisor::new(
        Arc::new(|_| {}),
        Arc::new(Notifier::new()),
        Arc::clone(&logs),
    );

    let mut desired = BTreeMap::new();
    desired.insert("e2e-restart".to_string(), cfg);
    supervisor.reconcile(desired);

    // Wait until two sessions have started *and* both have produced output.
    // Sampling on the marker alone would race the second process's first line.
    let saw_two = eventually(Duration::from_secs(15), || {
        let events = logs.events("e2e-restart");
        let starts = events
            .iter()
            .filter(|e| matches!(e, LogEvent::Started { .. }))
            .count();
        let lines = events
            .iter()
            .filter(|e| matches!(e, LogEvent::Line { .. }))
            .count();
        starts >= 2 && lines >= 2
    });

    let events = logs.events("e2e-restart");
    let lines = events
        .iter()
        .filter(|e| matches!(e, LogEvent::Line { .. }))
        .count();
    assert!(saw_two, "expected two sessions with output; got {events:?}");
    // History survives the restart: output from earlier sessions is still here.
    assert!(
        lines >= 2,
        "expected lines from both sessions; got {events:?}"
    );
}

#[test]
fn capture_on_keeps_stdin_waiters_alive() {
    // The regression the whole feature must not reintroduce: capture once set
    // the child's stdin to `NUL`, so any program that reads stdin (an MCP/LSP
    // server, an interactive tool) saw EOF and exited immediately, then was
    // marked Failed. With capture on, stdin is now a live pipe the parent keeps
    // open, so a stdin-waiting process must reach Running and stay there.
    //
    // The assertion is on the monitor *state*, not the log: `set /p` blocking on
    // a live stdin keeps the process in Running, whereas an EOF-ing stdin makes
    // it return at once, exit, and reach Failed (max_retries = 0).
    let cfg = App {
        name: "e2e-stdin".into(),
        command: "cmd.exe".into(),
        args: vec!["/c".into(), "set /p x=".into()],
        hide_window: true,
        restart_delay_sec: 0,
        max_retries: 0,
        capture_output: true,
        ..App::default()
    };

    let logs = Arc::new(LogStore::default());
    let seen_running = Arc::new(Mutex::new(false));
    let seen_failed = Arc::new(Mutex::new(false));
    let running = Arc::clone(&seen_running);
    let failed = Arc::clone(&seen_failed);
    let on_state: kandel_core::monitor::StateCallback = Arc::new(move |s| {
        if s.state == kandel_core::AppState::Running {
            *running.lock().unwrap() = true;
        }
        if s.state == kandel_core::AppState::Failed {
            *failed.lock().unwrap() = true;
        }
    });

    let supervisor = Supervisor::new(on_state, Arc::new(Notifier::new()), Arc::clone(&logs));
    let mut desired = BTreeMap::new();
    desired.insert("e2e-stdin".to_string(), cfg);
    supervisor.reconcile(desired);

    assert!(
        eventually(Duration::from_secs(5), || *seen_running.lock().unwrap()),
        "the stdin-reading process never reached Running"
    );
    // Give an EOF-ing stdin time to cause the exit that marks it Failed.
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !*seen_failed.lock().unwrap(),
        "the stdin-reading process exited (stdin EOF'd) and was marked Failed"
    );
    // Its stdout was still captured, so this did not cost us visibility.
    assert!(
        !logs.events("e2e-stdin").is_empty(),
        "capture should still record the session"
    );
}
