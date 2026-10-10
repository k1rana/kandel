<script lang="ts">
  import { getAboutInfo, openRepository } from "./api";
  import type { AboutInfo } from "./types";

  // The application mark, generated from the same geometry as the app icon by
  // `scripts/make_icons.py`, so the two can never drift apart.
  import markUrl from "../assets/brand-mark.svg";

  let { open, onClose }: { open: boolean; onClose: () => void } = $props();

  let info = $state<AboutInfo | null>(null);
  let error = $state("");
  let copied = $state(false);
  let copyFailed = $state(false);

  // Fetch lazily: the metadata is only needed once the popover is first opened,
  // and re-fetch is not needed since the version cannot change at runtime.
  $effect(() => {
    if (!open || info || error) return;
    getAboutInfo()
      .then((value) => (info = value))
      .catch((e) => (error = String(e)));
  });

  // Close on Escape while open, so the popover behaves like the other
  // dismissible surfaces in the app.
  $effect(() => {
    if (!open) return;
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  // The copy confirmation is transient: reset it when the popover closes so
  // reopening starts from a clean state.
  $effect(() => {
    if (!open) {
      copied = false;
      copyFailed = false;
    }
  });

  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  async function copyConfigPath() {
    if (!info) return;
    clearTimeout(copyTimer);
    try {
      await navigator.clipboard.writeText(info.configPath);
      copied = true;
      copyFailed = false;
    } catch {
      // Clipboard access can be refused (permissions, focus); say so rather
      // than silently doing nothing.
      copied = false;
      copyFailed = true;
    }
    copyTimer = setTimeout(() => {
      copied = false;
      copyFailed = false;
    }, 1600);
  }

  function openRepo() {
    void openRepository();
  }
</script>

{#if open}
  <div class="scrim" onclick={onClose} oncontextmenu={(e) => e.preventDefault()} role="presentation"></div>
  <div class="about" role="dialog" aria-label="About Kandel">
    <div class="head">
      <span class="mark" aria-hidden="true">
        <img src={markUrl} alt="" width="32" height="32" />
      </span>
      <div class="ident">
        <span class="name">{info?.name ?? "Kandel"}</span>
        <span class="ver tnum">v{info?.version ?? "…"}</span>
      </div>

      <!-- Source link: the GitHub mark, drawn as a button so it reads as a
           control. Kept achromatic, not lime: lime is the primary action and
           the health signal in this world, so it is not spent on a link. -->
      <button class="ghost-btn" type="button" title="View source on GitHub" aria-label="View source on GitHub" onclick={openRepo}>
        <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
          <path
            fill="currentColor"
            d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.012 8.012 0 0 0 16 8c0-4.42-3.58-8-8-8z"
          />
        </svg>
      </button>
    </div>

    <p class="blurb">
      Per-process supervision for the Windows interactive session. Keeps the apps you
      choose running, and brings them back when they die.
    </p>

    {#if error}
      <p class="error" role="alert">Could not read the build details: {error}</p>
    {:else if info}
      <dl class="rows">
        <div class="row">
          <dt>Config</dt>
          <dd class="path">
            <span class="path-text selectable" title={info.configPath}>{info.configPath}</span>
            <button
              class="copy"
              class:ok={copied}
              type="button"
              title={copied ? "Copied" : copyFailed ? "Could not copy" : "Copy path"}
              aria-label={copied ? "Copied" : copyFailed ? "Could not copy" : "Copy config path"}
              onclick={copyConfigPath}
            >
              {#if copied}
                <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
                  <path d="M2 6.5l2.6 2.6L10 3.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
                </svg>
              {:else}
                <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
                  <rect x="0.6" y="0.6" width="7" height="7" rx="1.4" fill="none" stroke="currentColor" stroke-width="1.1" />
                  <path d="M3.4 9.4v1.2a1.4 1.4 0 0 0 1.4 1.4h4.2a1.4 1.4 0 0 0 1.4-1.4V6.4A1.4 1.4 0 0 0 9 5h-1.2" fill="none" stroke="currentColor" stroke-width="1.1" />
                </svg>
              {/if}
            </button>
          </dd>
        </div>
      </dl>
    {/if}
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: transparent;
  }

  /* Anchored under the wordmark, in the same visual language as the row menu:
     a white card with hairline borders and soft radii, no shadow. */
  .about {
    position: fixed;
    top: 52px;
    left: var(--s-16);
    z-index: 90;
    width: min(400px, calc(100vw - 2 * var(--s-16)));
    padding: var(--s-20);
    background: var(--canvas);
    border: 1px solid var(--ink);
    border-radius: var(--r-inner);
    display: flex;
    flex-direction: column;
    gap: var(--s-16);
    animation: fade 0.14s var(--ease-out);
  }

  @keyframes fade {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--s-12);
  }
  .mark {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: none;
  }
  .mark img {
    display: block;
    border-radius: var(--r-input);
  }
  .ident {
    display: flex;
    align-items: baseline;
    gap: var(--s-8);
    min-width: 0;
    flex: 1;
  }
  .name {
    font-size: var(--text-subheading);
    font-weight: var(--w-medium);
    letter-spacing: var(--tracking-tight);
    color: var(--ink);
    white-space: nowrap;
  }
  .ver {
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
  }

  /* Both header/row icon buttons share one treatment. */
  .ghost-btn,
  .copy {
    display: grid;
    place-items: center;
    flex: none;
    border: 1px solid var(--ash);
    background: transparent;
    color: var(--graphite);
    cursor: pointer;
    transition:
      background 0.12s var(--ease-out),
      color 0.12s var(--ease-out),
      border-color 0.12s var(--ease-out);
  }
  .ghost-btn {
    width: 32px;
    height: 32px;
    border-radius: var(--r-input);
  }
  .copy {
    width: 26px;
    height: 26px;
    border-radius: 8px;
  }
  .ghost-btn:hover,
  .copy:hover {
    background: var(--wash);
    color: var(--ink);
    border-color: var(--stone);
  }
  .copy.ok {
    color: var(--ink);
    border-color: var(--ink);
  }

  .blurb {
    margin: 0;
    font-size: var(--text-body-sm);
    line-height: 1.55;
    color: var(--graphite);
  }

  .rows {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s-12);
    border-top: 1px solid var(--ash);
    padding-top: var(--s-16);
  }
  .row {
    display: grid;
    grid-template-columns: 64px 1fr;
    gap: var(--s-12);
    align-items: center;
  }
  dt {
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
  }
  dd {
    margin: 0;
    font-size: var(--text-body-sm);
    color: var(--ink);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The config row pairs the (truncated) path with its copy control. */
  .path {
    display: flex;
    align-items: center;
    gap: var(--s-8);
  }
  .path-text {
    flex: 1;
    min-width: 0;
    font-size: var(--text-caption);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error {
    margin: 0;
    font-size: var(--text-body-sm);
    color: var(--alert);
  }
</style>
