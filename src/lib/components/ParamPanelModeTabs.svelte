<script module lang="ts">
  export type ParamPanelTab = 'params' | 'assembly' | 'mesh' | 'edit' | 'litho' | 'newParams' | 'views';
</script>

<script lang="ts">
  let {
    activeTab = 'params',
    macroCode = '',
    assemblyEnabled = false,
    assemblyHasOverride = false,
    viewsEnabled = false,
    onActiveTabChange,
    onShowCode,
    onOpenInEditor,
  }: {
    activeTab?: ParamPanelTab;
    macroCode?: string;
    assemblyEnabled?: boolean;
    assemblyHasOverride?: boolean;
    viewsEnabled?: boolean;
    onActiveTabChange?: (tab: ParamPanelTab) => void;
    onShowCode?: () => void;
    onOpenInEditor?: () => void;
  } = $props();
</script>

<div class="panel-mode-tabs" role="group" aria-label="Parameter workspaces">
  <button
    class="panel-mode-tab"
    class:panel-mode-tab-active={activeTab === 'params'}
    aria-pressed={activeTab === 'params'}
    onclick={() => onActiveTabChange?.('params')}
  >
    PARAMETERS
  </button>
  {#if assemblyEnabled}
    <button
      class="panel-mode-tab"
      class:panel-mode-tab-active={activeTab === 'assembly'}
      class:panel-mode-tab-override={assemblyHasOverride && activeTab !== 'assembly'}
      aria-pressed={activeTab === 'assembly'}
      onclick={() => onActiveTabChange?.('assembly')}
    >
      ASSEMBLY
    </button>
  {/if}
  <button
    class="panel-mode-tab"
    class:panel-mode-tab-active={activeTab === 'mesh'}
    aria-pressed={activeTab === 'mesh'}
    onclick={() => onActiveTabChange?.('mesh')}
  >
    MESH
  </button>
  <button
    class="panel-mode-tab"
    class:panel-mode-tab-active={activeTab === 'edit'}
    aria-pressed={activeTab === 'edit'}
    onclick={() => onActiveTabChange?.('edit')}
  >
    ✏️ EDIT CONTROLS
  </button>
  {#if macroCode}
    <button
      class="panel-mode-tab"
      class:panel-mode-tab-active={activeTab === 'newParams'}
      aria-pressed={activeTab === 'newParams'}
      aria-label="new params"
      onclick={() => onActiveTabChange?.('newParams')}
    >
      NEW PARAMS
    </button>
  {/if}
  <button
    class="panel-mode-tab"
    class:panel-mode-tab-active={activeTab === 'litho'}
    aria-pressed={activeTab === 'litho'}
    onclick={() => onActiveTabChange?.('litho')}
  >
    LITHO
  </button>
  {#if viewsEnabled}
    <button
      class="panel-mode-tab"
      class:panel-mode-tab-active={activeTab === 'views'}
      aria-pressed={activeTab === 'views'}
      onclick={() => onActiveTabChange?.('views')}
    >
      VIEWS
    </button>
  {/if}
  {#if macroCode && onShowCode}
    <button class="panel-mode-tab panel-code-btn" onclick={onShowCode} title="View macro code">
      CODE
    </button>
  {/if}
  {#if macroCode && onOpenInEditor}
    <button
      class="panel-mode-tab panel-code-btn panel-file-btn"
      onclick={onOpenInEditor}
      title="Open model.ecky in your editor; saved edits come back as new versions"
    >
      OPEN FILE
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

  .panel-mode-tab-override {
    border-color: var(--primary);
    color: var(--primary);
  }

  .panel-code-btn {
    margin-left: auto;
    border-color: color-mix(in srgb, var(--secondary) 55%, var(--bg-300));
    color: var(--secondary);
  }
</style>
