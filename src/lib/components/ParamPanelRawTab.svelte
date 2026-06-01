<script lang="ts">
  import ParamPanelControlField from './ParamPanelControlField.svelte';
  import type { DesignParams, ParamValue, PartBinding, ResolvedUiField } from '../types/domain';

  type CadTone = 'neutral' | 'size' | 'x' | 'y' | 'z' | 'angle' | 'state' | 'mode';
  type RangeProps = { min: number; max: number; step: number };

  let {
    filteredFields,
    focusedFields,
    remainingFields,
    selectedPart = null,
    parameters,
    highlightedParamKey = null,
    liveApply = false,
    getRangeProps,
    getCadTone,
    onDraftValue,
    onUpdate,
    onPickImage,
    onSetFocusedControl,
    onClearFocusedControl,
  }: {
    filteredFields: ResolvedUiField[];
    focusedFields: ResolvedUiField[];
    remainingFields: ResolvedUiField[];
    selectedPart?: PartBinding | null;
    parameters: DesignParams;
    highlightedParamKey?: string | null;
    liveApply?: boolean;
    getRangeProps: (field: ResolvedUiField) => RangeProps;
    getCadTone: (field: ResolvedUiField) => CadTone;
    onDraftValue: (key: string, value: ParamValue) => void;
    onUpdate: (key: string, value: ParamValue) => void;
    onPickImage: (key: string) => Promise<void> | void;
    onSetFocusedControl: (primitiveId: string | null, parameterKey: string | null) => void;
    onClearFocusedControl: (event: MouseEvent | FocusEvent) => void;
  } = $props();
</script>

{#if filteredFields.length > 0 && focusedFields.length > 0}
  <div class="focused-section">
    <div class="controls-head">
      <div class="section-label">{selectedPart ? `${selectedPart.label} RAW` : 'RAW PART'}</div>
    </div>
    <div class="param-list">
      {#each focusedFields as field}
        <ParamPanelControlField
          elementId={field.key}
          {field}
          value={parameters[field.key]}
          rangeProps={field.type === 'range' || field.type === 'number' ? getRangeProps(field) : null}
          editable={!field.frozen}
          frozen={field.frozen}
          autoField={field._auto}
          focused={true}
          highlighted={highlightedParamKey === field.key}
          cadTone={getCadTone(field)}
          {liveApply}
          onDraftValue={(nextValue) => onDraftValue(field.key, nextValue)}
          onUpdate={(nextValue) => onUpdate(field.key, nextValue)}
          onPickImage={() => onPickImage(field.key)}
          onMouseEnter={() => onSetFocusedControl(null, field.key)}
          onMouseLeave={onClearFocusedControl}
          onFocusIn={() => onSetFocusedControl(null, field.key)}
          onFocusOut={onClearFocusedControl}
        />
      {/each}
    </div>
  </div>
{/if}

{#if filteredFields.length > 0 && remainingFields.length > 0}
  {#if focusedFields.length > 0}
    <div class="controls-head controls-head-secondary">
      <div class="section-label">OTHER RAW</div>
    </div>
  {/if}
  <div class="param-list">
    {#each remainingFields as field}
      <ParamPanelControlField
        elementId={field.key}
        {field}
        value={parameters[field.key]}
        rangeProps={field.type === 'range' || field.type === 'number' ? getRangeProps(field) : null}
        editable={!field.frozen}
        frozen={field.frozen}
        autoField={field._auto}
        highlighted={highlightedParamKey === field.key}
        cadTone={getCadTone(field)}
        {liveApply}
        onDraftValue={(nextValue) => onDraftValue(field.key, nextValue)}
        onUpdate={(nextValue) => onUpdate(field.key, nextValue)}
        onPickImage={() => onPickImage(field.key)}
        onMouseEnter={() => onSetFocusedControl(null, field.key)}
        onMouseLeave={onClearFocusedControl}
        onFocusIn={() => onSetFocusedControl(null, field.key)}
        onFocusOut={onClearFocusedControl}
      />
    {/each}
  </div>
{:else if filteredFields.length === 0}
  <div class="no-params">
    {selectedPart
      ? 'This part has no raw controls that match your search.'
      : 'No raw controls match your search.'}
  </div>
{/if}

<style>
  .focused-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: visible;
  }

  .controls-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }

  .controls-head-secondary {
    margin-top: 2px;
  }

  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .param-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 220px), 1fr));
    gap: 12px;
    overflow: visible;
  }

  .no-params {
    font-size: 0.7rem;
    color: var(--text-dim);
    font-style: italic;
    padding: 20px;
    text-align: center;
  }
</style>
