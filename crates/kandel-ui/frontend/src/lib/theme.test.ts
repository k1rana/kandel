import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const themeCss = readFileSync(resolve(here, "../theme.css"), "utf8");

/**
 * `theme.css` is a plain stylesheet imported from `main.ts`, not a Svelte
 * component. `:global(...)` only exists inside Svelte `<style>` blocks — and an
 * unknown selector in a comma list invalidates the ENTIRE rule in CSS. That
 * silently dropped `box-sizing: border-box` and made every `width:100%` input
 * overflow its padding and border, which produced a horizontal scrollbar and
 * let fields slide under the vertical scrollbar.
 */
describe("theme.css is plain CSS", () => {
  // Strip comments so the check tests code, not the explanatory prose.
  const code = themeCss.replace(/\/\*[\s\S]*?\*\//g, "");

  it("does not use the Svelte-only :global() construct", () => {
    expect(code).not.toContain(":global(");
  });

  it("applies box-sizing to every element", () => {
    expect(code).toMatch(/box-sizing:\s*border-box/);
  });

  it("keeps scrollbar-width/color out of the WebView2 path", () => {
    // Setting those standard properties non-auto makes Chromium ignore
    // ::-webkit-scrollbar; the Firefox fallback is gated behind @supports.
    const withoutFallback = themeCss.replace(
      /@supports not selector\(::-webkit-scrollbar\)\s*\{[\s\S]*?\n\}/,
      "",
    );
    expect(withoutFallback).not.toMatch(/scrollbar-width\s*:/);
    expect(withoutFallback).not.toMatch(/scrollbar-color\s*:/);
    expect(themeCss).toContain("::-webkit-scrollbar-thumb");
  });
});
