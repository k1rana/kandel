import { describe, expect, it } from "vitest";

import {
  applyTheme,
  nextSetting,
  readSetting,
  resolveTheme,
  settingPresentation,
  THEME_STORAGE_KEY,
  writeSetting,
  type ThemeSetting,
} from "./theme";

describe("resolveTheme", () => {
  it("returns the explicit theme regardless of the OS", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("follows the OS preference for system", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });
});

describe("nextSetting", () => {
  it("cycles light -> dark -> system -> light", () => {
    expect(nextSetting("light")).toBe("dark");
    expect(nextSetting("dark")).toBe("system");
    expect(nextSetting("system")).toBe("light");
  });

  it("returns to the starting point after a full cycle", () => {
    let s: ThemeSetting = "system";
    for (let i = 0; i < 3; i++) s = nextSetting(s);
    expect(s).toBe("system");
  });
});

describe("readSetting", () => {
  it("defaults to system when nothing is stored", () => {
    expect(readSetting({ getItem: () => null })).toBe("system");
  });

  it("reads a stored value", () => {
    expect(readSetting({ getItem: (k) => (k === THEME_STORAGE_KEY ? "dark" : null) })).toBe("dark");
  });

  it("ignores an unrecognized stored value", () => {
    expect(readSetting({ getItem: () => "sepia" })).toBe("system");
  });
});

describe("writeSetting", () => {
  it("stores the setting under the known key", () => {
    const store = new Map<string, string>();
    writeSetting({ setItem: (k, v) => void store.set(k, v) }, "dark");
    expect(store.get(THEME_STORAGE_KEY)).toBe("dark");
  });

  it("does not throw when storage rejects the write", () => {
    expect(() =>
      writeSetting(
        {
          setItem: () => {
            throw new Error("quota");
          },
        },
        "light",
      ),
    ).not.toThrow();
  });
});

describe("applyTheme", () => {
  it("sets data-theme and color-scheme on the root", () => {
    // A minimal fake root keeps this test in the node environment.
    const root = { dataset: {} as Record<string, string>, style: {} as { colorScheme?: string } };
    applyTheme(root as unknown as HTMLElement, "dark");
    expect(root.dataset.theme).toBe("dark");
    expect(root.style.colorScheme).toBe("dark");
    applyTheme(root as unknown as HTMLElement, "light");
    expect(root.dataset.theme).toBe("light");
    expect(root.style.colorScheme).toBe("light");
  });
});

describe("settingPresentation", () => {
  it("names each setting and titles the next action", () => {
    expect(settingPresentation("light").label).toBe("Light");
    expect(settingPresentation("dark").label).toBe("Dark");
    expect(settingPresentation("system").label).toBe("Auto");
    expect(settingPresentation("light").title).toContain("dark");
    expect(settingPresentation("dark").title).toContain("system");
    expect(settingPresentation("system").title).toContain("light");
  });
});
