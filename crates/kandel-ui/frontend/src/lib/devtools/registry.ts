import type { Component } from "svelte";

/**
 * A panel in the devtools dock, plus optional console helpers.
 *
 * Props are typed loosely on purpose: a tool names its own props, and the dock
 * renders every tool through the same slot. Each tool casts at its one
 * `registerDevTool` call, where the real props are in hand.
 */
export interface DevTool {
  id: string;
  label: string;
  component: Component<Record<string, unknown>>;
  /** Props for `component`, for a tool that needs seeding. */
  props?: Record<string, unknown>;
  /** Exposed as `window.kandel.<id>`. */
  actions?: Record<string, (...args: never[]) => unknown>;
}

const tools: DevTool[] = [];

export function registerDevTool(tool: DevTool): void {
  tools.push(tool);
}

export function registeredDevTools(): DevTool[] {
  return tools;
}
