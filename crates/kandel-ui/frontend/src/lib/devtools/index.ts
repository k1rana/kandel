/**
 * The development-only devtools dock.
 *
 * Tools register themselves (see `registry.ts`); this mounts the dock and wires
 * each tool's actions onto `window.kandel`, so a panel and a console shorthand
 * come from one declaration.
 *
 * Only ever bundled under the Vite dev server: `main.ts` imports this behind an
 * `import.meta.env.DEV` guard, which `vite build` folds to `false`, and nothing
 * else imports it. A shipped build therefore contains no dock and no test hooks.
 */

import { mount } from "svelte";

import DevToolsDock from "./DevToolsDock.svelte";
import { registeredDevTools } from "./registry";

// Importing a tool module registers it. Add new tools here.
import "./update-tool";

export { registerDevTool, type DevTool } from "./registry";

/** Mounts the dock and exposes every tool's actions on `window.kandel`. */
export function installDevTools(): void {
  const tools = registeredDevTools();
  if (!tools.length) return;

  const kandel: Record<string, unknown> = {};
  for (const tool of tools) {
    if (tool.actions) kandel[tool.id] = tool.actions;
  }
  Object.defineProperty(window, "kandel", { value: kandel, configurable: true });

  const host = document.createElement("div");
  host.id = "kandel-devtools";
  document.body.appendChild(host);
  mount(DevToolsDock, { target: host, props: { tools } });

  console.info(`[kandel] devtools ready — ${tools.map((t) => t.label).join(", ")}`);
}

declare global {
  interface Window {
    kandel: Record<string, unknown>;
  }
}
