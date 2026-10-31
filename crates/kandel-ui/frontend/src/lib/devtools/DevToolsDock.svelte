<script lang="ts">
  import type { DevTool } from "./registry";

  let { tools }: { tools: DevTool[] } = $props();

  let open = $state(false);
  // Seeded once; the tab set is fixed for the dock's lifetime.
  // svelte-ignore state_referenced_locally
  let activeId = $state(tools[0]?.id ?? "");
  let active = $derived(tools.find((tool) => tool.id === activeId) ?? tools[0]);
</script>

<div class="dock">
  {#if open}
    <div class="panel" role="group" aria-label="Devtools">
      <div class="head">
        <span class="title">Devtools</span>
        <button class="x" type="button" aria-label="Close devtools" onclick={() => (open = false)}>×</button>
      </div>

      {#if tools.length > 1}
        <div class="tabs" role="tablist">
          {#each tools as tool (tool.id)}
            <button
              type="button"
              role="tab"
              aria-selected={tool.id === active?.id}
              onclick={() => (activeId = tool.id)}
            >
              {tool.label}
            </button>
          {/each}
        </div>
      {/if}

      {#if active}
        {@const Tool = active.component}
        <Tool {...active.props} />
      {/if}
    </div>
  {:else}
    <button class="chip" type="button" onclick={() => (open = true)}>devtools</button>
  {/if}
</div>

<style>
  .dock {
    position: fixed;
    right: var(--s-16);
    bottom: var(--s-16);
    z-index: 200;
    font-family: var(--font);
  }

  .chip,
  .panel {
    background: var(--canvas);
    border: 1px solid var(--ink);
    border-radius: var(--r-inner);
  }
  .chip {
    padding: var(--s-8) var(--s-12);
    font-family: var(--font);
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--ink);
    cursor: pointer;
  }
  .chip:hover {
    background: var(--wash);
  }

  .panel {
    width: 272px;
    padding: var(--s-12);
    display: flex;
    flex-direction: column;
    gap: var(--s-12);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--s-8);
  }
  .title {
    flex: 1;
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
  }
  .x {
    border: none;
    background: none;
    padding: 0 4px;
    font-size: var(--text-body);
    line-height: 1;
    color: var(--graphite);
    cursor: pointer;
  }
  .x:hover {
    color: var(--ink);
  }

  .tabs {
    display: flex;
    gap: var(--s-8);
  }
  .tabs button {
    padding: 4px var(--s-8);
    border: 1px solid transparent;
    border-radius: var(--r-pill);
    background: transparent;
    color: var(--graphite);
    font-family: var(--font);
    font-size: var(--text-caption);
    cursor: pointer;
  }
  .tabs button[aria-selected="true"] {
    border-color: var(--ash);
    color: var(--ink);
  }
</style>
