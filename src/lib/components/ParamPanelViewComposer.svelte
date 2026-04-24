<script lang="ts">
  import Dropdown from '../Dropdown.svelte';
  import type { ControlViewScope, PartBinding } from '../types/domain';

  export type ViewComposerPrimitive = {
    primitiveId: string;
    label: string;
    partLabels: string[];
  };

  let {
    mode,
    label,
    scope,
    partId,
    modelParts = [],
    visiblePrimitives = [],
    selectedPrimitiveIds = [],
    canSave,
    onLabelChange,
    onScopeChange,
    onPartIdChange,
    onTogglePrimitive,
    onCancel,
    onSave,
  }: {
    mode: 'create' | 'edit';
    label: string;
    scope: ControlViewScope;
    partId: string | null;
    modelParts?: PartBinding[];
    visiblePrimitives?: ViewComposerPrimitive[];
    selectedPrimitiveIds?: string[];
    canSave: boolean;
    onLabelChange?: (value: string) => void;
    onScopeChange?: (value: ControlViewScope) => void;
    onPartIdChange?: (value: string | null) => void;
    onTogglePrimitive?: (primitiveId: string, checked: boolean) => void;
    onCancel?: () => void;
    onSave?: () => void;
  } = $props();

  function getInputValue(event: Event): string {
    return (event.currentTarget as HTMLInputElement).value;
  }

  function getInputChecked(event: Event): boolean {
    return (event.currentTarget as HTMLInputElement).checked;
  }
</script>

<div class="view-composer">
  <div class="controls-head">
    <div class="section-label">{mode === 'edit' ? 'EDIT VIEW' : 'NEW VIEW'}</div>
  </div>
  <div class="composer-grid">
    <div class="composer-field">
      <label class="composer-label" for="composer-view-label">VIEW NAME</label>
      <input
        id="composer-view-label"
        class="input-mono composer-input"
        value={label}
        oninput={(event) => onLabelChange?.(getInputValue(event))}
        placeholder="Connector / Fit / Printability..."
      />
    </div>
    <div class="composer-field">
      <div class="composer-label">SCOPE</div>
      <Dropdown
        options={[
          { id: 'global', name: 'Global' },
          { id: 'part', name: 'Part' },
        ]}
        value={scope}
        onchange={(value) => onScopeChange?.(value === 'part' ? 'part' : 'global')}
      />
    </div>
    {#if scope === 'part'}
      <div class="composer-field">
        <div class="composer-label">PART</div>
        <Dropdown
          options={modelParts.map((part) => ({ id: part.partId, name: part.label }))}
          value={partId}
          onchange={(value) => onPartIdChange?.(typeof value === 'string' ? value : null)}
          placeholder="Choose part..."
        />
      </div>
    {/if}
  </div>
  <div class="composer-note">
    Build a reusable semantic context from existing meaningful controls.
  </div>
  <div class="composer-list">
    {#if visiblePrimitives.length > 0}
      {#each visiblePrimitives as primitive}
        <label class="primitive-picker">
          <input
            class="ui-checkbox"
            type="checkbox"
            checked={selectedPrimitiveIds.includes(primitive.primitiveId)}
            onchange={(event) => onTogglePrimitive?.(primitive.primitiveId, getInputChecked(event))}
          />
          <div class="primitive-picker__body">
            <div class="primitive-picker__label">{primitive.label}</div>
            {#if primitive.partLabels.length > 0}
              <div class="primitive-picker__meta">{primitive.partLabels.join(', ')}</div>
            {/if}
          </div>
        </label>
      {/each}
    {:else}
      <div class="no-params">No primitives are available for this scope yet.</div>
    {/if}
  </div>
  <div class="composer-actions">
    <button class="btn btn-xs btn-ghost" onclick={() => onCancel?.()}>CANCEL</button>
    <button class="btn btn-xs btn-primary" onclick={() => onSave?.()} disabled={!canSave}>
      {mode === 'edit' ? 'SAVE VIEW' : 'CREATE VIEW'}
    </button>
  </div>
</div>

<style>
  .view-composer {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-200) 88%, var(--secondary) 12%);
    overflow: hidden;
  }

  .controls-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }

  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .composer-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 10px;
  }

  .composer-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .composer-label {
    color: var(--text-dim);
    font-size: 0.62rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .composer-input {
    min-width: 0;
  }

  .composer-note {
    color: var(--text-dim);
    font-size: 0.72rem;
    line-height: 1.45;
  }

  .composer-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 240px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .primitive-picker {
    display: flex;
    gap: 10px;
    padding: 8px;
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
  }

  .primitive-picker__body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .primitive-picker__label {
    color: var(--text);
    font-size: 0.74rem;
    font-weight: 700;
  }

  .primitive-picker__meta {
    color: var(--text-dim);
    font-size: 0.66rem;
    line-height: 1.4;
  }

  .composer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    flex-wrap: wrap;
  }

  .no-params {
    color: var(--text-dim);
    font-size: 0.74rem;
    line-height: 1.45;
  }
</style>
