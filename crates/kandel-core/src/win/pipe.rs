//! Named-pipe server for the core's status stream.
//!
//! The core **publishes** `\\.\pipe\kandel-core-<identity>-<session>` and serves
//! whichever UI attaches. This replaces the old model, where the stream was the
//! stdin of the UI child the core happened to spawn — a channel only that one
//! process could ever read.
//!
//! Framing is unchanged: newline-delimited JSON, one [`CoreEvent`] per line. Only
//! the transport moved, so the frontend needs no changes.
//!
//! Lifecycle:
//!   * One accept thread publishes instances. Each successful `ConnectNamedPipe`
//!     installs a fresh write end, replacing (and disconnecting) any previous
//!     client — a UI that reconnects after a core restart lands on the new core.
//!   * Every writer (status callbacks, the log sink) serializes its event and
//!     writes it under the same lock, so lines never interleave.
//!   * A write that fails means the UI detached; the write end is dropped and the
//!     accept thread hands the next client a snapshot.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::{WriteFile, PIPE_ACCESS_OUTBOUND};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;

use crate::CoreEvent;

use super::{to_wide_null, SendHandle};

/// Pipe buffer size for both directions. Comfortably above a burst of captured
/// log lines, so the server rarely has to wait on a slow reader.
const PIPE_BUFFER: u32 = 64 * 1024;

/// Pause between failed publishes, so a transient failure does not spin.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(250);

/// The Windows session this process runs in.
///
/// The pipe namespace is machine-wide, so the session id is folded into the pipe
/// name (see [`kandel_config::pipe_name`]) to keep different signed-in users off
/// each other's core — mirroring the session-local mutex the core already holds.
pub fn session_id() -> u32 {
    let mut sid = 0u32;
    // Best-effort: a failure falls back to session 0, which only matters on a
    // host where nothing else would work either.
    let _ = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut sid) };
    sid
}

/// The pipe name for this host's session.
pub fn local_pipe_name() -> String {
    kandel_config::pipe_name(session_id())
}

/// One connected client's write end.
struct WriteEnd(SendHandle);

impl WriteEnd {
    /// Publishes `name` and blocks until a client attaches.
    ///
    /// `None` when the instance cannot be created (for example the name is held
    /// by a foreign process); the caller retries.
    fn accept(name: &str) -> Option<Self> {
        let wide = to_wide_null(name);
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide.as_ptr()),
                PIPE_ACCESS_OUTBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                PIPE_BUFFER,
                PIPE_BUFFER,
                0,
                None,
            )
        };
        if handle.is_invalid() {
            return None;
        }

        // Blocks until a UI attaches. A failure still yields the end so the
        // caller can disconnect and retry rather than leak the handle.
        let _ = unsafe { ConnectNamedPipe(handle, None) };
        Some(WriteEnd(SendHandle(handle)))
    }

    /// Writes one already-serialized event plus its newline.
    fn write_line(&mut self, line: &[u8]) -> std::io::Result<()> {
        let mut written = 0u32;
        unsafe { WriteFile(self.0.raw(), Some(line), Some(&mut written), None) }.map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the UI detached from the status pipe",
            )
        })
    }
}

impl Drop for WriteEnd {
    fn drop(&mut self) {
        unsafe {
            let _ = DisconnectNamedPipe(self.0.raw());
            let _ = CloseHandle(self.0.raw());
        }
    }
}

/// Serializes `event` to one JSON line (with trailing newline).
fn encode(event: &CoreEvent) -> Vec<u8> {
    let mut line = serde_json::to_vec(event).unwrap_or_else(|_| b"{}".to_vec());
    line.push(b'\n');
    line
}

/// The core's status-pipe server.
///
/// Cloneable: every clone shares the same accept thread and current write end.
#[derive(Clone)]
pub struct PipeServer {
    inner: Arc<Mutex<Option<WriteEnd>>>,
}

impl PipeServer {
    /// Publishes the session pipe and starts serving.
    ///
    /// `snapshot` is called every time a UI attaches, so a client that connects
    /// (or reconnects) immediately receives the current statuses and log backlog
    /// instead of waiting for the next change.
    pub fn spawn(name: String, snapshot: Arc<dyn Fn() -> Vec<CoreEvent> + Send + Sync>) -> Self {
        let inner: Arc<Mutex<Option<WriteEnd>>> = Arc::new(Mutex::new(None));

        let accept_inner = Arc::clone(&inner);
        thread::Builder::new()
            .name("kandel-ipc-accept".into())
            .spawn(move || loop {
                let Some(end) = WriteEnd::accept(&name) else {
                    thread::sleep(ACCEPT_RETRY_DELAY);
                    continue;
                };

                // Install the new end first so writes that race the snapshot
                // arrive on the same client. Replacing any previous end
                // disconnects a stale client, which then reconnects.
                let snapshot_lines: Vec<Vec<u8>> = snapshot().iter().map(encode).collect();
                {
                    let mut guard = accept_inner.lock().unwrap();
                    *guard = Some(end);
                }
                for line in snapshot_lines {
                    if !write_to(&accept_inner, &line) {
                        // The client vanished mid-snapshot; go accept another.
                        break;
                    }
                }
            })
            .expect("spawning the IPC accept thread");

        PipeServer { inner }
    }

    /// Sends one event to the attached UI, if any.
    ///
    /// A send with no client attached is a no-op: the stream is advisory, and a
    /// client that attaches later gets the snapshot.
    pub fn send(&self, event: &CoreEvent) {
        let line = encode(event);
        write_to(&self.inner, &line);
    }
}

/// Writes `line` to the current client, clearing it on failure.
///
/// Returns `false` when there is no client or the write failed.
fn write_to(inner: &Arc<Mutex<Option<WriteEnd>>>, line: &[u8]) -> bool {
    let mut guard = inner.lock().unwrap();
    let Some(end) = guard.as_mut() else {
        return false;
    };
    if end.write_line(line).is_err() {
        // The UI detached; drop the end so the accept thread can hand the next
        // client a fresh one.
        *guard = None;
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{AppState, MonitorStatus};

    fn status(name: &str) -> MonitorStatus {
        MonitorStatus {
            name: name.into(),
            state: AppState::Running,
            pid: 1,
            enabled: true,
            command: "cmd.exe".into(),
            args: vec![],
            restart_count: 0,
        }
    }

    #[test]
    fn encodes_one_json_line_per_event() {
        let line = encode(&CoreEvent::Status(status("api")));
        assert_eq!(line.last(), Some(&b'\n'));
        // Exactly one newline, so the client's line reader sees one event.
        assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
        let text = std::str::from_utf8(&line).unwrap();
        assert!(text.contains("\"kind\":\"status\""), "{text}");
        assert!(text.contains("\"api\""), "{text}");
    }

    #[test]
    fn send_without_a_client_is_a_noop() {
        // No accept thread, empty slot: writing must not panic.
        let inner: Arc<Mutex<Option<WriteEnd>>> = Arc::new(Mutex::new(None));
        assert!(!write_to(&inner, b"{}\n"));
    }

    #[test]
    fn pipe_name_is_session_scoped() {
        assert!(local_pipe_name().starts_with(r"\\.\pipe\kandel-core"));
    }
}
