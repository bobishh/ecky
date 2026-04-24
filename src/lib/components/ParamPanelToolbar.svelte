<script lang="ts">
  type SaveValuesState = 'idle' | 'saving' | 'saved';

  let {
    searchQuery = '',
    editing = false,
    applying = false,
    reading = false,
    saveValuesState = 'idle',
    liveApply = false,
    activeVersionId = null,
    onSearchQueryChange,
    onApplyChanges,
    onSaveValues,
    onStartEditing,
    onSaveFields,
    onCancelEditing,
    onReadFromMacro,
    onLiveApplyChange,
  }: {
    searchQuery?: string;
    editing?: boolean;
    applying?: boolean;
    reading?: boolean;
    saveValuesState?: SaveValuesState;
    liveApply?: boolean;
    activeVersionId?: string | null;
    onSearchQueryChange?: (value: string) => void;
    onApplyChanges?: () => void;
    onSaveValues?: () => void;
    onStartEditing?: () => void;
    onSaveFields?: () => void;
    onCancelEditing?: () => void;
    onReadFromMacro?: () => void;
    onLiveApplyChange?: (checked: boolean) => void;
  } = $props();
</script>

<div class="panel-toolbar">
  <div class="search-box">
    <input
      type="text"
      placeholder="Search controls..."
      value={searchQuery}
      oninput={(event) => onSearchQueryChange?.((event.currentTarget as HTMLInputElement).value)}
      class="search-input"
    />
    {#if searchQuery}
      <button class="clear-search" onclick={() => onSearchQueryChange?.('')}>✕</button>
    {/if}
  </div>
</div>

<div class="panel-actions">
  {#if !editing}
    <div class="live-apply-group">
      <label class="live-toggle" title="Update geometry immediately on every change">
        <input
          class="ui-checkbox"
          type="checkbox"
          checked={liveApply}
          onchange={(event) => onLiveApplyChange?.((event.currentTarget as HTMLInputElement).checked)}
        />
        <span>LIVE</span>
      </label>
      <button
        class="btn btn-xs btn-primary apply-btn"
        onclick={onApplyChanges}
        disabled={liveApply || applying}
      >
        {#if applying}
          APPLYING...
        {:else}
          APPLY
        {/if}
      </button>
      <button
        class="btn btn-xs btn-ghost"
        onclick={onSaveValues}
        disabled={!activeVersionId || saveValuesState === 'saving'}
        title={activeVersionId ? 'Persist current values as defaults for this version' : 'Generate first to persist defaults'}
      >
        {#if saveValuesState === 'saving'}
          SAVING...
        {:else if saveValuesState === 'saved'}
          SAVED
        {:else}
          SAVE VALUES
        {/if}
      </button>
    </div>
    <button class="btn btn-xs" onclick={onStartEditing} title="Edit controls">✏️ EDIT CONTROLS</button>
  {:else}
    <div class="edit-toolbar-left">
      <button class="btn btn-xs btn-primary" onclick={onSaveFields}>💾 SAVE</button>
      <button class="btn btn-xs btn-ghost" onclick={onCancelEditing}>✕ CANCEL</button>
    </div>
    <button class="btn btn-xs btn-secondary" onclick={onReadFromMacro} title="Auto-detect parameters from macro code" disabled={reading}>
      {#if reading}
        ⏳ READING...
      {:else}
        🔍 READ FROM MACRO
      {/if}
    </button>
  {/if}
</div>

<style>
  .panel-toolbar {
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-bottom: 1px solid var(--bg-300);
    padding-bottom: 10px;
    margin-bottom: 4px;
  }

  .search-box {
    position: relative;
    width: 100%;
  }

  .search-input {
    width: 100%;
    min-height: 42px;
    padding: 10px 36px 10px 12px;
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.86rem;
    font-weight: 600;
    line-height: 1.2;
    outline: none;
    transition:
      border-color 0.2s,
      background-color 0.2s;
  }

  .search-input:focus {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--bg-100) 88%, var(--primary) 12%);
  }

  .clear-search {
    position: absolute;
    right: 10px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.95rem;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .clear-search:hover {
    color: var(--text);
  }

  .panel-actions {
    display: flex;
    gap: 8px;
    justify-content: space-between;
    align-items: center;
  }

  .live-apply-group {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  .edit-toolbar-left {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }

  .live-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 0.6rem;
    font-weight: bold;
    color: var(--text-dim);
    cursor: pointer;
    user-select: none;
    padding: 2px 6px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .live-toggle:has(input:checked) {
    color: var(--secondary);
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 10%, var(--bg-200));
  }
</style>
