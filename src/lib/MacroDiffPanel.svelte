<script lang="ts">
  import type { MacroDiffPanelModel } from './macroDiffPanel';

  let {
    model,
  }: {
    model: MacroDiffPanelModel | null;
  } = $props();

  let expanded = $state(true);
</script>

{#if model}
  <div class="macro-diff" data-testid="last-macro-diff">
    <button
      type="button"
      class="macro-diff__head"
      onclick={() => (expanded = !expanded)}
      aria-expanded={expanded}
    >
      <span class="macro-diff__title">LAST MACRO DIFF</span>
      <span class="macro-diff__meta" data-testid="last-macro-diff-meta">
        <span>{model.actorLabel}</span>
        <span>{model.timeLabel}</span>
        <span>{model.oldSummary} → {model.newSummary}</span>
      </span>
      <span class="macro-diff__toggle">{expanded ? '▾' : '▸'}</span>
    </button>

    {#if expanded}
      <div class="macro-diff__body">
        <div class="macro-diff__summary" data-testid="last-macro-diff-summary">
          {model.summary}
        </div>
        {#if model.hasDiff}
          <div class="macro-diff__rows" data-testid="last-macro-diff-rows">
            {#each model.rows as row}
              <div class="macro-diff__row" data-kind={row.kind}>
                <span class="macro-diff__line-no">{row.oldLineNumber ?? ''}</span>
                <span class="macro-diff__line-no">{row.newLineNumber ?? ''}</span>
                <span class="macro-diff__marker">
                  {row.kind === 'insert' ? '+' : row.kind === 'delete' ? '−' : ' '}
                </span>
                <span class="macro-diff__text">
                  {row.kind === 'delete' ? row.oldText : row.newText}
                </span>
              </div>
            {/each}
          </div>
        {:else}
          <div class="macro-diff__empty">NO LINE CHANGES IN LAST MACRO EVENT</div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .macro-diff {
    border-top: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    max-height: 40%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .macro-diff__head {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 8px 12px;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    cursor: pointer;
    text-align: left;
  }

  .macro-diff__title {
    color: var(--secondary);
    font-size: 0.68rem;
    font-weight: 800;
    letter-spacing: 0.08em;
  }

  .macro-diff__meta {
    display: flex;
    gap: 10px;
    min-width: 0;
    overflow: hidden;
    color: var(--text-muted);
    font-size: 0.68rem;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .macro-diff__toggle {
    margin-left: auto;
    color: var(--text-muted);
    font-size: 0.7rem;
  }

  .macro-diff__body {
    min-height: 0;
    overflow: auto;
    padding: 0 12px 10px;
    display: grid;
    gap: 8px;
  }

  .macro-diff__summary {
    color: var(--text-muted);
    font-size: 0.72rem;
    line-height: 1.35;
  }

  .macro-diff__rows {
    border: 1px solid var(--bg-300);
    background: var(--bg);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    line-height: 1.5;
    overflow: auto;
  }

  .macro-diff__row {
    display: grid;
    grid-template-columns: 34px 34px 16px minmax(0, 1fr);
    gap: 4px;
    padding: 0 8px;
    white-space: pre;
  }

  .macro-diff__row[data-kind='insert'] {
    background: color-mix(in srgb, var(--green, #3fb950) 12%, transparent);
  }

  .macro-diff__row[data-kind='delete'] {
    background: color-mix(in srgb, var(--red, #f85149) 12%, transparent);
  }

  .macro-diff__line-no {
    color: var(--text-dim);
    text-align: right;
    user-select: none;
  }

  .macro-diff__marker {
    color: var(--text-muted);
    user-select: none;
  }

  .macro-diff__text {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .macro-diff__empty {
    color: var(--text-dim);
    font-size: 0.7rem;
    text-align: center;
    padding: 6px 0;
  }
</style>
