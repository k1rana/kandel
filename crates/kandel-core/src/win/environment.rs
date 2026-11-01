//! Refreshing this process's environment from the OS.
//!
//! The core's environment is a snapshot frozen at launch, so a `PATH` directory
//! added later stays invisible until a restart. `CreateEnvironmentBlock` rebuilds
//! the current user's environment from the registry on every call; this module
//! applies that result to the process.

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::TOKEN_QUERY;
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Re-reads the current user's environment and applies it to this process.
///
/// Call before a config load and before each launch so a `PATH` (or other)
/// variable changed since the core started is picked up without a restart.
/// Best-effort: on failure the existing environment is kept and a warning logged.
pub fn refresh_process_environment() {
    match current_environment_block() {
        Ok(entries) => apply(entries),
        Err(e) => tracing::warn!(
            component = "environment",
            error = %e,
            "could not refresh the process environment; keeping the current values"
        ),
    }
}

/// Reads the current user's environment as ordered `(name, value)` pairs.
fn current_environment_block() -> anyhow::Result<Vec<(String, String)>> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;

        // A token is required: a null token reports the *calling process's*
        // environment, the stale snapshot being replaced.
        let mut block: *mut c_void = std::ptr::null_mut();
        let created = CreateEnvironmentBlock(&mut block, Some(token), false);
        // The token is only needed to build the block; close it either way.
        let _ = CloseHandle(token);
        created?;

        if block.is_null() {
            anyhow::bail!("CreateEnvironmentBlock returned a null block");
        }

        let entries = read_block(block as *const u16);
        let _ = DestroyEnvironmentBlock(block);
        Ok(entries)
    }
}

/// Copies a double-NUL-terminated UTF-16 block into owned entries.
///
/// # Safety
///
/// `block` must point at a valid `CreateEnvironmentBlock` result that is
/// terminated by two consecutive NULs.
unsafe fn read_block(block: *const u16) -> Vec<(String, String)> {
    let mut len = 0usize;
    loop {
        // Stop at the block's double NUL; the cap guards a block that never ends.
        if *block.add(len) == 0 && *block.add(len + 1) == 0 {
            len += 1;
            break;
        }
        len += 1;
        if len >= 1 << 20 {
            break;
        }
    }
    let encoded = std::slice::from_raw_parts(block, len);
    parse_block(encoded)
}

/// Splits a UTF-16 environment block into `(name, value)` pairs.
///
/// Entries are NUL-separated and end with an empty entry. Drive-relative
/// variables (`=C:=C:\dir`) have an empty name and are skipped: the usual
/// environment APIs cannot set them.
fn parse_block(encoded: &[u16]) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for chunk in encoded.split(|&c| c == 0) {
        if chunk.is_empty() {
            continue;
        }
        let entry = String::from_utf16_lossy(chunk);
        let Some((name, value)) = entry.split_once('=') else {
            continue;
        };
        if name.is_empty() || name.contains('=') {
            continue;
        }
        entries.push((name.to_string(), value.to_string()));
    }
    entries
}

/// Overrides matching variables and adds new ones, leaving variables the block
/// does not contain untouched, so a value the launcher set survives.
fn apply(entries: Vec<(String, String)>) {
    for (name, value) in entries {
        // `set_var` panics on an empty name or one containing `=`; values cannot
        // contain NUL because the block was split on it.
        if name.is_empty() || name.contains('=') {
            continue;
        }
        std::env::set_var(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn parse_block_splits_entries_and_skips_drive_variables() {
        let entries = parse_block(&block("A=1\0=C:=C:\\dir\0PATH=C:\\bin\0\0"));
        assert_eq!(
            entries,
            vec![
                ("A".to_string(), "1".to_string()),
                ("PATH".to_string(), "C:\\bin".to_string()),
            ]
        );
    }

    #[test]
    fn parse_block_ignores_entries_without_equals() {
        let entries = parse_block(&block("NOEQUALS\0B=2\0\0"));
        assert_eq!(entries, vec![("B".to_string(), "2".to_string())]);
    }

    #[test]
    fn parse_block_keeps_equals_inside_the_value() {
        let entries = parse_block(&block("TOKEN=a=b=c\0\0"));
        assert_eq!(entries, vec![("TOKEN".to_string(), "a=b=c".to_string())]);
    }

    #[test]
    fn current_block_exposes_the_path() {
        // Proves the live read works against the real OS, not just the parser.
        let entries = current_environment_block().expect("reading the current environment");
        assert!(
            entries
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("PATH")),
            "the environment block should contain PATH"
        );
    }

    #[test]
    fn refresh_is_best_effort_and_leaves_path_readable() {
        refresh_process_environment();
        assert!(std::env::var_os("PATH").is_some());
    }
}
