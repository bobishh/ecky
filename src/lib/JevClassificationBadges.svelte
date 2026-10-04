<script lang="ts">
  import type { ClassificationResult } from './tauri/contracts';

  let { result }: { result: ClassificationResult } = $props();
  const order = ['answer', 'plan', 'clarify', 'inspect', 'modify'];
  const badges = $derived(order.flatMap((intent) => {
    const probability = result.actionProbabilities[intent];
    return typeof probability === 'number' && Number.isFinite(probability)
      ? [{ intent, percent: Math.round(probability * 100), selected: intent === result.intent }]
      : [];
  }));
</script>

{#if badges.length}
  <div class="jev-classification" aria-label="Jev classification">
    <span class="jev-classification__source">JEV</span>
    {#each badges as badge (badge.intent)}
      <span class="jev-classification__badge" class:jev-classification__badge--selected={badge.selected}>
        {badge.intent.toUpperCase()} {badge.percent}%
      </span>
    {/each}
  </div>
{/if}

<style>
  .jev-classification {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    max-width: 100%;
    overflow: hidden;
    margin-top: 8px;
    font-size: var(--ui-font-caption);
  }
  .jev-classification__source {
    color: var(--secondary);
    font-weight: 700;
    letter-spacing: 0.08em;
    margin-right: 2px;
  }
  .jev-classification__badge {
    border: 1px solid var(--bg-300);
    border-radius: 0;
    padding: 2px 4px;
    color: var(--text-dim);
    white-space: nowrap;
  }
  .jev-classification__badge--selected {
    border-color: var(--secondary);
    color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 12%, var(--bg-100));
  }
</style>
