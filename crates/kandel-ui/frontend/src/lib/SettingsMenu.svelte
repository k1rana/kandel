<script lang="ts">
  import { THEME_OPTIONS, themeLabel, type ThemeSetting } from "./theme";

  let {
    setting,
    prefersDark,
    onSelect,
  }: {
    setting: ThemeSetting;
    /** The OS preference, so `Auto` can name the theme it follows. */
    prefersDark: boolean;
    onSelect: (next: ThemeSetting) => void;
  } = $props();

  let open = $state(false);
  let menuEl = $state<HTMLDivElement | undefined>();
  let triggerEl = $state<HTMLButtonElement | undefined>();

  function close(restoreFocus = true) {
    if (!open) return;
    open = false;
    if (restoreFocus) triggerEl?.focus();
  }

  function choose(value: ThemeSetting) {
    onSelect(value);
    close();
  }

  // Land focus on the active option when the menu opens, so the keyboard starts
  // where the current setting is.
  $effect(() => {
    if (!open || !menuEl) return;
    const active = menuEl.querySelector<HTMLElement>('[aria-checked="true"]');
    const first = menuEl.querySelector<HTMLElement>('[role="menuitemradio"]');
    (active ?? first)?.focus();
  });

  // Escape closes from anywhere, matching the About popover.
  $effect(() => {
    if (!open) return;
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  function onMenuKeydown(e: KeyboardEvent) {
    if (!menuEl) return;
    const items = Array.from(menuEl.querySelectorAll<HTMLElement>('[role="menuitemradio"]'));
    if (items.length === 0) return;

    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const dir = e.key === "ArrowDown" ? 1 : -1;
      const current = items.indexOf(document.activeElement as HTMLElement);
      const base = current === -1 ? (dir === 1 ? -1 : 0) : current;
      items[(base + dir + items.length) % items.length]?.focus();
    } else if (e.key === "Home") {
      e.preventDefault();
      items[0]?.focus();
    } else if (e.key === "End") {
      e.preventDefault();
      items[items.length - 1]?.focus();
    } else if (e.key === "Tab") {
      // Tabbing away should dismiss a topbar popover rather than leave it
      // floating behind the focus ring.
      close(false);
    }
  }
</script>

<div class="wrap">
  <button
    class="trigger"
    type="button"
    bind:this={triggerEl}
    title="Settings"
    aria-label="Settings"
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <!-- An authored 24-grid glyph drawn to read at 15px, like the other control
         icons; it carries the "settings" idiom the gear is known for. -->
    <svg
      viewBox="0 0 24 24"
      width="15"
      height="15"
      fill="none"
      stroke="currentColor"
      stroke-width="1.7"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3" />
      <path
        d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
      />
    </svg>
  </button>

  {#if open}
    <div
      class="scrim"
      onclick={() => close(false)}
      oncontextmenu={(e) => e.preventDefault()}
      role="presentation"
    ></div>

    <div
      class="menu"
      role="menu"
      aria-label="Settings"
      tabindex="-1"
      bind:this={menuEl}
      onkeydown={onMenuKeydown}
    >
      <div class="menu-head">Settings</div>
      <div class="menu-group">Theme</div>
      {#each THEME_OPTIONS as option (option.value)}
        <button
          class="menu-item"
          type="button"
          role="menuitemradio"
          aria-checked={setting === option.value}
          onclick={() => choose(option.value)}
        >
          <span>{themeLabel(option.value, prefersDark)}</span>
          <span class="check" aria-hidden="true">
            {#if setting === option.value}
              <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
                <path
                  d="M2 6.5l2.6 2.6L10 3.5"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.4"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            {/if}
          </span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .trigger {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    flex: none;
    padding: 0;
    border: none;
    border-radius: var(--r-input);
    background: transparent;
    color: var(--graphite);
    cursor: pointer;
    transition:
      background 0.15s var(--ease-out),
      color 0.15s var(--ease-out);
  }
  .trigger:hover,
  .trigger[aria-expanded="true"] {
    background: var(--wash);
    color: var(--ink);
  }

  .scrim {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: transparent;
  }

  /* Anchored under the gear, in the same visual language as the About popover
     and the row menu: a white card with a hairline ink border and soft radii,
     no shadow. */
  .menu {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 90;
    min-width: 200px;
    padding: var(--s-4);
    background: var(--canvas);
    border: 1px solid var(--ink);
    border-radius: var(--r-inner);
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

  .menu-head {
    padding: var(--s-8) var(--s-12);
    font-size: var(--text-caption);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
    border-bottom: 1px solid var(--ash);
    margin-bottom: var(--s-4);
  }
  .menu-group {
    padding: var(--s-8) var(--s-12) var(--s-4);
    font-size: var(--text-eyebrow);
    font-weight: var(--w-medium);
    text-transform: uppercase;
    letter-spacing: var(--tracking-micaps);
    color: var(--graphite);
  }

  .menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-16);
    width: 100%;
    padding: var(--s-8) var(--s-12);
    border: none;
    border-radius: var(--r-input);
    background: transparent;
    color: var(--ink);
    font-family: inherit;
    font-size: var(--text-body-sm);
    font-weight: var(--w-regular);
    text-align: left;
    cursor: pointer;
  }
  .menu-item:hover {
    background: var(--lime);
    color: var(--lime-ink);
  }
  .menu-item[aria-checked="true"] {
    font-weight: var(--w-medium);
  }
  .check {
    display: grid;
    place-items: center;
    width: 12px;
    height: 12px;
    flex: none;
  }
</style>
