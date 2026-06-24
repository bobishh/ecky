<script lang="ts">
  import { onMount, onDestroy } from 'svelte';

  type Point = {
    x: number;
    y: number;
  };

  type Tool = 'select' | 'pen' | 'line' | 'arrow' | 'text';

  type Shape =
    | { id: number; type: 'pen'; color: string; lineWidth: number; points: Point[] }
    | { id: number; type: 'line'; color: string; lineWidth: number; start: Point; end: Point }
    | { id: number; type: 'arrow'; color: string; lineWidth: number; start: Point; end: Point }
    | { id: number; type: 'text'; color: string; fontSize: number; pos: Point; text: string };

  type Bounds = { x: number; y: number; w: number; h: number };

  let {
    active = false,
    onDirtyChange,
    onClearAll,
  }: {
    active?: boolean;
    onDirtyChange?: (dirty: boolean) => void;
    onClearAll?: () => void;
  } = $props();

  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let hostEl = $state<HTMLDivElement | null>(null);
  let isDrawing = $state(false);
  let selectedColor = $state('#ff3333');
  let selectedSize = $state(4);
  let selectedTool = $state<Tool>('pen');
  let shapes = $state<Shape[]>([]);
  let currentShape = $state<Shape | null>(null);
  let textInputPos = $state<Point | null>(null);
  let textInputValue = $state('');
  let textInputEl = $state<HTMLInputElement | null>(null);
  let selectedId = $state<number | null>(null);
  let dragAnchor: Point | null = null;
  let dragOriginal: Shape | null = null;
  let _dirty = false;
  let nextId = 1;
  let resizeObserver: ResizeObserver | undefined;

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
  const TOOLS: { value: Tool; label: string }[] = [
    { value: 'select', label: 'Select' },
    { value: 'pen', label: 'Pen' },
    { value: 'line', label: 'Line' },
    { value: 'arrow', label: 'Arrow' },
    { value: 'text', label: 'Text' },
  ];

  export function getCanvas(): HTMLCanvasElement | null { return canvasEl; }
  export function hasDrawing(): boolean { return _dirty; }
  function setDirty(next: boolean) {
    if (_dirty === next) return;
    _dirty = next;
    onDirtyChange?.(next);
  }
  export function clear() {
    shapes = [];
    currentShape = null;
    isDrawing = false;
    selectedId = null;
    dragAnchor = null;
    dragOriginal = null;
    cancelTextInput();
    setDirty(false);
    if (!canvasEl) return;
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
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

  $effect(() => {
    if (!active) cancelTextInput();
  });

  $effect(() => {
    if (textInputPos && textInputEl) {
      textInputEl.focus();
    }
  });

  function syncCanvasSize() {
    if (!canvasEl || !hostEl) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvasEl.width = Math.round(hostEl.clientWidth * dpr);
    canvasEl.height = Math.round(hostEl.clientHeight * dpr);
    redrawAll();
  }

  function getPos(e: PointerEvent): Point {
    if (!canvasEl) return { x: 0, y: 0 };
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

  function handlePointerDown(e: PointerEvent) {
    if (!active || !canvasEl) return;
    const pos = getPos(e);

    if (selectedTool === 'select') {
      const hit = hitTestShapes(pos);
      selectedId = hit?.id ?? null;
      if (hit) {
        canvasEl.setPointerCapture(e.pointerId);
        dragAnchor = pos;
        dragOriginal = cloneShape(hit);
      }
      redrawAll();
      return;
    }

    if (selectedTool === 'text') {
      // WebKit's default pointerdown action re-focuses the click target's
      // nearest focusable ancestor after listeners run, which blurs an
      // input focused synchronously here unless the default is suppressed.
      e.preventDefault();
      commitTextInput();
      textInputPos = pos;
      textInputValue = '';
      return;
    }

    isDrawing = true;
    canvasEl.setPointerCapture(e.pointerId);
    const scaledWidth = selectedSize * dprScale();

    if (selectedTool === 'pen') {
      currentShape = { id: nextId++, type: 'pen', color: selectedColor, lineWidth: scaledWidth, points: [pos] };
    } else {
      currentShape = { id: nextId++, type: selectedTool, color: selectedColor, lineWidth: scaledWidth, start: pos, end: pos };
    }
  }

  function handlePointerMove(e: PointerEvent) {
    if (!canvasEl) return;

    if (selectedTool === 'select' && dragAnchor && dragOriginal && selectedId != null) {
      const pos = getPos(e);
      const dx = pos.x - dragAnchor.x;
      const dy = pos.y - dragAnchor.y;
      const id = selectedId;
      shapes = shapes.map((s) => (s.id === id ? translateShape(dragOriginal!, dx, dy) : s));
      redrawAll();
      return;
    }

    if (!isDrawing || !currentShape) return;
    const pos = getPos(e);

    if (currentShape.type === 'pen') {
      currentShape.points.push(pos);
    } else if (currentShape.type === 'line' || currentShape.type === 'arrow') {
      currentShape.end = pos;
    }
    redrawAll();
    drawShape(currentShape);
  }

  function handlePointerUp() {
    if (isDrawing && currentShape) {
      shapes = [...shapes, currentShape];
      currentShape = null;
      setDirty(true);
    }
    isDrawing = false;
    dragAnchor = null;
    dragOriginal = null;
  }

  function cloneShape(shape: Shape): Shape {
    return JSON.parse(JSON.stringify(shape));
  }

  function translateShape(shape: Shape, dx: number, dy: number): Shape {
    if (shape.type === 'pen') {
      return { ...shape, points: shape.points.map((p) => ({ x: p.x + dx, y: p.y + dy })) };
    }
    if (shape.type === 'text') {
      return { ...shape, pos: { x: shape.pos.x + dx, y: shape.pos.y + dy } };
    }
    return {
      ...shape,
      start: { x: shape.start.x + dx, y: shape.start.y + dy },
      end: { x: shape.end.x + dx, y: shape.end.y + dy },
    };
  }

  function getShapeBounds(shape: Shape): Bounds {
    if (shape.type === 'pen') {
      const xs = shape.points.map((p) => p.x);
      const ys = shape.points.map((p) => p.y);
      const pad = shape.lineWidth / 2 + 4;
      const minX = Math.min(...xs) - pad;
      const minY = Math.min(...ys) - pad;
      return { x: minX, y: minY, w: Math.max(...xs) - minX + pad, h: Math.max(...ys) - minY + pad };
    }
    if (shape.type === 'text') {
      const ctx = canvasEl?.getContext('2d');
      const width = ctx ? measureTextWidth(ctx, shape) : shape.text.length * shape.fontSize * 0.6;
      return { x: shape.pos.x - 4, y: shape.pos.y - 4, w: width + 8, h: shape.fontSize + 8 };
    }
    const pad = shape.lineWidth / 2 + 6;
    const minX = Math.min(shape.start.x, shape.end.x) - pad;
    const minY = Math.min(shape.start.y, shape.end.y) - pad;
    const maxX = Math.max(shape.start.x, shape.end.x) + pad;
    const maxY = Math.max(shape.start.y, shape.end.y) + pad;
    return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
  }

  function measureTextWidth(ctx: CanvasRenderingContext2D, shape: Extract<Shape, { type: 'text' }>): number {
    ctx.font = `${shape.fontSize}px sans-serif`;
    return ctx.measureText(shape.text).width;
  }

  function distanceToSegment(p: Point, a: Point, b: Point): number {
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const lengthSq = dx * dx + dy * dy;
    if (lengthSq === 0) return Math.hypot(p.x - a.x, p.y - a.y);
    let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / lengthSq;
    t = Math.max(0, Math.min(1, t));
    const projX = a.x + t * dx;
    const projY = a.y + t * dy;
    return Math.hypot(p.x - projX, p.y - projY);
  }

  function hitTestShapes(pos: Point): Shape | null {
    for (let i = shapes.length - 1; i >= 0; i--) {
      const shape = shapes[i];
      if (shape.type === 'line' || shape.type === 'arrow') {
        if (distanceToSegment(pos, shape.start, shape.end) <= Math.max(10, shape.lineWidth)) return shape;
        continue;
      }
      if (shape.type === 'pen') {
        for (let j = 1; j < shape.points.length; j++) {
          if (distanceToSegment(pos, shape.points[j - 1], shape.points[j]) <= Math.max(10, shape.lineWidth)) return shape;
        }
        continue;
      }
      const b = getShapeBounds(shape);
      if (pos.x >= b.x && pos.x <= b.x + b.w && pos.y >= b.y && pos.y <= b.y + b.h) return shape;
    }
    return null;
  }

  function deleteSelected() {
    if (selectedId == null) return;
    shapes = shapes.filter((s) => s.id !== selectedId);
    selectedId = null;
    redrawAll();
  }

  function commitTextInput() {
    if (textInputPos && textInputValue.trim()) {
      shapes = [
        ...shapes,
        { id: nextId++, type: 'text', color: selectedColor, fontSize: textFontSize(selectedSize), pos: textInputPos, text: textInputValue.trim() },
      ];
      setDirty(true);
      redrawAll();
    }
    textInputPos = null;
    textInputValue = '';
  }

  function cancelTextInput() {
    textInputPos = null;
    textInputValue = '';
  }

  function undo() {
    if (shapes.length === 0) return;
    shapes = shapes.slice(0, -1);
    redrawAll();
  }

  function drawShape(shape: Shape) {
    if (!canvasEl) return;
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;
    ctx.strokeStyle = shape.color;
    ctx.fillStyle = shape.color;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    if (shape.type === 'pen') {
      if (shape.points.length < 2) return;
      ctx.lineWidth = shape.lineWidth;
      ctx.beginPath();
      ctx.moveTo(shape.points[0].x, shape.points[0].y);
      for (let i = 1; i < shape.points.length; i++) {
        ctx.lineTo(shape.points[i].x, shape.points[i].y);
      }
      ctx.stroke();
    } else if (shape.type === 'line') {
      ctx.lineWidth = shape.lineWidth;
      ctx.beginPath();
      ctx.moveTo(shape.start.x, shape.start.y);
      ctx.lineTo(shape.end.x, shape.end.y);
      ctx.stroke();
    } else if (shape.type === 'arrow') {
      ctx.lineWidth = shape.lineWidth;
      drawArrow(ctx, shape.start, shape.end, shape.lineWidth);
    } else if (shape.type === 'text') {
      ctx.font = `${shape.fontSize}px sans-serif`;
      ctx.textBaseline = 'top';
      ctx.fillText(shape.text, shape.pos.x, shape.pos.y);
    }
  }

  function drawArrow(ctx: CanvasRenderingContext2D, start: Point, end: Point, lineWidth: number) {
    const headLength = Math.max(10, lineWidth * 3);
    const angle = Math.atan2(end.y - start.y, end.x - start.x);
    ctx.beginPath();
    ctx.moveTo(start.x, start.y);
    ctx.lineTo(end.x, end.y);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(end.x, end.y);
    ctx.lineTo(
      end.x - headLength * Math.cos(angle - Math.PI / 6),
      end.y - headLength * Math.sin(angle - Math.PI / 6),
    );
    ctx.lineTo(
      end.x - headLength * Math.cos(angle + Math.PI / 6),
      end.y - headLength * Math.sin(angle + Math.PI / 6),
    );
    ctx.closePath();
    ctx.fill();
  }

  function redrawAll() {
    if (!canvasEl) return;
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
    for (const shape of shapes) {
      drawShape(shape);
    }
    if (selectedId != null) {
      const selected = shapes.find((s) => s.id === selectedId);
      if (selected) drawSelectionOutline(ctx, getShapeBounds(selected));
    }
    setDirty(shapes.length > 0);
  }

  function drawSelectionOutline(ctx: CanvasRenderingContext2D, b: Bounds) {
    ctx.save();
    ctx.strokeStyle = '#39d0ff';
    ctx.lineWidth = 1.5;
    ctx.setLineDash([5, 4]);
    ctx.strokeRect(b.x, b.y, b.w, b.h);
    ctx.restore();
  }

  function textFontSize(sizeValue: number): number {
    return 20 + sizeValue * 4;
  }

  function textInputStyle(): string {
    if (!textInputPos || !canvasEl) return 'display: none';
    const scale = 1 / dprScale();
    const fontPx = textFontSize(selectedSize) * scale;
    return `left: ${textInputPos.x * scale}px; top: ${textInputPos.y * scale}px; color: ${selectedColor}; font-size: ${fontPx}px;`;
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
    class:active-cursor={active && selectedTool !== 'text' && selectedTool !== 'select'}
    class:text-cursor={active && selectedTool === 'text'}
    class:select-cursor={active && selectedTool === 'select'}
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={handlePointerUp}
    onpointerleave={handlePointerUp}
  ></canvas>

  {#if textInputPos}
    <input
      bind:this={textInputEl}
      class="drawing-text-input"
      style={textInputStyle()}
      bind:value={textInputValue}
      onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); commitTextInput(); } if (e.key === 'Escape') cancelTextInput(); }}
      onblur={commitTextInput}
      placeholder="Annotation…"
    />
  {/if}

  {#if active}
    <div class="draw-toolbar">
      <div class="toolbar-group">
        {#each TOOLS as t}
          <button
            class="tool-select-btn"
            class:selected={selectedTool === t.value}
            onclick={() => { selectedTool = t.value; if (t.value !== 'select') { selectedId = null; redrawAll(); } }}
            title={t.label}
          >{t.label}</button>
        {/each}
      </div>
      <div class="toolbar-sep"></div>
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
      <button class="tool-btn" onclick={undo} title="Undo" disabled={shapes.length === 0}>↩</button>
      <button class="tool-btn" onclick={deleteSelected} title="Delete Selected" disabled={selectedId == null}>🗑</button>
      <button class="tool-btn" onclick={() => { clear(); onClearAll?.(); }} title="Clear All &amp; Exit">✕</button>
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
  .drawing-canvas.text-cursor {
    cursor: text;
  }
  .drawing-canvas.select-cursor {
    cursor: pointer;
  }

  .drawing-text-input {
    position: absolute;
    z-index: 36;
    background: rgba(10, 14, 24, 0.85);
    border: 1px solid var(--bg-300);
    font-size: 13px;
    padding: 2px 4px;
    min-width: 120px;
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

  .tool-select-btn {
    background: transparent;
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.65rem;
    padding: 3px 6px;
    cursor: pointer;
  }
  .tool-select-btn.selected {
    border-color: var(--secondary);
    background: var(--bg-300);
  }
  .tool-select-btn:hover {
    border-color: var(--text);
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
