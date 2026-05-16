<script lang="ts">
  import { onDestroy } from 'svelte';
  import { buildCornerGlyph } from './genie/angularGeometry';
  import {
    DEFAULT_GENIE_TRAITS,
    resolveModeTraits,
    type GenieMode,
    type ResolvedGenieProfile,
  } from './genie/traits';
  import type { GenieTraits } from './types/domain';

  type Palette = {
    edge: string;
    node: string;
    glow: string;
    body: string;
    face: string;
    selected: string;
  };

  let {
    mode = 'idle',
    bubble = '',
    compact = false,
    badge = null,
    contextLabel = null,
    question = '',
    onDismiss = null,
    actions = null,
    traits = {},
    intensity = 1.0,
    wakeUp = 0,
    agentConnected = true,
    safeRightInset = 360,
  }: {
    mode?: GenieMode;
    bubble?: string;
    compact?: boolean;
    badge?: string | null;
    contextLabel?: string | null;
    question?: string;
    onDismiss?: (() => void) | null;
    actions?: Array<{ label: string; onclick: () => void }> | null;
    traits?: Partial<GenieTraits> | null;
    intensity?: number;
    wakeUp?: number;
    agentConnected?: boolean;
    safeRightInset?: number;
  } = $props();

  let copyFeedback = $state('');
  let copyFeedbackTimer: number | null = null;
  let wakePulse = $state(0);

  const MAX_BUBBLE_LEN = 1200;
  const effectiveMode = $derived(agentConnected ? mode : 'sleeping');
  const profile = $derived.by(() =>
    resolveModeTraits(traits ?? DEFAULT_GENIE_TRAITS, effectiveMode),
  );
  const glyph = $derived.by(() => buildCornerGlyph(profile));
  const motionScale = $derived(Math.min(2.6, Math.max(0.6, intensity)));

  $effect(() => {
    wakePulse = wakeUp;
  });

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

  function pickPalette(currentProfile: ResolvedGenieProfile): Palette {
    const primary = 'var(--primary, #4a8c5c)';
    const secondary = 'var(--secondary, #c8a620)';
    const red = 'var(--red, #ff6b6b)';
    const blendHue = (base: string, hue: number, lightness: number, amount: number): string => {
      const baseAmount = clamp(100 - amount, 0, 100);
      return `color-mix(in hsl, ${base} ${baseAmount}%, hsl(${normalizeHue(hue)} 70% ${lightness}%))`;
    };
    const colorHue =
      currentProfile.palettePreset === 'error'
        ? normalizeHue(currentProfile.colorHue)
        : normalizeOperationalHue(currentProfile.colorHue);
    const glowHue = normalizeHue(colorHue + currentProfile.glowHueShift);
    const baseEdge = blendHue(primary, colorHue - 4, 60, 14);

    switch (currentProfile.palettePreset) {
      case 'sleeping':
        return {
          edge: blendHue('#444', colorHue, 40, 10),
          node: blendHue('#666', colorHue, 50, 5),
          glow: `hsla(${normalizeHue(glowHue)}, 10%, 20%, 0.1)`,
          body: 'color-mix(in srgb, var(--bg-200) 82%, #555)',
          face: 'color-mix(in srgb, var(--bg-100) 76%, #666)',
          selected: 'color-mix(in srgb, var(--secondary) 28%, #666)',
        };
      case 'waking':
        return {
          edge: blendHue('#555', colorHue, 46, 18),
          node: blendHue('#898', colorHue, 60, 14),
          glow: `hsla(${normalizeHue(glowHue + 8)}, 18%, 28%, 0.16)`,
          body: 'color-mix(in srgb, var(--bg-200) 78%, #506055)',
          face: 'color-mix(in srgb, var(--bg-100) 76%, #70806e)',
          selected: 'color-mix(in srgb, var(--secondary) 34%, #6f7560)',
        };
      case 'repairing':
        return {
          edge: blendHue(secondary, colorHue + 6, 60, 16),
          node: blendHue('#f8f1df', colorHue + 2, 88, 10),
          glow: `hsl(${normalizeHue(glowHue - 4)} 54% 46% / 0.24)`,
          body: 'color-mix(in srgb, var(--bg-200) 70%, var(--secondary))',
          face: 'color-mix(in srgb, var(--bg-100) 78%, var(--secondary))',
          selected: 'var(--secondary)',
        };
      case 'rendering':
        return {
          edge: blendHue('#8be7ff', colorHue + 52, 70, 24),
          node: blendHue('#effcff', colorHue + 40, 92, 16),
          glow: `hsl(${normalizeHue(glowHue + 34)} 90% 68% / 0.36)`,
          body: 'color-mix(in srgb, var(--bg-200) 66%, #2aaec0)',
          face: 'color-mix(in srgb, var(--bg-100) 75%, #8be7ff)',
          selected: '#8be7ff',
        };
      case 'error':
        return {
          edge: blendHue(red, 6, 66, 10),
          node: blendHue('#ffe0e0', 8, 90, 8),
          glow: 'hsl(4 88% 66% / 0.42)',
          body: 'color-mix(in srgb, var(--bg-200) 72%, var(--red))',
          face: 'color-mix(in srgb, var(--bg-100) 74%, var(--red))',
          selected: 'var(--red)',
        };
      case 'thinking':
      case 'speaking':
      case 'light':
      case 'base':
      default:
        return {
          edge: baseEdge,
          node: blendHue('#e7f2eb', colorHue, 88, 10),
          glow: `hsl(${normalizeHue(glowHue - 8)} 56% 42% / 0.22)`,
          body: 'color-mix(in srgb, var(--bg-200) 70%, var(--primary))',
          face: 'color-mix(in srgb, var(--bg-100) 78%, var(--primary))',
          selected: 'var(--secondary)',
        };
    }
  }

  const palette = $derived.by(() => pickPalette(profile));
  const svgStyle = $derived(
    `--corner-edge: ${palette.edge}; --corner-node: ${palette.node}; --corner-glow: ${palette.glow}; --corner-body: ${palette.body}; --corner-face: ${palette.face}; --corner-selected: ${palette.selected}; --corner-motion: ${motionScale};`,
  );

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

  onDestroy(() => {
    if (copyFeedbackTimer) clearTimeout(copyFeedbackTimer);
  });
</script>

<div
  class="genie-shell"
  data-agent-connected={agentConnected ? 'true' : 'false'}
  style={`--genie-safe-right: ${Math.max(0, safeRightInset)}px;`}
>
  <svg
    class="genie-corner-svg"
    data-mode={effectiveMode}
    data-wake-pulse={wakePulse}
    viewBox="0 0 150 150"
    aria-hidden="true"
    style={svgStyle}
  >
    <defs>
      <filter id="genie-corner-soft-glow" x="-35%" y="-35%" width="170%" height="170%">
        <feGaussianBlur stdDeviation="3.2" result="blur" />
        <feMerge>
          <feMergeNode in="blur" />
          <feMergeNode in="SourceGraphic" />
        </feMerge>
      </filter>
    </defs>
    <ellipse class="genie-corner-glow" cx="75" cy="80" rx="46" ry="52" />
    <g class="genie-corner-glyph">
      <polygon class="genie-corner-body" points={glyph.bodyPoints} />
      <polygon class="genie-corner-face" points={glyph.facePoints} />
      {#each glyph.edges as edge}
        <line class="genie-corner-edge" x1={edge.x1} y1={edge.y1} x2={edge.x2} y2={edge.y2} />
      {/each}
      <line
        class="genie-corner-selected-edge"
        x1={glyph.selectedEdge.x1}
        y1={glyph.selectedEdge.y1}
        x2={glyph.selectedEdge.x2}
        y2={glyph.selectedEdge.y2}
      />
      {#each glyph.nodes as node}
        <circle class="genie-corner-node" cx={node.x} cy={node.y} r="3.1" />
      {/each}
      <g class="genie-corner-face-lines">
        <line x1="58" y1={glyph.eyeY + glyph.eyeSlant} x2="69" y2={glyph.eyeY - glyph.eyeSlant} />
        <line x1="82" y1={glyph.eyeY - glyph.eyeSlant} x2="94" y2={glyph.eyeY + glyph.eyeSlant} />
        <path d={`M 63 ${glyph.mouthY} Q 75 ${glyph.mouthY + glyph.mouthCurve * 1.8} 88 ${glyph.mouthY}`} />
      </g>
    </g>
  </svg>
  {#if cleanBubble}
    <div class="genie-bubble" class:genie-bubble--compact={compact} data-bubble-layout={compact ? 'compact' : 'full'}>
      <button class="bubble-copy" type="button" onclick={copyBubbleText} aria-label="Copy advisor response">
        {copyFeedback || 'COPY'}
      </button>
      <button class="bubble-close" type="button" onclick={() => onDismiss?.()} aria-label="Dismiss advisor bubble"></button>
      <div class="bubble-header">
        {#if compact}
          <div class="bubble-meta">
            {#if badge}
              <span class="bubble-badge">{badge}</span>
            {/if}
            {#if contextLabel}
              <span class="bubble-context">{contextLabel}</span>
            {/if}
          </div>
        {:else}
          <div class="bubble-speaker"><strong>ECKY EINACS:</strong></div>
        {/if}
      </div>
      {#if !compact && cleanQuestion}
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

  .genie-corner-svg {
    width: 150px;
    height: 150px;
    display: block;
    overflow: hidden;
  }

  .genie-corner-glyph {
    transform-origin: 75px 80px;
    animation: genieCornerHover calc(2.2s / var(--corner-motion, 1)) ease-in-out infinite;
  }

  .genie-corner-glow {
    fill: var(--corner-glow);
    filter: url('#genie-corner-soft-glow');
    opacity: 0.9;
  }

  .genie-corner-body {
    fill: var(--corner-body);
    stroke: color-mix(in srgb, var(--corner-edge) 56%, var(--bg-400));
    stroke-width: 2.4;
    stroke-linejoin: miter;
  }

  .genie-corner-face {
    fill: var(--corner-face);
    opacity: 0.74;
    stroke: color-mix(in srgb, var(--corner-edge) 42%, transparent);
    stroke-width: 1.2;
    stroke-linejoin: miter;
  }

  .genie-corner-edge,
  .genie-corner-selected-edge,
  .genie-corner-face-lines line,
  .genie-corner-face-lines path {
    vector-effect: non-scaling-stroke;
    stroke-linecap: square;
  }

  .genie-corner-edge {
    stroke: var(--corner-edge);
    stroke-width: 1.25;
    opacity: 0.54;
  }

  .genie-corner-selected-edge {
    stroke: var(--corner-selected);
    stroke-width: 2.9;
    opacity: 0.95;
  }

  .genie-corner-node {
    fill: var(--corner-node);
    stroke: color-mix(in srgb, var(--corner-edge) 45%, var(--bg-400));
    stroke-width: 1.2;
  }

  .genie-corner-face-lines line,
  .genie-corner-face-lines path {
    fill: none;
    stroke: color-mix(in srgb, var(--corner-node) 86%, var(--text));
    stroke-width: 2.3;
  }

  .genie-corner-svg[data-mode='thinking'] .genie-corner-edge,
  .genie-corner-svg[data-mode='repairing'] .genie-corner-edge,
  .genie-corner-svg[data-mode='rendering'] .genie-corner-edge {
    animation: genieCornerSolve calc(1.1s / var(--corner-motion, 1)) steps(2, end) infinite;
  }

  .genie-corner-svg[data-mode='speaking'] .genie-corner-face-lines path {
    animation: genieCornerSpeak 0.42s steps(2, end) infinite;
  }

  .genie-corner-svg[data-mode='error'] .genie-corner-glyph {
    animation:
      genieCornerHover 1.9s ease-in-out infinite,
      genieCornerError 0.34s steps(2, end) infinite;
  }

  @keyframes genieCornerHover {
    0%,
    100% {
      transform: translateY(-1px) rotate(-1deg);
    }
    50% {
      transform: translateY(3px) rotate(1deg);
    }
  }

  @keyframes genieCornerSolve {
    0%,
    100% {
      opacity: 0.42;
    }
    50% {
      opacity: 0.82;
    }
  }

  @keyframes genieCornerSpeak {
    0%,
    100% {
      transform: translateY(0);
    }
    50% {
      transform: translateY(1.6px);
    }
  }

  @keyframes genieCornerError {
    0%,
    100% {
      translate: -1px 0;
    }
    50% {
      translate: 1px 0;
    }
  }

  .genie-bubble {
    position: absolute;
    left: 126px;
    top: 6px;
    width: min(380px, max(248px, calc(100vw - var(--genie-safe-right, 360px) - 188px)));
    max-width: min(380px, max(248px, calc(100vw - var(--genie-safe-right, 360px) - 188px)));
    min-height: 74px;
    max-height: min(34vh, 240px);
    padding: 12px 72px 12px 14px;
    border: 2px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.74rem;
    line-height: 1.42;
    text-transform: none;
    letter-spacing: 0.01em;
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--bg-300) 85%, transparent), var(--shadow);
    backdrop-filter: blur(9px);
    pointer-events: auto;
    -webkit-user-select: text !important;
    user-select: text !important;
    overflow-y: auto;
  }

  .genie-bubble--compact {
    width: min(340px, max(236px, calc(100vw - var(--genie-safe-right, 360px) - 188px)));
    max-width: min(340px, max(236px, calc(100vw - var(--genie-safe-right, 360px) - 188px)));
    min-height: 66px;
    max-height: min(24vh, 176px);
    padding: 10px 64px 10px 12px;
    font-size: 0.72rem;
    line-height: 1.38;
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
    max-width: 100%;
  }

  .bubble-header {
    display: flex;
    align-items: flex-start;
    min-height: 16px;
    margin-bottom: 6px;
    min-width: 0;
  }

  .bubble-meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .bubble-badge,
  .bubble-context {
    min-width: 0;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 72%, transparent);
    padding: 2px 6px;
    font-size: 0.56rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .bubble-badge {
    color: var(--secondary);
    border-color: color-mix(in srgb, var(--secondary) 54%, var(--bg-300));
  }

  .bubble-context {
    color: var(--text-dim);
    max-width: 132px;
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
    color: var(--secondary);
    letter-spacing: 0.06em;
    font-size: 0.64rem;
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
      left: 14px;
      top: 126px;
      width: min(calc(100vw - 28px), 320px);
      max-width: min(calc(100vw - 28px), 320px);
      min-height: 72px;
      max-height: min(32vh, 220px);
      font-size: 0.72rem;
      line-height: 1.4;
    }

    .genie-bubble--compact {
      min-height: 64px;
      max-height: min(24vh, 160px);
    }
  }
</style>
