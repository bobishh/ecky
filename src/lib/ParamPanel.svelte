<script>
  let { uiSpec = null, parameters = {}, onchange } = $props();

  function update(key, value) {
    if (onchange) {
      onchange({ [key]: value });
    }
  }
</script>

<div class="param-panel">
  {#if uiSpec && uiSpec.fields}
    <div class="param-list">
      {#each uiSpec.fields as field}
        <div class="param-field">
          <label class="param-label" for={field.key}>{field.label}</label>
          
          {#if field.type === 'range'}
            <div class="range-group">
              <input 
                id={field.key}
                type="range" 
                min={field.min} 
                max={field.max} 
                step={field.step || 0.01}
                value={parameters[field.key]}
                oninput={(e) => update(field.key, parseFloat(e.target.value))}
              />
              <span class="range-value">{parameters[field.key]}</span>
            </div>
          {:else if field.type === 'number'}
            <input 
              id={field.key}
              type="number" 
              class="input-mono param-input"
              value={parameters[field.key]}
              oninput={(e) => update(field.key, parseFloat(e.target.value))}
            />
          {:else if field.type === 'select'}
            <select 
              id={field.key}
              class="input-mono param-select"
              value={parameters[field.key]}
              onchange={(e) => update(field.key, e.target.value)}
            >
              {#each field.options || [] as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          {/if}
        </div>
      {/each}
    </div>
  {:else}
    <div class="no-params">No interactive parameters found for this design.</div>
  {/if}
</div>

<style>
  .param-select {
    width: 100%;
    padding: 4px 8px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.75rem;
    outline: none;
    cursor: pointer;
  }

  .param-select:focus {
    border-color: var(--primary);
  }

  .param-panel {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .param-header {
    font-size: 0.6rem;
    color: var(--secondary);
    font-weight: bold;
    letter-spacing: 0.1em;
    margin-bottom: 4px;
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

  .no-params {
    font-size: 0.7rem;
    color: var(--text-dim);
    font-style: italic;
  }
</style>
