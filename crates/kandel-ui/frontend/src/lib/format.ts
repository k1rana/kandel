/** Pure helpers used by the UI. Kept free of DOM/Tauri dependencies so they are
 * straightforward to unit test. */

import type { AppStateInfo, EnvVar, LogEvent } from "./types";

/** Normalizes edited environment pairs into the persisted form.
 *
 * Names are trimmed; a pair with a blank name is dropped. Values are kept
 * verbatim, so a value may contain `=` and surrounding spaces are preserved.
 *
 * Names with interior whitespace are *kept* here (`"MY VAR"` does not trim to
 * an empty key) so validation can report them instead of silently dropping the
 * row; `App.svelte`'s save rejects them.
 */
export function normalizeEnv(pairs: EnvVar[]): EnvVar[] {
  return pairs
    .map((pair) => ({ key: pair.key.trim(), value: pair.value }))
    .filter((pair) => pair.key !== "");
}

/** Corrects an environment variable name as it is typed.
 *
 * A space (or any whitespace) cannot appear in a variable name — a shell cannot
 * reference `%MY VAR%` — so it is folded to `_` in place instead of being
 * rejected later. The replacement is one-for-one, which keeps the caret steady
 * while typing. Other invalid characters (`=`, NUL) are left for validation,
 * because silently rewriting them would hide a real mistake.
 */
export function sanitizeEnvKey(name: string): string {
  return name.replace(/\s/g, "_");
}

/** Formats an event timestamp as wall-clock `HH:MM:SS`. */
export function formatClock(ts: number): string {
  const d = new Date(ts);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

/** A log event flattened for rendering: dividers or lines. */
export type LogRow =
  | { kind: "divider"; ts: number }
  | { kind: "line"; ts: number; stream: string; text: string };

/** Maps raw log events to render rows, dropping anything malformed. */
export function logRows(events: LogEvent[]): LogRow[] {
  const rows: LogRow[] = [];
  for (const e of events) {
    if (e.type === "started") {
      rows.push({ kind: "divider", ts: e.ts });
    } else if (e.type === "line") {
      rows.push({ kind: "line", ts: e.ts, stream: e.stream, text: e.text });
    }
  }
  return rows;
}

/** Extracts a suggested app name from a command path (filename without extension). */
export function extractAppNameFromPath(filePath: string): string {
  const trimmed = filePath.trim();
  if (!trimmed) return "";

  const segments = trimmed.split(/[/\\]/);
  const filename = segments[segments.length - 1];
  if (!filename) return "";

  const lastDotIndex = filename.lastIndexOf(".");
  if (lastDotIndex > 0) {
    return filename.substring(0, lastDotIndex);
  }
  return filename;
}

/** Returns true when `path` ends with one of the supported `extensions`. */
export function isSupportedCommandFile(path: string, extensions: string[]): boolean {
  const normalized = path.trim().toLowerCase();
  return extensions.some((ext) => normalized.endsWith(ext.toLowerCase()));
}

/** Case-insensitive comparator for strings; falls back to default ordering. */
export function caseInsensitiveComparator(a: unknown, b: unknown): number {
  if (typeof a === "string" && typeof b === "string") {
    return a.localeCompare(b, undefined, { sensitivity: "base" });
  }
  if (a == null && b == null) return 0;
  if (a == null) return -1;
  if (b == null) return 1;
  if (a < b) return -1;
  if (a > b) return 1;
  return 0;
}

/** Clamps and rounds `value` into the inclusive range [min, max]. */
export function clampInt(value: number, min: number, max: number): number {
  if (Number.isNaN(value)) return min;
  return Math.min(max, Math.max(min, Math.round(value)));
}

/** The visual family a lifecycle state belongs to, used to pick its charge and
 * its text treatment. `charge` is the design vocabulary (a filled marker),
 * `tone` names which palette role carries it. */
export type StateTone = "alive" | "waiting" | "down" | "idle";

export interface StateView {
  /** Uppercased state word for the ledger. */
  label: string;
  /** Palette role for the charge and the state word. */
  tone: StateTone;
  /** Whether the state reads as healthy at a glance. */
  healthy: boolean;
}

/** Maps a raw lifecycle state string to its ledger representation. */
export function stateView(state: string): StateView {
  switch (state.toLowerCase()) {
    case "running":
      return { label: "Running", tone: "alive", healthy: true };
    case "starting":
      return { label: "Starting", tone: "waiting", healthy: false };
    case "retrying":
      return { label: "Retrying", tone: "waiting", healthy: false };
    case "failed":
      return { label: "Failed", tone: "down", healthy: false };
    default:
      return { label: "Stopped", tone: "idle", healthy: false };
  }
}

/** Formats the command and args for display in one line. */
export function formatCommandLine(status: AppStateInfo): string {
  return [status.command, ...(status.args ?? [])].join(" ");
}

/** A running tally of the ledger, for the header summary. */
export interface Tally {
  /** Total entries in the ledger. */
  total: number;
  /** Entries currently running. */
  running: number;
  /** Entries present but not running (stopped, retrying, failed, disabled). */
  attention: number;
}

/** Minimal shape needed to build a ledger row from config.
 *
 * Only `enabled` and `command` are required: the row's args come from live
 * status once it arrives. The UI's `AppConfig` DTO stores args as one
 * shell-formatted string, so it satisfies this shape directly. */
export interface ConfigEntry {
  enabled: boolean;
  command: string;
}

/**
 * State shown for a configured entry the core has not reported on yet.
 *
 * `Starting` is deliberate: the entry exists and the core is about to act on
 * it. `Stopped` would wrongly imply it had already exited.
 */
export const PENDING_STATE = "Starting";

/** Builds ledger rows from the config file.
 *
 * The core pushes live status over IPC, but that stream only exists once the
 * UI is launched by the core. Deriving rows from config keeps the ledger
 * correct the moment it opens — including when the UI runs without a core
 * (during development) or before the first status arrives.
 */
export function rowsFromConfig(configs: Record<string, ConfigEntry>): Record<string, AppStateInfo> {
  const rows: Record<string, AppStateInfo> = {};
  for (const [name, cfg] of Object.entries(configs)) {
    rows[name] = {
      name,
      state: PENDING_STATE,
      pid: 0,
      enabled: cfg.enabled,
      command: cfg.command,
      args: [],
      restartCount: 0,
    };
  }
  return rows;
}

/** Counts the ledger's apps and how many are healthy. */
export function summarize(apps: Record<string, AppStateInfo>): Tally {
  const list = Object.values(apps);
  const running = list.filter((a) => stateView(a.state).healthy).length;
  return {
    total: list.length,
    running,
    attention: list.length - running,
  };
}

/** Applies a status update to a map of app states and returns a new map.
 *
 * A `Removed` update deletes the entry; any other update upserts it.
 */
export function applyStateUpdate(
  apps: Record<string, AppStateInfo>,
  update: AppStateInfo,
): Record<string, AppStateInfo> {
  const next = { ...apps };
  if (update.state === "Removed") {
    delete next[update.name];
  } else {
    next[update.name] = update;
  }
  return next;
}
