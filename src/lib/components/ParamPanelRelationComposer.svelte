<script lang="ts">
  import Dropdown from '../Dropdown.svelte';
  import type { ControlRelationMode } from '../types/domain';

  export type RelationControlOption = {
    primitiveId: string;
    label: string;
  };

  let {
    controls = [],
    sourcePrimitiveId = null,
    targetPrimitiveId = null,
    mode,
    scale,
    offset,
    canSave,
    onSourceChange,
    onTargetChange,
    onModeChange,
    onScaleChange,
    onOffsetChange,
    onCancel,
    onSave,
  }: {
    controls?: RelationControlOption[];
    sourcePrimitiveId?: string | null;
    targetPrimitiveId?: string | null;
    mode: ControlRelationMode;
    scale: string;
    offset: string;
    canSave: boolean;
    onSourceChange?: (value: string | null) => void;
    onTargetChange?: (value: string | null) => void;
    onModeChange?: (value: ControlRelationMode) => void;
    onScaleChange?: (value: string) => void;
    onOffsetChange?: (value: string) => void;
    onCancel?: () => void;
    onSave?: () => void;
  } = $props();

  function getInputValue(event: Event): string {
    return (event.currentTarget as HTMLInputElement).value;
  }
</script>

<div class="view-composer">
  <div class="controls-head">
    <div class="section-label">NEW LINK</div>
  </div>
  <div class="composer-grid">
    <div class="composer-field">
      <div class="composer-label">SOURCE KNOB</div>
      <Dropdown
        options={controls.map((control) => ({ id: control.primitiveId, name: control.label }))}
        value={sourcePrimitiveId}
        onchange={(value) => onSourceChange?.(typeof value === 'string' ? value : null)}
        placeholder="Choose source..."
      />
    </div>
    <div class="composer-field">
      <div class="composer-label">TARGET KNOB</div>
      <Dropdown
        options={controls.map((control) => ({ id: control.primitiveId, name: control.label }))}
        value={targetPrimitiveId}
        onchange={(value) => onTargetChange?.(typeof value === 'string' ? value : null)}
        placeholder="Choose target..."
      />
    </div>
    <div class="composer-field">
      <div class="composer-label">MODE</div>
      <Dropdown
        options={[
          { id: 'mirror', name: 'Mirror value' },
          { id: 'scale', name: 'Scale source' },
          { id: 'offset', name: 'Offset source' },
        ]}
        value={mode}
        onchange={(value) => onModeChange?.(value === 'scale' || value === 'offset' ? value : 'mirror')}
      />
    </div>
    {#if mode === 'scale'}
      <div class="composer-field">
        <label class="composer-label" for="relation-scale">SCALE</label>
        <input
          id="relation-scale"
          class="input-mono composer-input"
          type="number"
          step="0.01"
          value={scale}
          oninput={(event) => onScaleChange?.(getInputValue(event))}
        />
      </div>
    {/if}
    {#if mode === 'offset'}
      <div class="composer-field">
        <label class="composer-label" for="relation-offset">OFFSET</label>
        <input
          id="relation-offset"
          class="input-mono composer-input"
          type="number"
          step="0.01"
          value={offset}
          oninput={(event) => onOffsetChange?.(getInputValue(event))}
        />
      </div>
    {/if}
  </div>
  <div class="composer-note">
    Linked knobs apply on semantic edits and persist with this version.
  </div>
  <div class="composer-actions">
    <button class="btn btn-xs btn-ghost" onclick={() => onCancel?.()}>CANCEL</button>
    <button class="btn btn-xs btn-primary" onclick={() => onSave?.()} disabled={!canSave}>
      CREATE LINK
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

  .composer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    flex-wrap: wrap;
  }
</style>
