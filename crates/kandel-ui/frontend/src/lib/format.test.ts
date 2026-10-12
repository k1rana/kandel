import { describe, expect, it } from "vitest";

import {
  applyStateUpdate,
  caseInsensitiveComparator,
  clampInt,
  extractAppNameFromPath,
  formatClock,
  formatCommandLine,
  isSupportedCommandFile,
  logRows,
  mergeLedgerRows,
  normalizeEnv,
  parseArgs,
  rowsFromConfig,
  sanitizeEnvKey,
  stateView,
  summarize,
} from "./format";
import type { AppStateInfo, LogEvent } from "./types";

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

describe("normalizeEnv", () => {
  it("trims names and keeps values verbatim", () => {
    expect(
      normalizeEnv([
        { key: "  PORT  ", value: " 3000 " },
        { key: "TOKEN", value: "a=b" },
      ]),
    ).toEqual([
      { key: "PORT", value: " 3000 " },
      { key: "TOKEN", value: "a=b" },
    ]);
  });

  it("drops blank rows and preserves order", () => {
    expect(
      normalizeEnv([
        { key: "", value: "" },
        { key: "B", value: "2" },
        { key: "   ", value: "ignored" },
        { key: "A", value: "1" },
      ]),
    ).toEqual([
      { key: "B", value: "2" },
      { key: "A", value: "1" },
    ]);
  });

  it("returns an empty list for no pairs", () => {
    expect(normalizeEnv([])).toEqual([]);
  });

  it("keeps an interior-space name so validation can reject it", () => {
    // Trimming must not turn "MY VAR" into an empty key (which would silently
    // drop the row); the input's sanitizer already folds spaces to `_`, so this
    // only guards the paste/hand-edit path.
    expect(normalizeEnv([{ key: " MY VAR ", value: "1" }])).toEqual([
      { key: "MY VAR", value: "1" },
    ]);
  });
});

describe("sanitizeEnvKey", () => {
  it("folds whitespace to underscores", () => {
    expect(sanitizeEnvKey("MY VAR")).toBe("MY_VAR");
    expect(sanitizeEnvKey("A B C")).toBe("A_B_C");
    expect(sanitizeEnvKey("lead ing")).toBe("lead_ing");
  });

  it("replaces one-for-one so the caret stays steady", () => {
    expect(sanitizeEnvKey("A B")).toHaveLength(3);
  });

  it("leaves valid names and other symbols untouched", () => {
    expect(sanitizeEnvKey("NODE_ENV")).toBe("NODE_ENV");
    expect(sanitizeEnvKey("my.app-flag")).toBe("my.app-flag");
    // `=` is left for validation, not silently rewritten.
    expect(sanitizeEnvKey("A=B")).toBe("A=B");
  });
});

describe("logRows", () => {
  it("maps started events to dividers and lines to rows", () => {
    const events: LogEvent[] = [
      { type: "started", ts: 1000 },
      { type: "line", ts: 1001, stream: "stdout", text: "hello" },
      { type: "line", ts: 1002, stream: "stderr", text: "oops" },
    ];
    expect(logRows(events)).toEqual([
      { kind: "divider", ts: 1000 },
      { kind: "line", ts: 1001, stream: "stdout", text: "hello" },
      { kind: "line", ts: 1002, stream: "stderr", text: "oops" },
    ]);
  });

  it("returns nothing for no events", () => {
    expect(logRows([])).toEqual([]);
  });
});

describe("formatClock", () => {
  it("formats a timestamp as HH:MM:SS", () => {
    // Construct from local time so the assertion is timezone-independent.
    const d = new Date(2024, 0, 1, 9, 5, 3);
    expect(formatClock(d.getTime())).toBe("09:05:03");
  });

  it("zero-pads each field", () => {
    const d = new Date(2024, 0, 1, 0, 0, 0);
    expect(formatClock(d.getTime())).toBe("00:00:00");
  });
});

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
    expect(stateView("Restarting")).toMatchObject({ label: "Restarting", tone: "waiting" });
    expect(stateView("Retrying")).toMatchObject({ label: "Retrying", tone: "waiting" });
    expect(stateView("Failed")).toMatchObject({ label: "Failed", tone: "down" });
    expect(stateView("Stopped")).toMatchObject({ label: "Stopped", tone: "idle" });
  });

  it("falls back to Stopped for unknown states", () => {
    expect(stateView("Weird").label).toBe("Stopped");
  });

  it("marks only running as healthy", () => {
    expect(stateView("Running").healthy).toBe(true);
    expect(stateView("Restarting").healthy).toBe(false);
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

  it("parses the config's args string into argv", () => {
    const rows = rowsFromConfig({ a: { enabled: true, command: "x", args: '-c "hello world"' } });
    expect(rows.a.args).toEqual(["-c", "hello world"]);
  });

  it("defaults args to an empty list when the config omits them", () => {
    const rows = rowsFromConfig({ a: { enabled: true, command: "x" } });
    expect(rows.a.args).toEqual([]);
  });

  it("returns nothing for an empty config", () => {
    expect(rowsFromConfig({})).toEqual({});
  });
});

describe("parseArgs", () => {
  it("splits on whitespace", () => {
    expect(parseArgs("-v --debug")).toEqual(["-v", "--debug"]);
  });

  it("keeps quoted runs together", () => {
    expect(parseArgs('-c "hello world"')).toEqual(["-c", "hello world"]);
    expect(parseArgs("-c 'hello world'")).toEqual(["-c", "hello world"]);
  });

  it("treats backslashes literally outside double quotes", () => {
    expect(parseArgs("C:\\Users\\alice\\app.exe")).toEqual(["C:\\Users\\alice\\app.exe"]);
  });

  it("escapes backslashes inside double quotes", () => {
    expect(parseArgs('"a\\"b"')).toEqual(['a"b']);
  });

  it("collapses runs of whitespace", () => {
    expect(parseArgs("  -a   -b\t-c ")).toEqual(["-a", "-b", "-c"]);
  });

  it("throws on an unterminated quote", () => {
    expect(() => parseArgs("'oops")).toThrow();
    expect(() => parseArgs('"oops')).toThrow();
  });
});

describe("mergeLedgerRows (config wins on identity, live on state)", () => {
  it("shows config rows before any status arrives", () => {
    const configs = { a: { enabled: true, command: "x" } };
    expect(Object.keys(mergeLedgerRows(configs, {}))).toEqual(["a"]);
  });

  it("takes state and pid from live status", () => {
    const configs = { a: { enabled: true, command: "x" } };
    const live = { a: status({ name: "a", state: "Running", pid: 99 }) };
    const merged = mergeLedgerRows(configs, live);
    expect(merged.a.state).toBe("Running");
    expect(merged.a.pid).toBe(99);
  });

  it("keeps the config's command even when live status is stale", () => {
    // The regression: a saved command edit must show immediately, not after the
    // core's next status lands (which waits out the stop timeout on a restart).
    const configs = { a: { enabled: true, command: "new.exe", args: "--new" } };
    const live = { a: status({ name: "a", command: "old.exe", args: ["--old"] }) };
    const merged = mergeLedgerRows(configs, live);
    expect(merged.a.command).toBe("new.exe");
    expect(merged.a.args).toEqual(["--new"]);
  });

  it("drops a row removed from config even if live status lingers", () => {
    const live = { gone: status({ name: "gone", state: "Running" }) };
    expect(mergeLedgerRows({}, live)).toEqual({});
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
