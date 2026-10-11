/** Thin wrappers around the Tauri IPC surface. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AboutInfo, AppConfig, AppStateInfo } from "./types";

/** Event name the core's status updates are forwarded under. */
export const STATE_EVENT = "app-state-update";

/** Event name the core's captured log events are forwarded under. */
export const LOG_EVENT = "app-log-update";

/** Returns the metadata shown in the About dialog. */
export function getAboutInfo(): Promise<AboutInfo> {
  return invoke<AboutInfo>("get_about_info");
}

/** Opens the project repository in the user's default browser. */
export function openRepository(): Promise<void> {
  return invoke("open_repository");
}

/** Loads every config entry keyed by app name. */
export function getFullConfig(): Promise<Record<string, AppConfig>> {
  return invoke<Record<string, AppConfig>>("get_full_config");
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

/**
 * Returns the log events the core already sent for `name`.
 *
 * The core replays an app's backlog whenever the UI is launched, but a log
 * panel opened later needs the same history; this fetches what the UI's stdin
 * listener has buffered.
 */
export function getAppLogs(name: string): Promise<string[]> {
  return invoke<string[]>("get_app_logs", { name });
}

/**
 * Fetches the statuses the core already sent.
 *
 * The core starts pushing before the webview can subscribe, so statuses sent
 * in that window would otherwise be lost. Call this *after* subscribing to
 * {@link onStateUpdate} so an update that arrives between the two is not
 * missed; the cache only moves forward, so applying the snapshot last is safe.
 */
export function getInitialStatuses(): Promise<string[]> {
  return invoke<string[]>("get_initial_statuses");
}
