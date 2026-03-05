<script>
  import { onDestroy, onMount } from 'svelte';

  let { mode = 'idle', bubble = '', question = '', onDismiss = null, traits = {}, intensity = 1.0 } = $props();

  const defaultTraits = {
    colorHue: 0,
    vertexCount: 12,
    jitterScale: 1.0,
    pulseScale: 1.0
  };
  const t = $derived({ ...defaultTraits, ...traits });

  let canvas;
  let frameId = 0;
  let dpr = 1;
  let ctx = null;
  let copyFeedback = $state('');
  let copyFeedbackTimer = 0;

  const SIZE = 150;
  const MAX_BUBBLE_LEN = 1200;

  const cleanBubble = $derived.by(() => {
    const text = `${bubble ?? ''}`.replace(/\s+/g, ' ').trim();
    if (!text) return '';
    return text.length > MAX_BUBBLE_LEN ? `${text.slice(0, MAX_BUBBLE_LEN - 1)}…` : text;
  });
  const cleanQuestion = $derived.by(() => `${question ?? ''}`.replace(/\s+/g, ' ').trim());

  async function copyBubbleText() {
    if (!cleanBubble) return;
    try {
      await navigator.clipboard.writeText(cleanBubble);
      copyFeedback = 'COPIED';
    } catch {
      copyFeedback = 'COPY FAILED';
    }
    if (copyFeedbackTimer) clearTimeout(copyFeedbackTimer);
    copyFeedbackTimer = window.setTimeout(() => {
      copyFeedback = '';
    }, 1400);
  }

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

    const applyHue = (color) => {
      if (t.colorHue === 0) return color;
      return `color-mix(in hcl, ${color}, hcl(${t.colorHue} 50 50))`;
    };

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

    if (mode === 'light') {
      return {
        edge: '#8fd8b1',
        node: '#dff9ea',
        glow: 'rgba(92, 199, 141, 0.30)'
      };
    }

    if (mode === 'rendering') {
      return {
        edge: '#8be7ff',
        node: '#def8ff',
        glow: 'rgba(101, 222, 255, 0.40)'
      };
    }

    if (mode === 'repairing') {
      return {
        edge: '#f2bf6f',
        node: '#fff0cf',
        glow: 'rgba(242, 191, 111, 0.34)'
      };
    }

    return {
      edge: applyHue(primary),
      node: '#d6eddc',
      glow: 'rgba(74, 140, 92, 0.34)'
    };
  }

  function draw(timestamp) {
    if (!ctx) return;

    const time = timestamp * 0.001;
    const isThinkingEcky = mode === 'thinking';
    const isLightEcky = mode === 'light';
    const isVoltageEcky = mode === 'rendering';
    const isRepairEcky = mode === 'repairing';
    const isSpeakingEcky = mode === 'speaking';
    const isErrorEcky = mode === 'error';
    const profile = {
      jitter: (isThinkingEcky ? 0.45 : isLightEcky ? 0.28 : isRepairEcky ? 0.95 : isVoltageEcky ? 1.65 : isErrorEcky ? 2.3 : isSpeakingEcky ? 1.15 : 0.65) * intensity * t.jitterScale,
      pulse: (isThinkingEcky ? 0.022 : isLightEcky ? 0.014 : isRepairEcky ? 0.044 : isVoltageEcky ? 0.075 : isSpeakingEcky ? 0.055 : isErrorEcky ? 0.06 : 0.03) * intensity * t.pulseScale,
      hover: (isThinkingEcky ? 0.9 : isLightEcky ? 0.6 : isRepairEcky ? 1.3 : isVoltageEcky ? 2.2 : isSpeakingEcky ? 1.8 : 1.4) * intensity,
      skew: isThinkingEcky ? 0.005 : isLightEcky ? 0.003 : isRepairEcky ? 0.01 : isVoltageEcky ? 0.02 : isErrorEcky ? 0.03 : 0.015
    };

    const centerX = SIZE * 0.48 + Math.sin(time * (isVoltageEcky ? 2.8 : isRepairEcky ? 2.0 : 1.45)) * (isThinkingEcky ? 0.6 : isLightEcky ? 0.45 : isRepairEcky ? 0.9 : isVoltageEcky ? 1.7 : 1);
    const centerY = SIZE * 0.58 + Math.sin(time * 2.2) * profile.hover;
    const radiusBase = isThinkingEcky ? 25 : isLightEcky ? 27 : isRepairEcky ? 29 : 32;
    const radius = radiusBase * (1 + Math.sin(time * 5.3) * profile.pulse);
    const palette = pickPalette();
    const points = [];
    const vertexCount = isThinkingEcky ? 20 : (isLightEcky ? 12 : (isRepairEcky ? 15 : (isVoltageEcky ? 16 : (isSpeakingEcky ? 14 : t.vertexCount))));
    const tilt = Math.sin(time * 1.1) * (isThinkingEcky ? 0.01 : isLightEcky ? 0.008 : isRepairEcky ? 0.018 : isVoltageEcky ? 0.035 : 0.02);

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, SIZE, SIZE);

    ctx.save();
    ctx.beginPath();
    ctx.arc(centerX, centerY, radius + 24, 0, Math.PI * 2);
    ctx.fillStyle = palette.glow;
    ctx.fill();
    ctx.restore();

    for (let i = 0; i < vertexCount; i++) {
      const baseAngle = (i / vertexCount) * Math.PI * 2 + tilt;
      const drift = Math.sin(time * 2.5 + i * 0.85) * profile.jitter;
      const asym = 1 + Math.sin(i * 1.65 + time * 0.8) * profile.skew;
      const modeWarp = isVoltageEcky
        ? Math.sin(time * 4 + i * 1.2) * 0.7
        : isRepairEcky
          ? Math.sin(time * 3.1 + i * 0.95) * 0.35
        : isErrorEcky
          ? Math.sin(time * 2.5 + i * 0.7) * 0.9
          : 0;
      const radial = radius + drift + modeWarp;
      const yStretch = isThinkingEcky ? 0.98 : 0.96;
      const x = centerX + Math.cos(baseAngle) * radial * asym;
      const y = centerY + Math.sin(baseAngle) * radial * yStretch;
      points.push({ x, y });
    }

    ctx.strokeStyle = palette.edge;
    ctx.lineWidth = 1.3;
    ctx.globalAlpha = 0.85;

    for (let i = 0; i < vertexCount; i++) {
      const next = (i + 1) % vertexCount;
      const chord = (i + (isThinkingEcky ? 3 : isLightEcky ? 4 : isRepairEcky ? 4 : isVoltageEcky ? 5 : 4)) % vertexCount;

      ctx.beginPath();
      ctx.moveTo(points[i].x, points[i].y);
      ctx.lineTo(points[next].x, points[next].y);
      ctx.stroke();

      const drawChord = isThinkingEcky || isLightEcky || isRepairEcky || isVoltageEcky || isErrorEcky;
      if (drawChord) {
        ctx.globalAlpha = isThinkingEcky ? 0.2 : isLightEcky ? 0.12 : isRepairEcky ? 0.14 : 0.16;
        ctx.beginPath();
        ctx.moveTo(points[i].x, points[i].y);
        ctx.lineTo(points[chord].x, points[chord].y);
        ctx.stroke();
      }
      ctx.globalAlpha = 0.85;
    }

    if (isThinkingEcky || isLightEcky || isRepairEcky) {
      ctx.globalAlpha = 0.2;
      for (let i = 0; i < vertexCount; i += (isLightEcky ? 3 : isRepairEcky ? 4 : 2)) {
        ctx.beginPath();
        ctx.moveTo(points[i].x, points[i].y);
        ctx.lineTo(centerX, centerY);
        ctx.stroke();
      }
      ctx.globalAlpha = 0.85;
    }

    ctx.fillStyle = palette.node;
    for (const point of points) {
      ctx.beginPath();
      ctx.arc(point.x, point.y, isThinkingEcky ? 2.2 : isLightEcky ? 2.0 : isRepairEcky ? 2.1 : 2.3, 0, Math.PI * 2);
      ctx.fill();
    }

    // Keep all visual elements attached to the core head mesh (no detached satellites).
    ctx.globalAlpha = 0.85;

    const eyeY = centerY - 5;
    const eyeBlink = isThinkingEcky ? 0.05 : (Math.sin(time * 4.5) > 0.92 ? 0.2 : 1);
    const leftEyeSize = mode === 'speaking' ? 3.4 : 2.7;
    const rightEyeSize = mode === 'speaking' ? 3.4 : 2.7;
    ctx.fillStyle = palette.node;

    if (isThinkingEcky || isLightEcky) {
      ctx.strokeStyle = palette.node;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(centerX - 14, eyeY);
      ctx.lineTo(centerX - 6, eyeY);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(centerX + 5, eyeY);
      ctx.lineTo(centerX + 13, eyeY);
      ctx.stroke();
    } else if (isRepairEcky) {
      ctx.strokeStyle = palette.node;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(centerX - 14, eyeY);
      ctx.lineTo(centerX - 8, eyeY + 1.5);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(centerX + 5, eyeY + 1.5);
      ctx.lineTo(centerX + 13, eyeY - 1.5);
      ctx.stroke();
    } else {
      ctx.save();
      ctx.translate(centerX - 11, eyeY - 1);
      ctx.scale(1, eyeBlink);
      ctx.beginPath();
      ctx.arc(0, 0, leftEyeSize, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();

      ctx.save();
      ctx.translate(centerX + 9, eyeY + 1);
      ctx.scale(1, eyeBlink);
      ctx.beginPath();
      ctx.arc(0, 0, rightEyeSize, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    const mouthOpen = mode === 'speaking' ? 4 + Math.abs(Math.sin(time * 14)) * 3.5 : mode === 'error' ? 1 : isRepairEcky ? 1.4 : isThinkingEcky ? 1.1 : isLightEcky ? 1.0 : 2.2;
    ctx.strokeStyle = palette.node;
    ctx.lineWidth = 2;
    ctx.beginPath();
    if (!isThinkingEcky && !isLightEcky && !isRepairEcky) {
      ctx.moveTo(centerX - 10, centerY + 10);
      ctx.quadraticCurveTo(centerX, centerY + 11 + mouthOpen, centerX + 10, centerY + 10);
    } else {
      ctx.moveTo(centerX - 9, centerY + 10);
      ctx.lineTo(centerX + 9, centerY + 10 + (isRepairEcky ? 1.5 : 0));
    }
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
    if (copyFeedbackTimer) clearTimeout(copyFeedbackTimer);
  });
</script>

<div class="genie-shell">
  <canvas bind:this={canvas} class="genie-canvas"></canvas>
  {#if cleanBubble}
    <div class="genie-bubble">
      <button class="bubble-copy" type="button" onclick={copyBubbleText} aria-label="Copy advisor response">
        {copyFeedback || 'COPY'}
      </button>
      <button class="bubble-close" type="button" onclick={() => onDismiss?.()} aria-label="Dismiss advisor bubble"></button>
      <div class="bubble-speaker"><strong>ECKY:</strong></div>
      {#if cleanQuestion}
        <div class="bubble-question-block">
          <div class="bubble-question-label">YOU ASKED</div>
          <div class="bubble-question">"{cleanQuestion}"</div>
        </div>
      {/if}
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

  .genie-bubble {
    position: absolute;
    left: 138px;
    top: 8px;
    width: clamp(420px, 54vw, 760px);
    max-width: min(78vw, 760px);
    min-height: 130px;
    max-height: min(52vh, 480px);
    padding: 16px 78px 16px 18px;
    border: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.82rem;
    line-height: 1.56;
    text-transform: none;
    letter-spacing: 0.01em;
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--bg-300) 85%, transparent), var(--shadow);
    backdrop-filter: blur(9px);
    pointer-events: auto;
    -webkit-user-select: text !important;
    user-select: text !important;
    overflow-y: auto;
  }

  .genie-bubble::before {
    content: '';
    position: absolute;
    left: -12px;
    top: 26px;
    width: 12px;
    height: 20px;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border-left: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    border-top: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    border-bottom: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
  }

  .genie-bubble::after {
    content: '';
    position: absolute;
    left: -18px;
    top: 31px;
    width: 6px;
    height: 10px;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border-left: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    border-top: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    border-bottom: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
  }

  .genie-bubble::selection,
  .genie-bubble *::selection {
    background: color-mix(in srgb, var(--primary) 52%, transparent);
    color: var(--text);
  }

  .bubble-copy,
  .bubble-close {
    position: absolute;
    top: 8px;
    height: 18px;
    border: 2px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, transparent);
    cursor: pointer;
    padding: 0;
    font-family: var(--font-mono);
    line-height: 1;
  }

  .bubble-copy {
    right: 34px;
    min-width: 38px;
    height: 18px;
    padding: 0 5px;
    color: var(--text-dim);
    font-size: 0.54rem;
    letter-spacing: 0.06em;
  }

  .bubble-copy:hover {
    border-color: var(--primary);
    color: var(--primary);
  }

  .bubble-close {
    right: 10px;
    width: 18px;
  }

  .bubble-close::before,
  .bubble-close::after {
    content: '';
    position: absolute;
    left: 3px;
    top: 7px;
    width: 10px;
    height: 2px;
    background: var(--text-dim);
  }

  .bubble-close::before {
    transform: rotate(45deg);
  }

  .bubble-close::after {
    transform: rotate(-45deg);
  }

  .bubble-close:hover {
    border-color: var(--secondary);
  }

  .bubble-close:hover::before,
  .bubble-close:hover::after {
    background: var(--secondary);
  }

  .bubble-text {
    white-space: pre-wrap;
    word-break: break-word;
    text-wrap: pretty;
    -webkit-user-select: text !important;
    user-select: text !important;
  }

  .bubble-question-block {
    margin-bottom: 10px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--bg-300) 85%, transparent);
    background: color-mix(in srgb, var(--bg) 54%, transparent);
    max-height: 18vh;
    overflow-y: auto;
  }

  .bubble-question-label {
    margin-bottom: 4px;
    color: var(--text-dim);
    font-size: 0.62rem;
    letter-spacing: 0.06em;
  }

  .bubble-question {
    color: var(--text-dim);
    font-size: 0.74rem;
    line-height: 1.45;
    -webkit-user-select: text !important;
    user-select: text !important;
  }

  .bubble-speaker {
    margin-bottom: 6px;
    color: var(--secondary);
    letter-spacing: 0.06em;
    font-size: 0.72rem;
  }

  @media (max-width: 960px) {
    .genie-bubble {
      width: min(86vw, 620px);
      max-width: min(86vw, 620px);
      min-height: 110px;
      max-height: min(46vh, 420px);
      font-size: 0.76rem;
      line-height: 1.5;
    }
  }
</style>
