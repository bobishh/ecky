<script lang="ts">
  import type { AssemblyNode } from '../types/domain';
  import { collectNodePartIds } from '../assemblyTree';

  let {
    nodes = [],
    selectedPartId = null,
    isolatedPartId = null,
    hiddenPartIds = [],
    ghostIsolatedSiblings = true,
    onSelectPart,
    onToggleIsolatePart,
    onToggleHidePart,
    onToggleGhostIsolatedSiblings,
    onResetVisibility,
  }: {
    nodes?: AssemblyNode[];
    selectedPartId?: string | null;
    isolatedPartId?: string | null;
    hiddenPartIds?: string[];
    ghostIsolatedSiblings?: boolean;
    onSelectPart?: (partId: string) => void;
    onToggleIsolatePart?: (partId: string) => void;
    onToggleHidePart?: (partIds: string[]) => void;
    onToggleGhostIsolatedSiblings?: () => void;
    onResetVisibility?: () => void;
  } = $props();

  let collapsedIds = $state<Record<string, boolean>>({});
  const hiddenSet = $derived(new Set(hiddenPartIds));
  const hasVisibilityOverride = $derived(Boolean(isolatedPartId) || hiddenPartIds.length > 0);

  function toggleCollapse(nodeId: string) {
    collapsedIds = { ...collapsedIds, [nodeId]: !collapsedIds[nodeId] };
  }

  function isNodeHidden(node: AssemblyNode): boolean {
    const ids = collectNodePartIds(node);
    return ids.length > 0 && ids.every((id) => hiddenSet.has(id));
  }
</script>

{#snippet renderNode(node: AssemblyNode, depth: number)}
  {@const isGroup = (node.children?.length ?? 0) > 0}
  {@const isCollapsed = Boolean(collapsedIds[node.id])}
  {@const partKey = node.partKey ?? null}
  {@const isSelected = Boolean(partKey && partKey === selectedPartId)}
  {@const isIsolated = Boolean(partKey && partKey === isolatedPartId)}
  {@const isHidden = isNodeHidden(node)}
  {@const paramCount = node.parameterKeys?.length ?? 0}
  {@const groupPartIds = collectNodePartIds(node)}

  <div
    class="assembly-row"
    class:assembly-row-selected={isSelected}
    class:assembly-row-isolated={isIsolated}
    class:assembly-row-hidden={isHidden}
    style={`padding-left: ${6 + depth * 12}px;`}
  >
    {#if isGroup}
      <button
        type="button"
        class="assembly-caret"
        aria-label={isCollapsed ? `Expand ${node.name}` : `Collapse ${node.name}`}
        onclick={() => toggleCollapse(node.id)}
      >
        {isCollapsed ? '▸' : '▾'}
      </button>
    {:else}
      <span class="assembly-caret-spacer" aria-hidden="true"></span>
    {/if}

    <button
      type="button"
      class="assembly-label"
      title={partKey ? `Select ${node.name}` : `${node.name} (${groupPartIds.length} parts)`}
      onclick={() => {
        if (partKey) {
          onSelectPart?.(partKey);
        } else if (isGroup) {
          toggleCollapse(node.id);
        }
      }}
    >
      <span class="assembly-name">{node.name.toLowerCase()}</span>
      {#if paramCount > 0}
        <span class="assembly-badge">{paramCount}</span>
      {/if}
    </button>

    <div class="assembly-actions">
      {#if partKey}
        <button
          type="button"
          class="assembly-action"
          class:assembly-action-active={isIsolated}
          title={isIsolated ? 'Exit Solo isolation' : `Solo isolate ${node.name}`}
          aria-label={isIsolated ? `Exit isolate ${node.name}` : `Isolate ${node.name}`}
          onclick={() => onToggleIsolatePart?.(partKey)}
        >
          S
        </button>
      {/if}
      {#if groupPartIds.length > 0}
        <button
          type="button"
          class="assembly-action"
          class:assembly-action-muted={isHidden}
          title={isHidden ? `Show ${node.name}` : `Hide ${node.name}`}
          aria-label={isHidden ? `Show ${node.name}` : `Hide ${node.name}`}
          onclick={() => onToggleHidePart?.(groupPartIds)}
        >
          {isHidden ? '○' : '●'}
        </button>
      {/if}
    </div>
  </div>

  {#if isGroup && !isCollapsed}
    {#each node.children ?? [] as child (child.id)}
      {@render renderNode(child, depth + 1)}
    {/each}
  {/if}
{/snippet}

<div class="assembly-tree" aria-label="Assembly tree">
  <div class="assembly-header">
    <span class="section-label">ASSEMBLY TREE</span>
    <div class="assembly-header-controls">
      {#if isolatedPartId}
        <button
          type="button"
          class="assembly-header-btn"
          class:assembly-header-btn-active={ghostIsolatedSiblings}
          title="Toggle ghost silhouettes for unselected parts"
          onclick={() => onToggleGhostIsolatedSiblings?.()}
        >
          {ghostIsolatedSiblings ? 'GHOST' : 'HIDE'}
        </button>
      {/if}
      {#if hasVisibilityOverride}
        <button
          type="button"
          class="assembly-header-btn"
          title="Restore full assembly view"
          onclick={() => onResetVisibility?.()}
        >
          SHOW ALL
        </button>
      {/if}
    </div>
  </div>

  <div class="assembly-list">
    {#each nodes as node (node.id)}
      {@render renderNode(node, 0)}
    {/each}
  </div>
</div>

<style>
  .assembly-tree {
    border: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-200) 75%, transparent);
  }

  .assembly-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 5px 6px;
    border-bottom: 1px solid var(--border);
  }

  .section-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }

  .assembly-header-controls {
    display: flex;
    gap: 4px;
  }

  .assembly-header-btn {
    padding: 1px 5px;
    border: 1px solid var(--border);
    background: var(--bg-100);
    color: var(--text-dim);
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.05em;
    cursor: pointer;
  }

  .assembly-header-btn:hover,
  .assembly-header-btn-active {
    border-color: var(--primary);
    color: var(--primary);
  }

  .assembly-list {
    display: flex;
    flex-direction: column;
  }

  .assembly-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 6px 3px 6px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
  }

  .assembly-row:last-child {
    border-bottom: none;
  }

  .assembly-row-selected {
    background: color-mix(in srgb, var(--primary) 14%, transparent);
  }

  .assembly-row-isolated {
    box-shadow: inset 2px 0 0 var(--primary);
  }

  .assembly-row-hidden .assembly-name {
    opacity: 0.45;
    text-decoration: line-through;
  }

  .assembly-caret {
    width: 14px;
    height: 16px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: 10px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .assembly-caret-spacer {
    width: 14px;
    flex-shrink: 0;
  }

  .assembly-label {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 1px 2px;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }

  .assembly-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .assembly-badge {
    font-size: 9px;
    padding: 0 4px;
    border: 1px solid var(--border);
    color: var(--text-dim);
    flex-shrink: 0;
  }

  .assembly-actions {
    display: flex;
    gap: 3px;
    flex-shrink: 0;
  }

  .assembly-action {
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border);
    background: var(--bg-100);
    color: var(--text-dim);
    font-size: 9px;
    font-weight: 700;
    cursor: pointer;
  }

  .assembly-action:hover {
    border-color: var(--secondary);
    color: var(--text);
  }

  .assembly-action-active {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--primary) 22%, var(--bg-100));
    color: var(--primary);
  }

  .assembly-action-muted {
    color: var(--text-dim);
    opacity: 0.55;
  }
</style>
