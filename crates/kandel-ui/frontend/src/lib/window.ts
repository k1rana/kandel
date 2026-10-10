/** Thin wrappers around the Tauri window API for custom chrome controls. */

import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";

/** The directions accepted by `startResizeDragging`, matching Tauri's enum. */
export type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

function appWindow() {
  return getCurrentWindow();
}

/** Minimizes the window. */
export function minimizeWindow(): Promise<void> {
  return appWindow().minimize();
}

/** Toggles between maximized and restored. */
export function toggleMaximizeWindow(): Promise<void> {
  return appWindow().toggleMaximize();
}

/** Closes the window (which exits the UI process). */
export function closeWindow(): Promise<void> {
  return appWindow().close();
}

/** Starts dragging the window (for custom titlebars). */
export function startDraggingWindow(): Promise<void> {
  return appWindow().startDragging();
}

/** Starts a native resize drag from a custom edge/corner handle. */
export function startResizeDragging(direction: ResizeDirection): Promise<void> {
  return appWindow().startResizeDragging(direction);
}

/** Returns whether the window is currently maximized. */
export function isWindowMaximized(): Promise<boolean> {
  return appWindow().isMaximized();
}

/** Subscribes to window resize events (used to track the maximized state). */
export function onWindowResized(
  handler: () => void,
): Promise<UnlistenFn> {
  return appWindow().onResized(handler);
}
