<script lang="ts">
  import { closeWindow, minimizeWindow, toggleMaximizeWindow } from "./window";
  import type { Tally } from "./format";
  import type { ThemeSetting } from "./theme";
  import About from "./About.svelte";
  import SettingsMenu from "./SettingsMenu.svelte";

  // The application mark, generated from the same geometry as the app icon.
  import markUrl from "../assets/brand-mark.svg";

  let {
    maximized = false,
    tally,
    onAdd,
    themeSetting,
    onSelectTheme,
    prefersDark = false,
    updateAvailable = false,
  }: {
    maximized?: boolean;
    tally: Tally;
    onAdd: () => void;
    themeSetting: ThemeSetting;
    onSelectTheme: (next: ThemeSetting) => void;
    /** The OS preference, forwarded to the Settings menu's `Auto` label. */
    prefersDark?: boolean;
    /**
     * Whether a newer release is known to exist.
     *
     * Drives the dot above the wordmark. The update flow lives in About, so the
     * wordmark carries the notice; until a source for it exists this stays
     * `false` and no dot is drawn.
     */
    updateAvailable?: boolean;
  } = $props();

  let aboutOpen = $state(false);

  // Describes the dot for assistive tech; the wordmark alone would not say why
  // it is marked.
  const UPDATE_NOTICE_ID = "kandel-update-notice";

  // Micro-caps summary of the ledger: total, and how many are alive.
  let summary = $derived(
    tally.total === 0
      ? "No apps yet"
      : `${tally.total} ${tally.total === 1 ? "app" : "apps"} · ${tally.running} running`,
  );
</script>

<!-- One white bar carries the wordmark, the live tally, the primary action,
     and the window controls. The bar itself is the drag region; the controls,
     the wordmark button, and the Add button sit outside it. -->
<header class="topbar">
  <div class="lead" data-tauri-drag-region>
    <button
      class="wordmark"
      type="button"
      title="About Kandel"
      aria-haspopup="dialog"
      aria-expanded={aboutOpen}
      aria-describedby={updateAvailable ? UPDATE_NOTICE_ID : undefined}
      onclick={() => (aboutOpen = !aboutOpen)}
    >
      <!-- With custom window chrome there is no OS title-bar icon, so the mark
           here is the only place the window shows its identity. -->
      <img class="mark" src={markUrl} alt="" width="18" height="18" />
      <span>Kandel</span>
      {#if updateAvailable}
        <span class="mark-dot" aria-hidden="true"></span>
        <span id={UPDATE_NOTICE_ID} class="visually-hidden">Update available</span>
      {/if}
    </button>
    <span class="rule" aria-hidden="true"></span>
    <span class="tally">{summary}</span>
  </div>

  <div class="trail">
    <button class="add" type="button" onclick={onAdd}>
      <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
        <path d="M6 1v10M1 6h10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
      </svg>
      Add app
    </button>

    <SettingsMenu setting={themeSetting} {prefersDark} onSelect={onSelectTheme} />

    <div class="wc">
      <button class="ctl" type="button" title="Minimize" aria-label="Minimize" onclick={() => minimizeWindow()}>
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
          <path d="M0 5h10" stroke="currentColor" stroke-width="1.1" />
        </svg>
      </button>
      <button
        class="ctl"
        type="button"
        title={maximized ? "Restore" : "Maximize"}
        aria-label={maximized ? "Restore" : "Maximize"}
        onclick={() => toggleMaximizeWindow()}
      >
        {#if maximized}
          <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
            <path d="M2.5 2.5V0.5h7v7h-2" fill="none" stroke="currentColor" stroke-width="1.1" />
            <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1.1" />
          </svg>
        {:else}
          <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
            <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1.1" />
          </svg>
        {/if}
      </button>
      <button class="ctl ctl-close" type="button" title="Close" aria-label="Close" onclick={() => closeWindow()}>
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
          <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" stroke="currentColor" stroke-width="1.1" />
        </svg>
      </button>
    </div>
  </div>
</header>

<About open={aboutOpen} onClose={() => (aboutOpen = false)} />

<style>
  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-16);
    height: 54px;
    padding-left: var(--s-20);
    background: var(--canvas);
    border-bottom: 1px solid var(--ash);
  }

  .lead {
    display: flex;
    align-items: center;
    gap: var(--s-12);
    min-width: 0;
    flex: 1;
    height: 100%;
  }
  .wordmark {
    /* Reads as the wordmark but is a button: it opens the About popover. */
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--s-8);
    font-family: inherit;
    font-size: var(--text-body);
    font-weight: var(--w-medium);
    letter-spacing: -0.01em;
    color: var(--ink);
    background: transparent;
    border: none;
    padding: 2px var(--s-8);
    margin-left: calc(-1 * var(--s-8));
    border-radius: var(--r-input);
    cursor: pointer;
    transition: background 0.12s var(--ease-out);
    white-space: nowrap;
  }
  .mark {
    display: block;
    border-radius: 5px;
    flex: none;
  }
  .wordmark:hover {
    background: var(--wash);
  }
  /* The update notice: a small ink dot riding the wordmark's top-right corner.
     Ink, not lime — lime already means "alive" on the state charge, so a green
     dot here would read as health rather than as an available update. */
  .mark-dot {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 7px;
    height: 7px;
    border-radius: var(--r-pill);
    background: var(--ink);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .rule {
    width: 1px;
    height: 18px;
    background: var(--ash);
    flex: none;
  }
  .tally {
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .trail {
    display: flex;
    align-items: center;
    gap: var(--s-8);
    padding-right: var(--s-4);
  }

  .add {
    display: inline-flex;
    align-items: center;
    gap: var(--s-8);
    height: 36px;
    padding: 0 var(--s-16);
    border: none;
    border-radius: var(--r-pill);
    background: var(--lime);
    color: var(--lime-ink);
    font-family: inherit;
    font-size: var(--text-body-sm);
    font-weight: var(--w-medium);
    cursor: pointer;
    transition: filter 0.15s var(--ease-out);
  }
  .add:hover {
    filter: brightness(0.95);
  }
  .add:active {
    filter: brightness(0.88);
  }

  .wc {
    display: flex;
    align-items: stretch;
    height: 54px;
    margin-left: var(--s-4);
  }
  .ctl {
    display: grid;
    place-items: center;
    width: 46px;
    border: none;
    background: transparent;
    color: var(--graphite);
    cursor: default;
    transition:
      background 0.12s var(--ease-out),
      color 0.12s var(--ease-out);
  }
  .ctl:hover {
    background: var(--wash);
    color: var(--ink);
  }
  .ctl-close:hover {
    background: var(--alert);
    color: #fff;
  }
</style>
