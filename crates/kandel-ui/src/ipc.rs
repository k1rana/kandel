//! Reads line-delimited JSON monitor status from stdin (the core process) and
//! forwards each update to the frontend as an `app-state-update` event.
//!
//! This listener is only active in **IPC mode**, which the core enables when it
//! launches the UI as its child with a piped stdin. In that mode, EOF on stdin
//! means the core exited, so the UI shuts itself down. When the UI is started
//! any other way (for example by `tauri dev`, or manually), IPC mode is off and
//! stdin is left alone — otherwise a non-piped stdin would report EOF
//! immediately and close the window.
//!
//! # Why statuses are cached
//!
//! The core starts writing statuses the moment it launches the UI, but the
//! frontend can only subscribe once the webview has mounted — later. Without a
//! cache, every status written in that window is emitted to zero listeners and
//! lost, leaving entries stuck on whatever the config seeded (observed as
//! "Starting"). The cache holds the latest status per app so the frontend can
//! fetch a snapshot on mount; live events then keep it current. Because the
//! cache only ever moves forward, applying a snapshot after subscribing can
//! never regress a newer value that already arrived by event.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

/// Event name emitted to the frontend for each status update.
pub const STATE_EVENT: &str = "app-state-update";

/// Command-line flag the core passes to the UI to enable IPC mode.
pub const IPC_FLAG: &str = "--ipc";

/// The latest status line per app name.
///
/// Keyed by app name so repeated updates for one app replace rather than
/// accumulate. Stores the raw JSON line the core sent, so the snapshot the
/// frontend fetches is byte-identical to an event payload.
#[derive(Default)]
pub struct StatusCache(Mutex<HashMap<String, String>>);

impl StatusCache {
    /// Records the latest status line for the app it names.
    ///
    /// A line that is not valid JSON, or has no `name`, is still forwarded to
    /// the frontend (which ignores it) but not cached, since there is nothing
    /// to key it on.
    pub fn record(&self, line: &str) {
        let Some(name) = status_name(line) else {
            return;
        };
        if let Ok(mut cache) = self.0.lock() {
            cache.insert(name, line.to_string());
        }
    }

    /// Returns every cached status line. Order is unspecified; the frontend
    /// sorts by name for display.
    pub fn snapshot(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|cache| cache.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Removes the cached status for `name` (used when an entry is deleted).
    pub fn remove(&self, name: &str) {
        if let Ok(mut cache) = self.0.lock() {
            cache.remove(name);
        }
    }
}

/// Extracts the `name` field from a status line, if present.
fn status_name(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value.get("name")?.as_str().map(str::to_string)
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
                        let _ = app.emit(STATE_EVENT, line);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_event_name_is_stable() {
        // The frontend depends on this exact event name.
        assert_eq!(STATE_EVENT, "app-state-update");
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
    fn status_name_reads_the_name_field() {
        assert_eq!(
            status_name(r#"{"name":"api","state":"Running"}"#).as_deref(),
            Some("api")
        );
    }

    #[test]
    fn status_name_is_none_without_a_name() {
        assert_eq!(status_name(r#"{"state":"Running"}"#), None);
        assert_eq!(status_name("not json"), None);
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
    fn cache_ignores_lines_it_cannot_key() {
        let cache = StatusCache::default();
        cache.record("not json");
        cache.record(r#"{"state":"Running"}"#);
        assert!(cache.snapshot().is_empty());
    }

    #[test]
    fn cache_remove_drops_one_entry() {
        let cache = StatusCache::default();
        cache.record(r#"{"name":"api","state":"Running"}"#);
        cache.record(r#"{"name":"web","state":"Running"}"#);
        cache.remove("api");
        let snap = cache.snapshot();
        assert_eq!(snap.len(), 1);
        assert!(snap[0].contains("web"));
    }
}
