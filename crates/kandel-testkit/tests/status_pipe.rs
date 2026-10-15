//! End-to-end test of the core's status pipe.
//!
//! Proves the property the whole named-pipe move exists for: a client that is
//! *not* launched by the core can attach to the core's stream, receive a
//! snapshot on connect, and then receive live events — and can detach and
//! reattach, getting a fresh snapshot each time.
//!
//! The client here is deliberately written with the raw Win32 API rather than
//! reusing `kandel-ui`'s `PipeClient`: that keeps the test on the *protocol*
//! (a named pipe at a known name carrying JSON lines) instead of on one
//! particular consumer, and avoids the testkit depending on the UI crate.

#![cfg(windows)]

use std::io::{BufRead, BufReader};
use std::os::windows::io::FromRawHandle;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kandel_core::ipc::CoreEvent;
use kandel_core::status::{AppState, MonitorStatus};
use kandel_core::win::pipe::PipeServer;

use windows::core::PCWSTR;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING,
};
use windows::Win32::System::Pipes::WaitNamedPipeW;

/// A unique pipe name per test run, so the test never attaches to (or is
/// confused by) a real core's pipe.
fn unique_pipe_name(tag: &str) -> String {
    format!(r"\\.\pipe\kandel-test-{}-{}", std::process::id(), tag)
}

fn status(name: &str) -> MonitorStatus {
    MonitorStatus {
        name: name.into(),
        state: AppState::Running,
        pid: 4242,
        enabled: true,
        command: "cmd.exe".into(),
        args: vec![],
        restart_count: 0,
    }
}

/// Connects as a client and returns a line reader over the pipe.
fn connect(name: &str) -> BufReader<std::fs::File> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

    let deadline = Instant::now() + Duration::from_secs(5);
    let handle = loop {
        // The server publishes asynchronously; wait for it to exist.
        let _ = unsafe { WaitNamedPipeW(PCWSTR(wide.as_ptr()), 500) };
        match unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                // The server's pipe is outbound-only, so a client reads. Asking
                // for write access too is refused with ERROR_ACCESS_DENIED.
                GENERIC_READ.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        } {
            Ok(h) => break h,
            Err(e) => {
                if Instant::now() >= deadline {
                    panic!("client could not attach to {name}: {e}");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let file = unsafe { std::fs::File::from_raw_handle(handle.0) };
    BufReader::new(file)
}

/// Reads lines until one satisfies `pred`, or the deadline passes.
fn read_until(reader: &mut impl BufRead, what: &str, pred: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if pred(&line) {
                    return line.trim_end().to_string();
                }
            }
            Err(_) => break,
        }
    }
    panic!("did not receive {what} within the deadline");
}

#[test]
fn client_attaches_gets_a_snapshot_then_live_events() {
    let name = unique_pipe_name("basic");
    let snapshot = Arc::new(|| vec![CoreEvent::Status(status("snapshot-app"))]);
    let server = PipeServer::spawn(name.clone(), snapshot);

    let mut client = connect(&name);

    // 1. The snapshot sent on attach arrives.
    let first = read_until(&mut client, "the snapshot status", |l| {
        l.contains("snapshot-app")
    });
    assert!(first.contains("\"kind\":\"status\""), "{first}");
    assert!(first.contains("Running"), "{first}");

    // 2. A live event sent afterward arrives on the same connection.
    server.send(&CoreEvent::Status(status("live-app")));
    let live = read_until(&mut client, "the live status", |l| l.contains("live-app"));
    assert!(live.contains("\"kind\":\"status\""), "{live}");
}

#[test]
fn client_can_detach_and_reattach_for_a_fresh_snapshot() {
    // Two different snapshots make it unambiguous which attach served which
    // line: the counter advances every time a client connects.
    let name = unique_pipe_name("reattach");
    let connects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let connects_for_snapshot = Arc::clone(&connects);
    let snapshot = Arc::new(move || {
        let n = connects_for_snapshot.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        vec![CoreEvent::Status(status(&format!("attach-{n}")))]
    });
    let _server = PipeServer::spawn(name.clone(), snapshot);

    {
        let mut first = connect(&name);
        let line = read_until(&mut first, "the first snapshot", |l| l.contains("attach-"));
        assert!(line.contains("attach-0"), "{line}");
        // Dropping the client disconnects it.
    }

    // Reconnecting yields a *second* snapshot, not a stale one — the property
    // that lets a UI survive a core restart.
    let mut second = connect(&name);
    let line = read_until(&mut second, "the second snapshot", |l| {
        l.contains("attach-1")
    });
    assert!(line.contains("attach-1"), "{line}");
}

#[test]
fn send_without_a_client_does_not_panic() {
    let name = unique_pipe_name("noclient");
    let server = PipeServer::spawn(name, Arc::new(Vec::<CoreEvent>::new));
    // No client ever attaches; these must be harmless no-ops.
    server.send(&CoreEvent::Status(status("nobody")));
    std::thread::sleep(Duration::from_millis(100));
    server.send(&CoreEvent::Status(status("still-nobody")));
}
