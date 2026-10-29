/** Shared frontend types. */

/** Metadata shown in the About popover. */
export interface AboutInfo {
  name: string;
  version: string;
  configPath: string;
  repository: string;
}

/** A single environment variable set on a managed process. */
export interface EnvVar {
  key: string;
  value: string;
}

/** A configuration entry as edited in the UI. Mirrors the Rust `AppConfig` DTO. */
export interface AppConfig {
  name: string;
  enabled: boolean;
  command: string;
  args: string;
  /** Ordered environment variables for this app. */
  env: EnvVar[];
  stopCommand: string;
  stopArgs: string;
  cwd: string;
  restartDelaySec: number;
  healthyTimeoutSec: number;
  hideWindow: boolean;
  maxRetries: number;
  stopTimeoutSec: number;
  /** Whether the process's stdout/stderr are captured into its log. */
  captureOutput: boolean;
  /**
   * Why the entry failed to load, when it is on disk but invalid.
   *
   * Empty for a valid entry. Non-empty means the other fields are a best-effort
   * draft.
   */
  errors?: string[];
  /**
   * Which kind of problem `errors` describes.
   *
   * `structural` is a TOML-writing problem the repair flow can rewrite; `value`
   * is a bad value that is shown as a ledger row in an error state and fixed by
   * editing.
   */
  errorKind?: "structural" | "value";
}

/** An entry that failed to load, kept aside for display. */
export interface ConfigIssue {
  name: string;
  messages: string[];
  /** A writing problem (repairable) versus a value problem (edit it). */
  kind: "structural" | "value";
}

/** The result of a config auto-repair run. Mirrors the Rust `RepairReport`. */
export interface RepairReport {
  /** Entries that were rewritten and now load. */
  repaired: string[];
  /** Entries that still need manual attention. */
  skipped: { name: string; reason: string }[];
}

/** A runtime status update pushed by the core over IPC. */
export interface AppStateInfo {
  name: string;
  pid: number;
  state: string;
  enabled: boolean;
  command: string;
  args: string[];
  restartCount: number;
  /**
   * Why the entry's config is unusable, when it is a value problem.
   *
   * Empty for a healthy entry. Non-empty means the row is in an error state and
   * the reasons are shown on hover.
   */
  errors?: string[];
}

/** Creates an empty form for a new app entry. */
export function makeEmptyForm(): AppConfig {
  return {
    name: "",
    enabled: true,
    command: "",
    args: "",
    env: [],
    stopCommand: "",
    stopArgs: "",
    cwd: "",
    restartDelaySec: 0,
    healthyTimeoutSec: 0,
    hideWindow: false,
    maxRetries: -1,
    stopTimeoutSec: 5,
    captureOutput: true,
  };
}

/** A single log event pushed by the core over IPC. */
export type LogEvent =
  | { type: "started"; ts: number }
  | { type: "line"; ts: number; stream: string; text: string };
