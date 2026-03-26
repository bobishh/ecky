<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { Snippet } from 'svelte';

  let {
    x = $bindable(100),
    y = $bindable(100),
    width = $bindable(800),
    height = $bindable(600),
    minWidth = 400,
    minHeight = 300,
    title = "",
    hidden = false,
    onclose,
    children
  }: {
    x?: number;
    y?: number;
    width?: number;
    height?: number;
    minWidth?: number;
    minHeight?: number;
    title?: string;
    hidden?: boolean;
    onclose: () => void;
    children: Snippet;
  } = $props();

  let dragging = $state(false);
  let resizing = $state(false);
  let dragStartOffset = $state({ x: 0, y: 0 });
  let resizeStartDim = $state({ width: 0, height: 0, x: 0, y: 0 });

  function handleDragStart(event: MouseEvent) {
    if (event.target instanceof Element && event.target.closest('button')) return;
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
    dragging = false;
    resizing = false;
    window.removeEventListener('mousemove', onGlobalMove);
    window.removeEventListener('mouseup', endInteraction);
  }

  onDestroy(() => {
    if (typeof window !== 'undefined') {
      window.removeEventListener('mousemove', onGlobalMove);
      window.removeEventListener('mouseup', endInteraction);
    }
  });
</script>

<div
  class="window"
  class:window--hidden={hidden}
  style="left: {x}px; top: {y}px; width: {width}px; height: {height}px;"
  role="dialog"
  aria-hidden={hidden}
>
  <div class="window-header" role="none" onmousedown={handleDragStart}>
    <span class="window-title">{title}</span>
    <button class="window-close" onclick={onclose}>&times;</button>
  </div>
  <div class="window-content">
    {@render children()}
  </div>
  <div class="window-resize-handle" role="none" onmousedown={handleResizeStart}></div>
</div>

<style>
  .window {
    position: fixed;
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    z-index: 1000;
  }

  .window--hidden {
    opacity: 0;
    visibility: hidden;
    pointer-events: none;
  }

  .window-header {
    padding: 6px 10px;
    background: var(--bg-200);
    border-bottom: 1px solid var(--bg-300);
    display: flex;
    align-items: center;
    justify-content: space-between;
    cursor: move;
    user-select: none;
  }

  .window-title {
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
  }

  .window-close:hover {
    color: var(--text);
  }

  .window-content {
    flex: 1;
    overflow: hidden;
    background: var(--bg);
  }

  .window-resize-handle {
    position: absolute;
    right: 0;
    bottom: 0;
    width: 12px;
    height: 12px;
    cursor: nwse-resize;
    background: linear-gradient(135deg, transparent 50%, var(--bg-300) 50%);
  }

  .window-resize-handle:hover {
    background: linear-gradient(135deg, transparent 50%, var(--primary) 50%);
  }
</style>
