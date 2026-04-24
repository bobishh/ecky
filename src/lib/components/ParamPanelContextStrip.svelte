<script lang="ts">
  import type { ControlView, ControlViewSource } from '../types/domain';
  import type { MaterializedSemanticView } from '../modelRuntime/semanticControls';

  let {
    controlViews,
    activeControlViewId,
    activeSemanticView = null,
    onSelectControlView,
    onOpenCreateViewComposer,
    onOpenPrimitiveComposer,
    onOpenAdvisoryComposer,
    onOpenRelationComposer,
    onOpenEditViewComposer,
    onDeleteManualView,
    shouldShowSemanticSource,
    semanticSourceLabel,
  }: {
    controlViews: ControlView[];
    activeControlViewId: string | null;
    activeSemanticView?: MaterializedSemanticView | null;
    onSelectControlView?: (viewId: string) => void;
    onOpenCreateViewComposer?: () => void;
    onOpenPrimitiveComposer?: () => void;
    onOpenAdvisoryComposer?: () => void;
    onOpenRelationComposer?: () => void;
    onOpenEditViewComposer?: (view: MaterializedSemanticView) => void;
    onDeleteManualView?: (viewId: string) => void;
    shouldShowSemanticSource?: (source: ControlViewSource | undefined) => boolean;
    semanticSourceLabel?: (source: ControlViewSource | undefined) => string;
  } = $props();
</script>

<div class="part-strip">
  <div class="context-strip-head">
    <div class="section-label">CONTEXTS</div>
    <div class="context-strip-actions">
      <button class="btn btn-xs btn-ghost" onclick={() => onOpenCreateViewComposer?.()}>
        + VIEW
      </button>
      <button class="btn btn-xs btn-ghost" onclick={() => onOpenPrimitiveComposer?.()}>
        + KNOB
      </button>
      <button class="btn btn-xs btn-ghost" onclick={() => onOpenAdvisoryComposer?.()}>
        + RULE
      </button>
      <button class="btn btn-xs btn-ghost" onclick={() => onOpenRelationComposer?.()}>
        + LINK
      </button>
      {#if activeSemanticView?.source === 'manual'}
        <button class="btn btn-xs btn-ghost" onclick={() => onOpenEditViewComposer?.(activeSemanticView)}>
          EDIT
        </button>
        <button class="btn btn-xs btn-ghost" onclick={() => onDeleteManualView?.(activeSemanticView.viewId)}>
          DELETE
        </button>
      {/if}
    </div>
  </div>
  <div class="part-strip-list">
    {#if controlViews.length > 0}
      {#each controlViews as view}
        <button
          class="view-chip"
          class:view-chip-active={view.viewId === activeControlViewId}
          onclick={() => onSelectControlView?.(view.viewId)}
        >
          <span>{view.label}</span>
          {#if shouldShowSemanticSource?.(view.source)}
            <span class="semantic-source-badge">{semanticSourceLabel?.(view.source)}</span>
          {/if}
        </button>
      {/each}
    {:else}
      <div class="no-params">
        No views yet. Create one to group raw controls into semantic contexts.
      </div>
    {/if}
  </div>
</div>

<style>
  .part-strip {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .context-strip-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .context-strip-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .part-strip-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .view-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text-dim);
    font-size: 0.64rem;
    font-weight: 700;
    cursor: pointer;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .view-chip-active {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-200));
    color: var(--text);
  }

  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .semantic-source-badge {
    padding: 1px 5px;
    border: 1px solid color-mix(in srgb, var(--primary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-200));
    color: var(--primary);
    font-family: var(--font-mono);
    font-size: 0.52rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    flex-shrink: 0;
  }

  .no-params {
    color: var(--text-dim);
    font-size: 0.74rem;
    line-height: 1.45;
  }
</style>
