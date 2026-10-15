//! Named-pipe client for the core's status stream (Windows).
//!
//! Attaches to `kandel_config::pipe_name` and reads newline-delimited JSON,
//! reporting end-of-stream so the caller can reconnect. The reader stays tiny:
//! all the policy (backoff, snapshot handling, caching) lives in [`crate::ipc`].
//!
//! On non-Windows hosts the functions are stubs, so the crate still builds for
//! frontend checks and tests.

use std::io::BufRead;

/// A connected pipe, yielding one line at a time.
pub struct PipeClient {
    #[cfg(windows)]
    reader: std::io::BufReader<std::fs::File>,
}

/// Opens the pipe `name`, waiting briefly for the core to publish it.
///
/// Returns an error when the core is not there yet (no such pipe, or every
/// instance is busy); the caller retries.
#[cfg(windows)]
pub fn connect(name: &str) -> std::io::Result<PipeClient> {
    use std::os::windows::io::FromRawHandle;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING,
    };
    use windows::Win32::System::Pipes::WaitNamedPipeW;

    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

    // Give a just-publishing core a moment instead of failing immediately. A
    // short wait keeps startup snappy while still covering the race where the UI
    // opens as the core is being launched.
    let _ = unsafe { WaitNamedPipeW(PCWSTR(wide.as_ptr()), 500) };

    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            // The core publishes an outbound pipe: read-only here. Requesting
            // write access would be refused with ERROR_ACCESS_DENIED.
            GENERIC_READ.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| std::io::Error::other(format!("opening {name}: {e}")))?;

    // Take ownership of the handle as a `File`, so `BufReader` (and its `Drop`)
    // handle reading and closing for us.
    let file = unsafe { std::fs::File::from_raw_handle(handle.0) };
    Ok(PipeClient {
        reader: std::io::BufReader::new(file),
    })
}

/// Non-Windows stub; never reached, since the reader only runs on Windows.
#[cfg(not(windows))]
pub fn connect(_name: &str) -> std::io::Result<PipeClient> {
    Err(std::io::Error::other("named pipes are Windows-only"))
}

impl PipeClient {
    /// Reads one line, or `None` when the core detached or closed the pipe.
    #[cfg(windows)]
    pub fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(line),
            Err(_) => None,
        }
    }

    #[cfg(not(windows))]
    pub fn read_line(&mut self) -> Option<String> {
        None
    }
}

/// The Windows session id of this process, for building the pipe name.
#[cfg(windows)]
pub fn current_session_id() -> u32 {
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::GetCurrentProcessId;

    let mut sid = 0u32;
    // Best-effort, mirroring the core: a failure means session 0, which only
    // matters on a host where nothing else works either.
    let _ = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut sid) };
    sid
}

#[cfg(not(windows))]
pub fn current_session_id() -> u32 {
    0
}
