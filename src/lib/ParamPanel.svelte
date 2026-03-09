<script>
  import { invoke } from '@tauri-apps/api/core';
  import Dropdown from './Dropdown.svelte';

  let { uiSpec = $bindable(null), parameters = {}, onchange, onspecchange, activeVersionId = null, macroCode = '' } = $props();

  let editing = $state(false);
  let editFields = $state([]);
  let live = $state(false);
  let localParams = $state({});
  let hasPendingChanges = $derived(JSON.stringify(localParams) !== JSON.stringify(parameters));
  let saveValuesState = $state('idle'); // idle | saving | saved
  let macroParamKeys = $state(null);
  let macroParseSeq = 0;

  let lastVersionId = $state(null);

  $effect(() => {
    const code = `${macroCode ?? ''}`.trim();
    const seq = ++macroParseSeq;

    if (!code) {
      macroParamKeys = null;
      return;
    }

    (async () => {
      try {
        const parsed = await invoke('parse_macro_params', { macroCode: code });
        if (seq !== macroParseSeq) return;
        const keys = new Set();
        for (const field of parsed?.fields || []) {
          if (field?.key) keys.add(field.key);
        }
        for (const key of Object.keys(parsed?.params || {})) {
          keys.add(key);
        }
        macroParamKeys = keys.size > 0 ? keys : null;
      } catch (e) {
        if (seq === macroParseSeq) {
          macroParamKeys = null;
        }
      }
    })();
  });

  $effect(() => {
    // If we switched to a different version/thread, we must reset everything
    if (activeVersionId !== lastVersionId) {
      localParams = { ...parameters };
      lastVersionId = activeVersionId;
      editing = false;
      editFields = [];
      return;
    }

    // Same version: keep local edits intact while user has pending changes or edits controls.
    // Otherwise, hard-sync to canonical persisted parameters (prunes stale keys).
    if (editing || hasPendingChanges) {
      return;
    }

    if (JSON.stringify(localParams) !== JSON.stringify(parameters)) {
      localParams = { ...parameters };
    }
  });

  // Merge: each key in localParams not covered by uiSpec.fields gets a generated "number" field
  const mergedFields = $derived.by(() => {
    const specFields = uiSpec?.fields || [];
    const filteredSpecFields = macroParamKeys
      ? specFields.filter(f => macroParamKeys.has(f.key))
      : specFields;
    const declaredKeys = new Set(filteredSpecFields.map(f => f.key));
    
    const extraFields = Object.entries(localParams)
      .filter(([key]) => !macroParamKeys || macroParamKeys.has(key))
      .filter(([key]) => !declaredKeys.has(key))
      .map(([key, val]) => ({
        key,
        label: key.replace(/[_-]/g, ' '),
        type: typeof val === 'boolean' ? 'checkbox' : 'number',
        _auto: true,
      }));
    
    const all = [...filteredSpecFields, ...extraFields];
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
    editFields = [...editFields, { key: '', label: '', type: 'number', min: undefined, max: undefined, step: undefined, min_from: '', max_from: '', freezed: false }];
  }

  function removeField(index) {
    editFields = editFields.filter((_, i) => i !== index);
  }

  let reading = $state(false);
  let applying = $state(false);
  let searchQuery = $state('');

  const filteredFields = $derived.by(() => {
    if (!searchQuery.trim()) return mergedFields;
    const query = searchQuery.toLowerCase();
    return mergedFields.filter(f => 
      f.key.toLowerCase().includes(query) || 
      (f.label && f.label.toLowerCase().includes(query))
    );
  });

  const filteredEditFields = $derived.by(() => {
    if (!searchQuery.trim()) return editFields;
    const query = searchQuery.toLowerCase();
    return editFields.filter(f => 
      f.key.toLowerCase().includes(query) || 
      (f.label && f.label.toLowerCase().includes(query))
    );
  });

  async function readFromMacro() {
    if (!macroCode) {
      console.warn('ParamPanel: macroCode is empty, skipping readFromMacro');
      return;
    }
    reading = true;
    console.log('ParamPanel: invoking parse_macro_params...');
    
    try {
      const result = await invoke('parse_macro_params', { macroCode: macroCode });
      console.log('ParamPanel: parse_macro_params result:', result);
      const { fields, params } = result;

      if (fields && fields.length > 0) {
        editFields = fields;
        localParams = { ...params };
        console.log('ParamPanel: Updated editFields with', fields.length, 'fields');
      } else {
        console.warn('ParamPanel: parse_macro_params returned no fields');
      }
    } catch (e) {
      console.error('ParamPanel: Failed to parse macro params:', e);
    } finally {
      reading = false;
    }
  }

  async function saveFields() {
    const parseOptionalNumber = (raw) => {
      if (raw === null || raw === undefined || raw === '') return undefined;
      const n = Number(raw);
      return Number.isFinite(n) ? n : undefined;
    };

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
          const parsedMin = parseOptionalNumber(f.min);
          const parsedMax = parseOptionalNumber(f.max);
          const parsedStep = parseOptionalNumber(f.step);
          if (parsedMin !== undefined) field.min = parsedMin;
          if (parsedMax !== undefined) field.max = parsedMax;
          if (parsedStep !== undefined && parsedStep > 0) field.step = parsedStep;
          if (f.min_from) field.min_from = f.min_from;
          if (f.max_from) field.max_from = f.max_from;
        }
        return field;
      });

    const newSpec = { fields: cleaned };
    uiSpec = newSpec;

    if (activeVersionId) {
      console.log('ParamPanel: Saving uiSpec to messageId:', activeVersionId, newSpec);
      try {
        await invoke('update_ui_spec', { messageId: activeVersionId, uiSpec: newSpec });
        console.log('ParamPanel: update_ui_spec success');
        
        // Also save parameters since readFromMacro might have updated them
        await invoke('update_parameters', { messageId: activeVersionId, parameters: localParams });
        console.log('ParamPanel: update_parameters success');
        
        // Notify parent and trigger re-render
        if (onspecchange) onspecchange(newSpec, localParams);
        if (onchange) await onchange(localParams);
      } catch (e) {
        console.error('ParamPanel: Failed to save ui_spec/params:', e);
      }
    } else {
      if (onspecchange) onspecchange(newSpec, localParams);
    }

    editing = false;
    editFields = [];
  }

  function update(key, value) {
    let clampedValue = value;
    const field = mergedFields.find(f => f.key === key);
    if (field && (field.type === 'range' || field.type === 'number')) {
      if (!Number.isFinite(value)) return;
      const props = getRangeProps(field);
      clampedValue = Math.max(props.min, Math.min(props.max, value));
    }

    let nextParams = { ...localParams, [key]: clampedValue };

    // Cascade clamping for dependent fields
    for (const otherField of mergedFields) {
      if (otherField.key !== key && (otherField.min_from === key || otherField.max_from === key)) {
        const otherVal = nextParams[otherField.key] ?? 0;
        let oMin = otherField.min ?? 0;
        if (otherField.min_from && nextParams[otherField.min_from] !== undefined) oMin = nextParams[otherField.min_from];
        let oMax = otherField.max ?? Math.max(200, otherVal * 4);
        if (otherField.max_from && nextParams[otherField.max_from] !== undefined) oMax = nextParams[otherField.max_from];
        
        const nextClamped = Math.max(oMin, Math.min(oMax, otherVal));
        if (nextClamped !== otherVal) {
          nextParams[otherField.key] = nextClamped;
        }
      }
    }

    localParams = nextParams;
    if (live && onchange) {
      onchange(localParams);
    }
  }

  async function applyChanges() {
    if (applying) return;
    console.log('ParamPanel: applyChanges clicked', { localParams, hasPendingChanges, live });
    if (onchange) {
      applying = true;
      try {
        await onchange(localParams);
      } catch (e) {
        console.error('ParamPanel: onchange failed', e);
      } finally {
        applying = false;
      }
    } else {
      console.warn('ParamPanel: onchange prop is missing!');
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
    const rawVal = Number(localParams[field.key]);
    const val = Number.isFinite(rawVal) ? rawVal : 0;
    let min = field.min !== undefined ? field.min : 0;
    if (field.min_from && localParams[field.min_from] !== undefined) {
      min = localParams[field.min_from];
    }

    let max = field.max !== undefined ? field.max : Math.max(200, val * 4);
    if (field.max_from && localParams[field.max_from] !== undefined) {
      max = localParams[field.max_from];
    }

    if (max < min) max = min;
    if (max === min) max = min + 1;
    const stepCandidate = field.step !== undefined ? Number(field.step) : (max - min > 50 ? 1 : 0.1);
    const step = Number.isFinite(stepCandidate) && stepCandidate > 0 ? stepCandidate : 1;
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
    <div class="search-box">
      <input 
        type="text" 
        placeholder="Search controls..." 
        bind:value={searchQuery}
        class="search-input"
      />
      {#if searchQuery}
        <button class="clear-search" onclick={() => searchQuery = ''}>✕</button>
      {/if}
    </div>
  </div>

  <div class="panel-actions">
    {#if !editing}
      <div class="live-apply-group">
        <label class="live-toggle" title="Update geometry immediately on every change">
          <input class="ui-checkbox" type="checkbox" bind:checked={live} />
          <span>LIVE</span>
        </label>
        <button 
          class="btn btn-xs btn-primary apply-btn" 
          onclick={applyChanges} 
          disabled={live || applying}
        >
          {#if applying}
            APPLYING...
          {:else}
            APPLY
          {/if}
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
      <div class="edit-toolbar-left">
        <button class="btn btn-xs btn-primary" onclick={saveFields}>💾 SAVE</button>
        <button class="btn btn-xs btn-ghost" onclick={cancelEditing}>✕ CANCEL</button>
      </div>
      <button class="btn btn-xs btn-secondary" onclick={readFromMacro} title="Auto-detect parameters from macro code" disabled={reading}>
        {#if reading}
          ⏳ READING...
        {:else}
          🔍 READ FROM MACRO
        {/if}
      </button>
    {/if}
  </div>

  {#if editing}
    <div class="edit-list">
      {#each filteredEditFields as field}
        {@const i = editFields.indexOf(field)}
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
              <input class="input-mono edit-input-sm flex-1" placeholder="min from (key)" bind:value={field.min_from} />
              <input class="input-mono edit-input-sm flex-1" placeholder="max from (key)" bind:value={field.max_from} />
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
  {:else if filteredFields.length > 0}
    <div class="param-list">
      {#each filteredFields as field}
        {@const range = getRangeProps(field)}
        <div class="param-field" class:auto-field={field._auto} class:param-freezed={field.freezed} class:field-checkbox={field.type === 'checkbox'}>
          <div class="field-header">
            <label class="param-label" for={field.key}>
              {field.label}
            </label>
            {#if field.freezed}<span class="frozen-badge" title="FROZEN">❄️</span>{/if}
          </div>
          
          <div class="field-control">
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
                placeholder="Select..."
              />
            {:else if field.type === 'checkbox'}
              <label class="checkbox-wrapper">
                <input 
                  id={field.key}
                  class="ui-checkbox"
                  type="checkbox" 
                  checked={localParams[field.key]}
                  onchange={(e) => update(field.key, e.target.checked)}
                  disabled={field.freezed}
                />
                <span class="checkbox-status">{localParams[field.key] ? 'ENABLED' : 'DISABLED'}</span>
              </label>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="no-params">No controls match your search.</div>
  {/if}
</div>

<style>
  .param-panel {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .panel-toolbar {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-bottom: 1px solid var(--bg-300);
    padding-bottom: 8px;
    margin-bottom: 4px;
  }

  .search-box {
    position: relative;
    width: 100%;
  }

  .search-input {
    width: 100%;
    padding: 6px 28px 6px 10px;
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.75rem;
    outline: none;
    transition: border-color 0.2s;
  }

  .search-input:focus {
    border-color: var(--primary);
  }

  .clear-search {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.8rem;
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
  }

  .edit-toolbar-left {
    display: flex;
    gap: 4px;
    align-items: center;
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

  .live-toggle input {
    display: none;
  }

  .apply-btn {
    min-width: 50px;
  }

  .param-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 12px;
  }

  .param-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px;
    background: color-mix(in srgb, var(--bg-200) 50%, transparent);
    border: 1px solid transparent;
    transition: all 0.2s;
  }

  .param-field:hover {
    border-color: var(--bg-400);
    background: var(--bg-200);
  }

  .field-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    min-height: 14px;
  }

  .param-label {
    font-size: 0.62rem;
    color: var(--text-dim);
    text-transform: uppercase;
    font-weight: bold;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: 0.02em;
  }

  .frozen-badge {
    font-size: 0.6rem;
    cursor: help;
  }

  .range-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  input[type="range"] {
    flex: 1;
    cursor: pointer;
    height: 4px;
    background: var(--bg-300);
    border-radius: 2px;
    appearance: none;
  }

  input[type="range"]::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    background: var(--secondary);
    border-radius: 50%;
    cursor: pointer;
    box-shadow: 0 0 5px rgba(0,0,0,0.3);
  }

  .range-value {
    font-size: 0.75rem;
    color: var(--secondary);
    font-weight: bold;
    min-width: 36px;
    text-align: right;
  }

  .param-input {
    width: 100%;
    padding: 4px 6px;
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.75rem;
  }

  .checkbox-wrapper {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .checkbox-status {
    font-size: 0.6rem;
    color: var(--text-dim);
    font-weight: bold;
  }

  .ui-checkbox:checked + .checkbox-status {
    color: var(--secondary);
  }

  .ui-checkbox {
    -webkit-appearance: none;
    appearance: none;
    width: 12px;
    height: 12px;
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
    display: inline-grid;
    place-content: center;
    cursor: pointer;
    margin: 0;
  }

  .ui-checkbox::after {
    content: '';
    width: 6px;
    height: 6px;
    background: var(--secondary);
    transform: scale(0);
    transition: transform 0.12s ease-in-out;
  }

  .ui-checkbox:checked::after {
    transform: scale(1);
  }

  .auto-field {
    border-left: 2px solid var(--bg-400);
  }

  .param-freezed {
    opacity: 0.5;
  }

  .no-params {
    font-size: 0.7rem;
    color: var(--text-dim);
    font-style: italic;
    padding: 20px;
    text-align: center;
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
