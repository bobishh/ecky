<script lang="ts">
  import Dropdown from '../Dropdown.svelte';
  import type {
    CheckboxField,
    ImageField,
    NumberField,
    RangeField,
    ResolvedUiField,
    SelectField,
    UiField,
  } from '../types/domain';

  type EditableNumber = number | '' | undefined;
  type EditableRangeField = Omit<RangeField, 'min' | 'max' | 'step'> & {
    min?: EditableNumber;
    max?: EditableNumber;
    step?: EditableNumber;
    _auto?: boolean;
  };
  type EditableNumberField = Omit<NumberField, 'min' | 'max' | 'step'> & {
    min?: EditableNumber;
    max?: EditableNumber;
    step?: EditableNumber;
    _auto?: boolean;
  };
  type EditableSelectField = SelectField & { _auto?: boolean };
  type EditableCheckboxField = CheckboxField & { _auto?: boolean };
  type EditableImageField = ImageField & { _auto?: boolean };
  type EditableUiField =
    | EditableRangeField
    | EditableNumberField
    | EditableSelectField
    | EditableCheckboxField
    | EditableImageField;
  type EditFieldEntry = {
    field: EditableUiField;
    index: number;
  };
  type EditableFieldPatch = Partial<EditableUiField>;

  let {
    fieldEntries,
    getAvailableTypes,
    onFieldChange,
    onAddSelectOption,
    onRemoveSelectOption,
    onOptionChange,
    onRemoveField,
    onAddField,
  }: {
    fieldEntries: EditFieldEntry[];
    getAvailableTypes: (field: EditableUiField | ResolvedUiField) => UiField['type'][];
    onFieldChange: (index: number, patch: EditableFieldPatch) => void;
    onAddSelectOption: (index: number) => void;
    onRemoveSelectOption: (fieldIndex: number, optionIndex: number) => void;
    onOptionChange: (
      fieldIndex: number,
      optionIndex: number,
      patch: { label?: string; value?: string | number },
    ) => void;
    onRemoveField: (index: number) => void;
    onAddField: () => void;
  } = $props();

  function parseEditableNumber(raw: string): EditableNumber {
    if (raw === '') return '';
    const value = Number(raw);
    return Number.isFinite(value) ? value : '';
  }
</script>

<div class="edit-list">
  {#each fieldEntries as { field, index }}
    <div class="edit-field" class:is-freezed={field.frozen}>
      <div class="edit-row">
        <input
          class="input-mono edit-input"
          placeholder="key"
          value={field.key}
          oninput={(event) => onFieldChange(index, { key: event.currentTarget.value })}
        />
        <input
          class="input-mono edit-input flex-2"
          placeholder="Label"
          value={field.label}
          oninput={(event) => onFieldChange(index, { label: event.currentTarget.value })}
        />
        <div class="edit-select-wrap">
          <Dropdown
            options={getAvailableTypes(field).map((type) => ({ id: type, name: type }))}
            value={field.type}
            onchange={(value) => onFieldChange(index, { type: value as UiField['type'] })}
            placeholder="Field Type"
          />
        </div>
        <label class="freeze-toggle" title="Freeze value and move to bottom">
          <input
            class="ui-checkbox ui-checkbox-sm"
            type="checkbox"
            checked={field.frozen}
            onchange={(event) => onFieldChange(index, { frozen: event.currentTarget.checked })}
          />
          <span>❄️</span>
        </label>
        <button class="btn btn-xs btn-ghost" onclick={() => onRemoveField(index)}>✕</button>
      </div>
      {#if field.type === 'range' || field.type === 'number'}
        <div class="edit-row edit-bounds">
          <input
            class="input-mono edit-input-sm"
            type="number"
            placeholder="min"
            value={field.min ?? ''}
            oninput={(event) => onFieldChange(index, { min: parseEditableNumber(event.currentTarget.value) })}
          />
          <input
            class="input-mono edit-input-sm"
            type="number"
            placeholder="max"
            value={field.max ?? ''}
            oninput={(event) => onFieldChange(index, { max: parseEditableNumber(event.currentTarget.value) })}
          />
          <input
            class="input-mono edit-input-sm"
            type="number"
            placeholder="step"
            value={field.step ?? ''}
            oninput={(event) => onFieldChange(index, { step: parseEditableNumber(event.currentTarget.value) })}
          />
          <input
            class="input-mono edit-input-sm flex-1"
            placeholder="min from (key)"
            value={field.minFrom ?? ''}
            oninput={(event) => onFieldChange(index, { minFrom: event.currentTarget.value })}
          />
          <input
            class="input-mono edit-input-sm flex-1"
            placeholder="max from (key)"
            value={field.maxFrom ?? ''}
            oninput={(event) => onFieldChange(index, { maxFrom: event.currentTarget.value })}
          />
        </div>
      {/if}
      {#if field.type === 'select'}
        <div class="edit-select-options">
          <div class="edit-row edit-info">
            <span class="info-tag">OPTIONS: {field.options?.length || 0}</span>
            <button class="btn btn-xs btn-ghost" onclick={() => onAddSelectOption(index)}>+ ADD OPTION</button>
          </div>
          {#if (field.options?.length || 0) > 0}
            {#each field.options || [] as option, optionIndex}
              <div class="edit-row edit-select-option-row">
                <input
                  class="input-mono edit-input flex-1"
                  placeholder="Option label"
                  value={option.label}
                  oninput={(event) => onOptionChange(index, optionIndex, { label: event.currentTarget.value })}
                />
                <input
                  class="input-mono edit-input flex-1"
                  placeholder="Option value"
                  value={option.value}
                  oninput={(event) => onOptionChange(index, optionIndex, { value: event.currentTarget.value })}
                />
                <button class="btn btn-xs btn-ghost" onclick={() => onRemoveSelectOption(index, optionIndex)}>✕</button>
              </div>
            {/each}
          {:else}
            <div class="edit-row edit-info">
              <span class="info-tag">No options yet. Add them manually.</span>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/each}
  <button class="btn btn-xs add-field-btn" onclick={onAddField}>+ ADD FIELD</button>
</div>

<style>
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

  .edit-input:focus,
  .edit-input-sm:focus {
    border-color: var(--primary);
    outline: none;
  }

  .flex-1 {
    flex: 1;
  }

  .flex-2 {
    flex: 2;
  }

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
    align-items: center;
    gap: 8px;
  }

  .edit-select-options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-left: 4px;
  }

  .edit-select-option-row {
    align-items: center;
  }

  .info-tag {
    background: var(--bg-300);
    padding: 1px 4px;
    border-radius: 2px;
  }

  .add-field-btn {
    align-self: flex-start;
  }

  .ui-checkbox {
    -webkit-appearance: none;
    appearance: none;
    width: 18px;
    height: 18px;
    border: 1px solid color-mix(in srgb, var(--cad-tone-color, var(--primary)) 36%, var(--bg-300));
    background: var(--bg-100);
    display: inline-grid;
    place-content: center;
    cursor: pointer;
    margin: 0;
  }

  .ui-checkbox::after {
    content: '';
    width: 10px;
    height: 10px;
    background: var(--cad-tone-color, var(--primary));
    transform: scale(0);
    transition: transform 0.12s ease-in-out;
  }

  .ui-checkbox:checked::after {
    transform: scale(1);
  }
</style>
