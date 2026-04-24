<script lang="ts">
  import Dropdown from '../Dropdown.svelte';
  import type { ControlViewSource, ParamValue, ResolvedUiField, UiField } from '../types/domain';

  type CadTone = 'neutral' | 'size' | 'x' | 'y' | 'z' | 'angle' | 'state' | 'mode';
  type RangeProps = { min: number; max: number; step: number };

  let {
    elementId,
    field,
    value,
    rangeProps = null,
    editable = true,
    frozen = false,
    autoField = false,
    focused = false,
    highlighted = false,
    cadTone = 'neutral',
    semanticSource = undefined,
    showSemanticSource = false,
    canEdit = false,
    onUpdate,
    onEdit,
    onPickImage,
    onMouseEnter,
    onMouseLeave,
    onFocusIn,
    onFocusOut,
  }: {
    elementId: string;
    field: UiField | ResolvedUiField;
    value: ParamValue | undefined;
    rangeProps?: RangeProps | null;
    editable?: boolean;
    frozen?: boolean;
    autoField?: boolean;
    focused?: boolean;
    highlighted?: boolean;
    cadTone?: CadTone;
    semanticSource?: ControlViewSource;
    showSemanticSource?: boolean;
    canEdit?: boolean;
    onUpdate?: (value: ParamValue) => void;
    onEdit?: () => void;
    onPickImage?: () => Promise<void> | void;
    onMouseEnter?: (event: MouseEvent) => void;
    onMouseLeave?: (event: MouseEvent) => void;
    onFocusIn?: (event: FocusEvent) => void;
    onFocusOut?: (event: FocusEvent) => void;
  } = $props();

  function getInputValue(event: Event): string {
    return (event.currentTarget as HTMLInputElement).value;
  }

  function getInputChecked(event: Event): boolean {
    return (event.currentTarget as HTMLInputElement).checked;
  }

  function asNumber(rawValue: ParamValue | undefined, fallback = 0): number {
    const numeric = Number(rawValue);
    return Number.isFinite(numeric) ? numeric : fallback;
  }
</script>

<div
  class="param-field"
  role="group"
  class:auto-field={autoField}
  class:param-freezed={frozen}
  class:param-field-focus={focused}
  class:field-select={field.type === 'select'}
  class:field-checkbox={field.type === 'checkbox'}
  class:highlight-pulse={highlighted}
  data-cad-tone={cadTone}
  data-param-key={field.key}
  onmouseenter={(event) => onMouseEnter?.(event)}
  onmouseleave={(event) => onMouseLeave?.(event)}
  onfocusin={(event) => onFocusIn?.(event)}
  onfocusout={(event) => onFocusOut?.(event)}
>
  <div class="field-header">
    <div class="field-title">
      <label class="param-label" for={elementId}>
        {field.label}
      </label>
      {#if showSemanticSource && semanticSource}
        <span class="semantic-source-badge">{semanticSource.toUpperCase()}</span>
      {/if}
    </div>
    {#if canEdit}
      <button class="btn btn-xs btn-ghost field-action-btn" onclick={() => onEdit?.()}>
        EDIT
      </button>
    {/if}
  </div>

  <div class="field-control">
    {#if field.type === 'range'}
      {@const range = rangeProps ?? { min: 0, max: 100, step: 1 }}
      <div class="range-group cad-range">
        <input
          id={elementId}
          type="range"
          min={range.min}
          max={range.max}
          step={range.step}
          value={asNumber(value, range.min)}
          oninput={(event) => onUpdate?.(parseFloat(getInputValue(event)))}
          disabled={!editable}
        />
        <input
          type="number"
          class="input-mono param-input param-input-compact"
          min={range.min}
          max={range.max}
          step={range.step}
          value={asNumber(value, range.min)}
          oninput={(event) => onUpdate?.(parseFloat(getInputValue(event)))}
          disabled={!editable}
        />
      </div>
    {:else if field.type === 'number'}
      <input
        id={elementId}
        type="number"
        class="input-mono param-input"
        value={asNumber(value, 0)}
        oninput={(event) => onUpdate?.(parseFloat(getInputValue(event)))}
        disabled={!editable}
      />
    {:else if field.type === 'select'}
      <Dropdown
        options={(field.options || []).map((option) => ({ id: option.value, name: option.label }))}
        value={typeof value === 'string' || typeof value === 'number' ? value : null}
        onchange={(nextValue: string | number | undefined) => {
          if (nextValue !== undefined) onUpdate?.(nextValue);
        }}
        disabled={!editable}
        placeholder="Select..."
      />
    {:else if field.type === 'checkbox'}
      <label class="checkbox-wrapper" class:checkbox-wrapper-checked={Boolean(value)}>
        <input
          id={elementId}
          class="ui-checkbox"
          type="checkbox"
          checked={Boolean(value)}
          onchange={(event) => onUpdate?.(getInputChecked(event))}
          disabled={!editable}
        />
        <span class="checkbox-status">{value ? 'ON' : 'OFF'}</span>
      </label>
    {:else if field.type === 'image'}
      <div class="image-field-wrapper">
        <button class="btn param-btn" onclick={() => onPickImage?.()} disabled={!editable}>
          {value ? String(value).split(/[/\\]/).pop() : 'Select Image...'}
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .param-field {
    --cad-tone-color: var(--cad-accent);
    display: flex;
    flex-direction: column;
    gap: 4px;
    position: relative;
    padding: 6px;
    overflow: hidden;
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--bg-100) 76%, transparent) 0%,
        color-mix(in srgb, var(--bg-200) 88%, #000 12%) 100%
      );
    border: 1px solid color-mix(in srgb, var(--bg-300) 82%, #000 18%);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 28%, transparent);
    transition: all 0.2s;
  }

  .param-field[data-cad-tone='x'],
  .param-field[data-cad-tone='size'] {
    --cad-tone-color: var(--cad-axis-x);
  }

  .param-field[data-cad-tone='y'] {
    --cad-tone-color: var(--cad-axis-y);
  }

  .param-field[data-cad-tone='z'] {
    --cad-tone-color: var(--cad-axis-z);
  }

  .param-field[data-cad-tone='angle'],
  .param-field[data-cad-tone='mode'],
  .param-field[data-cad-tone='state'] {
    --cad-tone-color: var(--cad-axis-angle);
  }

  .param-field:hover {
    border-color: color-mix(in srgb, var(--cad-tone-color) 35%, var(--bg-300));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--cad-tone-color) 8%, var(--bg-100)) 0%,
        color-mix(in srgb, var(--bg-200) 82%, #000 18%) 100%
      );
  }

  .param-field-focus {
    border-color: color-mix(in srgb, var(--primary) 55%, var(--bg-300));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--cad-tone-color) 10%, var(--bg-100)) 0%,
        color-mix(in srgb, var(--primary) 12%, var(--bg-200)) 100%
      );
  }

  .field-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }

  .field-title {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
    min-width: 0;
    flex-wrap: wrap;
  }

  .semantic-source-badge {
    padding: 1px 5px;
    border: 1px solid color-mix(in srgb, var(--primary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-200));
    color: var(--primary);
    font-family: var(--font-mono);
    font-size: 0.52rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    flex-shrink: 0;
  }

  .param-label {
    font-size: 0.72rem;
    color: var(--primary);
    text-transform: uppercase;
    font-weight: bold;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: 0.01em;
  }

  .field-action-btn {
    flex-shrink: 0;
  }

  .range-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .cad-range {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: 7px;
  }

  .field-control {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .param-input {
    width: 100%;
    padding: 4px 6px;
    background: var(--bg-100);
    border: 1px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.75rem;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 22%, transparent);
  }

  .param-input-compact {
    width: 86px;
    min-width: 86px;
  }

  .param-input:focus {
    outline: none;
    border-color: var(--primary);
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, #000 22%, transparent),
      0 0 0 1px color-mix(in srgb, var(--primary) 18%, transparent);
  }

  .checkbox-wrapper {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    min-height: 42px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--cad-tone-color) 28%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 82%, #000 18%);
    cursor: pointer;
  }

  .checkbox-wrapper-checked {
    background: color-mix(in srgb, var(--cad-tone-color) 12%, var(--bg-100));
  }

  .checkbox-status {
    font-size: 0.68rem;
    color: var(--primary);
    font-weight: bold;
    letter-spacing: 0.06em;
  }

  .ui-checkbox:checked + .checkbox-status {
    color: var(--cad-tone-color);
  }

  .ui-checkbox {
    -webkit-appearance: none;
    appearance: none;
    width: 18px;
    height: 18px;
    border: 1px solid color-mix(in srgb, var(--cad-tone-color) 36%, var(--bg-300));
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
    background: var(--cad-tone-color);
    transform: scale(0);
    transition: transform 0.12s ease-in-out;
  }

  .ui-checkbox:checked::after {
    transform: scale(1);
  }

  .image-field-wrapper {
    display: flex;
    min-width: 0;
  }

  .param-field :global(.select-trigger) {
    background: var(--bg-100);
    border-color: color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    color: var(--text);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 22%, transparent);
  }

  .param-field :global(.custom-select.is-open .select-trigger) {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
  }

  .param-field :global(.select-arrow) {
    color: var(--primary);
  }

  .param-field :global(.select-dropdown) {
    background: var(--bg-100);
    border-color: var(--primary);
  }

  .param-field :global(.select-option:hover) {
    background: color-mix(in srgb, var(--primary) 16%, var(--bg-200));
    color: var(--text);
  }

  .param-field :global(.select-option.is-selected) {
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
    color: var(--primary);
    border-left: 0;
    padding-left: 12px;
  }

  .auto-field {
    border-left: 0;
  }

  .param-freezed {
    opacity: 0.5;
  }

  .highlight-pulse {
    animation: highlightPulse 2s ease-in-out;
  }

  @keyframes highlightPulse {
    0% { background-color: transparent; }
    50% { background-color: var(--primary); color: var(--bg-100); }
    100% { background-color: transparent; }
  }
</style>
