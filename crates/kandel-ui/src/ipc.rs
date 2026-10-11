//! Reads line-delimited JSON events from stdin (the core process) and forwards
//! each to the frontend: lifecycle statuses as `app-state-update` events and
//! captured log events as `app-log-update` events.
//!
//! This listener is only active in **IPC mode**, which the core enables when it
//! launches the UI as its child with a piped stdin. In that mode, EOF on stdin
//! means the core exited, so the UI shuts itself down. When the UI is started
//! any other way (for example by `tauri dev`, or manually), IPC mode is off and
//! stdin is left alone — otherwise a non-piped stdin would report EOF
//! immediately and close the window.
//!
//! # Why messages are cached
//!
//! The core starts writing the moment it launches the UI, but the frontend can
//! only subscribe once the webview has mounted — later. Without a cache, every
//! message written in that window is emitted to zero listeners and lost, leaving
//! entries stuck on whatever the config seeded (observed as "Starting") and logs
//! empty. The caches hold the latest status per app and the event backlog per
//! app so the frontend can fetch a snapshot on mount; live events then keep it
//! current.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

/// Event name emitted to the frontend for each status update.
pub const STATE_EVENT: &str = "app-state-update";
/// Event name emitted to the frontend for each captured log event.
pub const LOG_EVENT: &str = "app-log-update";

/// Command-line flag the core passes to the UI to enable IPC mode.
pub const IPC_FLAG: &str = "--ipc";

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

/// Splits `IPC_FLAG` out of an argument iterator.
///
/// Returns `(ipc_mode, remaining_args)`. Every occurrence of the flag is
/// removed, so the remaining arguments are safe to hand to the shared
/// [`kandel_config::RuntimeFlags`] parser.
pub fn split_ipc_flag<I, S>(args: I) -> (bool, Vec<String>)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut ipc_mode = false;
    let mut rest = Vec::new();
    for arg in args {
        let arg = arg.into();
        if arg == IPC_FLAG {
            ipc_mode = true;
        } else {
            rest.push(arg);
        }
    }
    (ipc_mode, rest)
}

/// Spawns a background thread that caches and forwards stdin lines to `app`.
///
/// Exits the app on EOF, which is how the UI shuts down when the core exits.
/// Only call this in IPC mode; see the module docs.
pub fn spawn_stdin_listener(app: AppHandle) {
    std::thread::Builder::new()
        .name("kandel-ui-ipc".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            let reader = BufReader::new(stdin.lock());
            for line in reader.lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        if let Some(cache) = app.try_state::<StatusCache>() {
                            cache.record(&line);
                        }
                        let is_log = serde_json::from_str::<serde_json::Value>(&line)
                            .ok()
                            .and_then(|v| {
                                v.get("kind").and_then(|k| k.as_str().map(str::to_string))
                            })
                            .map(|k| k == "logs")
                            .unwrap_or(false);
                        let event = if is_log { LOG_EVENT } else { STATE_EVENT };
                        let _ = app.emit(event, line);
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            // Core exited (stdin closed): close the UI too.
            app.exit(0);
        })
        .expect("spawning stdin listener thread");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_event_name_is_stable() {
        // The frontend depends on these exact event names.
        assert_eq!(STATE_EVENT, "app-state-update");
        assert_eq!(LOG_EVENT, "app-log-update");
    }

    #[test]
    fn split_detects_ipc_flag() {
        let (ipc, rest) = split_ipc_flag(["--ipc", "-f", "config.toml"]);
        assert!(ipc);
        assert_eq!(rest, ["-f", "config.toml"]);
    }

    #[test]
    fn split_without_flag_leaves_args_untouched() {
        let (ipc, rest) = split_ipc_flag(["-f", "config.toml", "-log-format", "json"]);
        assert!(!ipc);
        assert_eq!(rest, ["-f", "config.toml", "-log-format", "json"]);
    }

    #[test]
    fn split_removes_every_occurrence() {
        let (ipc, rest) = split_ipc_flag(["--ipc", "-f", "c.toml", "--ipc"]);
        assert!(ipc);
        assert_eq!(rest, ["-f", "c.toml"]);
    }

    #[test]
    fn split_emits_args_parseable_by_runtime_flags() {
        let (ipc, rest) = split_ipc_flag(["--ipc", "-f", "C:/tmp/config.toml"]);
        assert!(ipc);
        let flags = kandel_config::RuntimeFlags::parse(rest).unwrap();
        assert!(flags.config_path.ends_with("config.toml"));
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
}
