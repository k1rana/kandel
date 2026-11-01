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
   * The wrapper is `display: contents`, so it adds no box of its own: the
   * element it wraps keeps the exact place its parent gave it, whether that is
   * a truncated line of text or a button in a row.
   *
   * The bubble is moved into the document body and `position: fixed`, so no
   * ancestor can clip it or become its containing block. The editor panel is
   * transformed, and a fixed child of a transformed element is positioned
   * against that element instead of against the window.
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
    // An empty message has nothing to say, and the trigger still works.
    if (!text) return;
    visible = true;
  }

  function hide() {
    visible = false;
  }

  /** Moves the bubble out of the trigger's ancestors and into the body. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  // The wrapper has no box of its own, so the anchor is the element it wraps.
  function anchorRect(): DOMRect | null {
    const child = triggerEl?.firstElementChild;
    return child ? child.getBoundingClientRect() : null;
  }

  // Measure the bubble against the trigger and the window, then place it.
  $effect(() => {
    // Reading the text ties the measurement to it: a message whose wording
    // changes size is clamped to the window again.
    if (!visible || !triggerEl || !tipEl || !text) {
      placed = false;
      return;
    }
    const anchor = anchorRect();
    if (!anchor) return;
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

  // The pointer events are the bubbling ones: the wrapper has no box, so
  // `mouseenter` would have no area to enter.
  function onPointerOver(e: MouseEvent) {
    const from = e.relatedTarget as Node | null;
    // Moving between the trigger's own children is not a new hover.
    if (from && triggerEl?.contains(from)) return;
    show();
  }

  function onPointerOut(e: MouseEvent) {
    const to = e.relatedTarget as Node | null;
    if (to && triggerEl?.contains(to)) return;
    hide();
  }

  function onFocusOut(e: FocusEvent) {
    const to = e.relatedTarget as Node | null;
    if (to && triggerEl?.contains(to)) return;
    hide();
  }
</script>

<span
  class="tip-trigger"
  data-tip-trigger
  bind:this={triggerEl}
  onmouseover={onPointerOver}
  onmouseout={onPointerOut}
  onfocusin={show}
  onfocusout={onFocusOut}
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
    use:portal
    style="left: {pos.left}px; top: {pos.top}px;"
  >
    {text}
  </div>
{/if}

<style>
  /* No box of its own: the element it wraps keeps the box the parent gave it,
     so wrapping never changes a row, a cell, or a line of text. */
  .tip-trigger {
    display: contents;
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
    /* A list of problems reads as a list, one problem per line. */
    white-space: pre-line;
    /* A path or a command line breaks where it has to, never off the window. */
    overflow-wrap: anywhere;
    pointer-events: none;
    opacity: 0;
  }
  .tip.placed {
    opacity: 1;
    transition: opacity 0.12s var(--ease-out);
  }
</style>
