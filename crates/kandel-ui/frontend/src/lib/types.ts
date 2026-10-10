/** Shared frontend types. */

/** Metadata shown in the About popover. */
export interface AboutInfo {
  name: string;
  version: string;
  configPath: string;
  repository: string;
}

/** A configuration entry as edited in the UI. Mirrors the Rust `AppConfig` DTO. */
export interface AppConfig {
  name: string;
  enabled: boolean;
  command: string;
  args: string;
  stopCommand: string;
  stopArgs: string;
  cwd: string;
  restartDelaySec: number;
  healthyTimeoutSec: number;
  hideWindow: boolean;
  maxRetries: number;
  stopTimeoutSec: number;
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
}

/** Creates an empty form for a new app entry. */
export function makeEmptyForm(): AppConfig {
  return {
    name: "",
    enabled: true,
    command: "",
    args: "",
    stopCommand: "",
    stopArgs: "",
    cwd: "",
    restartDelaySec: 0,
    healthyTimeoutSec: 0,
    hideWindow: false,
    maxRetries: -1,
    stopTimeoutSec: 5,
  };
}
