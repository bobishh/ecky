<script lang="ts">
  import Dropdown from '../Dropdown.svelte';
  import type { MaterializedSemanticControl } from '../modelRuntime/semanticControls';
  import type { AdvisoryCondition, AdvisorySeverity } from '../types/domain';

  let {
    label,
    message,
    severity,
    condition,
    threshold,
    candidateControls = [],
    selectedPrimitiveIds = [],
    canSave,
    onLabelChange,
    onMessageChange,
    onSeverityChange,
    onConditionChange,
    onThresholdChange,
    onTogglePrimitive,
    onCancel,
    onSave,
  }: {
    label: string;
    message: string;
    severity: AdvisorySeverity;
    condition: AdvisoryCondition;
    threshold: string;
    candidateControls?: MaterializedSemanticControl[];
    selectedPrimitiveIds?: string[];
    canSave: boolean;
    onLabelChange?: (value: string) => void;
    onMessageChange?: (value: string) => void;
    onSeverityChange?: (value: AdvisorySeverity) => void;
    onConditionChange?: (value: AdvisoryCondition) => void;
    onThresholdChange?: (value: string) => void;
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
    <div class="section-label">NEW RULE</div>
  </div>
  <div class="composer-grid">
    <div class="composer-field">
      <label class="composer-label" for="composer-advisory-label">RULE NAME</label>
      <input
        id="composer-advisory-label"
        class="input-mono composer-input"
        value={label}
        oninput={(event) => onLabelChange?.(getInputValue(event))}
        placeholder="Connector Fit / Thin Wall / Clearance Check..."
      />
    </div>
    <div class="composer-field">
      <div class="composer-label">SEVERITY</div>
      <Dropdown
        options={[
          { id: 'warning', name: 'Warning' },
          { id: 'info', name: 'Info' },
        ]}
        value={severity}
        onchange={(value) => onSeverityChange?.(value === 'info' ? 'info' : 'warning')}
      />
    </div>
    <div class="composer-field">
      <div class="composer-label">CONDITION</div>
      <Dropdown
        options={[
          { id: 'always', name: 'Always' },
          { id: 'below', name: 'Below threshold' },
          { id: 'above', name: 'Above threshold' },
        ]}
        value={condition}
        onchange={(value) =>
          onConditionChange?.(value === 'below' || value === 'above' ? value : 'always')}
      />
    </div>
    {#if condition !== 'always'}
      <div class="composer-field">
        <label class="composer-label" for="composer-advisory-threshold">THRESHOLD</label>
        <input
          id="composer-advisory-threshold"
          class="input-mono composer-input"
          type="number"
          step="0.01"
          value={threshold}
          oninput={(event) => onThresholdChange?.(getInputValue(event))}
          placeholder="1.2"
        />
      </div>
    {/if}
  </div>
  <div class="composer-field">
    <label class="composer-label" for="composer-advisory-message">MESSAGE</label>
    <input
      id="composer-advisory-message"
      class="input-mono composer-input"
      value={message}
      oninput={(event) => onMessageChange?.(getInputValue(event))}
      placeholder="Connector changes may require matching clearance adjustments."
    />
  </div>
  <div class="composer-note">
    Attach this rule to one or more semantic controls in the active context.
  </div>
  <div class="composer-list">
    {#if candidateControls.length > 0}
      {#each candidateControls as control}
        <label class="primitive-picker">
          <input
            class="ui-checkbox"
            type="checkbox"
            checked={selectedPrimitiveIds.includes(control.primitiveId)}
            onchange={(event) => onTogglePrimitive?.(control.primitiveId, getInputChecked(event))}
          />
          <div class="primitive-picker__body">
            <div class="primitive-picker__label">{control.label}</div>
            <div class="primitive-picker__meta">{control.rawField?.key || control.primitiveId}</div>
          </div>
        </label>
      {/each}
    {:else}
      <div class="no-params">Open a context first to attach a rule.</div>
    {/if}
  </div>
  <div class="composer-actions">
    <button class="btn btn-xs btn-ghost" onclick={() => onCancel?.()}>CANCEL</button>
    <button class="btn btn-xs btn-primary" onclick={() => onSave?.()} disabled={!canSave}>
      CREATE RULE
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
