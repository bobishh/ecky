<script lang="ts">
  import type { LivePaperSnapshot } from './livePaper';
  let { snapshot, selectedId, onSelect }: {
    snapshot: LivePaperSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  } = $props();
</script>

<g class="live-paper-geometry">
  {#each [...snapshot.accepted, ...snapshot.pending] as primitive (primitive.id)}
    {@const pending = snapshot.pending.some((item) => item.id === primitive.id)}
    <g
      class={['primitive', pending && 'pending', primitive.id === selectedId && 'selected']}
      role="button" tabindex="0" aria-label={`Select live ${primitive.kind} ${primitive.id}`}
      onclick={(event) => { event.stopPropagation(); onSelect(primitive.id); }}
      onkeydown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onSelect(primitive.id); } }}
    >
      {#if primitive.kind === 'point'}
        <circle data-testid={`live-primitive-${primitive.id}`} cx={primitive.positionMm[0]} cy={-primitive.positionMm[1]} r="0.8" class="point" />
      {:else if primitive.kind === 'line'}
        <line data-testid={`live-primitive-${primitive.id}`} x1={primitive.startMm[0]} y1={-primitive.startMm[1]} x2={primitive.endMm[0]} y2={-primitive.endMm[1]} />
      {:else if primitive.kind === 'circle'}
        <circle data-testid={`live-primitive-${primitive.id}`} cx={primitive.centerMm[0]} cy={-primitive.centerMm[1]} r={primitive.radiusMm} />
      {:else if primitive.closed}
        <polygon data-testid={`live-primitive-${primitive.id}`} points={primitive.pointsMm.map(([x, y]) => `${x},${-y}`).join(' ')} />
      {:else}
        <polyline data-testid={`live-primitive-${primitive.id}`} points={primitive.pointsMm.map(([x, y]) => `${x},${-y}`).join(' ')} />
      {/if}
    </g>
  {/each}
</g>

<style>
  .primitive { fill: none; stroke: var(--primary, #ad8158); stroke-width: 2; cursor: pointer; }
  .primitive :is(line, circle, polygon, polyline) { vector-effect: non-scaling-stroke; }
  .primitive .point { fill: var(--primary, #ad8158); }
  .pending { stroke: var(--secondary, #d1b391); stroke-dasharray: 5 4; }
  .selected { stroke: #f0d0a7; stroke-width: 3; }
</style>
