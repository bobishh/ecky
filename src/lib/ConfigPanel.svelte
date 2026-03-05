<script>
  import Dropdown from './Dropdown.svelte';
  import { invoke } from '@tauri-apps/api/core';

  let { config = $bindable(), availableModels = [], isLoadingModels = false, onfetch, onsave } = $props();

  let isSaving = $state(false);
  let message = $state('');

  const providers = [
    { id: 'gemini', name: 'Google Gemini' },
    { id: 'openai', name: 'OpenAI (or Compatible)' },
    { id: 'ollama', name: 'Ollama (Local)' }
  ];

  const selectedEngine = $derived(config.engines.find(e => e.id === config.selected_engine_id));

  async function handleSave() {
    isSaving = true;
    message = 'Saving registry...';
    try {
      if (onsave) await onsave();
      message = 'Registry saved successfully.';
    } catch (e) {
      message = `Error: ${e}`;
    } finally {
      isSaving = false;
    }
  }

  async function addEngine() {
    const id = `engine-${Date.now()}`;
    const defaultPrompt = await invoke('get_system_prompt');
    const newEngine = {
      id,
      name: 'New Engine',
      provider: 'gemini',
      api_key: '',
      model: '',
      base_url: '',
      system_prompt: defaultPrompt
    };
    config.engines = [...config.engines, newEngine];
    config.selected_engine_id = id;
  }

  function removeEngine(id) {
    config.engines = config.engines.filter(e => e.id !== id);
    if (config.selected_engine_id === id) {
      config.selected_engine_id = config.engines.length > 0 ? config.engines[0].id : '';
    }
  }

  function handleProviderChange() {
    if (selectedEngine) {
      selectedEngine.model = '';
    }
  }
  async function resetPrompt() {
    if (selectedEngine) {
      selectedEngine.system_prompt = await invoke('get_system_prompt');
    }
  }
</script>

<div class="config-container">
  <aside class="engine-list">
    <div class="list-header">
      <label>ENGINES</label>
      <button class="btn btn-xs" onclick={addEngine}>+ ADD</button>
    </div>
    <div class="list-content">
      {#each config.engines as engine}
        <button 
          class="engine-item {config.selected_engine_id === engine.id ? 'active' : ''}"
          onclick={() => { config.selected_engine_id = engine.id; }}
        >
          <span class="engine-name">{engine.name || '(unnamed)'}</span>
          <span class="engine-provider">{engine.provider}</span>
        </button>
      {/each}
    </div>
  </aside>

  <main class="engine-details">
    {#if selectedEngine}
      <div class="details-content">
        <div class="field-row">
          <div class="field flex-2">
            <label for="e-name">DISPLAY NAME</label>
            <input 
              id="e-name" 
              type="text" 
              value={selectedEngine.name} 
              class="input-mono" 
              placeholder="e.g. My Gemini" 
              oninput={(e) => selectedEngine.name = e.target.value}
            />
          </div>
          <div class="field flex-1">
            <label for="e-provider">PROVIDER</label>
            <Dropdown 
              options={providers} 
              value={selectedEngine.provider} 
              onchange={(val) => { selectedEngine.provider = val; handleProviderChange(); }} 
            />
          </div>
        </div>

        <div class="field">
          <label for="e-key">API KEY</label>
          <input 
            id="e-key" 
            type="password" 
            value={selectedEngine.api_key} 
            class="input-mono" 
            placeholder="Enter API key..." 
            oninput={(e) => selectedEngine.api_key = e.target.value}
          />
        </div>

        <div class="field-row">
          <div class="field flex-2">
            <label for="e-model">MODEL</label>
            <Dropdown 
              options={availableModels.length > 0 ? availableModels : (selectedEngine.model ? [selectedEngine.model] : [])} 
              value={selectedEngine.model} 
              placeholder={isLoadingModels ? "Fetching..." : "Fetch models first..."} 
              onchange={(val) => selectedEngine.model = val}
            />
          </div>
          <div class="field flex-1">
            <label for="e-baseurl">BASE URL (OPTIONAL)</label>
            <input 
              id="e-baseurl" 
              type="text" 
              value={selectedEngine.base_url} 
              class="input-mono" 
              placeholder="Default" 
              oninput={(e) => selectedEngine.base_url = e.target.value}
            />
          </div>
        </div>

        <div class="field prompt-field">
          <div class="prompt-header">
            <label for="e-prompt">SYSTEM PROMPT (template with $USER_PROMPT)</label>
            <button class="btn btn-xs" onclick={resetPrompt}>RESET TO DEFAULT</button>
          </div>
          <textarea 
            id="e-prompt" 
            value={selectedEngine.system_prompt} 
            class="input-mono system-prompt-input" 
            spellcheck="false"
            oninput={(e) => selectedEngine.system_prompt = e.target.value}
            placeholder="Template for LLM. Use $USER_PROMPT as placeholder for user intent."
          ></textarea>
        </div>

        <div class="danger-zone">
          <button class="btn btn-xs btn-ghost" onclick={() => removeEngine(selectedEngine.id)}>REMOVE ENGINE</button>
        </div>
      </div>
    {:else}
      <div class="no-engine">
        <p>No engine selected. Add one to begin.</p>
        <button class="btn btn-primary" onclick={addEngine}>ADD FIRST ENGINE</button>
      </div>
    {/if}

    <div class="config-footer">
      <span class="status-msg">{message}</span>
      <button class="btn btn-primary" onclick={handleSave} disabled={isSaving || config.engines.length === 0}>
        {isSaving ? 'SAVING...' : 'SAVE REGISTRY'}
      </button>
    </div>
  </main>
</div>

<style>
  .config-container {
    display: flex;
    height: 100%;
    width: 100%;
    background: var(--bg-100);
  }

  .engine-list {
    width: 240px;
    flex-shrink: 0;
    border-right: 1px solid var(--bg-300);
    display: flex;
    flex-direction: column;
  }

  .list-header {
    padding: 12px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--bg-300);
  }

  .list-content {
    flex: 1;
    overflow-y: auto;
  }

  .engine-item {
    width: 100%;
    padding: 12px;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--bg-300);
    display: flex;
    flex-direction: column;
    gap: 4px;
    cursor: pointer;
  }

  .engine-item:hover {
    background: var(--bg-200);
  }

  .engine-item.active {
    background: var(--bg-300);
    border-left: 3px solid var(--primary);
  }

  .engine-name {
    font-size: 0.8rem;
    font-weight: bold;
    color: var(--text);
  }

  .engine-provider {
    font-size: 0.65rem;
    color: var(--secondary);
    text-transform: uppercase;
  }

  .engine-details {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .details-content {
    flex: 1;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
    overflow-y: auto;
  }

  .field-row {
    display: flex;
    gap: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .key-row {
    display: flex;
    gap: 8px;
  }

  .key-row input {
    flex: 1;
  }

  .flex-1 { flex: 1; }
  .flex-2 { flex: 2; }

  label {
    font-size: 0.65rem;
    color: var(--text-dim);
    font-weight: bold;
    letter-spacing: 0.05em;
  }

  input, textarea {
    padding: 8px 12px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.8rem;
    outline: none;
    font-family: var(--font-mono);
    width: 100%;
  }

  input:focus, textarea:focus {
    border-color: var(--primary);
  }

  .prompt-field {
    flex: 1;
    min-height: 250px;
    display: flex;
    flex-direction: column;
  }

  .prompt-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 6px;
  }

  .prompt-header label {
    margin-bottom: 0;
  }

  .system-prompt-input {
    flex: 1;
    resize: none;
    line-height: 1.5;
  }

  .danger-zone {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--bg-300);
  }

  .config-footer {
    padding: 16px 24px;
    border-top: 1px solid var(--bg-300);
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .no-engine {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    color: var(--text-dim);
  }

  .btn-xs {
    padding: 2px 6px;
    font-size: 0.6rem;
  }

  .status-msg {
    font-size: 0.75rem;
    color: var(--secondary);
  }
</style>
