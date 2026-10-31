import { describe, expect, it } from "vitest";

import {
  applyStateUpdate,
  caseInsensitiveComparator,
  clampInt,
  envFromText,
  envToText,
  extractAppNameFromPath,
  formatClock,
  formatCommandLine,
  isSupportedCommandFile,
  insertionIndex,
  logRows,
  mergeLedgerRows,
  moveItem,
  normalizeEnv,
  parseArgs,
  rowsFromConfig,
  sanitizeEnvKey,
  splitConfig,
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

describe("envToText", () => {
  it("renders one KEY=value line per pair in order", () => {
    expect(envToText([{ key: "PORT", value: "3000" }, { key: "A", value: "1" }])).toBe(
      "PORT=3000\nA=1",
    );
  });

  it("returns an empty string for no pairs", () => {
    expect(envToText([])).toBe("");
  });

  it("skips untouched placeholder rows, which carry no data", () => {
    expect(
      envToText([
        { key: "PORT", value: "3000" },
        { key: "", value: "" },
        { key: "   ", value: "" },
      ]),
    ).toBe("PORT=3000");
  });

  it("keeps a value containing = verbatim", () => {
    expect(envToText([{ key: "TOKEN", value: "a=b=c" }])).toBe("TOKEN=a=b=c");
  });

  it("escapes a leading quote in the value", () => {
    // A value that starts with a quote would be re-read as a quoting wrapper
    // by the parser, so it is single-quoted (contents verbatim — the value's
    // own quotes are NOT escaped, the parser keeps single-quoted bodies verbatim).
    expect(envToText([{ key: "X", value: '"hi"' }])).toBe("X='\"hi\"'");
    expect(envToText([{ key: "Y", value: "it's here" }])).toBe("Y='it\\'s here'");
  });

  it("escapes a leading pound sign in the value", () => {
    expect(envToText([{ key: "COLOR", value: "#fff" }])).toBe("COLOR='#fff'");
  });

  it("single-quotes a value with trailing or interior whitespace", () => {
    expect(envToText([{ key: "A", value: " spaced " }])).toBe("A=' spaced '");
    expect(envToText([{ key: "B", value: "two words" }])).toBe("B='two words'");
  });

  it("leaves plain values bare", () => {
    expect(envToText([{ key: "C", value: "plain" }])).toBe("C=plain");
  });
});

describe("envFromText", () => {
  it("parses one KEY=value per line and keeps order", () => {
    expect(envFromText("PORT=3000\nTOKEN=a=b\n")).toEqual({
      pairs: [
        { key: "PORT", value: "3000" },
        { key: "TOKEN", value: "a=b" },
      ],
      issues: [],
    });
  });

  it("trims the key and surrounding spaces around the =", () => {
    expect(envFromText("  PORT = 3000  ")).toEqual({
      pairs: [{ key: "PORT", value: "3000" }],
      issues: [],
    });
  });

  it("folds whitespace in a name to underscores", () => {
    expect(envFromText("MY VAR=1")).toEqual({
      pairs: [{ key: "MY_VAR", value: "1" }],
      issues: [],
    });
  });

  it("returns empty for blank input and for blank/comment lines only", () => {
    expect(envFromText("")).toEqual({ pairs: [], issues: [] });
    expect(envFromText("\n\r\n")).toEqual({ pairs: [], issues: [] });
    expect(envFromText("# a comment\n; also a comment\n")).toEqual({ pairs: [], issues: [] });
  });

  it("skips comment lines between entries", () => {
    expect(envFromText("A=1\n# note\nB=2")).toEqual({
      pairs: [
        { key: "A", value: "1" },
        { key: "B", value: "2" },
      ],
      issues: [],
    });
  });

  it("strips an optional export or set prefix", () => {
    expect(envFromText("export PATH=/bin\nset NODE_ENV=test")).toEqual({
      pairs: [
        { key: "PATH", value: "/bin" },
        { key: "NODE_ENV", value: "test" },
      ],
      issues: [],
    });
  });

  it("strips a matching surrounding double-quote pair", () => {
    // A single-quoted wrapper is kept verbatim inside a double-quote wrapper.
    expect(envFromText('X="a=b \'quoted\'"')).toEqual({
      pairs: [{ key: "X", value: "a=b 'quoted'" }],
      issues: [],
    });
  });

  it("strips a matching surrounding single-quote pair and quotes are otherwise kept", () => {
    const input = "X='it\\'s'\nY='keep \"end\"'";
    expect(envFromText(input)).toEqual({
      pairs: [
        { key: "X", value: "it's" },
        { key: "Y", value: 'keep "end"' },
      ],
      issues: [],
    });
  });

  it("treats a bare KEY as a variable set to the empty string", () => {
    expect(envFromText("EMPTY\nset EMPTY2")).toEqual({
      pairs: [
        { key: "EMPTY", value: "" },
        { key: "EMPTY2", value: "" },
      ],
      issues: [],
    });
  });

  it("reports the first malformed line with its line number", () => {
    const result = envFromText("A=1\n=oops\nB=2");
    expect(result.issues).toEqual([
      { line: 2, message: "the name before '=' is empty" },
    ]);
    expect(result.pairs.length).toBeGreaterThan(0);
  });

  it("collects several issues and pairs what it can", () => {
    const result = envFromText("A=1\nno equals sign here\nB=2");
    expect(result.issues).toHaveLength(1);
    expect(result.issues[0].line).toBe(2);
    expect(result.issues[0].message).toContain("NAME=value");
    expect(result.pairs).toEqual([
      { key: "A", value: "1" },
      { key: "B", value: "2" },
    ]);
  });

  it("reports a name that is only whitespace", () => {
    const result = envFromText("   =1");
    expect(result.issues).toHaveLength(1);
    expect(result.issues[0].line).toBe(1);
  });

  it("skips a bare '=' as the render of an untouched empty row", () => {
    // envToText of a blank placeholder row emits '='; it has no data, so it
    // must not block the switch the way a malformed line would.
    expect(envFromText("=")).toEqual({ pairs: [], issues: [] });
    expect(envFromText("A=1\n=\nB=2")).toEqual({
      pairs: [
        { key: "A", value: "1" },
        { key: "B", value: "2" },
      ],
      issues: [],
    });
  });

  it("round-trips through envToText", () => {
    const pairs = [
      { key: "PORT", value: "3000" },
      { key: "TOKEN", value: "a=b" },
      { key: "NOTES", value: "two words" },
      { key: "QUOTED", value: "it's" },
      { key: "HASH", value: "#fff" },
      { key: "TEMPLATE", value: "${OTHER_VAR}" },
    ];
    expect(envFromText(envToText(pairs)).pairs).toEqual(pairs);
  });
});

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
    const apps = [
      status({ name: "a", state: "Running" }),
      status({ name: "b", state: "Failed" }),
      status({ name: "c", state: "Running" }),
    ];
    expect(summarize(apps)).toEqual({ total: 3, running: 2, attention: 1 });
  });

  it("is zeroed for an empty ledger", () => {
    expect(summarize([])).toEqual({ total: 0, running: 0, attention: 0 });
  });
});

describe("rowsFromConfig", () => {
  it("builds a row per config entry, in input order", () => {
    const rows = rowsFromConfig([
      { name: "api", enabled: true, command: "node" },
      { name: "paused", enabled: false, command: "worker.exe" },
    ]);
    expect(rows.map((r) => r.name)).toEqual(["api", "paused"]);
    expect(rows[0].command).toBe("node");
    expect(rows[1].enabled).toBe(false);
  });

  it("marks an unreported entry as Unknown, not a lifecycle guess", () => {
    // "Starting" would claim a launch is in progress and "Stopped" that it had
    // already exited; neither is known before a core reports.
    const rows = rowsFromConfig([{ name: "a", enabled: true, command: "x" }]);
    expect(rows[0].state).toBe("Unknown");
    expect(rows[0].pid).toBe(0);
    expect(rows[0].restartCount).toBe(0);
  });

  it("renders an unknown state as its own tone", () => {
    expect(stateView("Unknown")).toMatchObject({ label: "Unknown", tone: "unknown" });
    // Unknown is neither a lifecycle word nor a health reading.
    expect(stateView("Unknown").healthy).toBe(false);
  });

  it("renders a config error as a down tone, not a health reading", () => {
    expect(stateView("ConfigError")).toMatchObject({ label: "Config error", tone: "down" });
    expect(stateView("ConfigError").healthy).toBe(false);
  });

  it("parses the config's args string into argv", () => {
    const rows = rowsFromConfig([
      { name: "a", enabled: true, command: "x", args: '-c "hello world"' },
    ]);
    expect(rows[0].args).toEqual(["-c", "hello world"]);
  });

  it("defaults args to an empty list when the config omits them", () => {
    const rows = rowsFromConfig([{ name: "a", enabled: true, command: "x" }]);
    expect(rows[0].args).toEqual([]);
  });

  it("returns nothing for an empty config", () => {
    expect(rowsFromConfig([])).toEqual([]);
  });

  it("shows an entry with a value error in an error state", () => {
    const rows = rowsFromConfig([
      {
        name: "broken",
        enabled: true,
        command: "nope.exe",
        errors: ["command not found in PATH: nope"],
      },
    ]);
    expect(rows[0].state).toBe("ConfigError");
    expect(rows[0].errors).toEqual(["command not found in PATH: nope"]);
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
    const configs = [{ name: "a", enabled: true, command: "x" }];
    expect(mergeLedgerRows(configs, {}).map((r) => r.name)).toEqual(["a"]);
  });

  it("takes state and pid from live status", () => {
    const configs = [{ name: "a", enabled: true, command: "x" }];
    const live = { a: status({ name: "a", state: "Running", pid: 99 }) };
    const [row] = mergeLedgerRows(configs, live);
    expect(row.state).toBe("Running");
    expect(row.pid).toBe(99);
  });

  it("keeps the config's command even when live status is stale", () => {
    // The regression: a saved command edit must show immediately, not after the
    // core's next status lands (which waits out the stop timeout on a restart).
    const configs = [{ name: "a", enabled: true, command: "new.exe", args: "--new" }];
    const live = { a: status({ name: "a", command: "old.exe", args: ["--old"] }) };
    const [row] = mergeLedgerRows(configs, live);
    expect(row.command).toBe("new.exe");
    expect(row.args).toEqual(["--new"]);
  });

  it("keeps the input order, which is the file order", () => {
    const configs = [
      { name: "zebra", enabled: true, command: "x" },
      { name: "alpha", enabled: true, command: "x" },
    ];
    const live = {
      zebra: status({ name: "zebra", state: "Running" }),
      alpha: status({ name: "alpha", state: "Running" }),
    };
    expect(mergeLedgerRows(configs, live).map((r) => r.name)).toEqual(["zebra", "alpha"]);
  });

  it("drops a row removed from config even if live status lingers", () => {
    const live = { gone: status({ name: "gone", state: "Running" }) };
    expect(mergeLedgerRows([], live)).toEqual([]);
  });

  it("keeps a value error over a live status", () => {
    const configs = [
      {
        name: "broken",
        enabled: true,
        command: "nope.exe",
        errors: ["command not found in PATH: nope"],
      },
    ];
    // A stale live status must not mask the config error.
    const live = { broken: status({ name: "broken", state: "Running", pid: 42 }) };
    const [row] = mergeLedgerRows(configs, live);
    expect(row.state).toBe("ConfigError");
    expect(row.errors).toEqual(["command not found in PATH: nope"]);
    expect(row.pid).toBe(0);
  });
});

describe("splitConfig", () => {
  it("routes a structural error to issues and keeps value errors as entries", () => {
    const { entries, issues } = splitConfig([
      { name: "ok" },
      { name: "bad_toml", errors: ["unknown field `restart_delay`"], errorKind: "structural" as const },
      { name: "bad_value", errors: ["command not found in PATH: nope"], errorKind: "value" as const },
    ]);
    expect(entries.map((e) => e.name).sort()).toEqual(["bad_value", "ok"]);
    expect(issues).toEqual([
      { name: "bad_toml", messages: ["unknown field `restart_delay`"], kind: "structural" },
    ]);
  });

  it("keeps the input order within each output", () => {
    const { entries } = splitConfig([
      { name: "zebra" },
      { name: "alpha" },
      { name: "mango" },
    ]);
    expect(entries.map((e) => e.name)).toEqual(["zebra", "alpha", "mango"]);
  });

  it("collects every message for a multi-error entry", () => {
    const { issues } = splitConfig([
      {
        name: "bad",
        errors: ["env[0] key is empty", "stop_timeout_sec must be >= 0"],
        errorKind: "structural" as const,
      },
    ]);
    expect(issues[0].messages).toHaveLength(2);
  });

  it("treats an entry without an error as an entry", () => {
    const { entries, issues } = splitConfig([{ name: "a" }]);
    expect(entries.map((e) => e.name)).toEqual(["a"]);
    expect(issues).toEqual([]);
  });

  it("returns nothing for an empty config", () => {
    expect(splitConfig([])).toEqual({ entries: [], issues: [] });
  });
});

describe("moveItem", () => {
  const list = ["a", "b", "c", "d"];

  it("moves an item down", () => {
    expect(moveItem(list, 0, 2)).toEqual(["b", "c", "a", "d"]);
  });

  it("moves an item up", () => {
    expect(moveItem(list, 3, 0)).toEqual(["d", "a", "b", "c"]);
  });

  it("is a no-op when the target is the same index", () => {
    expect(moveItem(list, 1, 1)).toEqual(list);
  });

  it("clamps a target past either end", () => {
    expect(moveItem(list, 0, 99)).toEqual(["b", "c", "d", "a"]);
    expect(moveItem(list, 3, -5)).toEqual(["d", "a", "b", "c"]);
  });

  it("does not mutate the input", () => {
    moveItem(list, 0, 2);
    expect(list).toEqual(["a", "b", "c", "d"]);
  });

  it("ignores an out-of-range source", () => {
    expect(moveItem(list, 9, 0)).toEqual(list);
  });
});

describe("insertionIndex", () => {
  const rows = [
    { top: 0, height: 40 },
    { top: 40, height: 40 },
    { top: 80, height: 40 },
  ];

  it("returns the gap under the pointer, flipping at a row's midpoint", () => {
    expect(insertionIndex(rows, 10)).toBe(0);
    expect(insertionIndex(rows, 19)).toBe(0);
    // At or past the first row's midpoint, the gap is below it.
    expect(insertionIndex(rows, 20)).toBe(1);
    expect(insertionIndex(rows, 45)).toBe(1);
    expect(insertionIndex(rows, 65)).toBe(2);
  });

  it("returns the length when the pointer is below every row", () => {
    expect(insertionIndex(rows, 1000)).toBe(3);
  });

  it("returns 0 for an empty list", () => {
    expect(insertionIndex([], 500)).toBe(0);
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
