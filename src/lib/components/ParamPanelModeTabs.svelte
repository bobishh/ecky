<script lang="ts">
  import { cycleTopologyMode, topologyModeLabel, type TopologyMode } from '../viewerDisplayMode';

  let {
    activeTab = 'views',
    outlineEnabled = true,
    topologyMode = 'mesh',
    selectionMode = 'orbit',
    macroCode = '',
    onActiveTabChange,
    onShowCode,
    onViewerDisplayChange,
    onViewerSelectionModeChange,
  }: {
    activeTab?: 'views' | 'raw' | 'litho';
    outlineEnabled?: boolean;
    topologyMode?: TopologyMode;
    selectionMode?: 'orbit' | 'select' | 'measure';
    macroCode?: string;
    onActiveTabChange?: (tab: 'views' | 'raw' | 'litho') => void;
    onShowCode?: () => void;
    onViewerDisplayChange?: (display: { outlineEnabled: boolean; topologyMode: TopologyMode }) => void;
    onViewerSelectionModeChange?: (mode: 'orbit' | 'select' | 'measure') => void;
  } = $props();
</script>

<div class="panel-mode-tabs">
  <button
    class="panel-mode-tab"
    class:panel-mode-tab-active={activeTab === 'views'}
    onclick={() => onActiveTabChange?.('views')}
  >
    VIEWS
  </button>
  <button
    class="panel-mode-tab"
    class:panel-mode-tab-active={activeTab === 'raw'}
    onclick={() => onActiveTabChange?.('raw')}
  >
    RAW
  </button>
  <button
    class="panel-mode-tab"
    class:panel-mode-tab-active={activeTab === 'litho'}
    onclick={() => onActiveTabChange?.('litho')}
  >
    LITHO
  </button>
  <button
    class="panel-mode-tab panel-mode-tab-compact"
    class:panel-mode-tab-active={outlineEnabled}
    onclick={() => onViewerDisplayChange?.({ outlineEnabled: !outlineEnabled, topologyMode })}
    title="Toggle part outlines in the viewport"
  >
    OUTLINE
  </button>
  <button
    class="panel-mode-tab panel-mode-tab-compact"
    class:panel-mode-tab-active={topologyMode !== 'off'}
    onclick={() => onViewerDisplayChange?.({ outlineEnabled, topologyMode: cycleTopologyMode(topologyMode) })}
    title="Cycle topology overlay: off, feature edges, mesh wireframe"
  >
    {topologyModeLabel(topologyMode)}
  </button>
  <button
    class="panel-mode-tab panel-mode-tab-compact"
    class:panel-mode-tab-active={selectionMode === 'orbit'}
    onclick={() => onViewerSelectionModeChange?.('orbit')}
    title="Camera orbit mode"
  >
    ORBIT
  </button>
  <button
    class="panel-mode-tab panel-mode-tab-compact"
    class:panel-mode-tab-active={selectionMode === 'select'}
    onclick={() => onViewerSelectionModeChange?.('select')}
    title="Lock camera and click model targets for parameter focus"
  >
    SELECT
  </button>
  <button
    class="panel-mode-tab panel-mode-tab-compact"
    class:panel-mode-tab-active={selectionMode === 'measure'}
    onclick={() => onViewerSelectionModeChange?.('measure')}
    title="Measure mode keeps parameter focus unchanged on viewport click/drag"
  >
    MEASURE
  </button>
  {#if macroCode && onShowCode}
    <button class="panel-mode-tab panel-code-btn" onclick={onShowCode} title="View macro code">
      CODE
    </button>
  {/if}
</div>

<style>
  .panel-mode-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    overflow: visible;
    align-items: stretch;
    min-width: 0;
  }

  .panel-mode-tab {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 100%;
    padding: 5px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text-dim);
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    line-height: 1.3;
    text-align: left;
    cursor: pointer;
  }

  .panel-mode-tab-active {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-200));
    color: var(--text);
  }

  .panel-mode-tab-compact {
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .panel-code-btn {
    margin-left: auto;
    border-color: color-mix(in srgb, var(--secondary) 55%, var(--bg-300));
    color: var(--secondary);
  }
</style>
