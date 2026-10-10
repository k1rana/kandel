//! Windows platform primitives used by the supervisor.

pub mod cmdline;
pub mod console;
pub mod process;
pub mod window;

use windows::Win32::Foundation::HANDLE;

/// Converts a string to a NUL-terminated UTF-16 buffer for `*W` APIs.
pub fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A `HANDLE` wrapper that is safe to move between threads.
///
/// Win32 kernel object handles are process-wide and may be used from any
/// thread; the raw `HANDLE` type is not `Send`/`Sync` only because it wraps a
/// pointer. This wrapper asserts the invariant the supervisor relies on.
#[derive(Debug, Clone, Copy)]
pub struct SendHandle(pub HANDLE);

// SAFETY: kernel object handles are valid across threads within a process.
unsafe impl Send for SendHandle {}
// SAFETY: see above.
unsafe impl Sync for SendHandle {}
