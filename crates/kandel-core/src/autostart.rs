//! Windows sign-in auto-start management via `HKCU\...\Run`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE};
use winreg::RegKey;

use kandel_config::{LogFormat, RuntimeFlags};

use crate::win::cmdline;

const AUTO_START_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const AUTO_START_VALUE_NAME: &str = "Kandel";

/// Manages logon auto-start registration for the current user.
#[derive(Debug, Clone)]
pub struct AutoStartManager {
    executable_path: PathBuf,
    runtime_flags: RuntimeFlags,
}

impl AutoStartManager {
    /// Creates a manager bound to the given runtime flags.
    pub fn new(runtime_flags: RuntimeFlags) -> Result<Self> {
        let executable_path = std::env::current_exe().context("resolving current executable")?;
        Ok(AutoStartManager {
            executable_path,
            runtime_flags,
        })
    }

    /// Returns whether auto-start is enabled for this executable.
    pub fn enabled(&self) -> Result<bool> {
        let Some(value) = self.read_value()? else {
            return Ok(false);
        };
        Ok(registered_executable_matches(&value, &self.executable_path))
    }

    /// Enables or disables auto-start for this executable.
    pub fn set_enabled(&self, enabled: bool) -> Result<()> {
        if !enabled {
            return self.delete_value();
        }
        self.write_value(&build_auto_start_command(
            &self.executable_path,
            &self.runtime_flags,
        ))
    }

    fn read_value(&self) -> Result<Option<String>> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(AUTO_START_KEY_PATH, KEY_QUERY_VALUE) {
            Ok(key) => key,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("opening Run key"),
        };

        match key.get_value::<String, _>(AUTO_START_VALUE_NAME) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("reading Run value"),
        }
    }

    fn write_value(&self, value: &str) -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(AUTO_START_KEY_PATH)
            .context("creating Run key")?;
        key.set_value(AUTO_START_VALUE_NAME, &value)
            .context("writing Run value")
    }

    fn delete_value(&self) -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(AUTO_START_KEY_PATH, KEY_SET_VALUE) {
            Ok(key) => key,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).context("opening Run key"),
        };
        match key.delete_value(AUTO_START_VALUE_NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context("deleting Run value"),
        }
    }
}

/// Builds the `Run` value command line for the given executable and flags.
pub fn build_auto_start_command(executable: &Path, flags: &RuntimeFlags) -> String {
    let mut parts = vec![cmdline::escape_arg(&executable.to_string_lossy())];
    parts.push("-f".to_string());
    parts.push(cmdline::escape_arg(&flags.config_path.to_string_lossy()));
    if let Some(log_file) = &flags.log_file {
        parts.push("-log-file".to_string());
        parts.push(cmdline::escape_arg(&log_file.to_string_lossy()));
    }
    let format = match flags.log_format {
        LogFormat::Json => "json",
        LogFormat::Text => "text",
    };
    parts.push("-log-format".to_string());
    parts.push(format.to_string());
    parts.join(" ")
}

/// Returns true when `command_line`'s first token refers to `executable`.
///
/// Arguments are ignored; comparison is case-insensitive and path-normalized.
pub fn registered_executable_matches(command_line: &str, executable: &Path) -> bool {
    let trimmed = command_line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(first) = split_first_token(trimmed) else {
        return false;
    };
    normalized_path_equals(&first, &executable.to_string_lossy())
}

/// Splits the first argument out of a Windows command line, honoring quotes.
fn split_first_token(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let n = chars.len();

    while i < n && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= n {
        return None;
    }

    if chars[i] == '"' {
        i += 1;
        let mut token = String::new();
        while i < n {
            match chars[i] {
                '"' => break,
                '\\' if i + 1 < n && chars[i + 1] == '"' => {
                    token.push('"');
                    i += 2;
                    continue;
                }
                c => token.push(c),
            }
            i += 1;
        }
        (!token.is_empty()).then_some(token)
    } else {
        let mut token = String::new();
        while i < n && !chars[i].is_whitespace() {
            token.push(chars[i]);
            i += 1;
        }
        (!token.is_empty()).then_some(token)
    }
}

fn normalized_path_equals(left: &str, right: &str) -> bool {
    match (normalize_path(left), normalize_path(right)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(&b),
        _ => false,
    }
}

fn normalize_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        Some(candidate.to_string_lossy().into_owned())
    } else {
        std::path::absolute(candidate)
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(config: &str, log_file: Option<&str>, format: LogFormat) -> RuntimeFlags {
        RuntimeFlags {
            config_path: PathBuf::from(config),
            log_file: log_file.map(PathBuf::from),
            log_format: format,
            open_ui: false,
        }
    }

    #[test]
    fn build_command_quotes_exe_and_flags() {
        let command = build_auto_start_command(
            Path::new(r"C:\Program Files\Kandel\kandel.exe"),
            &flags(
                r"C:\Users\Daisu\AppData\Roaming\Kandel\config file.toml",
                Some(r"C:\Logs\kandel log.txt"),
                LogFormat::Json,
            ),
        );
        assert_eq!(
            command,
            r#""C:\Program Files\Kandel\kandel.exe" -f "C:\Users\Daisu\AppData\Roaming\Kandel\config file.toml" -log-file "C:\Logs\kandel log.txt" -log-format json"#
        );
    }

    #[test]
    fn build_command_omits_empty_optional_flags() {
        let command = build_auto_start_command(
            Path::new(r"C:\Kandel\kandel.exe"),
            &flags(
                r"C:\Users\Daisu\.config\kandel\config.toml",
                None,
                LogFormat::Text,
            ),
        );
        assert_eq!(
            command,
            r"C:\Kandel\kandel.exe -f C:\Users\Daisu\.config\kandel\config.toml -log-format text"
        );
    }

    #[test]
    fn matches_ignores_arguments() {
        let command =
            r#""C:\Program Files\Kandel\kandel.exe" -f C:\different-config.toml -log-format json"#;
        assert!(registered_executable_matches(
            command,
            Path::new(r"C:\Program Files\Kandel\kandel.exe")
        ));
    }

    #[test]
    fn matches_is_case_insensitive() {
        assert!(registered_executable_matches(
            r#""c:\program files\kandel\KANDEL.EXE" -f x"#,
            Path::new(r"C:\Program Files\Kandel\kandel.exe")
        ));
    }

    #[test]
    fn rejects_different_executable() {
        let command = r#""C:\Program Files\Old Kandel\kandel.exe" -f C:\config.toml"#;
        assert!(!registered_executable_matches(
            command,
            Path::new(r"C:\Program Files\Kandel\kandel.exe")
        ));
    }

    #[test]
    fn rejects_empty_value() {
        assert!(!registered_executable_matches(
            "   ",
            Path::new(r"C:\Program Files\Kandel\kandel.exe")
        ));
    }

    #[test]
    fn split_first_token_handles_quotes() {
        assert_eq!(
            split_first_token(r#""C:\a b\app.exe" -f x"#).as_deref(),
            Some(r"C:\a b\app.exe")
        );
        assert_eq!(
            split_first_token(r"C:\app.exe -f x").as_deref(),
            Some(r"C:\app.exe")
        );
    }
}
