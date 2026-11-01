<script lang="ts">
  import { getAboutInfo, openRepository } from "./api";
  import type { AboutInfo } from "./types";
  import { checkForUpdate, updateState } from "./update.svelte";
  import { relativeAge, statusWord, type UpdateStatus } from "./update";
  import Tooltip from "./Tooltip.svelte";
  import UpdateModal from "./UpdateModal.svelte";

  // The application mark, generated from the same geometry as the app icon by
  // `scripts/make_icons.py`, so the two can never drift apart.
  import markUrl from "../assets/brand-mark.svg";

  let { open, onClose }: { open: boolean; onClose: () => void } = $props();

  let info = $state<AboutInfo | null>(null);
  let error = $state("");
  let copied = $state(false);
  let copyFailed = $state(false);

  // What the copy control has to say right now: the action, or the outcome.
  let copyLabel = $derived(copied ? "Copied" : copyFailed ? "Could not copy" : "Copy path");

  // The release-notes modal, opened from the update button. It takes only the
  // version; the notes are fetched and cached by the store.
  let notesOpen = $state(false);
  let notesVersion = $state("");

  let status = $derived(updateState());
  let checking = $derived(status.status === "checking");

  // Announced to screen readers when a check finishes, since the visible text
  // only covers the actionable states. Empty while idle or checking, so the
  // region does not fire on a state that has no outcome yet.
  let announcement = $derived(
    !open || status.status === "idle" || checking ? "" : statusWord(status.status),
  );

  // Only the update itself needs a check; a failed check is retried by hand.
  $effect(() => {
    if (open && status.status === "idle") void checkForUpdate();
  });

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

  /**
   * Opens the release notes, refreshing first.
   *
   * Opening the notes is the moment the user is deciding, so it is also the
   * moment to re-check: an update published since the startup check should be
   * offered rather than reported as current. The check is silent, so the button
   * behind the modal does not blank out mid-flight.
   */
  function openNotes() {
    notesVersion = status.version ?? "";
    notesOpen = true;
    // Silent: the button behind the modal keeps its dot instead of blanking, and
    // the modal leans on the store's cache, so this costs no extra request.
    void checkForUpdate();
  }

  // Keep the open modal in step with a fresher check: if a newer version is
  // found while the notes are open, show it rather than the stale one the modal
  // was opened with. The notes for it are fetched by the same effect.
  $effect(() => {
    if (!notesOpen || status.status !== "available") return;
    if (status.version && status.version !== notesVersion) notesVersion = status.version;
  });

  /**
   * Runs the control's current job: install when a release is on offer,
   * otherwise check. One button, two verbs, decided by state.
   */
  function onControlClick() {
    if (checking) return;
    if (status.status === "available") openNotes();
    else void checkForUpdate(true);
  }

  /**
   * Names the control's current job, and the result when there is nothing to
   * do. The verb leads, so the button never reads as both a check and an
   * install at once.
   */
  function checkTitle(s: { status: UpdateStatus; checkedAt: number; version: string | null }): string {
    switch (s.status) {
      case "checking":
        return "Checking for updates…";
      case "available":
        return `Install v${s.version} · release notes`;
      case "idle":
        return "Check for updates";
      case "failed":
        return `Could not check · retry · last checked ${relativeAge(s.checkedAt, Date.now())}`;
      case "current":
        return `Up to date · last checked ${relativeAge(s.checkedAt, Date.now())}`;
    }
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

      <!-- Source link, then the update control. The source link stays
           achromatic; the update control fills with lime only when a release is
           on offer, because lime marks the one action worth taking. -->
      <Tooltip text="View source on GitHub">
        <button class="ghost-btn" type="button" aria-label="View source on GitHub" onclick={openRepo}>
          <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
            <path
              fill="currentColor"
              d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.012 8.012 0 0 0 16 8c0-4.42-3.58-8-8-8z"
            />
          </svg>
        </button>
      </Tooltip>

      <!-- One control carries the whole update story. It is the refresh action
           until a release is found, then it becomes the install action: same
           slot, same tooltip family, icon swapped. A corner dot marks state
           without moving anything. -->
      <Tooltip text={checkTitle(status)}>
        <button
          class="ghost-btn check"
          class:is-checking={checking}
          class:is-failed={status.status === "failed"}
          class:has-update={status.status === "available"}
          type="button"
          aria-label={checkTitle(status)}
          aria-disabled={checking}
          onclick={onControlClick}
        >
          {#if status.status === "available"}
            <!-- Download: a filled lime control, because lime is this world's
                 only filled action and this is now the primary one. -->
            <svg viewBox="0 0 12 12" width="14" height="14" aria-hidden="true">
              <path
                d="M6 1.5v6M3.4 5.2 6 7.8l2.6-2.6M1.8 9.4v1.1h8.4V9.4"
                fill="none"
                stroke="currentColor"
                stroke-width="1.3"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          {:else}
            <svg viewBox="0 0 12 12" width="14" height="14" aria-hidden="true">
              <path
                d="M10.2 5.2a4.2 4.2 0 1 0-.6 2.6M10.2 2.2v3h-3"
                fill="none"
                stroke="currentColor"
                stroke-width="1.2"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          {/if}
          <span class="dot" aria-hidden="true"></span>
        </button>
      </Tooltip>
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
            <!-- The path is cut to fit the row; the tooltip carries the whole
                 of it, which is what the user needs to paste somewhere else. -->
            <Tooltip text={info.configPath}>
              <span class="path-text selectable">{info.configPath}</span>
            </Tooltip>
            <Tooltip text={copyLabel}>
              <button
                class="copy"
                class:ok={copied}
                type="button"
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
            </Tooltip>
          </dd>
        </div>
      </dl>

      <!-- The update state lives entirely in the header control above: its icon,
           its dot, and its tooltip. Nothing repeats it here. The live region
           carries the outcome to screen readers, who cannot hover. -->
      <p class="visually-hidden" aria-live="polite">{announcement}</p>
    {/if}
  </div>
{/if}

<!-- Release notes, over the About popover, so the popover is not also to be
     dismissed before the notes can be read. -->
<UpdateModal open={notesOpen} version={notesVersion} onClose={() => (notesOpen = false)} />

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

  /* The corner dot marks state without moving the control: ink when a release
     is available, lime while a check is in flight, red when one failed. */
  .check {
    position: relative;
  }
  .check .dot {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: transparent;
  }
  .check.is-checking .dot {
    background: var(--lime);
    box-shadow: 0 0 0 1.5px var(--canvas);
  }
  .check.is-failed .dot {
    background: var(--alert);
    box-shadow: 0 0 0 1.5px var(--canvas);
  }
  /* With a release on offer the control is the filled action, so it needs no
     separate notice dot: the lime fill is the notice. */
  .check.has-update {
    background: var(--lime);
    border-color: var(--lime);
    color: var(--lime-ink);
  }
  .check.has-update:hover {
    filter: brightness(0.95);
    border-color: var(--lime);
    color: var(--lime-ink);
  }
  .check.is-checking svg {
    animation: spin 0.9s linear infinite;
  }
  /* `aria-disabled`, not `disabled`: a genuinely disabled button fires no hover
     and no focus, so the tooltip that explains the wait could never be read. */
  .check[aria-disabled="true"] {
    cursor: default;
    color: var(--graphite);
  }
  .check[aria-disabled="true"]:hover {
    background: transparent;
    border-color: var(--ash);
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .check.is-checking svg {
      animation: none;
    }
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

  /* Screen-reader only, for the live region that announces a check result. */
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
