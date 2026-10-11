<script lang="ts">
  import { startResizeDragging, type ResizeDirection } from "./window";

  // Eight invisible hit areas around the window edge. On Windows, Tauri handles
  // the actual resize natively once a drag starts, so these only need to
  // initiate it with the correct direction.
  const edges: { dir: ResizeDirection; cls: string }[] = [
    { dir: "North", cls: "n" },
    { dir: "South", cls: "s" },
    { dir: "West", cls: "w" },
    { dir: "East", cls: "e" },
    { dir: "NorthWest", cls: "nw" },
    { dir: "NorthEast", cls: "ne" },
    { dir: "SouthWest", cls: "sw" },
    { dir: "SouthEast", cls: "se" },
  ];

  function onDown(e: MouseEvent, dir: ResizeDirection) {
    if (e.buttons !== 1) return;
    e.preventDefault();
    // The native drag only starts if the `core:window:allow-start-resize-dragging`
    // capability is granted; without it the invoke rejects and the corner looks
    // dead. Surface that instead of letting the rejection go unhandled.
    startResizeDragging(dir).catch((err) => {
      console.error(`Could not start a ${dir} resize drag:`, err);
    });
  }
</script>

{#each edges as edge (edge.dir)}
  <div
    class="rz {edge.cls}"
    onmousedown={(e) => onDown(e, edge.dir)}
    role="presentation"
    aria-hidden="true"
  ></div>
{/each}

<style>
  .rz {
    position: fixed;
    z-index: 100;
  }

  /* Edges */
  .n {
    top: 0;
    left: 8px;
    right: 8px;
    height: 5px;
    cursor: ns-resize;
  }
  .s {
    bottom: 0;
    left: 8px;
    right: 8px;
    height: 5px;
    cursor: ns-resize;
  }
  .w {
    top: 8px;
    bottom: 8px;
    left: 0;
    width: 5px;
    cursor: ew-resize;
  }
  .e {
    top: 8px;
    bottom: 8px;
    right: 0;
    width: 5px;
    cursor: ew-resize;
  }

  /* Corners */
  .nw {
    top: 0;
    left: 0;
    width: 12px;
    height: 12px;
    cursor: nwse-resize;
  }
  .ne {
    top: 0;
    right: 0;
    width: 12px;
    height: 12px;
    cursor: nesw-resize;
  }
  .sw {
    bottom: 0;
    left: 0;
    width: 12px;
    height: 12px;
    cursor: nesw-resize;
  }
  .se {
    bottom: 0;
    right: 0;
    width: 12px;
    height: 12px;
    cursor: nwse-resize;
  }
</style>
