<script lang="ts">
  import { installUpdate } from "./api";
  import { parseReleaseNotes } from "./markdown";
  import { currentUpdate, loadNotes, notesState, retryNotes } from "./update.svelte";

  /**
   * The release-notes modal, opened from About's update button.
   *
   * It takes only the version; the notes come from the store, which fetches
   * them from the GitHub API and caches them per tag (see `releaseNotes.ts`).
   */
  let {
    open,
    version,
    onClose,
  }: {
    open: boolean;
    version: string;
    onClose: () => void;
  } = $props();

  let panelEl = $state<HTMLDivElement | null>(null);
  let closeEl = $state<HTMLButtonElement | null>(null);
  let progress = $state<number | null>(null);
  let progressTotal = $state<number | null>(null);
  let installError = $state("");

  let notes = $derived(notesState());
  let blocks = $derived(notes.body ? parseReleaseNotes(notes.body) : []);

  // Load on open, and again if the version changes while it is open (a silent
  // re-check behind the modal can find a newer release).
  $effect(() => {
    if (open && version) void loadNotes(version);
  });

  // Move focus to the close control on open, and hand it back on close, so the
  // modal is usable from the keyboard and returns the user where they were.
  $effect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    closeEl?.focus();
    return () => previous?.focus?.();
  });

  // Trap Tab inside the panel, so focus cannot wander behind the scrim.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key !== "Tab" || !panelEl) return;
    const focusable = panelEl.querySelectorAll<HTMLElement>(
      'button:not([aria-disabled="true"]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
    );
    if (!focusable.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  async function runInstall() {
    const update = currentUpdate();
    if (!update) {
      installError = "This build cannot install the update automatically. Use the Releases page.";
      return;
    }
    installError = "";
    progress = 0;
    try {
      await installUpdate(update, (downloaded, total) => {
        progress = downloaded;
        if (total) progressTotal = total;
      });
    } catch (e) {
      progress = null;
      installError = `Could not install the update: ${e}`;
    }
  }
</script>

{#if open}
  <div class="scrim" onclick={onClose} oncontextmenu={(e) => e.preventDefault()} role="presentation"></div>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    aria-labelledby="release-title"
    tabindex="-1"
    bind:this={panelEl}
    onkeydown={onKeydown}
  >
    <div class="head">
      <div class="titles">
        <span class="eyebrow">Release notes</span>
        <h2 id="release-title" class="ver tnum">v{version}</h2>
      </div>
      <button class="ghost-btn" type="button" aria-label="Close release notes" bind:this={closeEl} onclick={onClose}>
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
          <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" stroke="currentColor" stroke-width="1.1" />
        </svg>
      </button>
    </div>

    <div class="body">
      {#if notes.status === "loading"}
        <p class="note-para muted">Loading release notes…</p>
      {:else if notes.status === "rate-limited"}
        <p class="note-para muted">
          GitHub's request limit was reached, so the notes are unavailable for now.
          {#if notes.resetAt}
            Try again after {new Date(notes.resetAt).toLocaleTimeString()}.
          {/if}
        </p>
      {:else if notes.status === "error"}
        <p class="note-para muted">
          {notes.message || "Could not load the release notes."}
          <button class="link" type="button" onclick={() => void retryNotes()}>Try again</button>
        </p>
      {:else if blocks.length}
        {#if notes.stale}
          <p class="stale">Showing notes cached earlier; GitHub is unreachable right now.</p>
        {/if}
        {#each blocks as block, i (i)}
          {#if block.kind === "heading"}
            <p class="note-heading" class:is-title={block.level <= 2}>
              {#each block.spans as span, j (j)}
                {#if span.kind === "strong"}<strong>{span.text}</strong>
                {:else if span.kind === "code"}<code>{span.text}</code>
                {:else}{span.text}{/if}
              {/each}
            </p>
          {:else if block.kind === "list"}
            <ul class="note-list">
              {#each block.items as item, j (j)}
                <li>
                  {#each item as span, k (k)}
                    {#if span.kind === "strong"}<strong>{span.text}</strong>
                    {:else if span.kind === "code"}<code>{span.text}</code>
                    {:else}{span.text}{/if}
                  {/each}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="note-para">
              {#each block.spans as span, j (j)}
                {#if span.kind === "strong"}<strong>{span.text}</strong>
                {:else if span.kind === "code"}<code>{span.text}</code>
                {:else}{span.text}{/if}
              {/each}
            </p>
          {/if}
        {/each}
      {:else}
        <p class="note-para muted">No release notes were published for this version.</p>
      {/if}
    </div>

    <div class="foot">
      {#if progress !== null}
        <p class="progress" role="status">
          Downloading… {progressTotal ? Math.round((progress / progressTotal) * 100) : "…"}%
        </p>
      {/if}
      {#if installError}
        <p class="error" role="alert">{installError}</p>
      {/if}
      <div class="actions">
        <button class="ghost" type="button" onclick={onClose}>Later</button>
        <button class="install" type="button" disabled={progress !== null} onclick={runInstall}>
          Install update
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: var(--scrim);
  }

  .modal {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 110;
    width: min(520px, calc(100vw - 2 * var(--s-32)));
    max-height: calc(100vh - 2 * var(--s-32));
    display: flex;
    flex-direction: column;
    background: var(--canvas);
    border: 1px solid var(--ink);
    border-radius: var(--r-inner);
  }

  .head {
    display: flex;
    align-items: flex-start;
    gap: var(--s-12);
    padding: var(--s-20);
    border-bottom: 1px solid var(--ash);
  }
  .titles {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s-4);
  }
  .eyebrow {
    font-size: var(--text-eyebrow);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
  }
  .ver {
    margin: 0;
    font-size: var(--text-subheading);
    font-weight: var(--w-medium);
    letter-spacing: var(--tracking-tight);
    color: var(--ink);
  }

  .ghost-btn {
    display: grid;
    place-items: center;
    flex: none;
    width: 32px;
    height: 32px;
    border: 1px solid var(--ash);
    border-radius: var(--r-input);
    background: transparent;
    color: var(--graphite);
    cursor: pointer;
    transition:
      background 0.12s var(--ease-out),
      color 0.12s var(--ease-out),
      border-color 0.12s var(--ease-out);
  }
  .ghost-btn:hover {
    background: var(--wash);
    color: var(--ink);
    border-color: var(--stone);
  }

  /* Only the body scrolls; the head and foot stay pinned, as the editor does. */
  .body {
    overflow-y: auto;
    padding: var(--s-16) var(--s-20);
    display: flex;
    flex-direction: column;
    gap: var(--s-8);
  }

  .note-heading {
    margin: var(--s-8) 0 0;
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
  }
  /* The leading version heading is the modal's subject, not a section label. */
  .note-heading.is-title {
    margin-top: 0;
    font-size: var(--text-body);
    text-transform: none;
    letter-spacing: normal;
    color: var(--ink);
  }

  .note-list {
    margin: 0;
    padding-left: var(--s-20);
    display: flex;
    flex-direction: column;
    gap: var(--s-4);
  }
  .note-list li,
  .note-para {
    font-size: var(--text-body-sm);
    line-height: 1.5;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  .note-para {
    margin: 0;
  }
  .note-para.muted {
    color: var(--graphite);
  }
  .stale {
    margin: 0 0 var(--s-4);
    font-size: var(--text-caption);
    color: var(--degraded);
  }
  /* A retry inside a sentence, not a second button competing with the footer. */
  .link {
    border: none;
    background: none;
    padding: 0;
    font: inherit;
    color: var(--ink);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .link:hover {
    color: var(--graphite);
  }
  strong {
    font-weight: var(--w-medium);
  }
  code {
    font-family: var(--font);
    font-size: var(--text-caption);
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--wash);
    color: var(--ink);
  }

  .foot {
    padding: var(--s-16) var(--s-20);
    border-top: 1px solid var(--ash);
    display: flex;
    flex-direction: column;
    gap: var(--s-12);
  }
  .progress {
    margin: 0;
    font-size: var(--text-body-sm);
    color: var(--graphite);
  }
  .error {
    margin: 0;
    font-size: var(--text-body-sm);
    color: var(--alert);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--s-8);
  }

  .ghost,
  .install {
    padding: var(--s-8) var(--s-16);
    border-radius: var(--r-pill);
    font: inherit;
    font-size: var(--text-body-sm);
    font-weight: var(--w-medium);
    cursor: pointer;
    transition:
      background 0.12s var(--ease-out),
      filter 0.12s var(--ease-out);
  }
  .ghost {
    border: 1px solid var(--ash);
    background: transparent;
    color: var(--ink);
  }
  .ghost:hover {
    background: var(--wash);
    border-color: var(--stone);
  }
  /* Lime is the only filled action, as everywhere else in this world. */
  .install {
    border: none;
    background: var(--lime);
    color: var(--lime-ink);
  }
  .install:hover:not(:disabled) {
    filter: brightness(0.95);
  }
  .install:disabled {
    cursor: default;
    filter: none;
    opacity: 0.6;
  }
</style>
