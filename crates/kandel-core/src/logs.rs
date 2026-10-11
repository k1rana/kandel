//! Per-app output logs.
//!
//! Each managed process's stdout and stderr are captured (when
//! `capture_output` is enabled) and appended to a bounded, in-memory log that
//! **survives restarts**: the store is keyed by app name and owned by the core,
//! not by a [`Monitor`](crate::monitor::Monitor), so a restart appends to the
//! existing history instead of clearing it. Each launch begins with a
//! timestamped [`LogEvent::Started`] marker so the history reads as a sequence
//! of sessions.
//!
//! The store is deliberately in-memory only. The log is a *view* of what the
//! user's app printed this session, not a durable audit trail; the app's own
//! redirect or a `log_file` remains the way to persist output.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Maximum number of events retained per app before the oldest are dropped.
pub const DEFAULT_LOG_CAP: usize = 2000;

/// Stream name for a line captured from the child's standard output.
pub const STREAM_STDOUT: &str = "stdout";
/// Stream name for a line captured from the child's standard error.
pub const STREAM_STDERR: &str = "stderr";

/// A change to an app's log.
///
/// Serialized with a `type` tag so the UI can render session dividers
/// (`started`) differently from captured lines (`line`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LogEvent {
    /// A new session began; render this as a timestamped divider before the
    /// lines that follow.
    Started { ts: i64 },
    /// A captured line from `stdout` or `stderr`.
    Line {
        ts: i64,
        stream: String,
        text: String,
    },
}

impl LogEvent {
    /// Milliseconds since the Unix epoch for this event.
    pub fn ts(&self) -> i64 {
        match self {
            LogEvent::Started { ts } | LogEvent::Line { ts, .. } => *ts,
        }
    }
}

/// Emits a log event to the UI. Installed by the core's IPC layer.
pub type LogSink = Box<dyn Fn(&str, &LogEvent) + Send + Sync>;

/// Returns the current wall-clock time in milliseconds since the Unix epoch.
///
/// A pre-epoch clock (always a misconfiguration) is clamped to zero rather than
/// panicking.
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Bounded, restart-surviving log storage for every app, plus its event sink.
struct Inner {
    logs: BTreeMap<String, VecDeque<LogEvent>>,
    sink: Option<LogSink>,
}

/// Stores per-app logs and forwards each change to an optional sink.
pub struct LogStore {
    cap: usize,
    inner: Mutex<Inner>,
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new(DEFAULT_LOG_CAP)
    }
}

impl LogStore {
    /// Creates a store retaining at most `cap` events per app.
    pub fn new(cap: usize) -> Self {
        LogStore {
            cap: cap.max(1),
            inner: Mutex::new(Inner {
                logs: BTreeMap::new(),
                sink: None,
            }),
        }
    }

    /// Installs the sink that receives every subsequent event.
    pub fn set_sink(&self, sink: LogSink) {
        self.inner.lock().unwrap().sink = Some(sink);
    }

    /// Appends `event` for `name`, then emits it.
    ///
    /// The store lock is held across the emit so events reach the UI in the
    /// same order they were recorded; the sink does not call back into the
    /// store, so this cannot deadlock.
    fn push(&self, name: &str, event: LogEvent) {
        let mut inner = self.inner.lock().unwrap();
        let cap = self.cap;
        let entry = inner.logs.entry(name.to_string()).or_default();
        entry.push_back(event.clone());
        while entry.len() > cap {
            entry.pop_front();
        }
        if let Some(sink) = &inner.sink {
            sink(name, &event);
        }
    }

    /// Records a session-start marker for `name` and returns its timestamp.
    pub fn record_started(&self, name: &str) -> i64 {
        let ts = now_millis();
        self.push(name, LogEvent::Started { ts });
        ts
    }

    /// Records a captured output line for `name`.
    pub fn record_line(&self, name: &str, stream: &str, text: String) {
        self.push(
            name,
            LogEvent::Line {
                ts: now_millis(),
                stream: stream.to_string(),
                text,
            },
        );
    }

    /// Returns the retained events for `name`, oldest first.
    pub fn events(&self, name: &str) -> Vec<LogEvent> {
        self.inner
            .lock()
            .unwrap()
            .logs
            .get(name)
            .map(|entry| entry.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns every app's retained events, for seeding a freshly opened UI.
    pub fn snapshot(&self) -> Vec<(String, Vec<LogEvent>)> {
        self.inner
            .lock()
            .unwrap()
            .logs
            .iter()
            .map(|(name, events)| (name.clone(), events.iter().cloned().collect()))
            .collect()
    }

    /// Drops the log for `name` (used when an entry is deleted).
    pub fn remove(&self, name: &str) {
        self.inner.lock().unwrap().logs.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn events_are_retained_per_app_in_order() {
        let store = LogStore::default();
        store.record_line("api", STREAM_STDOUT, "first".into());
        store.record_line("api", STREAM_STDOUT, "second".into());
        store.record_line("web", STREAM_STDOUT, "other".into());

        let api = store.events("api");
        assert_eq!(api.len(), 2);
        match &api[0] {
            LogEvent::Line { text, .. } => assert_eq!(text, "first"),
            other => panic!("expected a line, got {other:?}"),
        }
        assert_eq!(store.events("web").len(), 1);
        assert!(store.events("missing").is_empty());
    }

    #[test]
    fn cap_drops_the_oldest_events() {
        let store = LogStore::new(3);
        for i in 0..5 {
            store.record_line("app", STREAM_STDOUT, format!("line {i}"));
        }
        let events = store.events("app");
        assert_eq!(events.len(), 3);
        match &events[0] {
            LogEvent::Line { text, .. } => assert_eq!(text, "line 2"),
            other => panic!("expected a line, got {other:?}"),
        }
    }

    #[test]
    fn record_started_appends_a_marker_without_resetting() {
        let store = LogStore::default();
        store.record_line("app", STREAM_STDOUT, "before".into());
        store.record_started("app");
        store.record_line("app", STREAM_STDOUT, "after".into());

        let events = store.events("app");
        assert_eq!(events.len(), 3);
        assert!(matches!(events[1], LogEvent::Started { .. }));
        match &events[2] {
            LogEvent::Line { text, .. } => assert_eq!(text, "after"),
            other => panic!("expected a line, got {other:?}"),
        }
    }

    #[test]
    fn started_event_is_tagged_for_the_wire() {
        let json = serde_json::to_string(&LogEvent::Started { ts: 42 }).unwrap();
        assert_eq!(json, r#"{"type":"started","ts":42}"#);
        let back: LogEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, LogEvent::Started { ts: 42 });
    }

    #[test]
    fn line_event_is_tagged_for_the_wire() {
        let event = LogEvent::Line {
            ts: 7,
            stream: STREAM_STDERR.to_string(),
            text: "boom".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"type":"line","ts":7,"stream":"stderr","text":"boom"}"#
        );
        let back: LogEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn sink_receives_every_event_in_order() {
        let store = LogStore::default();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = Arc::clone(&seen);
        store.set_sink(Box::new(move |name, event| {
            captured
                .lock()
                .unwrap()
                .push(format!("{name}:{}", serde_json::to_string(event).unwrap()));
        }));

        store.record_started("app");
        store.record_line("app", STREAM_STDERR, "boom".into());

        let got = seen.lock().unwrap();
        assert_eq!(got.len(), 2);
        assert!(got[0].contains("\"type\":\"started\""));
        assert!(got[1].starts_with("app:"));
        assert!(got[1].contains("boom"));
    }

    #[test]
    fn snapshot_includes_every_app() {
        let store = LogStore::default();
        store.record_line("a", STREAM_STDOUT, "x".into());
        store.record_started("b");
        let snap = store.snapshot();
        assert_eq!(snap.len(), 2);
        assert!(snap.iter().any(|(name, _)| name == "a"));
        assert!(snap.iter().any(|(name, _)| name == "b"));
    }

    #[test]
    fn remove_drops_one_apps_log() {
        let store = LogStore::default();
        store.record_line("a", STREAM_STDOUT, "x".into());
        store.record_line("b", STREAM_STDOUT, "y".into());
        store.remove("a");
        assert!(store.events("a").is_empty());
        assert_eq!(store.events("b").len(), 1);
    }

    #[test]
    fn ts_reads_both_variants() {
        assert_eq!(LogEvent::Started { ts: 5 }.ts(), 5);
        assert_eq!(
            LogEvent::Line {
                ts: 9,
                stream: STREAM_STDOUT.into(),
                text: String::new()
            }
            .ts(),
            9
        );
    }
}
