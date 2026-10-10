//! System tray icon, menu, and the main message loop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

use crate::autostart::AutoStartManager;
use crate::dialog::show_error_dialog;
use crate::notify::{Notifier, TRAY_GUID};

/// Embedded tray icon (PNG). Generated from `logo.png` at build time.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// Callbacks invoked from the tray menu.
pub struct TrayCallbacks {
    /// Opens the management UI.
    pub on_open_ui: Box<dyn Fn() + Send + 'static>,
    /// Opens `config.toml` with the Windows "Open with" dialog.
    pub on_open_config_with: Box<dyn Fn() + Send + 'static>,
    /// Stops all monitors and exits.
    pub on_quit: Box<dyn Fn() + Send + 'static>,
}

/// The action a tray input event should trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Open the management UI.
    OpenUi,
    /// Do nothing.
    Ignore,
}

/// Decides what a tray mouse event should do.
///
/// Split out from the message loop so the gesture rules are testable without a
/// live tray.
///
/// Only a left **double-click** opens the UI. A single left-click is
/// deliberately ignored: Windows delivers a double-click as
/// `DOWN, UP, DBLCLK, UP`, so treating a single click as "open" as well would
/// fire the open action three times per double-click. Right-clicks are ignored
/// here because the tray library owns them for the context menu.
pub fn action_for_icon_event(event: &TrayIconEvent, icon_id: &tray_icon::TrayIconId) -> TrayAction {
    match event {
        TrayIconEvent::DoubleClick { id, button, .. }
            if id == icon_id && *button == MouseButton::Left =>
        {
            TrayAction::OpenUi
        }
        _ => TrayAction::Ignore,
    }
}

/// Builds the tray icon and runs the message loop until the user quits.
///
/// `version` is shown in a disabled header entry. `auto_start` enables the
/// auto-start toggle when present. `notifier` receives the tray's window so
/// balloon notices can be shown.
pub fn run(
    version: &str,
    auto_start: Option<&AutoStartManager>,
    notifier: Arc<Notifier>,
    callbacks: TrayCallbacks,
) -> Result<()> {
    let menu = Menu::new();

    let header = MenuItem::new(format!("Kandel v{version}"), false, None);
    menu.append(&header)?;
    menu.append(&PredefinedMenuItem::separator())?;

    let open = MenuItem::new("Open Settings", true, None);
    menu.append(&open)?;

    let open_config = MenuItem::new("Open config with...", true, None);
    menu.append(&open_config)?;

    let auto_start_item = auto_start.map(|manager| {
        let enabled = manager.enabled().unwrap_or(false);
        CheckMenuItem::new("Auto-start Kandel", true, enabled, None)
    });
    if let Some(item) = &auto_start_item {
        menu.append(item)?;
        menu.append(&PredefinedMenuItem::separator())?;
    }

    let quit = MenuItem::new("Quit", true, None);
    menu.append(&quit)?;

    let icon = load_icon(ICON_PNG)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Kandel Process Monitor — double-click to open settings")
        .with_icon(icon)
        // A stable GUID gives the icon a durable identity that the balloon
        // notifier can address by name, instead of the crate's private counter.
        .with_guid(TRAY_GUID)
        // The menu belongs to right-click only. Left-click means "open the
        // window", and leaving this at its `true` default would make a
        // left-click pop the menu instead.
        .with_menu_on_left_click(false)
        .build()
        .context("building tray icon")?;

    // The tray exists now, so any notice raised during startup can be shown.
    notifier.attach(tray.window_handle());

    // Cache ids for comparison inside the loop.
    let tray_id = tray.id().clone();
    let open_id = open.id().clone();
    let open_config_id = open_config.id().clone();
    let quit_id = quit.id().clone();
    let auto_start_id = auto_start_item.as_ref().map(|i| i.id().clone());

    let mut quitting = false;
    let mut message = MSG::default();

    loop {
        // Pump pending Win32 messages for this thread.
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // Mouse events on the icon itself. Double-click is the expected
        // Windows gesture for opening a tray utility's window; the context
        // menu stays on right-click.
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if action_for_icon_event(&event, &tray_id) == TrayAction::OpenUi {
                (callbacks.on_open_ui)();
            }
        }

        // Drain menu events.
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == open_id {
                (callbacks.on_open_ui)();
            } else if event.id == open_config_id {
                (callbacks.on_open_config_with)();
            } else if Some(&event.id) == auto_start_id.as_ref() {
                handle_auto_start_toggle(auto_start, auto_start_item.as_ref());
            } else if event.id == quit_id {
                (callbacks.on_quit)();
                quitting = true;
            }
        }

        if quitting {
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn handle_auto_start_toggle(auto_start: Option<&AutoStartManager>, item: Option<&CheckMenuItem>) {
    let (Some(manager), Some(item)) = (auto_start, item) else {
        return;
    };

    let next_enabled = !item.is_checked();
    if let Err(e) = manager.set_enabled(next_enabled) {
        tracing::error!(component = "tray", enabled = next_enabled, error = %e, "failed to update logon startup");
        show_error_dialog(
            "Kandel - Error",
            &format!("Failed to update the logon startup setting:\n\n{e}"),
        );
        return;
    }

    match manager.enabled() {
        Ok(enabled) => item.set_checked(enabled),
        Err(e) => {
            tracing::error!(component = "tray", error = %e, "failed to refresh logon startup");
            show_error_dialog(
                "Kandel - Error",
                &format!(
                    "The logon startup setting was updated, but its final state could not be confirmed:\n\n{e}"
                ),
            );
        }
    }
}

/// Decodes `png_bytes` into a tray [`Icon`].
fn load_icon(png_bytes: &[u8]) -> Result<Icon> {
    let decoder = png::Decoder::new(png_bytes);
    let mut reader = decoder.read_info().context("decoding tray icon PNG")?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .context("reading tray icon frame")?;
    buf.truncate(info.buffer_size());

    // Normalize to RGBA regardless of the source color type.
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let (pixels, _) = buf.as_chunks::<3>();
            pixels
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect()
        }
        other => anyhow::bail!("unsupported tray icon color type: {other:?}"),
    };

    Icon::from_rgba(rgba, info.width, info.height).context("creating tray icon from RGBA")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tray_icon::dpi::{PhysicalPosition, PhysicalSize};
    use tray_icon::{MouseButtonState, Rect, TrayIconId};

    fn rect() -> Rect {
        Rect {
            size: PhysicalSize::new(16, 16),
            position: PhysicalPosition::new(0.0, 0.0),
        }
    }

    fn icon_id() -> TrayIconId {
        TrayIconId::new("main")
    }

    fn double_click(button: MouseButton, id: &TrayIconId) -> TrayIconEvent {
        TrayIconEvent::DoubleClick {
            id: id.clone(),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: rect(),
            button,
        }
    }

    fn click(button: MouseButton, state: MouseButtonState, id: &TrayIconId) -> TrayIconEvent {
        TrayIconEvent::Click {
            id: id.clone(),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: rect(),
            button,
            button_state: state,
        }
    }

    #[test]
    fn left_double_click_opens_the_ui() {
        let id = icon_id();
        assert_eq!(
            action_for_icon_event(&double_click(MouseButton::Left, &id), &id),
            TrayAction::OpenUi
        );
    }

    #[test]
    fn a_single_left_click_does_not_open_the_ui() {
        // Windows delivers a double-click as DOWN, UP, DBLCLK, UP. If a single
        // click also opened the UI it would fire three times per double-click.
        let id = icon_id();
        assert_eq!(
            action_for_icon_event(&click(MouseButton::Left, MouseButtonState::Up, &id), &id),
            TrayAction::Ignore
        );
        assert_eq!(
            action_for_icon_event(&click(MouseButton::Left, MouseButtonState::Down, &id), &id),
            TrayAction::Ignore
        );
    }

    #[test]
    fn right_clicks_are_ignored() {
        // Right-click is the context menu's gesture, owned by the tray library.
        let id = icon_id();
        assert_eq!(
            action_for_icon_event(&double_click(MouseButton::Right, &id), &id),
            TrayAction::Ignore
        );
        assert_eq!(
            action_for_icon_event(&click(MouseButton::Right, MouseButtonState::Up, &id), &id),
            TrayAction::Ignore
        );
    }

    #[test]
    fn events_for_another_icon_are_ignored() {
        let id = icon_id();
        let other = TrayIconId::new("other");
        assert_eq!(
            action_for_icon_event(&double_click(MouseButton::Left, &other), &id),
            TrayAction::Ignore
        );
    }

    #[test]
    fn hover_events_are_ignored() {
        let id = icon_id();
        let enter = TrayIconEvent::Enter {
            id: id.clone(),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: rect(),
        };
        assert_eq!(action_for_icon_event(&enter, &id), TrayAction::Ignore);
    }

    #[test]
    fn embedded_icon_decodes() {
        let icon = load_icon(ICON_PNG).unwrap();
        // Icon does not expose dimensions, but a successful decode is the point.
        drop(icon);
    }
}
