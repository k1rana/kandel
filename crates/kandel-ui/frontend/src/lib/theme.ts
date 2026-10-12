/** Theme selection: light, dark, or following the OS. */

import { getCurrentWindow } from "@tauri-apps/api/window";

export type Theme = "light" | "dark";
export type ThemeSetting = Theme | "system";

/** The key the setting is persisted under. */
export const THEME_STORAGE_KEY = "kandel.theme";

/** A pickable theme setting, as listed in the Settings menu. */
export interface ThemeOption {
  value: ThemeSetting;
  /** Menu label. */
  label: string;
}

/**
 * The theme settings, in menu order. `Auto` leads because following the OS is
 * the default and the least surprising choice for a background utility.
 */
export const THEME_OPTIONS: readonly ThemeOption[] = [
  { value: "system", label: "Auto" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/**
 * The menu label for a setting.
 *
 * `Auto` names the theme it currently follows — `Auto (Dark)` — so the option
 * is not opaque while the OS is in charge. The explicit settings show their own
 * name. Base labels come from {@link THEME_OPTIONS} so the two cannot drift.
 */
export function themeLabel(setting: ThemeSetting, prefersDark: boolean): string {
  const base = THEME_OPTIONS.find((option) => option.value === setting)?.label ?? setting;
  return setting === "system" ? `${base} (${prefersDark ? "Dark" : "Light"})` : base;
}

/** Resolves a setting against the current OS preference. */
export function resolveTheme(setting: ThemeSetting, prefersDark: boolean): Theme {
  if (setting === "system") return prefersDark ? "dark" : "light";
  return setting;
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

