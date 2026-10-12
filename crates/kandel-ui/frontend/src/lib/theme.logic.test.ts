import { describe, expect, it } from "vitest";

import {
  applyTheme,
  readSetting,
  resolveTheme,
  THEME_OPTIONS,
  THEME_STORAGE_KEY,
  themeLabel,
  writeSetting,
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

describe("THEME_OPTIONS", () => {
  it("lists Auto, Light, Dark in that order", () => {
    expect(THEME_OPTIONS.map((o) => o.value)).toEqual(["system", "light", "dark"]);
    expect(THEME_OPTIONS.map((o) => o.label)).toEqual(["Auto", "Light", "Dark"]);
  });
});

describe("themeLabel", () => {
  it("names the explicit settings plainly", () => {
    expect(themeLabel("light", true)).toBe("Light");
    expect(themeLabel("dark", false)).toBe("Dark");
  });

  it("resolves Auto to the theme the OS is following", () => {
    expect(themeLabel("system", true)).toBe("Auto (Dark)");
    expect(themeLabel("system", false)).toBe("Auto (Light)");
  });
});
