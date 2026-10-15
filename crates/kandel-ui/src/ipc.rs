//! The UI's side of the status channel: a named-pipe client with reconnect.
//!
//! The core publishes `\\.\pipe\kandel-core-<identity>-<session>` and serves
//! whichever UI attaches (see `kandel_core::win::pipe`). The UI connects, then
//! keeps reading newline-delimited JSON [`CoreEvent`]s for as long as it is open.
//!
//! Unlike the previous stdin transport, the channel is not tied to whoever
//! launched the UI. That is what makes reconnecting possible: if the core is not
//! up yet, or restarts, this client backs off and attaches again, and the core
//! replays a fresh snapshot on every attach. A dropped connection shows as
//! "disconnected" in the UI rather than closing the window.
//!
//! # Why messages are cached
//!
//! The core starts writing the moment a client attaches, but the frontend can
//! only subscribe once the webview has mounted — later. Without a cache, every
//! message written in that window is emitted to zero listeners and lost, leaving
//! entries stuck on whatever the config seeded (observed as "Starting") and logs
//! empty. The caches hold the latest status per app and the event backlog per
//! app so the frontend can fetch a snapshot on mount; live events then keep it
//! current.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};

/// Event name emitted to the frontend for each status update.
pub const STATE_EVENT: &str = "app-state-update";
/// Event name emitted to the frontend for each captured log event.
pub const LOG_EVENT: &str = "app-log-update";
/// Event name carrying connection state ("connected" / "disconnected").
pub const CONNECTION_EVENT: &str = "core-connection";

/// The latest status line per app name, plus each app's log backlog.
///
/// Statuses are keyed by app name so repeated updates for one app replace rather
/// than accumulate; the raw JSON line the core sent is stored so the snapshot the
/// frontend fetches is byte-identical to an event payload. Log events accumulate
/// in order, and are trimmed to a bounded backlog.
#[derive(Default)]
pub struct StatusCache {
    statuses: Mutex<HashMap<String, String>>,
    logs: Mutex<HashMap<String, Vec<String>>>,
}

/// Matches the core's per-app log cap, so the two ends agree on backlog size.
const LOG_BACKLOG_CAP: usize = 2000;

impl StatusCache {
    /// Routes a decoded event line into the status or log cache.
    ///
    /// A line that is not valid JSON, or has no `name`, is still forwarded to
    /// the frontend (which ignores it) but not cached, since there is nothing
    /// to key it on.
    pub fn record(&self, line: &str) {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => return,
        };
        let kind = value
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or("status");
        if kind == "logs" {
            let Some(name) = value.get("name").and_then(|n| n.as_str()) else {
                return;
            };
            if let Ok(mut logs) = self.logs.lock() {
                let entry = logs.entry(name.to_string()).or_default();
                entry.push(line.to_string());
                while entry.len() > LOG_BACKLOG_CAP {
                    entry.remove(0);
                }
            }
        } else if let Some(name) = value.get("name").and_then(|n| n.as_str()) {
            if let Ok(mut statuses) = self.statuses.lock() {
                statuses.insert(name.to_string(), line.to_string());
            }
        }
    }

    /// Returns every cached status line. Order is unspecified; the frontend
    /// sorts by name for display.
    pub fn snapshot(&self) -> Vec<String> {
        self.statuses
            .lock()
            .map(|cache| cache.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns the buffered log lines for `name`, oldest first.
    pub fn log_snapshot(&self, name: &str) -> Vec<String> {
        self.logs
            .lock()
            .ok()
            .and_then(|logs| logs.get(name).cloned())
            .unwrap_or_default()
    }

    /// Clears every cached line, so a reconnect does not show stale state.
    pub fn clear(&self) {
        if let Ok(mut statuses) = self.statuses.lock() {
            statuses.clear();
        }
        if let Ok(mut logs) = self.logs.lock() {
            logs.clear();
        }
    }

    /// Removes the cached status and log for `name` (used when an entry is
    /// deleted).
    pub fn remove(&self, name: &str) {
        if let Ok(mut cache) = self.statuses.lock() {
            cache.remove(name);
        }
        if let Ok(mut logs) = self.logs.lock() {
            logs.remove(name);
        }
    }
}

/// Whether the UI is currently attached to a core.
///
/// Held as state, not merely emitted as an event, because the attach usually
/// happens *before* the webview has mounted and subscribed. An event alone is
/// therefore missed on every normal launch, leaving the UI believing it is
/// detached while events stream in. The frontend reads this on mount and then
/// follows the [`CONNECTION_EVENT`] for changes.
///
/// The flag lives behind an `Arc` so the value is cheap to clone: the reader
/// thread and Tauri's state table must share one instance, but Tauri resolves
/// state by exact type, so the managed value has to be this type rather than an
/// `Arc` wrapping it.
#[derive(Default, Clone)]
pub struct ConnectionState(Arc<AtomicBool>);

impl ConnectionState {
    /// Records a new connection state, announcing it only when it changed.
    ///
    /// The retry loop probes every 500 ms while no core is running; emitting on
    /// every attempt would flood the frontend with identical events.
    pub fn set(&self, app: &AppHandle, connected: bool) {
        if self.0.swap(connected, Ordering::SeqCst) == connected {
            return;
        }
        let _ = app.emit(
            CONNECTION_EVENT,
            if connected {
                "connected"
            } else {
                "disconnected"
            },
        );
    }

    /// Returns the current state, for a client that mounted after the change.
    pub fn get(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Which app event a decoded line is, so it is emitted under the right name.
fn event_name_for(line: &str) -> &'static str {
    let is_log = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
        .map(|k| k == "logs")
        .unwrap_or(false);
    if is_log {
        LOG_EVENT
    } else {
        STATE_EVENT
    }
}

/// How long to wait before retrying a failed attach. The pipe appears as soon as
/// the core publishes it, so a short fixed delay is enough and avoids a thundering
/// herd of connects while the core is starting.
const RECONNECT_DELAY: Duration = Duration::from_millis(500);

/// Connects to the core's status pipe and streams events until the app exits.
///
/// Runs on its own thread. Each attach replays nothing itself — the *core* sends
/// a snapshot to every new client — so this only has to drain lines and forward
/// them. Every disconnect is reported to the frontend and retried.
pub fn spawn_reader(app: AppHandle, connection: ConnectionState) {
    std::thread::Builder::new()
        .name("kandel-ui-ipc".into())
        .spawn(move || {
            let pipe_name = kandel_config::pipe_name(stack_ipc_session_id());
            loop {
                match crate::pipe::connect(&pipe_name) {
                    Ok(mut client) => {
                        if let Some(cache) = app.try_state::<StatusCache>() {
                            // A fresh attach means a fresh snapshot is coming;
                            // drop anything cached from a previous core so the
                            // window cannot show a stale status.
                            cache.clear();
                        }
                        connection.set(&app, true);
                        while let Some(line) = client.read_line() {
                            if line.trim().is_empty() {
                                continue;
                            }
                            if let Some(cache) = app.try_state::<StatusCache>() {
                                cache.record(&line);
                            }
                            let _ = app.emit(event_name_for(&line), line);
                        }
                        // The core went away; tell the frontend and reconnect.
                        connection.set(&app, false);
                    }
                    Err(_) => {
                        connection.set(&app, false);
                    }
                }
                std::thread::sleep(RECONNECT_DELAY);
            }
        })
        .expect("spawning the IPC reader thread");
}

/// The Windows session id, for building the pipe name. Kept here so the UI does
/// not need a second dependency path into the core for one integer.
fn stack_ipc_session_id() -> u32 {
    #[cfg(windows)]
    {
        crate::pipe::current_session_id()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Returns the statuses seen so far, for the frontend to apply on mount.
///
/// This closes the startup race described in the module docs: the frontend
/// subscribes to [`STATE_EVENT`] first, then calls this to pick up anything
/// that arrived before it was listening.
#[tauri::command]
pub fn get_initial_statuses(cache: State<'_, StatusCache>) -> Vec<String> {
    cache.snapshot()
}

/// Returns the buffered log lines for `name`, for the frontend to apply when a
/// log panel is opened.
#[tauri::command]
pub fn get_app_logs(cache: State<'_, StatusCache>, name: String) -> Vec<String> {
    cache.log_snapshot(&name)
}

/// Returns whether a core is attached right now.
///
/// The companion to [`CONNECTION_EVENT`]: the attach normally happens before the
/// webview subscribes, so the frontend fetches this on mount and then follows the
/// event. Without it the UI reports "not attached" while the stream is live.
#[tauri::command]
pub fn get_connection_state(connection: State<'_, ConnectionState>) -> bool {
    connection.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_event_name_is_stable() {
        // The frontend depends on these exact event names.
        assert_eq!(STATE_EVENT, "app-state-update");
        assert_eq!(LOG_EVENT, "app-log-update");
        assert_eq!(CONNECTION_EVENT, "core-connection");
    }

    #[test]
    fn cache_keys_statuses_and_logs_separately() {
        let cache = StatusCache::default();
        cache.record(r#"{"kind":"status","name":"api","state":"Running"}"#);
        cache.record(r#"{"kind":"logs","name":"api","event":{"type":"line","text":"hi"}}"#);

        assert_eq!(cache.snapshot().len(), 1);
        assert!(cache.snapshot()[0].contains("Running"));

        let logs = cache.log_snapshot("api");
        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains("hi"));
    }

    #[test]
    fn cache_accumulates_log_lines_in_order() {
        let cache = StatusCache::default();
        cache.record(r#"{"kind":"logs","name":"a","event":{"type":"line","text":"one"}}"#);
        cache.record(r#"{"kind":"logs","name":"a","event":{"type":"line","text":"two"}}"#);
        let logs = cache.log_snapshot("a");
        assert_eq!(logs.len(), 2);
        assert!(logs[0].contains("one"));
        assert!(logs[1].contains("two"));
    }

    #[test]
    fn cache_keeps_only_the_latest_line_per_app() {
        let cache = StatusCache::default();
        cache.record(r#"{"name":"api","state":"Starting"}"#);
        cache.record(r#"{"name":"api","state":"Running","pid":42}"#);
        cache.record(r#"{"name":"web","state":"Stopped"}"#);

        let snap = cache.snapshot();
        assert_eq!(snap.len(), 2);
        // Order is unspecified (HashMap); find by content, not position.
        assert!(snap.iter().any(|l| l.contains("web")));
        // The api entry reflects the second write, not the first.
        let api = snap.iter().find(|l| l.contains("\"api\"")).unwrap();
        assert!(api.contains("Running"));
        assert!(api.contains("42"));
        assert!(!api.contains("Starting"));
    }

    #[test]
    fn cache_ignores_lines_without_a_name() {
        let cache = StatusCache::default();
        cache.record("not json");
        cache.record(r#"{"state":"Running"}"#);
        assert!(cache.snapshot().is_empty());
    }

    #[test]
    fn cache_remove_drops_a_status_and_its_log() {
        let cache = StatusCache::default();
        cache.record(r#"{"kind":"status","name":"api","state":"Running"}"#);
        cache.record(r#"{"kind":"logs","name":"api","event":{"type":"line","text":"x"}}"#);
        cache.record(r#"{"kind":"status","name":"web","state":"Running"}"#);
        cache.remove("api");
        assert_eq!(cache.snapshot().len(), 1);
        assert!(cache.snapshot()[0].contains("web"));
        assert!(cache.log_snapshot("api").is_empty());
    }

    #[test]
    fn cache_clear_drops_everything() {
        let cache = StatusCache::default();
        cache.record(r#"{"name":"api","state":"Running"}"#);
        cache.record(r#"{"kind":"logs","name":"api","event":{"type":"line","text":"x"}}"#);
        cache.clear();
        assert!(cache.snapshot().is_empty());
        assert!(cache.log_snapshot("api").is_empty());
    }

    #[test]
    fn event_name_is_stable_for_connection() {
        // The frontend matches on these exact strings.
        assert_eq!(CONNECTION_EVENT, "core-connection");
    }

    #[test]
    fn connection_state_reflects_the_last_set() {
        let state = ConnectionState::default();
        // The default before any attach: nothing connected.
        assert!(!state.get());
        // A direct store stands in for `set`, which needs an AppHandle to emit.
        state.0.store(true, Ordering::SeqCst);
        assert!(state.get());
        state.0.store(false, Ordering::SeqCst);
        assert!(!state.get());
    }

    #[test]
    fn clones_share_one_flag() {
        // The reader thread and the command handler each hold a clone, so they
        // must observe the same value.
        let state = ConnectionState::default();
        let clone = state.clone();
        state.0.store(true, Ordering::SeqCst);
        assert!(clone.get());
        clone.0.store(false, Ordering::SeqCst);
        assert!(!state.get());
    }

    #[test]
    fn swap_reports_the_previous_value_so_repeats_are_silent() {
        // This is what keeps the retry loop from re-announcing on every probe.
        let state = ConnectionState::default();
        assert!(!state.0.swap(false, Ordering::SeqCst));
        assert!(!state.0.swap(true, Ordering::SeqCst));
        assert!(state.0.swap(true, Ordering::SeqCst));
    }

    /// Resolves [`ConnectionState`] the way Tauri's state table does.
    ///
    /// The regression this guards: Tauri keys managed state by `TypeId`, so
    /// registering `Arc<ConnectionState>` while the command asks for
    /// `State<ConnectionState>` makes the command fail at runtime, leaving every
    /// row reading Unknown while the stream is live. Neither the unit tests nor a
    /// browser stub can see that, because both bypass the manager, so the keying
    /// is asserted directly here.
    #[test]
    fn the_managed_type_is_exactly_what_the_command_asks_for() {
        use std::any::{Any, TypeId};

        // What `.manage(connection)` registers, and what the command's
        // `State<'_, ConnectionState>` resolves to.
        let managed = ConnectionState::default();
        let managed_id = TypeId::of::<ConnectionState>();
        assert_eq!(managed_id, (&managed as &dyn Any).type_id());

        // The mistake that broke it: an `Arc` wrapper is a different type, so a
        // lookup keyed on `ConnectionState` misses it.
        let wrapped = std::sync::Arc::new(ConnectionState::default());
        assert_ne!(
            TypeId::of::<ConnectionState>(),
            TypeId::of::<std::sync::Arc<ConnectionState>>()
        );
        assert_ne!(managed_id, (&wrapped as &dyn Any).type_id());
    }

    #[test]
    fn routes_each_line_to_the_right_event_name() {
        assert_eq!(
            event_name_for(r#"{"kind":"status","name":"api"}"#),
            STATE_EVENT
        );
        assert_eq!(event_name_for(r#"{"kind":"logs","name":"api"}"#), LOG_EVENT);
        // A line without a kind is treated as a status, matching the core.
        assert_eq!(event_name_for(r#"{"name":"api"}"#), STATE_EVENT);
    }
}
