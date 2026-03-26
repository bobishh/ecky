<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    DEFAULT_GENIE_TRAITS,
    resolveModeTraits,
    seededSigned,
    seededUnit,
    type GenieMode,
    type ResolvedGenieProfile,
  } from './genie/traits';
  import type { GenieTraits } from './types/domain';

  type Palette = {
    edge: string;
    node: string;
    glow: string;
  };
  type Point = {
    x: number;
    y: number;
  };

  const TAU = Math.PI * 2;

  let {
    mode = 'idle',
    bubble = '',
    question = '',
    onDismiss = null,
    actions = null,
    traits = {},
    intensity = 1.0,
    wakeUp = 0,
    agentConnected = true,
  }: {
    mode?: GenieMode;
    bubble?: string;
    question?: string;
    onDismiss?: (() => void) | null;
    actions?: Array<{ label: string; onclick: () => void }> | null;
    traits?: Partial<GenieTraits> | null;
    intensity?: number;
    wakeUp?: number;
    agentConnected?: boolean;
  } = $props();

  const WAKE_DUR = 650;
  let wakeUpStartTime: number | null = null;

  $effect(() => {
    if (wakeUp > 0) wakeUpStartTime = performance.now();
  });

  const profile = $derived.by(() => {
    const effectiveMode = agentConnected ? mode : 'sleeping';
    return resolveModeTraits(traits ?? DEFAULT_GENIE_TRAITS, effectiveMode);
  });

  let canvas: HTMLCanvasElement;
  let frameId = 0;
  let dpr = 1;
  let ctx: CanvasRenderingContext2D | null = null;
  let copyFeedback = $state('');
  let copyFeedbackTimer: number | null = null;

  const SIZE = 150;
  const MAX_BUBBLE_LEN = 1200;

  const cleanBubble = $derived.by(() => {
    const text = `${bubble ?? ''}`.replace(/\s+/g, ' ').trim();
    if (!text) return '';
    return text.length > MAX_BUBBLE_LEN ? `${text.slice(0, MAX_BUBBLE_LEN - 1)}…` : text;
  });
  const cleanQuestion = $derived.by(() => `${question ?? ''}`.replace(/\s+/g, ' ').trim());

  function clamp(value: number, min: number, max: number): number {
    return Math.max(min, Math.min(max, value));
  }

  function normalizeHue(value: number): number {
    return ((value % 360) + 360) % 360;
  }

  function normalizeOperationalHue(value: number): number {
    const hue = normalizeHue(value);
    if (hue < 90 || hue > 220) return 154;
    if (hue < 118) return 126;
    if (hue > 188) return 178;
    return hue;
  }

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

  function pickPalette(currentProfile: ResolvedGenieProfile): Palette {
    const css = getComputedStyle(document.documentElement);
    const primary = css.getPropertyValue('--primary').trim() || '#4a8c5c';
    const secondary = css.getPropertyValue('--secondary').trim() || '#c8a620';
    const red = css.getPropertyValue('--red').trim() || '#ff6b6b';
    const blendHue = (base: string, hue: number, lightness: number, amount: number): string => {
      const baseAmount = clamp(100 - amount, 0, 100);
      return `color-mix(in hsl, ${base} ${baseAmount}%, hsl(${normalizeHue(hue)} 70% ${lightness}%))`;
    };
    const colorHue =
      currentProfile.palettePreset === 'error'
        ? normalizeHue(currentProfile.colorHue)
        : normalizeOperationalHue(currentProfile.colorHue);
    const glowHue = normalizeHue(colorHue + currentProfile.glowHueShift);

    switch (currentProfile.palettePreset) {
      case 'sleeping':
        return {
          edge: blendHue('#444', colorHue, 40, 10),
          node: blendHue('#666', colorHue, 50, 5),
          glow: `hsla(${normalizeHue(glowHue)}, 10%, 20%, 0.1)`,
        };
      case 'waking':
        return {
          edge: blendHue('#555', colorHue, 46, 18),
          node: blendHue('#898', colorHue, 60, 14),
          glow: `hsla(${normalizeHue(glowHue + 8)}, 18%, 28%, 0.16)`,
        };
      case 'thinking':
        return {
          edge: blendHue(primary, colorHue - 8, 62, 18),
          node: blendHue('#eef7f1', colorHue - 6, 90, 12),
          glow: `hsl(${normalizeHue(glowHue - 10)} 58% 44% / 0.26)`,
        };
      case 'light':
        return {
          edge: blendHue(primary, colorHue - 24, 72, 38),
          node: blendHue('#effbf3', colorHue - 14, 90, 22),
          glow: `hsl(${glowHue} 78% 68% / 0.24)`,
        };
      case 'repairing':
        return {
          edge: blendHue(secondary, colorHue + 6, 60, 16),
          node: blendHue('#f8f1df', colorHue + 2, 88, 10),
          glow: `hsl(${normalizeHue(glowHue - 4)} 54% 46% / 0.24)`,
        };
      case 'rendering':
        return {
          edge: blendHue('#8be7ff', colorHue + 52, 70, 24),
          node: blendHue('#effcff', colorHue + 40, 92, 16),
          glow: `hsl(${normalizeHue(glowHue + 34)} 90% 68% / 0.36)`,
        };
      case 'speaking':
        return {
          edge: blendHue(primary, colorHue + 4, 62, 18),
          node: blendHue('#f2fff6', colorHue + 2, 92, 12),
          glow: `hsl(${normalizeHue(glowHue - 6)} 60% 46% / 0.24)`,
        };
      case 'error':
        return {
          edge: blendHue(red, 6, 66, 10),
          node: blendHue('#ffe0e0', 8, 90, 8),
          glow: 'hsl(4 88% 66% / 0.42)',
        };
      case 'base':
      default:
        return {
          edge: blendHue(primary, colorHue - 4, 60, 14),
          node: blendHue('#e7f2eb', colorHue, 88, 10),
          glow: `hsl(${normalizeHue(glowHue - 8)} 56% 42% / 0.22)`,
        };
    }
  }

  function drawEyeDots(
    context: CanvasRenderingContext2D,
    currentProfile: ResolvedGenieProfile,
    centerX: number,
    centerY: number,
    eyeBlink: number,
  ) {
    const eyeY = centerY - 5 + currentProfile.seedOffsets.eyeY * 1.5;
    const eyeSpacing = currentProfile.eyeSpacing;
    const leftX = centerX - eyeSpacing * 0.5 + currentProfile.seedOffsets.eyeX * 1.4;
    const rightX = centerX + eyeSpacing * 0.5 + currentProfile.seedOffsets.eyeX * 0.7;

    for (const eyeX of [leftX, rightX]) {
      context.save();
      context.translate(eyeX, eyeY);
      context.scale(1, eyeBlink);
      context.beginPath();
      context.ellipse(0, 0, currentProfile.eyeSize, currentProfile.eyeSize * 0.92, 0, 0, TAU);
      context.fill();
      context.restore();
    }
  }

  function drawEyeLines(
    context: CanvasRenderingContext2D,
    currentProfile: ResolvedGenieProfile,
    centerX: number,
    centerY: number,
    slant: number,
  ) {
    const eyeY = centerY - 5 + currentProfile.seedOffsets.eyeY * 1.5;
    const eyeSpacing = currentProfile.eyeSpacing;
    const leftX = centerX - eyeSpacing * 0.5 + currentProfile.seedOffsets.eyeX * 1.4;
    const rightX = centerX + eyeSpacing * 0.5 + currentProfile.seedOffsets.eyeX * 0.7;
    const width = currentProfile.eyeSize * 2.8;

    context.beginPath();
    context.moveTo(leftX - width * 0.5, eyeY + slant);
    context.lineTo(leftX + width * 0.5, eyeY - slant);
    context.stroke();

    context.beginPath();
    context.moveTo(rightX - width * 0.5, eyeY - slant);
    context.lineTo(rightX + width * 0.5, eyeY + slant);
    context.stroke();
  }

  function drawFace(
    context: CanvasRenderingContext2D,
    currentProfile: ResolvedGenieProfile,
    centerX: number,
    centerY: number,
    time: number,
    currentMode: GenieMode,
    palette: Palette,
  ) {
    const now = performance.now();
    const eyeOpenProgress =
      wakeUpStartTime !== null
        ? Math.min(1, (now - wakeUpStartTime) / WAKE_DUR)
        : 1;
    const blinkPulse = Math.sin(
      time * (3.4 + currentProfile.pulseScale * 0.9) + currentProfile.seedOffsets.blink,
    );
    const eyeBlinkBase =
      currentMode === 'speaking'
        ? 0.72 + Math.abs(Math.sin(time * 9.5 + currentProfile.seedOffsets.blink)) * 0.35
        : blinkPulse > 0.965
          ? 0.18
          : 1;
    const eyeBlink = eyeOpenProgress < 1 ? eyeOpenProgress * eyeBlinkBase : eyeBlinkBase;

    context.fillStyle = palette.node;
    context.strokeStyle = palette.node;
    context.lineCap = 'round';

    if (currentMode === 'waking') {
      // Asymmetric waking eyes: left stays closed (bar), right opens with eyeOpenProgress
      const eyeY = centerY - 5 + currentProfile.seedOffsets.eyeY * 1.5;
      const eyeSpacing = currentProfile.eyeSpacing;
      const leftX = centerX - eyeSpacing * 0.5 + currentProfile.seedOffsets.eyeX * 1.4;
      const rightX = centerX + eyeSpacing * 0.5 + currentProfile.seedOffsets.eyeX * 0.7;
      const barWidth = currentProfile.eyeSize * 2.8;
      context.lineWidth = 2.2;
      // Left eye: horizontal bar (still sleeping)
      context.beginPath();
      context.moveTo(leftX - barWidth * 0.5, eyeY);
      context.lineTo(leftX + barWidth * 0.5, eyeY);
      context.stroke();
      // Right eye: opening dot (0.08 at minimum so it's just cracking open)
      const rightBlink = Math.max(0.08, eyeOpenProgress * eyeBlinkBase);
      context.save();
      context.translate(rightX, eyeY);
      context.scale(1, rightBlink);
      context.beginPath();
      context.ellipse(0, 0, currentProfile.eyeSize, currentProfile.eyeSize * 0.92, 0, 0, TAU);
      context.fill();
      context.restore();
    } else {
      switch (currentProfile.eyeStyle) {
        case 'bar':
          context.lineWidth = 2.2;
          drawEyeLines(context, currentProfile, centerX, centerY, 0);
          break;
        case 'slant':
          context.lineWidth = 2.2;
          drawEyeLines(
            context,
            currentProfile,
            centerX,
            centerY,
            1.2 + currentProfile.seedOffsets.eyeX * 0.8,
          );
          break;
        case 'dot':
        default:
          drawEyeDots(context, currentProfile, centerX, centerY, eyeBlink);
          break;
      }
    }

    const mouthMotion =
      currentMode === 'speaking'
        ? Math.abs(Math.sin(time * 12.8 + currentProfile.seedOffsets.mouth))
        : 0.25 + (Math.sin(time * 2.3 + currentProfile.seedOffsets.mouth) + 1) * 0.25;
    const mouthOpen =
      currentProfile.mouthOpenBase + mouthMotion * currentProfile.mouthOpenAmplitude;
    const mouthWidth = 9.5 + Math.abs(currentProfile.mouthCurve) * 1.6;
    const mouthY = centerY + 10 + currentProfile.seedOffsets.eyeY * 0.5;

    context.lineWidth = 2.1;
    context.beginPath();
    if (currentProfile.mouthStyle === 'line') {
      const slant = currentProfile.mouthCurve * 0.35;
      context.moveTo(centerX - mouthWidth, mouthY + slant);
      context.lineTo(centerX + mouthWidth, mouthY - slant + mouthOpen * 0.1);
    } else {
      context.moveTo(centerX - mouthWidth, mouthY);
      context.quadraticCurveTo(
        centerX,
        mouthY + currentProfile.mouthCurve * 1.8 + mouthOpen,
        centerX + mouthWidth,
        mouthY,
      );
    }
    context.stroke();
  }

  function draw(timestamp: number) {
    if (!ctx) return;

    const time = timestamp * 0.001;
    const currentProfile = profile;
    const currentIntensity = clamp(intensity, 0.6, 2.6);
    const palette = pickPalette(currentProfile);
    const points: Point[] = [];
    const centerX =
      SIZE * 0.48 +
      Math.sin(
        time * currentProfile.centerOrbitFrequency + currentProfile.seedOffsets.orbit,
      ) *
        currentProfile.centerOrbitAmplitude *
        currentIntensity +
      Math.cos(time * 0.92 + currentProfile.seedOffsets.hover) *
        currentProfile.seedOffsets.eyeX *
        0.8;
    const centerY =
      SIZE * 0.58 +
      Math.sin(time * 2.15 + currentProfile.seedOffsets.hover) *
        currentProfile.hoverScale *
        currentIntensity;
    const radius =
      currentProfile.radiusBase *
      (1 +
        Math.sin(time * 4.9 + currentProfile.seedOffsets.pulse) *
          0.022 *
          currentProfile.pulseScale *
          currentIntensity);
    const tilt =
      Math.sin(time * 1.08 + currentProfile.seedOffsets.orbit) *
      currentProfile.tiltScale *
      5.5;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, SIZE, SIZE);

    ctx.save();
    ctx.beginPath();
    ctx.arc(centerX, centerY, radius + 24, 0, Math.PI * 2);
    ctx.fillStyle = palette.glow;
    ctx.fill();
    ctx.restore();

    for (let i = 0; i < currentProfile.vertexCount; i++) {
      const baseAngle = (i / currentProfile.vertexCount) * TAU + tilt;
      const vertexJitter = 0.55 + seededUnit(currentProfile.seed, 100 + i) * 0.95;
      const warpWeight = 0.45 + seededUnit(currentProfile.seed, 200 + i) * 0.95;
      const angleJitter = seededSigned(currentProfile.seed, 300 + i) * 0.06;
      const drift =
        Math.sin(
          time * (2.2 + currentProfile.jitterScale * 0.45) +
            i * 0.8 +
            currentProfile.seedOffsets.jitter,
        ) *
        0.7 *
        currentIntensity *
        currentProfile.jitterScale *
        vertexJitter;
      const warp =
        Math.sin(
          time * (1.8 + currentProfile.warpScale * 1.5) +
            i * (0.74 + warpWeight * 0.18) +
            currentProfile.seedOffsets.warp,
        ) *
        currentProfile.warpScale *
        currentIntensity *
        0.9 *
        warpWeight;
      const asymmetryWave =
        1 +
        (currentProfile.asymmetry - 1) *
          Math.sin(baseAngle * 2 + currentProfile.seedOffsets.asym * 2 + time * 0.22);
      const radial = radius + drift + warp;
      const x = centerX + Math.cos(baseAngle + angleJitter) * radial * asymmetryWave;
      const y =
        centerY +
        Math.sin(baseAngle + angleJitter) *
          radial *
          currentProfile.stretchY *
          (1 + seededSigned(currentProfile.seed, 400 + i) * 0.03);
      points.push({ x, y });
    }

    ctx.strokeStyle = palette.edge;
    ctx.lineWidth = currentProfile.lineWidth;
    ctx.globalAlpha = 0.85;

    for (let i = 0; i < currentProfile.vertexCount; i++) {
      const next = (i + 1) % currentProfile.vertexCount;
      const chord = (i + currentProfile.chordSkip) % currentProfile.vertexCount;

      ctx.beginPath();
      ctx.moveTo(points[i].x, points[i].y);
      ctx.lineTo(points[next].x, points[next].y);
      ctx.stroke();

      ctx.globalAlpha = currentProfile.chordAlpha;
      ctx.beginPath();
      ctx.moveTo(points[i].x, points[i].y);
      ctx.lineTo(points[chord].x, points[chord].y);
      ctx.stroke();
      ctx.globalAlpha = 0.85;
    }

    if (currentProfile.spokeStride) {
      ctx.globalAlpha = Math.min(0.24, currentProfile.chordAlpha + 0.04);
      for (let i = 0; i < currentProfile.vertexCount; i += currentProfile.spokeStride) {
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
      ctx.arc(point.x, point.y, currentProfile.nodeRadius, 0, TAU);
      ctx.fill();
    }

    // Keep all visual elements attached to the core head mesh (no detached satellites).
    ctx.globalAlpha = 0.85;
    drawFace(ctx, currentProfile, centerX, centerY, time, mode, palette);

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
      <div class="bubble-speaker"><strong>ECKY EINACS:</strong></div>
      {#if cleanQuestion}
        <div class="bubble-question-block">
          <div class="bubble-question-label">YOU ASKED</div>
          <div class="bubble-question">"{cleanQuestion}"</div>
        </div>
      {/if}
      <div class="bubble-text">{cleanBubble}</div>
      {#if actions?.length}
        <div class="bubble-actions">
          {#each actions as action}
            <button class="bubble-action-btn" type="button" onclick={action.onclick}>{action.label}</button>
          {/each}
        </div>
      {/if}
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

  .bubble-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 14px;
    padding-top: 10px;
    border-top: 1px solid color-mix(in srgb, var(--bg-300) 70%, transparent);
  }

  .bubble-action-btn {
    padding: 5px 14px;
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    font-weight: bold;
    letter-spacing: 0.06em;
    cursor: pointer;
  }

  .bubble-action-btn:hover {
    border-color: var(--primary);
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-300));
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
