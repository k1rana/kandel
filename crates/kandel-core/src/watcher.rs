//! Watches `config.toml` for changes and triggers reconciliation.
//!
//! Filesystem events are debounced (multiple events for a single save are
//! coalesced into one reload), and reloads are retried a few times to tolerate
//! transient "file in use" errors from atomic writes performed by external
//! tools.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecursiveMode, Watcher};

use kandel_config::{load_config_lenient, LoadedConfig};

/// How long to wait after the last event before reloading.
pub const DEBOUNCE_DELAY: Duration = Duration::from_millis(500);
/// Number of times a reload is attempted before giving up.
pub const RELOAD_MAX_ATTEMPTS: usize = 3;
/// Delay between reload attempts.
pub const RELOAD_RETRY_DELAY: Duration = Duration::from_millis(200);

/// Spawns a background thread that watches the directory containing
/// `config_path` and calls `on_change` after each debounced change.
///
/// Watching the directory (rather than the file) handles atomic writes that
/// replace the file via rename. Returns a join handle; the thread runs until
/// the watcher or channel fails.
pub fn spawn_config_watcher<F>(config_path: PathBuf, on_change: F) -> std::io::Result<()>
where
    F: Fn() + Send + 'static,
{
    let dir = config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let filename = config_path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(std::io::Error::other)?;

    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(std::io::Error::other)?;

    thread::Builder::new()
        .name("kandel-config-watcher".into())
        .spawn(move || {
            // Keep the watcher alive for the thread's lifetime.
            let _watcher = watcher;
            run_watch_loop(&rx, &filename, &on_change);
        })?;

    Ok(())
}

/// The debouncing event loop. Separated for clarity and to keep the callback
/// generic.
fn run_watch_loop<F>(rx: &mpsc::Receiver<notify::Result<Event>>, filename: &str, on_change: &F)
where
    F: Fn(),
{
    loop {
        // Block for the first event.
        match rx.recv() {
            Ok(Ok(event)) => {
                if !event_is_relevant(&event, filename) {
                    continue;
                }
            }
            Ok(Err(_)) => continue,
            Err(_) => return,
        }

        // Coalesce further events until the debounce window elapses.
        let mut deadline = Instant::now() + DEBOUNCE_DELAY;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match rx.recv_timeout(deadline - now) {
                Ok(Ok(event)) => {
                    if event_is_relevant(&event, filename) {
                        deadline = Instant::now() + DEBOUNCE_DELAY;
                    }
                }
                Ok(Err(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        on_change();
    }
}

/// Returns true when `event` refers to the config file and is a write, create,
/// or rename.
pub fn event_is_relevant(event: &Event, filename: &str) -> bool {
    if filename.is_empty() {
        return false;
    }
    let touches_config = event
        .paths
        .iter()
        .any(|p| p.file_name().map(|f| f == filename).unwrap_or(false));
    if !touches_config {
        return false;
    }
    matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Any
    )
}

/// Loads `config_path` leniently, retrying transient failures.
///
/// Returns the last error after [`RELOAD_MAX_ATTEMPTS`] attempts. A file that
/// parses but contains bad entries is still an `Ok`: they come back in
/// [`LoadedConfig::invalid`].
pub fn load_with_retry(config_path: &Path) -> Result<LoadedConfig, kandel_config::ConfigError> {
    let mut last_err = None;
    for attempt in 0..RELOAD_MAX_ATTEMPTS {
        match load_config_lenient(config_path) {
            Ok(loaded) => return Ok(loaded),
            Err(e) => {
                tracing::warn!(
                    component = "reload",
                    attempt = attempt + 1,
                    error = %e,
                    "config reload attempt failed"
                );
                last_err = Some(e);
                if attempt + 1 < RELOAD_MAX_ATTEMPTS {
                    thread::sleep(RELOAD_RETRY_DELAY);
                }
            }
        }
    }
    Err(last_err.expect("at least one attempt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn event(kind: EventKind, path: &str) -> Event {
        Event {
            kind,
            paths: vec![PathBuf::from(path)],
            attrs: Default::default(),
        }
    }

    #[test]
    fn relevant_for_write_to_config() {
        let e = event(
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            r"C:\cfg\config.toml",
        );
        assert!(event_is_relevant(&e, "config.toml"));
    }

    #[test]
    fn relevant_for_create_and_rename() {
        let create = event(
            EventKind::Create(notify::event::CreateKind::File),
            "/cfg/config.toml",
        );
        assert!(event_is_relevant(&create, "config.toml"));

        let rename = event(
            EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Any)),
            "/cfg/config.toml",
        );
        assert!(event_is_relevant(&rename, "config.toml"));
    }

    #[test]
    fn irrelevant_for_other_file() {
        let e = event(
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            "/cfg/other.txt",
        );
        assert!(!event_is_relevant(&e, "config.toml"));
    }

    #[test]
    fn irrelevant_for_remove() {
        let e = event(
            EventKind::Remove(notify::event::RemoveKind::File),
            "/cfg/config.toml",
        );
        assert!(!event_is_relevant(&e, "config.toml"));
    }

    #[test]
    fn irrelevant_for_access() {
        let e = event(
            EventKind::Access(notify::event::AccessKind::Any),
            "/cfg/config.toml",
        );
        assert!(!event_is_relevant(&e, "config.toml"));
    }

    #[test]
    fn load_with_retry_reports_error_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        assert!(load_with_retry(&missing).is_err());
    }

    #[test]
    fn watcher_fires_callback_on_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, "[\"a\"]\ncommand = \"cmd.exe\"\n").unwrap();

        let (tx, rx) = channel::<()>();
        spawn_config_watcher(config_path.clone(), move || {
            let _ = tx.send(());
        })
        .unwrap();

        // Give the watcher a moment to start, then perform an atomic write.
        thread::sleep(Duration::from_millis(200));
        let tmp = dir.path().join(".tmp-config.toml");
        std::fs::write(
            &tmp,
            "[\"a\"]\ncommand = \"cmd.exe\"\n[\"b\"]\ncommand = \"cmd.exe\"\n",
        )
        .unwrap();
        std::fs::rename(&tmp, &config_path).unwrap();

        let got = rx.recv_timeout(Duration::from_secs(5));
        assert!(got.is_ok(), "watcher did not fire within the timeout");
    }
}
