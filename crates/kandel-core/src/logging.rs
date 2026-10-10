//! Logging setup using `tracing`.

use std::fs::OpenOptions;
use std::sync::Mutex;

use anyhow::{Context, Result};
use kandel_config::{LogFormat, RuntimeFlags};

/// Installs the global `tracing` subscriber according to `flags`.
///
/// Logs go to `flags.log_file` in append mode when set, otherwise to stderr.
/// Calling this more than once is harmless; later calls are ignored.
pub fn configure(flags: &RuntimeFlags) -> Result<()> {
    let file = match &flags.log_file {
        Some(path) => Some(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("opening log file {}", path.display()))?,
        )),
        None => None,
    };

    // `try_init` fails only when a global subscriber is already installed.
    let _ = match (flags.log_format, file) {
        (LogFormat::Text, None) => tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .try_init(),
        (LogFormat::Json, None) => tracing_subscriber::fmt()
            .json()
            .with_writer(std::io::stderr)
            .try_init(),
        (LogFormat::Text, Some(file)) => tracing_subscriber::fmt().with_writer(file).try_init(),
        (LogFormat::Json, Some(file)) => tracing_subscriber::fmt()
            .json()
            .with_writer(file)
            .try_init(),
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kandel_config::LogFormat;
    use std::path::PathBuf;

    #[test]
    fn configure_with_stderr_does_not_error() {
        let flags = RuntimeFlags {
            config_path: PathBuf::from("/tmp/config.toml"),
            log_file: None,
            log_format: LogFormat::Text,
            open_ui: false,
        };
        configure(&flags).unwrap();
    }

    #[test]
    fn writes_json_log_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("kandel.log");
        // Use a dedicated subscriber rather than the global one so the test is
        // deterministic and does not depend on global state.
        {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .unwrap();
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_writer(Mutex::new(file))
                .finish();
            tracing::subscriber::with_default(subscriber, || {
                tracing::info!(component = "test", "hello json");
            });
        }
        let contents = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            contents.contains("\"message\":\"hello json\""),
            "{contents}"
        );
        assert!(contents.contains("component"), "{contents}");
    }
}
