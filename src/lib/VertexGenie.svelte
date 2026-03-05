<script>
  import { onDestroy, onMount } from 'svelte';

  let { mode = 'idle', bubble = '', label = 'ECKBERT', onDismiss = null } = $props();

  let canvas;
  let frameId = 0;
  let dpr = 1;
  let ctx = null;

  const SIZE = 150;
  const MAX_BUBBLE_LEN = 220;

  const cleanBubble = $derived.by(() => {
    const text = `${bubble ?? ''}`.replace(/\s+/g, ' ').trim();
    if (!text) return '';
    return text.length > MAX_BUBBLE_LEN ? `${text.slice(0, MAX_BUBBLE_LEN - 1)}…` : text;
  });

  function resizeCanvas() {
    if (!canvas) return;
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(SIZE * dpr);
    canvas.height = Math.round(SIZE * dpr);
    canvas.style.width = `${SIZE}px`;
    canvas.style.height = `${SIZE}px`;
    ctx = canvas.getContext('2d');
  }

  function pickPalette() {
    const css = getComputedStyle(document.documentElement);
    const primary = css.getPropertyValue('--primary').trim() || '#4a8c5c';
    const secondary = css.getPropertyValue('--secondary').trim() || '#c8a620';
    const red = css.getPropertyValue('--red').trim() || '#ff6b6b';

    if (mode === 'error') {
      return {
        edge: red,
        node: '#ffd1d1',
        glow: 'rgba(255, 107, 107, 0.42)'
      };
    }

    if (mode === 'thinking') {
      return {
        edge: secondary,
        node: '#f6eabb',
        glow: 'rgba(200, 166, 32, 0.36)'
      };
    }

    return {
      edge: primary,
      node: '#d6eddc',
      glow: 'rgba(74, 140, 92, 0.34)'
    };
  }

  function draw(timestamp) {
    if (!ctx) return;

    const time = timestamp * 0.001;
    const profile = {
      jitter: mode === 'thinking' ? 4.4 : mode === 'error' ? 6.0 : mode === 'speaking' ? 2.4 : 1.6,
      pulse: mode === 'speaking' ? 0.14 : mode === 'thinking' ? 0.07 : 0.04,
      hover: mode === 'error' ? 1.0 : 2.8
    };

    const centerX = SIZE * 0.48;
    const centerY = SIZE * 0.58 + Math.sin(time * 2.2) * profile.hover;
    const radius = 32 * (1 + Math.sin(time * 5.3) * profile.pulse);
    const palette = pickPalette();
    const points = [];
    const vertexCount = 14;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, SIZE, SIZE);

    ctx.save();
    ctx.beginPath();
    ctx.arc(centerX, centerY, radius + 24, 0, Math.PI * 2);
    ctx.fillStyle = palette.glow;
    ctx.fill();
    ctx.restore();

    for (let i = 0; i < vertexCount; i++) {
      const angle = (i / vertexCount) * Math.PI * 2;
      const drift = Math.sin(time * 2.5 + i * 0.85) * profile.jitter;
      const x = centerX + Math.cos(angle) * (radius + drift);
      const y = centerY + Math.sin(angle) * (radius + drift);
      points.push({ x, y });
    }

    ctx.strokeStyle = palette.edge;
    ctx.lineWidth = 1.3;
    ctx.globalAlpha = 0.85;

    for (let i = 0; i < vertexCount; i++) {
      const next = (i + 1) % vertexCount;
      const chord = (i + 4) % vertexCount;

      ctx.beginPath();
      ctx.moveTo(points[i].x, points[i].y);
      ctx.lineTo(points[next].x, points[next].y);
      ctx.stroke();

      ctx.globalAlpha = 0.22;
      ctx.beginPath();
      ctx.moveTo(points[i].x, points[i].y);
      ctx.lineTo(points[chord].x, points[chord].y);
      ctx.stroke();
      ctx.globalAlpha = 0.85;
    }

    ctx.fillStyle = palette.node;
    for (const point of points) {
      ctx.beginPath();
      ctx.arc(point.x, point.y, 2.4, 0, Math.PI * 2);
      ctx.fill();
    }

    const eyeY = centerY - 5;
    const eyeBlink = mode === 'thinking' ? (Math.sin(time * 4.5) > 0.92 ? 0.2 : 1) : 1;
    const eyeSize = mode === 'speaking' ? 3.6 : 3.1;
    ctx.fillStyle = palette.node;

    ctx.save();
    ctx.translate(centerX - 10, eyeY);
    ctx.scale(1, eyeBlink);
    ctx.beginPath();
    ctx.arc(0, 0, eyeSize, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    ctx.save();
    ctx.translate(centerX + 10, eyeY);
    ctx.scale(1, eyeBlink);
    ctx.beginPath();
    ctx.arc(0, 0, eyeSize, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    const mouthOpen = mode === 'speaking' ? 4 + Math.abs(Math.sin(time * 14)) * 4 : mode === 'error' ? 1 : 2.6;
    ctx.strokeStyle = palette.node;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(centerX - 10, centerY + 11);
    ctx.quadraticCurveTo(centerX, centerY + 11 + mouthOpen, centerX + 10, centerY + 11);
    ctx.stroke();
    ctx.globalAlpha = 1;

    frameId = requestAnimationFrame(draw);
  }

  onMount(() => {
    resizeCanvas();
    frameId = requestAnimationFrame(draw);
    window.addEventListener('resize', resizeCanvas);
  });

  onDestroy(() => {
    if (frameId) cancelAnimationFrame(frameId);
    window.removeEventListener('resize', resizeCanvas);
  });
</script>

<div class="genie-shell">
  <canvas bind:this={canvas} class="genie-canvas"></canvas>
  <div class="genie-tag">{label}</div>
  {#if cleanBubble}
    <div class="genie-bubble">
      <button class="bubble-close" type="button" onclick={() => onDismiss?.()} aria-label="Dismiss advisor bubble">[x]</button>
      <div class="bubble-text">{cleanBubble}</div>
    </div>
  {/if}
</div>

<style>
  .genie-shell {
    position: relative;
    width: 150px;
    height: 150px;
    pointer-events: none;
  }

  .genie-canvas {
    width: 150px;
    height: 150px;
    display: block;
  }

  .genie-tag {
    position: absolute;
    left: 8px;
    bottom: 8px;
    padding: 2px 6px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.58rem;
    letter-spacing: 0.08em;
  }

  .genie-bubble {
    position: absolute;
    left: 122px;
    top: 8px;
    width: fit-content;
    max-width: min(52vw, 420px);
    padding: 11px 36px 11px 14px;
    border: 1px solid color-mix(in srgb, var(--primary) 35%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    line-height: 1.35;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    box-shadow: var(--shadow);
    backdrop-filter: blur(6px);
    pointer-events: auto;
  }

  .genie-bubble::before {
    content: '';
    position: absolute;
    left: -8px;
    top: 20px;
    width: 12px;
    height: 12px;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border-left: 1px solid color-mix(in srgb, var(--primary) 35%, var(--bg-300));
    border-bottom: 1px solid color-mix(in srgb, var(--primary) 35%, var(--bg-300));
    transform: rotate(45deg);
  }

  .bubble-close {
    position: absolute;
    top: 6px;
    right: 8px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 72%, transparent);
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.58rem;
    line-height: 1;
    cursor: pointer;
    padding: 2px 4px;
    text-transform: uppercase;
  }

  .bubble-close:hover {
    color: var(--secondary);
  }

  .bubble-text {
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
