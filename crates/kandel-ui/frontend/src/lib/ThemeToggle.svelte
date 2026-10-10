<script lang="ts">
  import type { ThemeSetting } from "./theme";
  import { settingPresentation } from "./theme";

  let { setting, onToggle }: { setting: ThemeSetting; onToggle: () => void } = $props();

  let presentation = $derived(settingPresentation(setting));
</script>

<button
  class="theme"
  type="button"
  title={presentation.title}
  aria-label={presentation.title}
  onclick={onToggle}
>
  <!-- Drawn 12px glyphs, not a font symbol or emoji. -->
  {#if setting === "light"}
    <svg viewBox="0 0 12 12" width="13" height="13" aria-hidden="true">
      <circle cx="6" cy="6" r="2.6" fill="none" stroke="currentColor" stroke-width="1.1" />
      <g stroke="currentColor" stroke-width="1.1" stroke-linecap="round">
        <path d="M6 0.4v1.5M6 10.1v1.5M0.4 6h1.5M10.1 6h1.5" />
        <path d="M2.04 2.04l1.06 1.06M8.9 8.9l1.06 1.06M9.96 2.04L8.9 3.1M3.1 8.9L2.04 9.96" />
      </g>
    </svg>
  {:else if setting === "dark"}
    <svg viewBox="0 0 12 12" width="13" height="13" aria-hidden="true">
      <path
        d="M10.2 7.3A4.6 4.6 0 0 1 4.7 1.8a4.7 4.7 0 1 0 5.5 5.5z"
        fill="none"
        stroke="currentColor"
        stroke-width="1.1"
        stroke-linejoin="round"
      />
    </svg>
  {:else}
    <svg viewBox="0 0 12 12" width="13" height="13" aria-hidden="true">
      <path
        d="M3.1 8.1a1.4 1.4 0 1 0 0-2.8 1.4 1.4 0 0 0 0 2.8z"
        fill="currentColor"
      />
      <path
        d="M6 1v3M6 8v3M11 6H8M4 6H1"
        stroke="currentColor"
        stroke-width="1.1"
        stroke-linecap="round"
      />
    </svg>
  {/if}
  <span class="label">{presentation.label}</span>
</button>

<style>
  .theme {
    display: inline-flex;
    align-items: center;
    gap: var(--s-8);
    height: 36px;
    padding: 0 var(--s-12) 0 var(--s-12);
    border: 1px solid var(--ash);
    border-radius: var(--r-pill);
    background: transparent;
    color: var(--graphite);
    font-family: inherit;
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    cursor: pointer;
    transition:
      background 0.15s var(--ease-out),
      color 0.15s var(--ease-out),
      border-color 0.15s var(--ease-out);
  }
  .theme:hover {
    background: var(--wash);
    color: var(--ink);
    border-color: var(--stone);
  }
  .label {
    line-height: 1;
  }
</style>
