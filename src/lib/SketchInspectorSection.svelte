<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    ariaLabel,
    open = true,
    className = '',
    children,
    summaryExtra,
  }: {
    title: string;
    ariaLabel: string;
    open?: boolean;
    className?: string;
    children: Snippet;
    summaryExtra?: Snippet;
  } = $props();
</script>

<details class={`sketch-inspector-section ${className}`} aria-label={ariaLabel} {open}>
  <summary class="sketch-inspector-section__summary">
    <span>{title}</span>
    {#if summaryExtra}
      <span class="sketch-inspector-section__summary-extra">
        {@render summaryExtra()}
      </span>
    {/if}
  </summary>
  <div class="sketch-inspector-section__body">
    {@render children()}
  </div>
</details>

<style>
  .sketch-inspector-section {
    flex: 0 0 auto;
    min-height: 0;
    display: block;
    padding: 8px;
    border: 1px solid color-mix(in srgb, var(--secondary) 40%, var(--bg-300));
    color: var(--text);
    background: color-mix(in srgb, var(--bg-200) 84%, black 16%);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    line-height: 1.45;
    white-space: normal;
    overflow: hidden;
  }

  .sketch-inspector-section__summary {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    color: var(--secondary);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 0.62rem;
    letter-spacing: 0.08em;
    overflow: hidden;
  }

  .sketch-inspector-section__summary span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sketch-inspector-section__summary::-webkit-details-marker {
    color: var(--primary);
  }

  .sketch-inspector-section[open] .sketch-inspector-section__summary {
    margin-bottom: 6px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--bg-300);
  }

  .sketch-inspector-section__summary-extra {
    color: var(--secondary);
  }

  .sketch-inspector-section__body {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
  }
</style>
