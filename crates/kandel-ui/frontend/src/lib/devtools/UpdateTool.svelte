<script lang="ts">
  import { checkForUpdate, notesState, updateState } from "../update.svelte";

  /**
   * The updater's panel in the devtools dock.
   *
   * Actions arrive as a prop so this component and the module that registers it
   * do not import each other.
   */
  let {
    actions,
    version: initialVersion,
  }: {
    actions: {
      available: (version?: string) => void;
      current: () => void;
      fail: () => void;
      pending: () => void;
      notes: (body: string) => void;
      real: () => void;
    };
    version: string;
  } = $props();

  // Seeded once from the prop; the field is then the panel's own.
  // svelte-ignore state_referenced_locally
  let version = $state(initialVersion);

  let update = $derived(updateState());
  let notes = $derived(notesState());

  /** The live status, so the panel doubles as a readout. */
  let summary = $derived(
    update.status === "available" ? `available v${update.version}` : update.status,
  );

  const notesSamples: Array<{ label: string; body: string }> = [
    {
      label: "Full notes",
      body: `## [9.9.9](https://github.com/k1rana/kandel/compare/v9.9.8...v9.9.9) (2026-10-31)

### Bug Fixes

* **ui:** show release notes in a modal ([abc1234](https://github.com/k1rana/kandel/commit/abc1234))
* **updater:** fetch notes from the client and cache them ([def5678](https://github.com/k1rana/kandel/commit/def5678))

### Features

* **tray:** show a notice when a release is available ([cafe012](https://github.com/k1rana/kandel/commit/cafe012))

A paragraph of prose, to check how wrapped copy reads in the modal.`,
    },
    { label: "Empty", body: "" },
    { label: "Long paragraph", body: "x".repeat(400) },
  ];
</script>

<div class="tool">
  <div class="status">
    <span class="label">Status</span>
    <span class="live" data-status={update.status}>{summary}</span>
  </div>

  <label class="field">
    <span>Version</span>
    <input bind:value={version} spellcheck="false" />
  </label>

  <div class="row">
    <button type="button" onclick={() => actions.available(version)}>Available</button>
    <button type="button" onclick={() => actions.current()}>Current</button>
    <button type="button" onclick={() => actions.fail()}>Fail</button>
    <button type="button" onclick={() => actions.pending()}>Pending</button>
  </div>

  <div class="row">
    <button type="button" onclick={() => actions.real()}>Real endpoint</button>
    <button type="button" onclick={() => void checkForUpdate(true)}>Check now</button>
  </div>

  <div class="notes">
    <span class="label">Notes</span>
    {#each notesSamples as sample (sample.label)}
      <button class="sm" type="button" onclick={() => actions.notes(sample.body)}>{sample.label}</button>
    {/each}
  </div>
  <span class="meta">notes: {notes.status}{notes.stale ? " (stale)" : ""}</span>
</div>

<style>
  .tool {
    display: flex;
    flex-direction: column;
    gap: var(--s-8);
  }

  .status {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s-8);
  }
  .label {
    font-size: var(--text-eyebrow);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
  }
  .live {
    font-size: var(--text-caption);
    color: var(--graphite);
  }
  .live[data-status="available"] {
    color: var(--ink);
  }
  .live[data-status="failed"] {
    color: var(--alert);
  }

  .field {
    display: flex;
    align-items: center;
    gap: var(--s-8);
    font-size: var(--text-caption);
    color: var(--graphite);
  }
  .field input {
    flex: 1;
    min-width: 0;
    padding: 4px var(--s-8);
    border: 1px solid var(--ash);
    border-radius: var(--r-input);
    background: var(--well);
    color: var(--ink);
    font-family: var(--font);
    font-size: var(--text-caption);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-8);
  }
  .row button,
  .sm {
    padding: 4px var(--s-8);
    border: 1px solid var(--ash);
    border-radius: var(--r-pill);
    background: transparent;
    color: var(--ink);
    font-family: var(--font);
    font-size: var(--text-caption);
    cursor: pointer;
  }
  .row button:hover,
  .sm:hover {
    background: var(--wash);
    border-color: var(--stone);
  }

  .notes {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s-8);
    border-top: 1px solid var(--ash);
    padding-top: var(--s-8);
  }
  .meta {
    font-size: var(--text-caption);
    color: var(--graphite);
  }
</style>
