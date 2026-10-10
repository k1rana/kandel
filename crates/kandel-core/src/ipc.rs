//! Inter-process communication between the core and the UI.
//!
//! The protocol is intentionally narrow: the core launches the UI as a child
//! process and writes a line-delimited JSON stream of [`MonitorStatus`] updates
//! to the child's stdin. The UI never sends commands back; config changes flow
//! through `config.toml` and the file watcher.

use std::io::Write;

use crate::status::MonitorStatus;

/// Writes newline-delimited JSON status updates to a writer.
pub struct StatusWriter<W: Write> {
    inner: W,
}

impl<W: Write> StatusWriter<W> {
    /// Creates a writer around `inner`.
    pub fn new(inner: W) -> Self {
        StatusWriter { inner }
    }

    /// Serializes and writes a single status update followed by a newline.
    pub fn send(&mut self, status: &MonitorStatus) -> std::io::Result<()> {
        serde_json::to_writer(&mut self.inner, status)?;
        self.inner.write_all(b"\n")?;
        self.inner.flush()
    }

    /// Returns a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Consumes the writer, returning the inner value.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn writes_one_json_object_per_line() {
        let mut buf = Vec::new();
        {
            let mut writer = StatusWriter::new(&mut buf);
            writer.send(&status("a", AppState::Running)).unwrap();
            writer.send(&status("b", AppState::Stopped)).unwrap();
        }
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: MonitorStatus = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first.name, "a");
        assert_eq!(first.state, AppState::Running);

        let second: MonitorStatus = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second.name, "b");
        assert_eq!(second.state, AppState::Stopped);
    }

    #[test]
    fn output_has_trailing_newline() {
        let mut buf = Vec::new();
        StatusWriter::new(&mut buf)
            .send(&status("a", AppState::Running))
            .unwrap();
        assert!(buf.ends_with(b"\n"));
    }
}
