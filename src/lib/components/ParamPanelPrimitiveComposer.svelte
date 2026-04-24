<script lang="ts">
  import Dropdown from '../Dropdown.svelte';
  import type { ControlPrimitiveKind, ControlViewSource, PartBinding, ResolvedUiField } from '../types/domain';

  type PrimitiveBindingDraft = {
    parameterKey: string;
    scale: string;
    offset: string;
    min: string;
    max: string;
  };

  type ActiveViewSummary = {
    label: string;
    source?: ControlViewSource;
  } | null;

  let {
    mode,
    editingId = null,
    label,
    scope,
    partId,
    attachToView,
    activeSemanticView = null,
    modelParts = [],
    candidateFields = [],
    selectedParameterKeys = [],
    selectedFields = [],
    bindingDrafts = {},
    kindPreview = null,
    canSave,
    onLabelChange,
    onScopeChange,
    onPartIdChange,
    onAttachToViewChange,
    onToggleParameter,
    onUpdateDraft,
    onCancel,
    onDelete,
    onSave,
  }: {
    mode: 'create' | 'edit';
    editingId?: string | null;
    label: string;
    scope: 'global' | 'part';
    partId: string | null;
    attachToView: boolean;
    activeSemanticView?: ActiveViewSummary;
    modelParts?: PartBinding[];
    candidateFields?: ResolvedUiField[];
    selectedParameterKeys?: string[];
    selectedFields?: ResolvedUiField[];
    bindingDrafts?: Record<string, PrimitiveBindingDraft>;
    kindPreview?: ControlPrimitiveKind | null;
    canSave: boolean;
    onLabelChange?: (value: string) => void;
    onScopeChange?: (value: 'global' | 'part') => void;
    onPartIdChange?: (value: string | null) => void;
    onAttachToViewChange?: (value: boolean) => void;
    onToggleParameter?: (key: string, checked: boolean) => void;
    onUpdateDraft?: (key: string, field: 'scale' | 'offset' | 'min' | 'max', value: string) => void;
    onCancel?: () => void;
    onDelete?: (primitiveId: string) => void;
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
    <div class="section-label">{mode === 'edit' ? 'EDIT KNOB' : 'NEW KNOB'}</div>
  </div>
  <div class="composer-grid">
    <div class="composer-field">
      <label class="composer-label" for="composer-primitive-label">KNOB NAME</label>
      <input
        id="composer-primitive-label"
        class="input-mono composer-input"
        value={label}
        oninput={(event) => onLabelChange?.(getInputValue(event))}
        placeholder="Connector Size / Hose Fit / Wall Thickness..."
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
    Pick one or more raw params to drive with a single semantic knob. Mixed field types are not allowed in one knob yet.
  </div>
  <label class="primitive-picker">
    <input
      class="ui-checkbox"
      type="checkbox"
      checked={attachToView}
      onchange={(event) => onAttachToViewChange?.(getInputChecked(event))}
    />
    <div class="primitive-picker__body">
      <div class="primitive-picker__label">Add to current context</div>
      <div class="primitive-picker__meta">
        {#if activeSemanticView}
          {activeSemanticView.source === 'manual'
            ? `Updates ${activeSemanticView.label}.`
            : `Creates a custom context from ${activeSemanticView.label}.`}
        {:else}
          Creates a custom context for this knob.
        {/if}
      </div>
    </div>
  </label>
  <div class="composer-list">
    {#if candidateFields.length > 0}
      {#each candidateFields as field}
        <label class="primitive-picker">
          <input
            class="ui-checkbox"
            type="checkbox"
            checked={selectedParameterKeys.includes(field.key)}
            onchange={(event) => onToggleParameter?.(field.key, getInputChecked(event))}
          />
          <div class="primitive-picker__body">
            <div class="primitive-picker__label">{field.label}</div>
            <div class="primitive-picker__meta">{field.key}</div>
          </div>
        </label>
      {/each}
    {:else}
      <div class="no-params">No raw params are available for this scope.</div>
    {/if}
  </div>
  {#if selectedFields.length > 0 && kindPreview === 'number'}
    <div class="binding-editor">
      <div class="section-label">BINDINGS</div>
      {#each selectedFields as field}
        {@const draft = bindingDrafts[field.key]}
        <div class="binding-row">
          <div class="binding-row__label">{field.label}</div>
          <input
            class="input-mono binding-input"
            type="number"
            step="0.01"
            value={draft?.scale ?? '1'}
            oninput={(event) => onUpdateDraft?.(field.key, 'scale', getInputValue(event))}
            placeholder="scale"
          />
          <input
            class="input-mono binding-input"
            type="number"
            step="0.01"
            value={draft?.offset ?? '0'}
            oninput={(event) => onUpdateDraft?.(field.key, 'offset', getInputValue(event))}
            placeholder="offset"
          />
          <input
            class="input-mono binding-input"
            type="number"
            step="0.01"
            value={draft?.min ?? ''}
            oninput={(event) => onUpdateDraft?.(field.key, 'min', getInputValue(event))}
            placeholder="min"
          />
          <input
            class="input-mono binding-input"
            type="number"
            step="0.01"
            value={draft?.max ?? ''}
            oninput={(event) => onUpdateDraft?.(field.key, 'max', getInputValue(event))}
            placeholder="max"
          />
        </div>
      {/each}
    </div>
  {/if}
  <div class="composer-note">
    {#if kindPreview}
      This knob will behave as a {kindPreview}.
    {:else if selectedParameterKeys.length > 0}
      Select params of the same kind only.
    {:else}
      Choose the raw params this knob should control.
    {/if}
  </div>
  <div class="composer-actions">
    <button class="btn btn-xs btn-ghost" onclick={() => onCancel?.()}>CANCEL</button>
    {#if mode === 'edit' && editingId}
      <button class="btn btn-xs btn-ghost" onclick={() => onDelete?.(editingId)}>
        DELETE KNOB
      </button>
    {/if}
    <button class="btn btn-xs btn-primary" onclick={() => onSave?.()} disabled={!canSave}>
      {mode === 'edit' ? 'SAVE KNOB' : 'CREATE KNOB'}
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
    font-weight: 700;
    letter-spacing: 0.08em;
  }

  .composer-input {
    width: 100%;
  }

  .composer-note {
    color: var(--text-dim);
    font-size: 0.68rem;
    line-height: 1.4;
  }

  .composer-list {
    display: grid;
    gap: 8px;
    max-height: 220px;
    overflow: auto;
    padding-right: 4px;
  }

  .primitive-picker {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    cursor: pointer;
  }

  .primitive-picker__body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .primitive-picker__label {
    color: var(--text);
    font-size: 0.78rem;
    font-weight: 700;
  }

  .primitive-picker__meta {
    color: var(--text-dim);
    font-size: 0.64rem;
    line-height: 1.35;
  }

  .binding-editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: hidden;
  }

  .binding-row {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) repeat(4, minmax(0, 0.7fr));
    gap: 8px;
    align-items: center;
  }

  .binding-row__label {
    color: var(--text);
    font-size: 0.7rem;
    font-weight: 700;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .binding-input {
    min-width: 0;
    padding: 6px 8px;
    font-size: 0.7rem;
  }

  .composer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .ui-checkbox {
    -webkit-appearance: none;
    appearance: none;
    width: 18px;
    height: 18px;
    border: 1px solid color-mix(in srgb, var(--cad-accent) 36%, var(--bg-300));
    background: var(--bg-100);
    display: inline-grid;
    place-content: center;
    cursor: pointer;
    margin: 0;
    flex: 0 0 auto;
  }

  .ui-checkbox::after {
    content: '';
    width: 10px;
    height: 10px;
    background: var(--cad-accent);
    transform: scale(0);
    transition: transform 0.12s ease-in-out;
  }

  .ui-checkbox:checked::after {
    transform: scale(1);
  }

  .no-params {
    color: var(--text-dim);
    font-size: 0.74rem;
    line-height: 1.45;
  }
</style>
