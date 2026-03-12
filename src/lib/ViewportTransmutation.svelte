<script lang="ts">
  type ViewportBusyPhase =
    | 'generating'
    | 'repairing'
    | 'rendering'
    | 'committing'
    | null;

  let {
    phase = 'generating',
    text = null,
  }: {
    phase?: ViewportBusyPhase;
    text?: string | null;
  } = $props();

  type Point = {
    x: number;
    y: number;
  };

  type Spark = {
    id: number;
    x: number;
    y: number;
    delay: number;
    duration: number;
    scale: number;
    drift: number;
  };

  function polarPoint(index: number, total: number, radius: number): Point {
    const angle = -Math.PI / 2 + (Math.PI * 2 * index) / total;
    return {
      x: 100 + Math.cos(angle) * radius,
      y: 100 + Math.sin(angle) * radius,
    };
  }

  const outerNodes = Array.from({ length: 8 }, (_, index) => polarPoint(index, 8, 70));
  const innerNodes = Array.from({ length: 6 }, (_, index) => polarPoint(index, 6, 40));
  const latticeLines = outerNodes.map((point, index) => ({
    from: point,
    to: outerNodes[(index + 3) % outerNodes.length],
  }));
  const innerLines = innerNodes.map((point, index) => ({
    from: point,
    to: innerNodes[(index + 2) % innerNodes.length],
  }));
  const sparks: Spark[] = Array.from({ length: 18 }, (_, index) => ({
    id: index,
    x: 8 + ((index * 17) % 84),
    y: 12 + ((index * 29) % 76),
    delay: (index % 6) * 0.35,
    duration: 2.8 + (index % 5) * 0.55,
    scale: 0.7 + (index % 4) * 0.24,
    drift: 12 + (index % 7) * 5,
  }));

  const phaseLabel = $derived.by(() => {
    switch (phase) {
      case 'repairing':
        return 'REWEAVING';
      case 'rendering':
        return 'SOLIDIFYING';
      case 'committing':
        return 'SEALING';
      case 'generating':
      default:
        return 'SUMMONING';
    }
  });

  const accessibilityLabel = $derived.by(() => {
    const trimmed = `${text ?? ''}`.trim();
    if (trimmed) return trimmed;
    switch (phase) {
      case 'repairing':
        return 'Restitching the geometry lattice.';
      case 'rendering':
        return 'Turning the spell into stable solids.';
      case 'committing':
        return 'Sealing the artifact into the thread.';
      case 'generating':
      default:
        return 'Preparing the next transformation.';
    }
  });

  const outerPolygon = outerNodes.map((point) => `${point.x},${point.y}`).join(' ');
  const innerPolygon = innerNodes.map((point) => `${point.x},${point.y}`).join(' ');
</script>

<div
  class="viewport-transmutation"
  data-phase={phase ?? 'generating'}
  aria-live="polite"
  aria-busy="true"
>
  <div class="viewport-transmutation__backdrop"></div>
  <div class="viewport-transmutation__atmosphere viewport-transmutation__atmosphere-a"></div>
  <div class="viewport-transmutation__atmosphere viewport-transmutation__atmosphere-b"></div>
  <div class="viewport-transmutation__sweep viewport-transmutation__sweep-a"></div>
  <div class="viewport-transmutation__sweep viewport-transmutation__sweep-b"></div>
  <div class="viewport-transmutation__flash viewport-transmutation__flash-a"></div>
  <div class="viewport-transmutation__flash viewport-transmutation__flash-b"></div>

  {#each sparks as spark}
    <span
      class="viewport-transmutation__spark"
      style={`left: ${spark.x}%; top: ${spark.y}%; --spark-delay: ${spark.delay}s; --spark-duration: ${spark.duration}s; --spark-size: ${4 * spark.scale}px; --spark-drift: ${spark.drift}px;`}
    ></span>
  {/each}

  <div class="viewport-transmutation__center">
    <svg
      class="viewport-transmutation__sigil"
      viewBox="0 0 200 200"
      role="img"
      aria-label={accessibilityLabel}
    >
      <circle class="ring ring-outer" cx="100" cy="100" r="82"></circle>
      <circle class="ring ring-mid" cx="100" cy="100" r="60"></circle>
      <circle class="ring ring-inner" cx="100" cy="100" r="28"></circle>
      <polygon class="mesh mesh-outer" points={outerPolygon}></polygon>
      <polygon class="mesh mesh-inner" points={innerPolygon}></polygon>

      {#each latticeLines as line}
        <line
          class="lattice lattice-outer"
          x1={line.from.x}
          y1={line.from.y}
          x2={line.to.x}
          y2={line.to.y}
        ></line>
      {/each}

      {#each innerLines as line}
        <line
          class="lattice lattice-inner"
          x1={line.from.x}
          y1={line.from.y}
          x2={line.to.x}
          y2={line.to.y}
        ></line>
      {/each}

      {#each outerNodes as point}
        <circle class="node node-outer" cx={point.x} cy={point.y} r="3.4"></circle>
      {/each}

      {#each innerNodes as point}
        <circle class="node node-inner" cx={point.x} cy={point.y} r="2.4"></circle>
      {/each}

      <circle class="core-ring" cx="100" cy="100" r="14"></circle>
      <circle class="core" cx="100" cy="100" r="7"></circle>
    </svg>

    <div class="viewport-transmutation__halo"></div>
  </div>
</div>

<style>
  .viewport-transmutation {
    --tone-main: color-mix(in srgb, var(--green) 82%, var(--primary) 18%);
    --tone-accent: color-mix(in srgb, var(--secondary) 68%, var(--green) 32%);
    --tone-core: color-mix(in srgb, var(--green) 44%, white 56%);
    --tone-dim: color-mix(in srgb, var(--tone-main) 28%, transparent);
    position: absolute;
    inset: 0;
    z-index: 6;
    overflow: hidden;
    pointer-events: auto;
    cursor: progress;
    background:
      radial-gradient(circle at 50% 50%, color-mix(in srgb, var(--tone-main) 16%, transparent) 0%, transparent 42%),
      linear-gradient(180deg, color-mix(in srgb, var(--bg) 92%, #020406 8%) 0%, color-mix(in srgb, var(--bg-100) 96%, #000 4%) 100%);
  }

  .viewport-transmutation[data-phase='repairing'] {
    --tone-main: color-mix(in srgb, var(--secondary) 86%, #ff9f4f 14%);
    --tone-accent: color-mix(in srgb, var(--primary) 72%, #ffe0a8 28%);
    --tone-core: color-mix(in srgb, var(--secondary) 58%, #fff2d4 42%);
  }

  .viewport-transmutation[data-phase='rendering'],
  .viewport-transmutation[data-phase='committing'] {
    --tone-main: color-mix(in srgb, #7de8ff 72%, #0ab7ff 28%);
    --tone-accent: color-mix(in srgb, #d8f8ff 44%, var(--secondary) 56%);
    --tone-core: color-mix(in srgb, #c7fbff 68%, white 32%);
  }

  .viewport-transmutation__backdrop,
  .viewport-transmutation__atmosphere,
  .viewport-transmutation__sweep,
  .viewport-transmutation__flash {
    position: absolute;
    inset: 0;
  }

  .viewport-transmutation__backdrop {
    background:
      radial-gradient(circle at 50% 42%, color-mix(in srgb, var(--tone-main) 28%, transparent) 0%, transparent 34%),
      radial-gradient(circle at 50% 68%, color-mix(in srgb, var(--tone-accent) 12%, transparent) 0%, transparent 42%);
  }

  .viewport-transmutation__atmosphere {
    mix-blend-mode: screen;
    opacity: 0.65;
  }

  .viewport-transmutation__atmosphere-a {
    background:
      radial-gradient(circle at 24% 28%, color-mix(in srgb, var(--tone-main) 18%, transparent) 0%, transparent 24%),
      radial-gradient(circle at 76% 34%, color-mix(in srgb, var(--tone-accent) 16%, transparent) 0%, transparent 22%),
      radial-gradient(circle at 52% 80%, color-mix(in srgb, var(--tone-main) 14%, transparent) 0%, transparent 26%);
    animation: transmutation-fog-a 12s ease-in-out infinite alternate;
  }

  .viewport-transmutation__atmosphere-b {
    background:
      linear-gradient(
        120deg,
        transparent 0%,
        color-mix(in srgb, var(--tone-main) 7%, transparent) 42%,
        transparent 68%
      );
    filter: blur(18px);
    animation: transmutation-fog-b 9s ease-in-out infinite alternate;
  }

  .viewport-transmutation__sweep {
    opacity: 0.55;
    mix-blend-mode: screen;
  }

  .viewport-transmutation__sweep-a {
    background: linear-gradient(
      90deg,
      transparent 0%,
      color-mix(in srgb, var(--tone-main) 22%, transparent) 48%,
      transparent 100%
    );
    transform: translateX(-120%) skewX(-24deg);
    animation: transmutation-sweep-a 6.4s linear infinite;
  }

  .viewport-transmutation__sweep-b {
    background: linear-gradient(
      180deg,
      transparent 0%,
      color-mix(in srgb, var(--tone-accent) 18%, transparent) 50%,
      transparent 100%
    );
    transform: translateY(120%) skewY(10deg);
    animation: transmutation-sweep-b 8.8s linear infinite;
  }

  .viewport-transmutation__flash {
    opacity: 0;
    mix-blend-mode: screen;
  }

  .viewport-transmutation__flash-a {
    background: radial-gradient(circle at 50% 46%, color-mix(in srgb, var(--tone-core) 14%, transparent) 0%, transparent 26%);
    animation: transmutation-flash-a 5.2s ease-in-out infinite;
  }

  .viewport-transmutation__flash-b {
    background: radial-gradient(circle at 50% 46%, color-mix(in srgb, white 10%, transparent) 0%, transparent 18%);
    animation: transmutation-flash-b 7.4s ease-in-out infinite;
  }

  .viewport-transmutation__spark {
    position: absolute;
    width: var(--spark-size);
    height: var(--spark-size);
    border-radius: 999px;
    background: var(--tone-core);
    box-shadow:
      0 0 10px color-mix(in srgb, var(--tone-main) 42%, transparent),
      0 0 20px color-mix(in srgb, var(--tone-accent) 18%, transparent);
    opacity: 0.18;
    animation: transmutation-spark var(--spark-duration) ease-in-out infinite;
    animation-delay: var(--spark-delay);
  }

  .viewport-transmutation__center {
    position: absolute;
    left: 50%;
    top: 47%;
    width: min(48vmin, 360px);
    aspect-ratio: 1;
    transform: translate(-50%, -50%);
    display: grid;
    place-items: center;
  }

  .viewport-transmutation__halo {
    position: absolute;
    width: 66%;
    aspect-ratio: 1;
    border-radius: 999px;
    background:
      radial-gradient(circle, color-mix(in srgb, var(--tone-main) 24%, transparent) 0%, transparent 64%);
    filter: blur(18px);
    animation: transmutation-halo 4.4s ease-in-out infinite;
  }

  .viewport-transmutation__sigil {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: visible;
    filter:
      drop-shadow(0 0 12px color-mix(in srgb, var(--tone-main) 26%, transparent))
      drop-shadow(0 0 28px color-mix(in srgb, var(--tone-accent) 12%, transparent));
    animation:
      transmutation-spin 18s linear infinite,
      transmutation-breathe 3.8s ease-in-out infinite;
  }

  .viewport-transmutation[data-phase='repairing'] .viewport-transmutation__sigil {
    animation-duration: 20s, 2.7s;
  }

  .viewport-transmutation[data-phase='rendering'] .viewport-transmutation__sigil,
  .viewport-transmutation[data-phase='committing'] .viewport-transmutation__sigil {
    animation-duration: 22s, 4.2s;
  }

  .ring,
  .mesh,
  .lattice,
  .node,
  .core-ring,
  .core {
    fill: none;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .ring {
    stroke: color-mix(in srgb, var(--tone-main) 62%, var(--tone-accent) 38%);
    stroke-width: 1.3;
    opacity: 0.82;
  }

  .ring-mid {
    stroke-dasharray: 8 10;
    opacity: 0.72;
  }

  .ring-inner {
    stroke-dasharray: 5 6;
    opacity: 0.88;
  }

  .mesh {
    stroke: color-mix(in srgb, var(--tone-accent) 56%, transparent);
    stroke-width: 0.9;
    fill: color-mix(in srgb, var(--tone-main) 8%, transparent);
    opacity: 0.62;
  }

  .lattice {
    stroke: color-mix(in srgb, var(--tone-main) 34%, transparent);
    stroke-width: 0.8;
    opacity: 0.58;
  }

  .node {
    fill: var(--tone-core);
    stroke: color-mix(in srgb, white 22%, transparent);
    stroke-width: 0.6;
    opacity: 0.92;
  }

  .node-inner {
    opacity: 0.76;
  }

  .core-ring {
    stroke: color-mix(in srgb, var(--tone-core) 68%, transparent);
    stroke-width: 1.2;
    opacity: 0.78;
  }

  .core {
    fill: var(--tone-core);
    stroke: color-mix(in srgb, white 24%, transparent);
    stroke-width: 0.8;
    filter: drop-shadow(0 0 10px color-mix(in srgb, var(--tone-core) 42%, transparent));
  }

  @keyframes transmutation-spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes transmutation-breathe {
    0%,
    100% {
      transform: scale(0.98);
      opacity: 0.78;
    }
    50% {
      transform: scale(1.02);
      opacity: 1;
    }
  }

  @keyframes transmutation-halo {
    0%,
    100% {
      transform: scale(0.92);
      opacity: 0.52;
    }
    50% {
      transform: scale(1.08);
      opacity: 0.84;
    }
  }

  @keyframes transmutation-fog-a {
    from {
      transform: translate3d(-2%, -1%, 0) scale(1);
      opacity: 0.46;
    }
    to {
      transform: translate3d(2%, 1.5%, 0) scale(1.08);
      opacity: 0.72;
    }
  }

  @keyframes transmutation-fog-b {
    from {
      transform: translate3d(-4%, 0, 0);
      opacity: 0.2;
    }
    to {
      transform: translate3d(3%, 3%, 0);
      opacity: 0.44;
    }
  }

  @keyframes transmutation-sweep-a {
    0% {
      transform: translateX(-120%) skewX(-24deg);
      opacity: 0;
    }
    18% {
      opacity: 0.24;
    }
    50% {
      opacity: 0.48;
    }
    100% {
      transform: translateX(120%) skewX(-24deg);
      opacity: 0;
    }
  }

  @keyframes transmutation-sweep-b {
    0% {
      transform: translateY(120%) skewY(10deg);
      opacity: 0;
    }
    28% {
      opacity: 0.18;
    }
    58% {
      opacity: 0.36;
    }
    100% {
      transform: translateY(-120%) skewY(10deg);
      opacity: 0;
    }
  }

  @keyframes transmutation-flash-a {
    0%,
    58%,
    100% {
      opacity: 0;
    }
    62% {
      opacity: 0.1;
    }
    67% {
      opacity: 0.22;
    }
    72% {
      opacity: 0;
    }
  }

  @keyframes transmutation-flash-b {
    0%,
    74%,
    100% {
      opacity: 0;
    }
    77% {
      opacity: 0.08;
    }
    81% {
      opacity: 0.14;
    }
    86% {
      opacity: 0;
    }
  }

  @keyframes transmutation-spark {
    0%,
    100% {
      opacity: 0.08;
      transform: translate3d(0, 0, 0) scale(0.82);
    }
    30% {
      opacity: 0.72;
      transform: translate3d(calc(var(--spark-drift) * -0.35), calc(var(--spark-drift) * -0.55), 0)
        scale(1);
    }
    68% {
      opacity: 0.26;
      transform: translate3d(calc(var(--spark-drift) * 0.55), calc(var(--spark-drift) * -0.2), 0)
        scale(1.14);
    }
  }

  @media (max-width: 900px) {
    .viewport-transmutation__center {
      width: min(62vmin, 320px);
      top: 44%;
    }
  }
</style>
