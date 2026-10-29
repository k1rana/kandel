/** Thin wrappers around the Tauri IPC surface. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AboutInfo, AppConfig, AppStateInfo, RepairReport } from "./types";

/** Event name the core's status updates are forwarded under. */
export const STATE_EVENT = "app-state-update";

/** Event name the core's captured log events are forwarded under. */
export const LOG_EVENT = "app-log-update";

/** Event name carrying the core connection state. */
export const CONNECTION_EVENT = "core-connection";

/** Whether the UI is currently attached to a core. */
export type ConnectionState = "connected" | "disconnected";

/** Returns the metadata shown in the About dialog. */
export function getAboutInfo(): Promise<AboutInfo> {
  return invoke<AboutInfo>("get_about_info");
}

/** Opens the project repository in the user's default browser. */
export function openRepository(): Promise<void> {
  return invoke("open_repository");
}

/** Loads every config entry keyed by app name.
 *
 * Invalid entries are included with `errors` and `errorKind` set, so the UI can
 * list value errors as rows and route structural ones to repair. */
export function getFullConfig(): Promise<Record<string, AppConfig>> {
  return invoke<Record<string, AppConfig>>("get_full_config");
}

/** Rewrites entries with malformed TOML from their best-effort draft. */
export function repairConfig(): Promise<RepairReport> {
  return invoke<RepairReport>("repair_config");
}

/** Saves `cfg`, renaming the entry when `oldName` differs from `cfg.name`. */
export function updateAppConfig(oldName: string, cfg: AppConfig): Promise<void> {
  return invoke("update_app_config", { oldName, cfg });
}

/** Deletes the entry named `name`. */
export function deleteAppConfig(name: string): Promise<void> {
  return invoke("delete_app_config", { name });
}

/** Returns the executable/script extensions accepted by this environment. */
export function getCommandExtensions(): Promise<string[]> {
  return invoke<string[]>("get_command_extensions");
}

/** Opens a native file dialog to select a command path. */
export function selectCommandPath(current: string): Promise<string | null> {
  return invoke<string | null>("select_command_path", { current });
}

/** Subscribes to core status updates pushed over IPC. */
export async function onStateUpdate(
  handler: (status: AppStateInfo) => void,
): Promise<UnlistenFn> {
  return listen<string>(STATE_EVENT, (event) => {
    try {
      handler(JSON.parse(event.payload) as AppStateInfo);
    } catch (e) {
      console.error("Failed to parse state update:", e);
    }
  });
}

/** A captured log event together with the app it belongs to. */
export interface AppLogUpdate {
  name: string;
  event: import("./types").LogEvent;
}

/** Subscribes to core log events pushed over IPC. */
export async function onLogUpdate(
  handler: (update: AppLogUpdate) => void,
): Promise<UnlistenFn> {
  return listen<string>(LOG_EVENT, (event) => {
    try {
      handler(JSON.parse(event.payload) as AppLogUpdate);
    } catch (e) {
      console.error("Failed to parse log update:", e);
    }
  });
}

/** Subscribes to core connection state changes. */
export async function onConnectionChange(
  handler: (state: ConnectionState) => void,
): Promise<UnlistenFn> {
  return listen<string>(CONNECTION_EVENT, (event) => {
    handler(event.payload as ConnectionState);
  });
}

/**
 * Reads whether a core is attached right now.
 *
 * The companion to {@link onConnectionChange}: the reader attaches as soon as the
 * UI process starts, which is normally before the webview has subscribed, so the
 * event is missed on a normal launch. Subscribe to the event first, then call
 * this, so a change landing between the two is not lost.
 */
export function getConnectionState(): Promise<boolean> {
  return invoke<boolean>("get_connection_state");
}

/**
 * Returns the log events the core already sent for `name`.
 *
 * The core replays an app's backlog whenever a UI attaches to its pipe, and the
 * UI caches what it receives; a log panel opened later fetches that history.
 */
export function getAppLogs(name: string): Promise<string[]> {
  return invoke<string[]>("get_app_logs", { name });
}

/**
 * Fetches the statuses received so far.
 *
 * The core sends a snapshot the instant a UI attaches, which can be before the
 * webview has subscribed, so those statuses would otherwise be lost. Call this
 * *after* subscribing to {@link onStateUpdate} so an update that arrives between
 * the two is not missed; the cache only moves forward, so applying the snapshot
 * last is safe. Call it again after a reconnect: attaching clears the cache, so
 * the fresh snapshot replaces the previous core's state.
 */
export function getInitialStatuses(): Promise<string[]> {
  return invoke<string[]>("get_initial_statuses");
}
