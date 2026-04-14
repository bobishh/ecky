<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { Snippet } from 'svelte';
  import type { WindowId } from './stores/windowStore';
  import { bringToFront, updateRect, closeWindow, windowRegistry } from './stores/windowStore';

  let {
    windowId,
    x = $bindable(100),
    y = $bindable(100),
    width = $bindable(800),
    height = $bindable(600),
    z = 1000,
    minWidth = 400,
    minHeight = 300,
    title = "",
    hidden = false,
    highlighted = false,
    onclose,
    children
  }: {
    windowId?: WindowId;
    x?: number;
    y?: number;
    width?: number;
    height?: number;
    z?: number;
    minWidth?: number;
    minHeight?: number;
    title?: string;
    hidden?: boolean;
    highlighted?: boolean;
    onclose: () => void;
    children: Snippet;
  } = $props();

  let dragging = $state(false);
  let resizing = $state(false);
  let dragStartOffset = $state({ x: 0, y: 0 });
  let resizeStartDim = $state({ width: 0, height: 0, x: 0, y: 0 });

  function clampToViewport() {
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    x = Math.max(0, Math.min(x, vw - 100));
    y = Math.max(0, Math.min(y, vh - 50));
  }

  function handlePointerDown() {
    if (windowId) {
      bringToFront(windowId);
    }
  }

  function handleDragStart(event: MouseEvent) {
    if (event.target instanceof Element && (event.target.closest('button') || event.target.closest('input') || event.target.closest('select'))) return;
    event.stopPropagation();

    dragging = true;
    dragStartOffset = {
      x: event.clientX - x,
      y: event.clientY - y
    };

    window.addEventListener('mousemove', onGlobalMove);
    window.addEventListener('mouseup', endInteraction);
  }

  function handleResizeStart(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();

    resizing = true;
    resizeStartDim = {
      x: event.clientX,
      y: event.clientY,
      width: width,
      height: height
    };

    window.addEventListener('mousemove', onGlobalMove);
    window.addEventListener('mouseup', endInteraction);
  }

  function onGlobalMove(event: MouseEvent) {
    if (dragging) {
      x = event.clientX - dragStartOffset.x;
      y = event.clientY - dragStartOffset.y;
    } else if (resizing) {
      const dx = event.clientX - resizeStartDim.x;
      const dy = event.clientY - resizeStartDim.y;
      width = Math.max(minWidth, resizeStartDim.width + dx);
      height = Math.max(minHeight, resizeStartDim.height + dy);
    }
  }

  function endInteraction() {
    const wasDragging = dragging;
    const wasResizing = resizing;
    dragging = false;
    resizing = false;
    window.removeEventListener('mousemove', onGlobalMove);
    window.removeEventListener('mouseup', endInteraction);

    if ((wasDragging || wasResizing) && windowId) {
      clampToViewport();
      updateRect(windowId, { x, y, width, height });
    }
  }

  function handleClose() {
    if (windowId) {
      closeWindow(windowId);
    }
    onclose();
  }

  onDestroy(() => {
    if (typeof window !== 'undefined') {
      window.removeEventListener('mousemove', onGlobalMove);
      window.removeEventListener('mouseup', endInteraction);
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="window"
  class:window--hidden={hidden}
  class:window--interacting={dragging || resizing}
  class:window--highlighted={highlighted}
  data-window-id={windowId ?? undefined}
  style="left: {x}px; top: {y}px; width: {width}px; height: {height}px; z-index: {2000 + z};"
  role="dialog"
  aria-hidden={hidden}
  onmousedown={handlePointerDown}
>
  {#if dragging || resizing}
    <div class="window-glass-pane"></div>
  {/if}
  <div class="window-header" role="none" onmousedown={handleDragStart}>
    <span class="window-title">{title}</span>
    <button class="window-close" onclick={handleClose}>&times;</button>
  </div>
  <div class="window-content">
    {@render children()}
  </div>
  <div class="window-resize-handle" role="none" onmousedown={handleResizeStart}></div>
</div>

<style>
  .window {
    position: fixed;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--bg-300) 85%, transparent), 0 8px 32px rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(9px);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .window--hidden {
    opacity: 0;
    visibility: hidden;
    pointer-events: none;
  }

  .window--interacting {
    user-select: none;
  }

  .window--highlighted {
    border-color: var(--primary);
    box-shadow: 0 0 0 2px var(--primary), 0 0 40px rgba(74, 140, 92, 0.5), 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .window-glass-pane {
    position: absolute;
    inset: 0;
    z-index: 9999;
    cursor: inherit;
  }

  .window-header {
    padding: 6px 10px;
    background: var(--bg-200);
    border-bottom: 1px solid color-mix(in srgb, var(--primary) 20%, var(--bg-300));
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    cursor: move;
    user-select: none;
  }

  .window-title {
    font-family: var(--font-mono);
    font-size: 0.65rem;
    font-weight: bold;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .window-close {
    background: none;
    border: none;
    color: var(--text-dim);
    font-size: 1.1rem;
    cursor: pointer;
    line-height: 1;
    padding: 0 4px;
    flex: 0 0 auto;
  }

  .window-close:hover {
    color: var(--text);
  }

  .window-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: color-mix(in srgb, var(--bg) 95%, transparent);
  }

  .window-resize-handle {
    position: absolute;
    right: 2px;
    bottom: 2px;
    width: 18px;
    height: 18px;
    cursor: nwse-resize;
    background: linear-gradient(135deg, transparent 50%, var(--bg-300) 50%);
    z-index: 3;
  }

  .window-resize-handle:hover {
    background: linear-gradient(135deg, transparent 50%, var(--primary) 50%);
  }
</style>
