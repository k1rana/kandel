<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * A tooltip in the app's own visual language.
   *
   * The native `title` attribute is not used because its bubble is drawn by the
   * OS/webview: it never appears on keyboard focus, it can take a second or more
   * to show, and it is suppressed entirely on a disabled control. This shows on
   * both hover and focus, immediately, and works on a control that only looks
   * disabled (`aria-disabled`).
   *
   * The bubble is `position: fixed` so it is never clipped by the popover or
   * modal it is anchored inside, and `pointer-events: none` so it cannot steal
   * the hover that keeps it open.
   */
  let {
    text,
    placement = "bottom",
    children,
  }: {
    text: string;
    /** Preferred side; flips to fit when it would overflow the window. */
    placement?: "top" | "bottom";
    children: Snippet;
  } = $props();

  let visible = $state(false);
  let triggerEl = $state<HTMLSpanElement | null>(null);
  let tipEl = $state<HTMLDivElement | null>(null);
  // Hidden until measured, so it never flashes at 0,0 on first show.
  let placed = $state(false);
  let pos = $state({ left: 0, top: 0 });

  function show() {
    visible = true;
  }

  function hide() {
    visible = false;
  }

  // Measure the bubble against the trigger and the window, then place it. Runs
  // reactively so a text change that resizes the bubble re-clamps it.
  $effect(() => {
    if (!visible || !triggerEl || !tipEl) {
      placed = false;
      return;
    }
    const anchor = triggerEl.getBoundingClientRect();
    const tip = tipEl.getBoundingClientRect();
    const pad = 8;

    let left = anchor.left + anchor.width / 2 - tip.width / 2;
    left = Math.max(pad, Math.min(left, window.innerWidth - pad - tip.width));

    let top = placement === "top" ? anchor.top - pad - tip.height : anchor.bottom + pad;
    // Flip to the other side when the preferred one would leave the window.
    if (top < pad) top = anchor.bottom + pad;
    if (top + tip.height > window.innerHeight - pad) top = anchor.top - pad - tip.height;
    top = Math.max(pad, top);

    pos = { left, top };
    placed = true;
  });

  // Escape dismisses it, matching the app's other dismissible surfaces.
  $effect(() => {
    if (!visible) return;
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") hide();
    };
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });
</script>

<span
  class="tip-trigger"
  bind:this={triggerEl}
  onmouseenter={show}
  onmouseleave={hide}
  onfocusin={show}
  onfocusout={hide}
  role="presentation"
>
  {@render children()}
</span>

{#if visible}
  <div
    class="tip"
    class:placed
    role="tooltip"
    bind:this={tipEl}
    style="left: {pos.left}px; top: {pos.top}px;"
  >
    {text}
  </div>
{/if}

<style>
  /* A flex item sized to its trigger, so wrapping a button does not change the
     row's layout. */
  .tip-trigger {
    display: inline-flex;
    flex: none;
  }

  /* Matches the pointer tooltip in App.svelte: canvas ground, ink hairline,
     input radius, no shadow (the world separates by tone, never by shadow). */
  .tip {
    position: fixed;
    z-index: 95;
    max-width: 42ch;
    padding: var(--s-8) var(--s-12);
    background: var(--canvas);
    border: 1px solid var(--ink);
    border-radius: var(--r-input);
    color: var(--ink);
    font-size: var(--text-caption);
    line-height: 1.4;
    pointer-events: none;
    opacity: 0;
  }
  .tip.placed {
    opacity: 1;
    transition: opacity 0.12s var(--ease-out);
  }
</style>
