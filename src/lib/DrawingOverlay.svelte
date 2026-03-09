<script>
  import { onMount, onDestroy } from 'svelte';

  let { active = false } = $props();

  let canvasEl = $state(null);
  let hostEl = $state(null);
  let isDrawing = $state(false);
  let selectedColor = $state('#ff3333');
  let selectedSize = $state(4);
  let strokes = $state([]);
  let currentStroke = null;
  let _dirty = false;
  let resizeObserver;

  const COLORS = [
    { value: '#ff3333', label: 'Red' },
    { value: '#33ff33', label: 'Green' },
    { value: '#3399ff', label: 'Blue' },
    { value: '#ffdd33', label: 'Yellow' },
    { value: '#ffffff', label: 'White' },
  ];
  const SIZES = [
    { value: 2, label: 'S' },
    { value: 5, label: 'M' },
    { value: 10, label: 'L' },
  ];

  export function getCanvas() { return canvasEl; }
  export function hasDrawing() { return _dirty; }
  export function clear() {
    if (!canvasEl) return;
    const ctx = canvasEl.getContext('2d');
    ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
    strokes = [];
    _dirty = false;
  }

  onMount(() => {
    if (!hostEl) return;
    syncCanvasSize();
    resizeObserver = new ResizeObserver(syncCanvasSize);
    resizeObserver.observe(hostEl);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
  });

  function syncCanvasSize() {
    if (!canvasEl || !hostEl) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvasEl.width = Math.round(hostEl.clientWidth * dpr);
    canvasEl.height = Math.round(hostEl.clientHeight * dpr);
    redrawStrokes();
  }

  function getPos(e) {
    const rect = canvasEl.getBoundingClientRect();
    const scaleX = canvasEl.width / rect.width;
    const scaleY = canvasEl.height / rect.height;
    return {
      x: (e.clientX - rect.left) * scaleX,
      y: (e.clientY - rect.top) * scaleY,
    };
  }

  function dprScale() {
    if (!canvasEl) return 1;
    return canvasEl.width / canvasEl.getBoundingClientRect().width;
  }

  function handlePointerDown(e) {
    if (!active) return;
    isDrawing = true;
    canvasEl.setPointerCapture(e.pointerId);
    const pos = getPos(e);
    const scaledWidth = selectedSize * dprScale();
    currentStroke = { color: selectedColor, lineWidth: scaledWidth, points: [pos] };
    const ctx = canvasEl.getContext('2d');
    ctx.strokeStyle = currentStroke.color;
    ctx.lineWidth = currentStroke.lineWidth;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.beginPath();
    ctx.moveTo(pos.x, pos.y);
  }

  function handlePointerMove(e) {
    if (!isDrawing || !currentStroke) return;
    const pos = getPos(e);
    currentStroke.points.push(pos);
    const ctx = canvasEl.getContext('2d');
    ctx.strokeStyle = currentStroke.color;
    ctx.lineWidth = currentStroke.lineWidth;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.lineTo(pos.x, pos.y);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(pos.x, pos.y);
    _dirty = true;
  }

  function handlePointerUp() {
    if (isDrawing && currentStroke) {
      strokes = [...strokes, currentStroke];
      currentStroke = null;
    }
    isDrawing = false;
  }

  function undo() {
    if (strokes.length === 0) return;
    strokes = strokes.slice(0, -1);
    redrawStrokes();
  }

  function redrawStrokes() {
    if (!canvasEl) return;
    const ctx = canvasEl.getContext('2d');
    ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
    for (const stroke of strokes) {
      if (stroke.points.length < 2) continue;
      ctx.strokeStyle = stroke.color;
      ctx.lineWidth = stroke.lineWidth;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      ctx.beginPath();
      ctx.moveTo(stroke.points[0].x, stroke.points[0].y);
      for (let i = 1; i < stroke.points.length; i++) {
        ctx.lineTo(stroke.points[i].x, stroke.points[i].y);
      }
      ctx.stroke();
    }
    _dirty = strokes.length > 0;
  }
</script>

<div
  bind:this={hostEl}
  class="drawing-host"
  class:drawing-active={active}
  style="pointer-events: {active ? 'auto' : 'none'}"
>
  <canvas
    bind:this={canvasEl}
    class="drawing-canvas"
    class:active-cursor={active}
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={handlePointerUp}
    onpointerleave={handlePointerUp}
  ></canvas>

  {#if active}
    <div class="draw-toolbar">
      <div class="toolbar-group">
        {#each COLORS as c}
          <button
            class="color-swatch"
            class:selected={selectedColor === c.value}
            style="background: {c.value}"
            onclick={() => selectedColor = c.value}
            title={c.label}
          ></button>
        {/each}
      </div>
      <div class="toolbar-sep"></div>
      <div class="toolbar-group">
        {#each SIZES as s}
          <button
            class="size-btn"
            class:selected={selectedSize === s.value}
            onclick={() => selectedSize = s.value}
            title={s.label}
          >
            <span class="size-dot" style="width: {s.value + 2}px; height: {s.value + 2}px"></span>
          </button>
        {/each}
      </div>
      <div class="toolbar-sep"></div>
      <button class="tool-btn" onclick={undo} title="Undo" disabled={strokes.length === 0}>↩</button>
      <button class="tool-btn" onclick={clear} title="Clear All">✕</button>
    </div>
  {/if}
</div>

<style>
  .drawing-host {
    position: absolute;
    inset: 0;
    z-index: 30;
    overflow: hidden;
  }
  .drawing-host.drawing-active {
    border: 2px solid var(--primary);
    border-style: dashed;
  }
  .drawing-canvas {
    width: 100%;
    height: 100%;
    display: block;
  }
  .drawing-canvas.active-cursor {
    cursor: crosshair;
  }

  .draw-toolbar {
    position: absolute;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 35;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    background: rgba(10, 14, 24, 0.92);
    border: 1px solid var(--bg-300);
    backdrop-filter: blur(8px);
    pointer-events: auto;
  }
  .toolbar-group {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .toolbar-sep {
    width: 1px;
    height: 18px;
    background: var(--bg-300);
  }

  .color-swatch {
    width: 18px;
    height: 18px;
    border: 2px solid var(--bg-300);
    cursor: pointer;
    padding: 0;
  }
  .color-swatch.selected {
    border-color: var(--secondary);
    box-shadow: 0 0 6px var(--secondary);
  }
  .color-swatch:hover {
    border-color: var(--text);
  }

  .size-btn {
    width: 24px;
    height: 24px;
    border: 1px solid var(--bg-300);
    background: transparent;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
  }
  .size-btn.selected {
    border-color: var(--secondary);
    background: var(--bg-300);
  }
  .size-btn:hover {
    border-color: var(--text);
  }
  .size-dot {
    display: block;
    background: var(--text);
    border-radius: 50%;
  }

  .tool-btn {
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    font-size: 0.7rem;
    padding: 3px 7px;
    cursor: pointer;
    font-weight: bold;
  }
  .tool-btn:hover {
    border-color: var(--primary);
    color: var(--primary);
  }
  .tool-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }
</style>
