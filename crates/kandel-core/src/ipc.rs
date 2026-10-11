//! Inter-process communication between the core and the UI.
//!
//! The protocol is intentionally narrow: the core launches the UI as a child
//! process and writes a line-delimited JSON stream of [`CoreEvent`] envelopes to
//! the child's stdin. The UI never sends commands back; config changes flow
//! through `config.toml` and the file watcher.
//!
//! Two kinds of event share the stream: a monitor's lifecycle [`MonitorStatus`]
//! and an app's captured [`LogEvent`]. They are discriminated by the `kind`
//! field so the UI can route each to the right place.

use serde::{Deserialize, Serialize};

use crate::logs::LogEvent;
use crate::status::MonitorStatus;

/// A message from the core to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CoreEvent {
    /// A monitor's lifecycle state changed.
    Status(MonitorStatus),
    /// A log line (or session marker) was captured for the named app.
    Logs { name: String, event: LogEvent },
}

/// Writes newline-delimited JSON events to a writer.
pub struct EventWriter<W: std::io::Write> {
    inner: W,
}

impl<W: std::io::Write> EventWriter<W> {
    /// Creates a writer around `inner`.
    pub fn new(inner: W) -> Self {
        EventWriter { inner }
    }

    /// Serializes one event and writes it followed by a newline.
    pub fn send(&mut self, event: &CoreEvent) -> std::io::Result<()> {
        serde_json::to_writer(&mut self.inner, event)?;
        self.inner.write_all(b"\n")?;
        self.inner.flush()
    }
}

/// Backwards-compatible alias used by the existing status path.
pub type StatusWriter<W> = EventWriter<W>;

impl<W: std::io::Write> StatusWriter<W> {
    /// Serializes and writes a single monitor status.
    pub fn send_status(&mut self, status: &MonitorStatus) -> std::io::Result<()> {
        self.send(&CoreEvent::Status(status.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::STREAM_STDOUT;
    use crate::status::AppState;

    fn status(name: &str, state: AppState) -> MonitorStatus {
        MonitorStatus {
            name: name.into(),
            state,
            pid: 42,
            enabled: true,
            command: "cmd.exe".into(),
            args: vec!["/c".into()],
            restart_count: 1,
        }
    }

    #[test]
    fn status_envelope_is_tagged() {
        let json =
            serde_json::to_string(&CoreEvent::Status(status("a", AppState::Running))).unwrap();
        assert!(json.contains("\"kind\":\"status\""), "{json}");
        let back: CoreEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CoreEvent::Status(status("a", AppState::Running)));
    }

    #[test]
    fn log_envelope_carries_the_app_name() {
        let event = CoreEvent::Logs {
            name: "api".into(),
            event: LogEvent::Line {
                ts: 5,
                stream: STREAM_STDOUT.into(),
                text: "hi".into(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"logs\""), "{json}");
        assert!(json.contains("\"name\":\"api\""), "{json}");
        let back: CoreEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn writer_emits_one_event_per_line() {
        let mut buf = Vec::new();
        {
            let mut writer = EventWriter::new(&mut buf);
            writer
                .send(&CoreEvent::Status(status("a", AppState::Running)))
                .unwrap();
            writer
                .send(&CoreEvent::Logs {
                    name: "a".into(),
                    event: LogEvent::Started { ts: 1 },
                })
                .unwrap();
        }
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.ends_with('\n'));
        assert!(text.contains("\"kind\":\"status\""));
        assert!(text.contains("\"kind\":\"logs\""));
    }
}
