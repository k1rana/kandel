//! Kandel core: the resident process supervisor.
//!
//! The crate is split so that the portable reconciliation logic can be tested
//! on any host, while the Windows-specific process supervision lives behind
//! `#[cfg(windows)]`.

pub mod ipc;
pub mod logs;
pub mod reconciler;
pub mod status;

#[cfg(windows)]
pub mod supervisor;

#[cfg(windows)]
pub mod autostart;
#[cfg(windows)]
pub mod dialog;
#[cfg(windows)]
pub mod logging;
#[cfg(windows)]
pub mod monitor;
#[cfg(windows)]
pub mod notify;
#[cfg(windows)]
pub mod openwith;
#[cfg(windows)]
pub mod shell;
#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod watcher;
#[cfg(windows)]
pub mod win;

#[cfg(windows)]
pub mod app;
#[cfg(windows)]
pub mod ipc_server;

pub use ipc::CoreEvent;
pub use logs::{LogEvent, LogStore};
pub use status::{AppState, MonitorStatus};
