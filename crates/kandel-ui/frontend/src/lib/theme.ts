/** Theme selection: light, dark, or following the OS. */

import { getCurrentWindow } from "@tauri-apps/api/window";

export type Theme = "light" | "dark";
export type ThemeSetting = Theme | "system";

/** The key the setting is persisted under. */
export const THEME_STORAGE_KEY = "kandel.theme";

/** Resolves a setting against the current OS preference. */
export function resolveTheme(setting: ThemeSetting, prefersDark: boolean): Theme {
  if (setting === "system") return prefersDark ? "dark" : "light";
  return setting;
}

/** Cycles light -> dark -> system, the order the toggle steps through. */
export function nextSetting(current: ThemeSetting): ThemeSetting {
  switch (current) {
    case "light":
      return "dark";
    case "dark":
      return "system";
    default:
      return "light";
  }
}

/** Reads the persisted setting, defaulting to `system`. */
export function readSetting(storage: Pick<Storage, "getItem">): ThemeSetting {
  const raw = storage.getItem(THEME_STORAGE_KEY);
  return raw === "light" || raw === "dark" || raw === "system" ? raw : "system";
}

/** Persists the setting. Storage can throw (private mode); ignore that. */
export function writeSetting(storage: Pick<Storage, "setItem">, setting: ThemeSetting): void {
  try {
    storage.setItem(THEME_STORAGE_KEY, setting);
  } catch {
    // Non-fatal: the theme still applies for this session.
  }
}

/**
 * Applies a resolved theme to the document root.
 *
 * Sets `data-theme` for the CSS variables and `color-scheme` so the browser's
 * own surfaces (scrollbars, form controls, the canvas behind the page) match.
 */
export function applyTheme(root: HTMLElement, theme: Theme): void {
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

/** Tells the window to follow the theme, so the OS frame matches. */
export function syncWindowTheme(theme: Theme): void {
  try {
    void getCurrentWindow().setTheme(theme);
  } catch {
    // Outside Tauri (e.g. a plain browser during development) this is a no-op.
  }
}

/** The label + icon shown by the toggle for each setting. */
export function settingPresentation(setting: ThemeSetting): {
  label: string;
  title: string;
} {
  switch (setting) {
    case "light":
      return { label: "Light", title: "Theme: light. Click for dark." };
    case "dark":
      return { label: "Dark", title: "Theme: dark. Click to follow the system." };
    default:
      return { label: "Auto", title: "Theme: follows the system. Click for light." };
  }
}
