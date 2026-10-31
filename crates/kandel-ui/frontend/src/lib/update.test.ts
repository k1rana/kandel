import { describe, expect, it } from "vitest";

import {
  applyOutcome,
  markChecking,
  markCheckingManual,
  markFailed,
  outcomeOf,
  relativeAge,
  statusWord,
} from "./update";

describe("outcomeOf", () => {
  it("reports an available version", () => {
    expect(outcomeOf({ version: "1.3.0" })).toEqual({ status: "available", version: "1.3.0" });
  });

  it("reports current when the check found nothing", () => {
    expect(outcomeOf(null)).toEqual({ status: "current" });
  });
});

describe("applyOutcome", () => {
  it("records the version and the check time", () => {
    const next = applyOutcome({ status: "available", version: "1.3.0" }, 1000);
    expect(next).toEqual({ status: "available", version: "1.3.0", checkedAt: 1000 });
  });

  it("clears a stale version when the build is current", () => {
    const next = applyOutcome({ status: "current" }, 2000);
    expect(next).toEqual({ status: "current", version: null, checkedAt: 2000 });
  });
});

describe("markChecking", () => {
  it("keeps the last result visible while a check is in flight", () => {
    const before = applyOutcome({ status: "available", version: "1.3.0" }, 1000);
    const next = markChecking(before);
    expect(next.status).toBe("checking");
    // The dot and tooltip must not blank out mid-check.
    expect(next.version).toBe("1.3.0");
    expect(next.checkedAt).toBe(1000);
  });
});

describe("markCheckingManual", () => {
  it("blanks the previous result, so a user-asked check visibly resolves", () => {
    expect(markCheckingManual()).toEqual({ status: "checking", version: null, checkedAt: 0 });
  });
});

describe("markFailed", () => {
  it("is its own state, never 'current'", () => {
    const next = markFailed(500);
    expect(next.status).toBe("failed");
    expect(next.version).toBeNull();
    // The attempt still happened, so the timestamp moves.
    expect(next.checkedAt).toBe(500);
  });
});

describe("relativeAge", () => {
  it("reads as 'just now' under a minute", () => {
    expect(relativeAge(0, 30_000)).toBe("just now");
  });

  it("counts minutes, then hours, then days", () => {
    expect(relativeAge(0, 5 * 60_000)).toBe("5 min ago");
    expect(relativeAge(0, 2 * 3_600_000)).toBe("2 h ago");
    expect(relativeAge(0, 3 * 86_400_000)).toBe("3 d ago");
  });
});

describe("statusWord", () => {
  it("names every state, including the failure", () => {
    expect(statusWord("failed")).toBe("Check failed");
    expect(statusWord("available")).toBe("Update available");
    expect(statusWord("current")).toBe("Up to date");
    expect(statusWord("idle")).toBe("Not checked");
  });
});

describe("markChecking after a failure", () => {
  it("leaves no stale version behind", () => {
    const failed = markFailed(100);
    const next = markChecking(failed);
    expect(next.status).toBe("checking");
    expect(next.version).toBeNull();
  });
});
