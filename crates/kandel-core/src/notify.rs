//! Tray balloon notifications.
//!
//! When a monitored app exhausts its retries, the core tells the user through a
//! **balloon attached to Kandel's own tray icon**, not a `MessageBox`.
//!
//! Why a balloon rather than a dialog:
//!
//! * It is **non-blocking**. A modal `MessageBox` runs on the monitor thread and
//!   freezes that app's supervision until someone clicks it; with several apps
//!   failing at once the dialogs also stack up.
//! * It carries **Kandel's icon** automatically, because the balloon belongs to
//!   the tray icon. A `MessageBox` shows a generic Windows error glyph — and the
//!   previous dialogs passed no owner window, so they could open behind other
//!   windows with no taskbar presence at all.
//!
//! A balloon is not a WinRT toast: toasts need a registered AUMID (i.e. an
//! installed Start Menu shortcut) and otherwise appear as an unknown app with no
//! icon. The core owns a tray icon already, so the balloon is both simpler and
//! better-behaved here. The trade-off is that Windows draws the balloon, so it
//! cannot take the app's design system, and Windows 10/11 ignores action buttons
//! on it.

use std::collections::VecDeque;
use std::sync::Mutex;

use windows::core::GUID;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_GUID, NIF_INFO, NIIF_ERROR, NIIF_INFO, NIIF_RESPECT_QUIET_TIME,
    NIIF_WARNING, NIM_MODIFY, NOTIFYICONDATAW,
};

/// The stable identity of Kandel's tray icon.
///
/// The tray icon is registered with this GUID (see `tray.rs`), so the balloon
/// below can address it by identity instead of by the crate's private counter
/// id. The value is arbitrary but must never change: it is what Windows keys the
/// icon on.
pub const TRAY_GUID: u128 = 0x6f1c8b2e_7a4d_4f3a_9c1e_2b8d5a7f0c31;

/// How prominently a notice should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Something failed.
    Error,
    /// Something needs attention but is not broken.
    Warning,
    /// Purely informational.
    Info,
}

impl Severity {
    fn info_flags(self) -> windows::Win32::UI::Shell::NOTIFY_ICON_INFOTIP_FLAGS {
        // `NIIF_RESPECT_QUIET_TIME` suppresses the balloon during Focus Assist /
        // quiet hours, which is what a well-behaved background app should do.
        (match self {
            Severity::Error => NIIF_ERROR,
            Severity::Warning => NIIF_WARNING,
            Severity::Info => NIIF_INFO,
        }) | NIIF_RESPECT_QUIET_TIME
    }
}

/// A notice waiting for the tray icon to become available.
struct Pending {
    severity: Severity,
    title: String,
    body: String,
}

/// How many notices are held before the tray exists. Bounded so a failing storm
/// cannot grow memory without limit.
const MAX_PENDING: usize = 8;

#[derive(Default)]
struct Inner {
    /// The tray icon's window, once it exists.
    hwnd: Option<isize>,
    /// Notices raised before the tray was ready.
    pending: VecDeque<Pending>,
}

/// Shows tray balloon notifications, buffering any that arrive before the tray
/// icon has been created.
///
/// The core reconciles the config (starting monitors) *before* it builds the
/// tray, so an app that fails immediately would otherwise have nowhere to
/// notify. Notices raised in that window are queued and flushed on
/// [`Notifier::attach`].
#[derive(Default)]
pub struct Notifier {
    inner: Mutex<Inner>,
}

impl Notifier {
    /// Creates a notifier with no tray attached yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds the notifier to the tray icon's window and flushes anything queued.
    ///
    /// `hwnd` is the tray's window handle as `tray-icon` reports it
    /// (`windows_sys`), which is the same pointer the `windows` crate uses.
    /// Called by the tray right after it builds the icon.
    pub fn attach(&self, hwnd: *mut std::ffi::c_void) {
        let mut inner = self.lock();

        // Flush anything raised before the tray existed, oldest first, so the
        // user reads the notices in the order they happened.
        while let Some(notice) = inner.pending.pop_front() {
            show(hwnd, notice.severity, &notice.title, &notice.body);
        }

        inner.hwnd = Some(hwnd as isize);
    }

    /// Raises a notice. Queues it when the tray is not ready yet.
    pub fn notify(&self, severity: Severity, title: &str, body: &str) {
        let mut inner = self.lock();

        match inner.hwnd {
            Some(raw) => show(raw as *mut _, severity, title, body),
            None => {
                if inner.pending.len() >= MAX_PENDING {
                    // Prefer the newest failures, which reflect the current
                    // state, and record what was dropped.
                    inner.pending.pop_front();
                    tracing::warn!(
                        component = "notify",
                        "dropping the oldest queued notice: too many raised before the tray was ready"
                    );
                }
                inner.pending.push_back(Pending {
                    severity,
                    title: title.to_string(),
                    body: body.to_string(),
                });
            }
        }
    }

    /// Notifies that an app has been marked Failed.
    pub fn app_failed(&self, name: &str, reason: &str) {
        self.notify(
            Severity::Error,
            &format!("{name} was marked Failed"),
            reason,
        );
    }

    /// A poisoned lock means another thread panicked mid-notify; the tray window
    /// is still usable, so recover rather than propagate a panic into a monitor
    /// thread.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Sends one balloon for the tray icon identified by [`TRAY_GUID`].
fn show(hwnd: *mut std::ffi::c_void, severity: Severity, title: &str, body: &str) {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND(hwnd),
        uFlags: NIF_INFO | NIF_GUID,
        guidItem: GUID::from_u128(TRAY_GUID),
        dwInfoFlags: severity.info_flags(),
        ..Default::default()
    };

    write_wide(&mut data.szInfoTitle, title);
    write_wide(&mut data.szInfo, body);

    let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    if !ok.as_bool() {
        tracing::warn!(
            component = "notify",
            title,
            "the shell refused the notification balloon"
        );
    }
}

/// Copies `s` into a fixed-size NUL-terminated UTF-16 buffer, truncating on a
/// character boundary so a surrogate pair is never split.
fn write_wide(dest: &mut [u16], s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    let cap = dest.len().saturating_sub(1);
    let take = units.len().min(cap);

    // If truncating would cut a surrogate pair, drop the lone high surrogate.
    let take = if take > 0 && (0xD800..0xDC00).contains(&units[take - 1]) {
        take - 1
    } else {
        take
    };

    dest[..take].copy_from_slice(&units[..take]);
    dest[take] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    #[test]
    fn writes_and_terminates() {
        let mut buf = [0u16; 16];
        write_wide(&mut buf, "hello");
        assert_eq!(read(&buf), "hello");
        assert_eq!(buf[5], 0);
    }

    #[test]
    fn truncates_to_fit_and_stays_terminated() {
        let mut buf = [0u16; 5];
        write_wide(&mut buf, "abcdefgh");
        assert_eq!(read(&buf), "abcd");
        assert_eq!(buf[4], 0);
    }

    #[test]
    fn never_splits_a_surrogate_pair() {
        // "😀" is one supplementary char = a surrogate pair. With room for only
        // one unit after the terminator, the pair must be dropped whole.
        let mut buf = [0u16; 2];
        write_wide(&mut buf, "😀");
        assert_eq!(read(&buf), "");
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn empty_input_is_just_a_terminator() {
        let mut buf = [0u16; 4];
        write_wide(&mut buf, "");
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn buffers_notices_until_attached() {
        let n = Notifier::new();
        // No tray yet: the notice must not be lost.
        n.notify(Severity::Error, "t", "b");
        assert_eq!(n.lock().pending.len(), 1);
    }

    #[test]
    fn pending_queue_is_bounded() {
        let n = Notifier::new();
        for i in 0..(MAX_PENDING + 4) {
            n.notify(Severity::Info, "t", &format!("{i}"));
        }
        let inner = n.lock();
        assert_eq!(inner.pending.len(), MAX_PENDING);
        // The newest survive.
        assert_eq!(
            inner.pending.back().unwrap().body,
            format!("{}", MAX_PENDING + 3)
        );
    }

    #[test]
    fn app_failed_names_the_app() {
        let n = Notifier::new();
        n.app_failed("sleeper", "exit code 1");
        let inner = n.lock();
        let notice = inner.pending.front().unwrap();
        assert!(notice.title.contains("sleeper"));
        assert!(notice.title.contains("Failed"));
        assert_eq!(notice.body, "exit code 1");
        assert_eq!(notice.severity, Severity::Error);
    }

    #[test]
    fn severity_maps_to_distinct_flags() {
        assert_ne!(Severity::Error.info_flags(), Severity::Info.info_flags());
        assert_ne!(Severity::Warning.info_flags(), Severity::Info.info_flags());
    }
}
