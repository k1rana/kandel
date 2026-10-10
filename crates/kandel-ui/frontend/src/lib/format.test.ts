import { describe, expect, it } from "vitest";

import {
  applyStateUpdate,
  caseInsensitiveComparator,
  clampInt,
  extractAppNameFromPath,
  formatCommandLine,
  isSupportedCommandFile,
  rowsFromConfig,
  stateView,
  summarize,
} from "./format";
import type { AppStateInfo } from "./types";

function status(overrides: Partial<AppStateInfo> = {}): AppStateInfo {
  return {
    name: "app",
    pid: 1,
    state: "Running",
    enabled: true,
    command: "app.exe",
    args: [],
    restartCount: 0,
    ...overrides,
  };
}

describe("extractAppNameFromPath", () => {
  it("strips directories and extension", () => {
    expect(extractAppNameFromPath("C:\\Program Files\\App\\myapp.exe")).toBe("myapp");
    expect(extractAppNameFromPath("/usr/bin/tool.ps1")).toBe("tool");
  });

  it("keeps names without an extension", () => {
    expect(extractAppNameFromPath("C:\\bin\\makefile")).toBe("makefile");
  });

  it("handles empty input", () => {
    expect(extractAppNameFromPath("")).toBe("");
    expect(extractAppNameFromPath("   ")).toBe("");
  });

  it("does not treat a leading dot as an extension", () => {
    expect(extractAppNameFromPath("C:\\bin\\.gitignore")).toBe(".gitignore");
  });
});

describe("isSupportedCommandFile", () => {
  const exts = [".exe", ".cmd"];

  it("accepts supported extensions case-insensitively", () => {
    expect(isSupportedCommandFile("C:\\bin\\app.EXE", exts)).toBe(true);
    expect(isSupportedCommandFile("C:\\bin\\run.cmd", exts)).toBe(true);
  });

  it("rejects unsupported extensions", () => {
    expect(isSupportedCommandFile("C:\\bin\\notes.txt", exts)).toBe(false);
  });
});

describe("caseInsensitiveComparator", () => {
  it("compares strings case-insensitively", () => {
    expect(caseInsensitiveComparator("apple", "Banana")).toBeLessThan(0);
    expect(caseInsensitiveComparator("Apple", "apple")).toBe(0);
  });

  it("orders nullish values first", () => {
    expect(caseInsensitiveComparator(null, 1)).toBe(-1);
    expect(caseInsensitiveComparator(1, null)).toBe(1);
    expect(caseInsensitiveComparator(null, null)).toBe(0);
  });

  it("compares numbers", () => {
    expect(caseInsensitiveComparator(1, 2)).toBeLessThan(0);
    expect(caseInsensitiveComparator(2, 1)).toBeGreaterThan(0);
  });
});

describe("clampInt", () => {
  it("clamps and rounds", () => {
    expect(clampInt(5.6, 0, 10)).toBe(6);
    expect(clampInt(-5, 0, 10)).toBe(0);
    expect(clampInt(50, 0, 10)).toBe(10);
  });

  it("returns min for NaN", () => {
    expect(clampInt(Number.NaN, -1, 10)).toBe(-1);
  });
});

describe("stateView", () => {
  it("maps known states to the ledger vocabulary", () => {
    expect(stateView("Running")).toMatchObject({ label: "Running", tone: "alive", healthy: true });
    expect(stateView("Starting")).toMatchObject({ label: "Starting", tone: "waiting" });
    expect(stateView("Retrying")).toMatchObject({ label: "Retrying", tone: "waiting" });
    expect(stateView("Failed")).toMatchObject({ label: "Failed", tone: "down" });
    expect(stateView("Stopped")).toMatchObject({ label: "Stopped", tone: "idle" });
  });

  it("falls back to Stopped for unknown states", () => {
    expect(stateView("Weird").label).toBe("Stopped");
  });

  it("marks only running as healthy", () => {
    expect(stateView("Running").healthy).toBe(true);
    expect(stateView("Retrying").healthy).toBe(false);
    expect(stateView("Failed").healthy).toBe(false);
  });
});

describe("summarize", () => {
  it("counts total, running, and attention", () => {
    const apps = {
      a: status({ name: "a", state: "Running" }),
      b: status({ name: "b", state: "Failed" }),
      c: status({ name: "c", state: "Running" }),
    };
    expect(summarize(apps)).toEqual({ total: 3, running: 2, attention: 1 });
  });

  it("is zeroed for an empty ledger", () => {
    expect(summarize({})).toEqual({ total: 0, running: 0, attention: 0 });
  });
});

describe("rowsFromConfig", () => {
  it("builds a row per config entry", () => {
    const rows = rowsFromConfig({
      api: { enabled: true, command: "node" },
      paused: { enabled: false, command: "worker.exe" },
    });
    expect(Object.keys(rows).sort()).toEqual(["api", "paused"]);
    expect(rows.api.command).toBe("node");
    expect(rows.paused.enabled).toBe(false);
  });

  it("marks an unreported entry as Starting, not Stopped", () => {
    // "Stopped" would imply it had already exited; the core has simply not
    // reported yet.
    const rows = rowsFromConfig({ a: { enabled: true, command: "x" } });
    expect(rows.a.state).toBe("Starting");
    expect(rows.a.pid).toBe(0);
    expect(rows.a.restartCount).toBe(0);
  });

  it("starts a seeded row with no args (live status supplies them)", () => {
    const rows = rowsFromConfig({ a: { enabled: true, command: "x" } });
    expect(rows.a.args).toEqual([]);
  });

  it("returns nothing for an empty config", () => {
    expect(rowsFromConfig({})).toEqual({});
  });
});

describe("ledger merge (config decides which rows exist)", () => {
  // Mirrors the derive in App.svelte: config keys win, live supplies state.
  function merge(
    configRows: Record<string, AppStateInfo>,
    live: Record<string, AppStateInfo>,
  ) {
    const out: Record<string, AppStateInfo> = {};
    for (const [name, row] of Object.entries(configRows)) out[name] = live[name] ?? row;
    return out;
  }

  it("shows config rows before any status arrives", () => {
    const rows = rowsFromConfig({ a: { enabled: true, command: "x" } });
    expect(Object.keys(merge(rows, {}))).toEqual(["a"]);
  });

  it("lets live status override the seeded row", () => {
    const rows = rowsFromConfig({ a: { enabled: true, command: "x" } });
    const live = { a: status({ name: "a", state: "Running", pid: 99 }) };
    const merged = merge(rows, live);
    expect(merged.a.state).toBe("Running");
    expect(merged.a.pid).toBe(99);
  });

  it("drops a row removed from config even if live status lingers", () => {
    const live = { gone: status({ name: "gone", state: "Running" }) };
    expect(merge({}, live)).toEqual({});
  });
});

describe("formatCommandLine", () => {
  it("joins command and args", () => {
    expect(formatCommandLine(status({ command: "npm", args: ["run", "dev"] }))).toBe(
      "npm run dev",
    );
  });

  it("handles missing args", () => {
    expect(formatCommandLine(status({ command: "app.exe", args: [] }))).toBe("app.exe");
  });
});

describe("applyStateUpdate", () => {
  it("upserts a new entry", () => {
    const next = applyStateUpdate({}, status({ name: "a" }));
    expect(next.a.state).toBe("Running");
  });

  it("updates an existing entry", () => {
    const first = applyStateUpdate({}, status({ name: "a", state: "Starting" }));
    const next = applyStateUpdate(first, status({ name: "a", state: "Running" }));
    expect(next.a.state).toBe("Running");
  });

  it("removes an entry on Removed", () => {
    const first = applyStateUpdate({}, status({ name: "a" }));
    const next = applyStateUpdate(first, status({ name: "a", state: "Removed" }));
    expect(next.a).toBeUndefined();
  });

  it("does not mutate the input map", () => {
    const first = { a: status({ name: "a" }) };
    const next = applyStateUpdate(first, status({ name: "b" }));
    expect(Object.keys(first)).toEqual(["a"]);
    expect(Object.keys(next).sort()).toEqual(["a", "b"]);
  });
});
