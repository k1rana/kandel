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

/* ── Environment bulk text ──────────────────────────────────────────────────

   The editor's bulk mode is a plain-text view of the same pairs the row editor
   holds. Parsing mirrors what a shell or a .env loader accepts, kept more
   forgiving than strict dotenv: pasted output from `export`, `set`, or a .env
   file should all land without hand-cleaning.

   One line, one variable. Values are kept verbatim (they may contain `=` and
   spaces); a quoting wrapper is stripped only when it actually wraps the whole
   value, so `X="a"-"b"` stays as written. Names with interior whitespace are
   folded to `_` here — the bulk view is the paste/hand-edit surface, where the
   row editor's sanitizer does not run, and a folded name cannot be mistaken
   for acceptance of an invalid name. A reference to another variable
   (`${PORT}`) is deliberately NOT interpolated: `config.toml` is the template
   of record and the core resolves it at launch, so the text view and the row
   view must show the same literal characters. */

/** A single unparsable line in bulk text. */
export interface EnvTextIssue {
  /** 1-based line number in the text. */
  line: number;
  /** What is wrong with that line. */
  message: string;
}

/** The result of parsing bulk environment text. */
export interface EnvParseResult {
  /** Pairs derived from the parseable lines, in source order. */
  pairs: EnvVar[];
  /** One issue per line that could not be parsed. */
  issues: EnvTextIssue[];
}

/** True when the value must be single-quoted to survive a round trip through
 * the text view. */
function valueNeedsQuotes(value: string): boolean {
  return value !== value.trim() || /\s/.test(value) || /^["'#]/.test(value);
}

/** Serializes pairs into the bulk text view.
 *
 * Plain values stay bare so the common case reads exactly as the user wrote
 * it: `PORT=3000`. Values that a bare form would corrupt on re-parse (leading
 * quote, leading comment marker, whitespace) are single-quoted, which
 * {@link envFromText} strips back symmetrically. */
export function envToText(pairs: EnvVar[]): string {
  return pairs
    .filter((pair) => !(pair.key.trim() === "" && pair.value === ""))
    .map((pair) => {
      if (pair.value !== "" && valueNeedsQuotes(pair.value)) {
        return `${pair.key}='${pair.value.replace(/'/g, "\\'")}'`;
      }
      return `${pair.key}=${pair.value}`;
    })
    .join("\n");
}

/** A `KEY` alone — which this surface treats as a variable set to the empty
 * string — optionally wearing an `export` or `set` prefix. */
const BARE_KEY_RE = /^(?:export\s+|set\s+)?([A-Za-z_][\w.-]*)$/;

/** Parses the bulk text view back into pairs.
 *
 * Unparseable lines are reported, not swallowed: `issues` carries one entry
 * per bad line and the editor blocks saving while any remain. Parseable lines
 * still yield pairs, so work the user has already typed is never lost. */
export function envFromText(text: string): EnvParseResult {
  const pairs: EnvVar[] = [];
  const issues: EnvTextIssue[] = [];

  text.split(/\n/).forEach((raw, index) => {
    const line = raw.replace(/\r$/, "").trim();
    if (line === "" || /^[;#]/.test(line)) return;

    let body = line.replace(/^(export|set)\s+/, "");
    const eq = body.indexOf("=");

    if (eq === -1) {
      const bare = BARE_KEY_RE.exec(body);
      if (bare) {
        pairs.push({ key: sanitizeEnvKey(bare[1]), value: "" });
        return;
      }
      issues.push({
        line: index + 1,
        message: "use NAME=value — this line has no '='",
      });
      return;
    }

    const name = body.slice(0, eq).trim();
    if (!name) {
      // A bare '=' is what an untouched "Add variable" row serializes to; it
      // carries no data, so it is skipped the way a blank line would be. A
      // name with a value, though, is a real mistake and stays an issue.
      if (body.slice(eq + 1).trim() === "") return;
      issues.push({ line: index + 1, message: "the name before '=' is empty" });
      return;
    }

    body = body.slice(eq + 1).trim();
    let value = body;
    // Unwrap ONE quoting wrapper, symmetric with envToText. Single quotes are
    // the serializer's own mechanism, so their contents are kept verbatim
    // (only the \' escape is honored); a double-quote wrapper is a user's
    // pasting convention and its escapes are folded back to bare chars.
    if (value.length >= 2 && ((value[0] === '"' && value.endsWith('"')) || (value[0] === "'" && value.endsWith("'")))) {
      if (value[0] === "'") value = value.slice(1, -1).replace(/\\'/g, "'");
      else value = value.slice(1, -1).replace(/\\"/g, '"');
    }

    pairs.push({ key: sanitizeEnvKey(name), value });
  });

  return { pairs, issues };
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
export type StateTone = "alive" | "waiting" | "down" | "idle" | "unknown";

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
    case "restarting":
      return { label: "Restarting", tone: "waiting", healthy: false };
    case "retrying":
      return { label: "Retrying", tone: "waiting", healthy: false };
    case "failed":
      return { label: "Failed", tone: "down", healthy: false };
    case "unknown":
      return { label: "Unknown", tone: "unknown", healthy: false };
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
 * The UI's `AppConfig` DTO satisfies this shape directly: it stores `args` as
 * one shell-formatted string, which {@link parseArgs} turns back into argv. */
export interface ConfigEntry {
  enabled: boolean;
  command: string;
  /** Shell-formatted arguments, as stored in the config DTO. */
  args?: string;
}

/** Splits a shell-like argument string into individual arguments.
 *
 * Mirrors the backend `parse_args` so the ledger shows the same argv the core
 * will launch: single- and double-quoted runs are respected, and backslashes
 * escape only inside double quotes (elsewhere `\` is literal, which matters for
 * Windows paths). Throws on an unterminated quote, like the Rust version.
 */
export function parseArgs(input: string): string[] {
  const args: string[] = [];
  let current = "";
  let inSingle = false;
  let inDouble = false;
  let escaped = false;

  for (const ch of input) {
    if (escaped) {
      current += ch;
      escaped = false;
      continue;
    }
    if (ch === "\\" && inDouble) {
      escaped = true;
      continue;
    }
    if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
      continue;
    }
    if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
      continue;
    }
    if (/\s/.test(ch) && !inSingle && !inDouble) {
      if (current !== "") {
        args.push(current);
        current = "";
      }
      continue;
    }
    current += ch;
  }

  if (inSingle) throw new Error("unterminated single quote in args string");
  if (inDouble) throw new Error("unterminated double quote in args string");
  if (current !== "") args.push(current);
  return args;
}

/** Parses `input`, returning `[]` when it is absent or malformed.
 *
 * The backend validates and re-serializes `args` before it reaches the UI, so a
 * parse failure is not expected here; this only keeps a malformed value from
 * blanking the row. */
function parseArgsSafe(input: string | undefined): string[] {
  if (input === undefined) return [];
  try {
    return parseArgs(input);
  } catch {
    return [];
  }
}

/**
 * State of a configured entry before a supervisor reports on it.
 *
 * Rows come from `config.toml`, which says an entry *exists*, not how it is
 * running. Until a core reports, the UI cannot know, so the row says so. That
 * is also the state while no core is attached at all, so one word covers both.
 */
export const PENDING_STATE = "Unknown";

/** Builds ledger rows from the config file.
 *
 * Config is the source of truth for *which* entries exist, so the ledger is
 * complete the moment the window opens, before any status arrives and whether
 * or not a core is running. It supplies only what config knows: identity, the
 * command, and whether the entry is enabled. Runtime facts (state, PID, restart
 * count) are left unknown until a core reports them.
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
      args: parseArgsSafe(cfg.args),
      restartCount: 0,
    };
  }
  return rows;
}

/** Merges config rows with live status into ledger rows.
 *
 * Config decides *what* an entry is (`enabled`, `command`, `args`), so an edit
 * saved to `config.toml` is reflected immediately; live status only supplies
 * *how* it is running (`state`, `pid`, `restartCount`). Taking the whole entry
 * from live status would hide a just-saved edit until the core's next status
 * lands — which, for a restart, waits out the graceful-stop timeout.
 */
export function mergeLedgerRows(
  configs: Record<string, ConfigEntry>,
  live: Record<string, AppStateInfo>,
): Record<string, AppStateInfo> {
  const merged: Record<string, AppStateInfo> = {};
  for (const [name, row] of Object.entries(rowsFromConfig(configs))) {
    const status = live[name];
    merged[name] = status
      ? { ...status, enabled: row.enabled, command: row.command, args: row.args }
      : row;
  }
  return merged;
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
