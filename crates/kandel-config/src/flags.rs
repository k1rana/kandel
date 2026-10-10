//! Shared runtime flags and default paths used by both binaries.

use std::path::PathBuf;

/// Common runtime options parsed from the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeFlags {
    /// Path to `config.toml`.
    pub config_path: PathBuf,
    /// Optional log file. When empty, logs go to stderr.
    pub log_file: Option<PathBuf>,
    /// Log format: `text` or `json`.
    pub log_format: LogFormat,
    /// Open the management UI at startup instead of waiting for a tray click.
    pub open_ui: bool,
}

/// Supported log output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-readable text (default).
    #[default]
    Text,
    /// Structured JSON.
    Json,
}

impl LogFormat {
    /// Parses a `-log-format` value.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "text" => Ok(LogFormat::Text),
            "json" => Ok(LogFormat::Json),
            other => Err(format!(
                "unsupported format {other:?} (allowed: text, json)"
            )),
        }
    }
}

/// The default config location: `%USERPROFILE%\.config\kandel\config.toml`.
///
/// On non-Windows hosts this follows the XDG convention via the home directory,
/// which keeps the crate testable off-Windows.
pub fn default_config_path() -> Option<PathBuf> {
    let home = home_dir()?;
    Some(home.join(".config").join("kandel").join("config.toml"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
}

impl RuntimeFlags {
    /// Parses flags from an iterator of arguments (excluding the program name).
    ///
    /// Supported flags: `-f <path>`, `-log-file <path>`, `-log-format <text|json>`,
    /// and `-ui` (open the management UI at startup).
    pub fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut config_path: Option<String> = None;
        let mut log_file: Option<String> = None;
        let mut log_format = LogFormat::Text;
        let mut open_ui = false;

        let mut iter = args.into_iter().map(Into::into);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-f" => {
                    config_path = Some(
                        iter.next()
                            .ok_or_else(|| "flag -f requires a value".to_string())?,
                    );
                }
                "-log-file" => {
                    log_file = Some(
                        iter.next()
                            .ok_or_else(|| "flag -log-file requires a value".to_string())?,
                    );
                }
                "-log-format" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "flag -log-format requires a value".to_string())?;
                    log_format = LogFormat::parse(&value)?;
                }
                // Opens the management UI immediately instead of waiting for a
                // tray click. Useful in development, where starting the UI is
                // otherwise only reachable through the tray.
                "-ui" => open_ui = true,
                other => return Err(format!("unexpected argument {other:?}")),
            }
        }

        let config_path = match config_path {
            Some(p) => PathBuf::from(p),
            None => default_config_path()
                .ok_or_else(|| "could not determine default config path".to_string())?,
        };

        let config_path = absolute(&config_path);

        Ok(RuntimeFlags {
            config_path,
            log_file: log_file.map(PathBuf::from),
            log_format,
            open_ui,
        })
    }

    /// Builds flags with an explicit config path (used for tests and embedding).
    pub fn with_config_path(config_path: PathBuf) -> Self {
        RuntimeFlags {
            config_path: absolute(&config_path),
            log_file: None,
            log_format: LogFormat::Text,
            open_ui: false,
        }
    }
}

fn absolute(path: &PathBuf) -> PathBuf {
    if path.is_absolute() {
        path.clone()
    } else {
        std::path::absolute(path).unwrap_or_else(|_| path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_path_flag() {
        let flags = RuntimeFlags::parse(["-f", "C:/tmp/custom.toml"]).unwrap();
        assert!(flags.config_path.is_absolute());
        assert!(flags.config_path.ends_with("custom.toml"));
    }

    #[test]
    fn parses_log_flags() {
        let flags = RuntimeFlags::parse([
            "-f",
            "/tmp/c.toml",
            "-log-format",
            "json",
            "-log-file",
            "/tmp/l.log",
        ])
        .unwrap();
        assert_eq!(flags.log_format, LogFormat::Json);
        assert_eq!(flags.log_file, Some(PathBuf::from("/tmp/l.log")));
    }

    #[test]
    fn defaults_to_text_format() {
        let flags = RuntimeFlags::parse(["-f", "/tmp/c.toml"]).unwrap();
        assert_eq!(flags.log_format, LogFormat::Text);
    }

    #[test]
    fn rejects_unknown_format() {
        let err = RuntimeFlags::parse(["-f", "/tmp/c.toml", "-log-format", "yaml"]).unwrap_err();
        assert!(err.contains("yaml"));
    }

    #[test]
    fn rejects_missing_flag_value() {
        assert!(RuntimeFlags::parse(["-f"]).is_err());
        assert!(RuntimeFlags::parse(["-log-file"]).is_err());
    }

    #[test]
    fn ui_flag_defaults_to_closed() {
        let flags = RuntimeFlags::parse(["-f", "/tmp/c.toml"]).unwrap();
        assert!(!flags.open_ui);
    }

    #[test]
    fn ui_flag_opens_the_ui() {
        let flags = RuntimeFlags::parse(["-f", "/tmp/c.toml", "-ui"]).unwrap();
        assert!(flags.open_ui);
    }

    #[test]
    fn rejects_unknown_argument() {
        assert!(RuntimeFlags::parse(["--bogus"]).is_err());
    }

    #[test]
    fn default_config_path_ends_with_kandel() {
        let path = default_config_path().expect("home dir");
        assert!(path.ends_with(".config/kandel/config.toml"));
    }
}
