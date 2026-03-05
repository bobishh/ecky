<script>
  import { invoke } from '@tauri-apps/api/core';
  import Dropdown from './Dropdown.svelte';

  let { uiSpec = $bindable(null), parameters = {}, onchange, activeVersionId = null } = $props();

  let editing = $state(false);
  let editFields = $state([]);
  let live = $state(false);
  let localParams = $state({ ...parameters });
  let hasPendingChanges = $derived(JSON.stringify(localParams) !== JSON.stringify(parameters));
  let saveValuesState = $state('idle'); // idle | saving | saved

  $effect(() => {
    // Sync local params if parameters change from outside (e.g. version load)
    localParams = { ...parameters };
  });

  // Merge: any key in parameters not covered by uiSpec.fields gets a generated "number" field
  const mergedFields = $derived.by(() => {
    const specFields = uiSpec?.fields || [];
    const declaredKeys = new Set(specFields.map(f => f.key));
    
    const extraFields = Object.entries(parameters)
      .filter(([key]) => !declaredKeys.has(key))
      .map(([key, val]) => ({
        key,
        label: key.replace(/[_-]/g, ' '),
        type: typeof val === 'boolean' ? 'checkbox' : 'number',
        _auto: true,
      }));
    
    const all = [...specFields, ...extraFields];
    // Sort: non-freezed first, then freezed
    return all.sort((a, b) => {
      if (a.freezed === b.freezed) return 0;
      return a.freezed ? 1 : -1;
    });
  });

  function startEditing() {
    editFields = mergedFields.map(f => ({ ...f }));
    editing = true;
  }

  function cancelEditing() {
    editing = false;
    editFields = [];
  }

  function addField() {
    editFields = [...editFields, { key: '', label: '', type: 'number', min: undefined, max: undefined, step: undefined, freezed: false }];
  }

  function removeField(index) {
    editFields = editFields.filter((_, i) => i !== index);
  }

  async function saveFields() {
    const cleaned = editFields
      .filter(f => f.key.trim())
      .map(f => {
        const field = { 
          key: f.key.trim(), 
          label: f.label || f.key, 
          type: f.type,
          freezed: !!f.freezed
        };
        // Preserve options for select types
        if (f.type === 'select' && f.options) {
          field.options = f.options;
        }
        if (f.type === 'range' || f.type === 'number') {
          if (f.min !== undefined && f.min !== '') field.min = Number(f.min);
          if (f.max !== undefined && f.max !== '') field.max = Number(f.max);
          if (f.step !== undefined && f.step !== '') field.step = Number(f.step);
        }
        return field;
      });

    const newSpec = { fields: cleaned };
    uiSpec = newSpec;

    if (activeVersionId) {
      try {
        await invoke('update_ui_spec', { messageId: activeVersionId, uiSpec: newSpec });
      } catch (e) {
        console.error('Failed to save ui_spec:', e);
      }
    }

    editing = false;
    editFields = [];
  }

  function update(key, value) {
    localParams = { ...localParams, [key]: value };
    if (live && onchange) {
      onchange({ [key]: value });
    }
  }

  function applyChanges() {
    if (onchange) {
      onchange(localParams);
    }
  }

  async function saveValues() {
    if (!activeVersionId) return;
    saveValuesState = 'saving';
    try {
      await invoke('update_parameters', { messageId: activeVersionId, parameters: localParams });
      saveValuesState = 'saved';
      setTimeout(() => {
        if (saveValuesState === 'saved') saveValuesState = 'idle';
      }, 1500);
    } catch (e) {
      console.error('Failed to save defaults:', e);
      saveValuesState = 'idle';
    }
  }

  function getRangeProps(field) {
    const val = localParams[field.key] || 0;
    const min = field.min !== undefined ? field.min : 0;
    let max = field.max !== undefined ? field.max : Math.max(200, val * 4);
    const step = field.step !== undefined ? field.step : (max > 50 ? 1 : 0.1);
    return { min, max, step };
  }

  const FIELD_TYPES = ['range', 'number', 'select', 'checkbox'];

  function getAvailableTypes(field) {
    // If it's boolean in parameters, don't allow range/number?
    // User said "booleans, it can't be turned to range"
    const val = parameters[field.key];
    if (typeof val === 'boolean' || field.type === 'checkbox') {
      return ['checkbox'];
    }
    if (field.type === 'select') {
      return ['select'];
    }
    return ['range', 'number'];
  }
</script>

<div class="param-panel">
  <div class="panel-toolbar">
    {#if !editing}
      <div class="live-apply-group">
        <label class="live-toggle" title="Update geometry immediately on every change">
          <input class="ui-checkbox" type="checkbox" bind:checked={live} />
          <span>LIVE</span>
        </label>
        <button 
          class="btn btn-xs btn-primary apply-btn" 
          onclick={applyChanges} 
          disabled={!hasPendingChanges || live}
        >
          APPLY
        </button>
        <button
          class="btn btn-xs btn-ghost"
          onclick={saveValues}
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
      <button class="btn btn-xs" onclick={startEditing} title="Edit controls">✏️ EDIT CONTROLS</button>
    {:else}
      <button class="btn btn-xs" onclick={saveFields}>💾 SAVE</button>
      <button class="btn btn-xs btn-ghost" onclick={cancelEditing}>✕ CANCEL</button>
    {/if}
  </div>

  {#if editing}
    <div class="edit-list">
      {#each editFields as field, i}
        <div class="edit-field" class:is-freezed={field.freezed}>
          <div class="edit-row">
            <input class="input-mono edit-input" placeholder="key" bind:value={field.key} />
            <input class="input-mono edit-input flex-2" placeholder="Label" bind:value={field.label} />
            <div class="edit-select-wrap">
              <Dropdown
                options={getAvailableTypes(field).map(t => ({ id: t, name: t }))}
                bind:value={field.type}
                placeholder="Field Type"
              />
            </div>
            <label class="freeze-toggle" title="Freeze value and move to bottom">
              <input class="ui-checkbox ui-checkbox-sm" type="checkbox" bind:checked={field.freezed} />
              <span>❄️</span>
            </label>
            <button class="btn btn-xs btn-ghost" onclick={() => removeField(i)}>✕</button>
          </div>
          {#if field.type === 'range' || field.type === 'number'}
            <div class="edit-row edit-bounds">
              <input class="input-mono edit-input-sm" type="number" placeholder="min" bind:value={field.min} />
              <input class="input-mono edit-input-sm" type="number" placeholder="max" bind:value={field.max} />
              <input class="input-mono edit-input-sm" type="number" placeholder="step" bind:value={field.step} />
            </div>
          {/if}
          {#if field.type === 'select'}
            <div class="edit-row edit-info">
              <span class="info-tag">ENUM: {field.options?.length || 0} options (intrinsic)</span>
            </div>
          {/if}
        </div>
      {/each}
      <button class="btn btn-xs add-field-btn" onclick={addField}>+ ADD FIELD</button>
    </div>
  {:else if mergedFields.length > 0}
    <div class="param-list">
      {#each mergedFields as field}
        {@const range = getRangeProps(field)}
        <div class="param-field" class:auto-field={field._auto} class:param-freezed={field.freezed}>
          <label class="param-label" for={field.key}>
            {field.label}
            {#if field.freezed}<span class="frozen-badge">❄️ FROZEN</span>{/if}
          </label>
          
          {#if field.type === 'range'}
            <div class="range-group">
              <input 
                id={field.key}
                type="range" 
                min={range.min} 
                max={range.max} 
                step={range.step}
                value={localParams[field.key]}
                oninput={(e) => update(field.key, parseFloat(e.target.value))}
                disabled={field.freezed}
              />
              <span class="range-value">{localParams[field.key]}</span>
            </div>
          {:else if field.type === 'number'}
            <input 
              id={field.key}
              type="number" 
              class="input-mono param-input"
              value={localParams[field.key]}
              oninput={(e) => update(field.key, parseFloat(e.target.value))}
              disabled={field.freezed}
            />
          {:else if field.type === 'select'}
            <Dropdown
              options={(field.options || []).map(option => ({ id: option.value, name: option.label }))}
              value={localParams[field.key]}
              onchange={(val) => update(field.key, val)}
              disabled={field.freezed}
              placeholder="Select value..."
            />
          {:else if field.type === 'checkbox'}
            <div class="checkbox-group">
              <input 
                id={field.key}
                class="ui-checkbox"
                type="checkbox" 
                checked={localParams[field.key]}
                onchange={(e) => update(field.key, e.target.checked)}
                disabled={field.freezed}
              />
              <span class="checkbox-status">{localParams[field.key] ? 'ENABLED' : 'DISABLED'}</span>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {:else}
    <div class="no-params">No interactive parameters found for this design.</div>
  {/if}
</div>

<style>
  .param-panel {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .panel-toolbar {
    display: flex;
    gap: 12px;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--bg-300);
    padding-bottom: 8px;
  }

  .live-apply-group {
    display: flex;
    gap: 12px;
    align-items: center;
  }

  .live-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 0.65rem;
    font-weight: bold;
    color: var(--text-dim);
    cursor: pointer;
    user-select: none;
  }

  .live-toggle input {
    margin: 0;
  }

  .live-toggle:has(input:checked) {
    color: var(--secondary);
  }

  .apply-btn {
    min-width: 60px;
  }

  .param-list {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .param-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .param-label {
    font-size: 0.7rem;
    color: var(--text-dim);
    text-transform: uppercase;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .frozen-badge {
    font-size: 0.55rem;
    color: var(--blue);
    background: color-mix(in srgb, var(--blue) 15%, transparent);
    padding: 1px 4px;
    border-radius: 2px;
  }

  .range-group {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  input[type="range"] {
    flex: 1;
    cursor: pointer;
  }

  input[type="range"]:disabled {
    cursor: not-allowed;
    opacity: 0.3;
  }

  .range-value {
    font-size: 0.75rem;
    color: var(--secondary);
    font-weight: bold;
    min-width: 36px;
    text-align: right;
  }

  .param-input {
    width: 80px;
    padding: 4px 8px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.75rem;
  }

  .param-input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .checkbox-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .checkbox-status {
    font-size: 0.65rem;
    color: var(--secondary);
    font-weight: bold;
  }

  .ui-checkbox {
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    display: inline-grid;
    place-content: center;
    cursor: pointer;
    margin: 0;
  }

  .ui-checkbox::after {
    content: '';
    width: 8px;
    height: 8px;
    background: var(--secondary);
    transform: scale(0);
    transition: transform 0.12s ease-in-out;
  }

  .ui-checkbox:checked {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 16%, var(--bg-200));
  }

  .ui-checkbox:checked::after {
    transform: scale(1);
  }

  .ui-checkbox:focus-visible {
    outline: 1px solid var(--primary);
    outline-offset: 1px;
  }

  .ui-checkbox:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .ui-checkbox-sm {
    width: 12px;
    height: 12px;
  }

  .ui-checkbox-sm::after {
    width: 6px;
    height: 6px;
  }

  .auto-field {
    border-left: 2px solid var(--bg-400);
    padding-left: 8px;
  }

  .param-freezed {
    opacity: 0.6;
    background: rgba(0,0,0,0.1);
    padding: 4px;
    border-radius: 4px;
  }

  .no-params {
    font-size: 0.7rem;
    color: var(--text-dim);
    font-style: italic;
  }

  /* Edit mode */
  .edit-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .edit-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
  }

  .edit-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .edit-input {
    flex: 1;
    padding: 4px 6px;
    background: var(--bg);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.7rem;
  }

  .edit-input:focus, .edit-input-sm:focus {
    border-color: var(--primary);
    outline: none;
  }

  .flex-2 { flex: 2; }

  .edit-select-wrap {
    width: 132px;
  }

  .edit-bounds {
    padding-left: 4px;
  }

  .edit-input-sm {
    width: 60px;
    padding: 3px 5px;
    background: var(--bg);
    border: 1px solid var(--bg-300);
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.65rem;
  }

  .freeze-toggle {
    display: flex;
    align-items: center;
    gap: 2px;
    cursor: pointer;
    font-size: 0.8rem;
    user-select: none;
  }

  .freeze-toggle input {
    margin: 0;
  }

  .edit-info {
    font-size: 0.6rem;
    color: var(--text-dim);
    padding-left: 4px;
  }

  .info-tag {
    background: var(--bg-300);
    padding: 1px 4px;
    border-radius: 2px;
  }

  .add-field-btn {
    align-self: flex-start;
  }

  .btn-xs {
    padding: 2px 6px;
    font-size: 0.6rem;
  }
</style>
