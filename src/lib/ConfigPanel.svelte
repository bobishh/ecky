<script>
  import { onMount } from 'svelte';
  import Dropdown from './Dropdown.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';

  let { config = $bindable(), availableModels = [], isLoadingModels = false, onfetch, onsave } = $props();

  let isSaving = $state(false);
  let message = $state('');
  let activeSection = $state('engines'); // 'engines' or 'assets'

  // Recording state
  let isRecording = $state(false);
  let recordingTarget = $state(null); // 'hum' or 'ding'
  let mediaRecorder = null;
  let audioChunks = [];
  let recordingTimer = $state(0);
  let timerInterval = null;
  let micOptions = $state([]);
  let selectedMicId = $state('');

  const providers = [
    { id: 'gemini', name: 'Google Gemini' },
    { id: 'openai', name: 'OpenAI (or Compatible)' },
    { id: 'ollama', name: 'Ollama (Local)' }
  ];

  const formats = [
    { id: 'MP3', name: 'MP3 (Audio)' },
    { id: 'WAV', name: 'WAV (Audio)' },
    { id: 'STL', name: 'STL (3D Mesh)' },
    { id: 'STEP', name: 'STEP (BRep)' },
    { id: 'PNG', name: 'PNG (Reference)' },
    { id: 'JPG', name: 'JPG (Reference)' },
    { id: 'JSON', name: 'JSON (Data)' }
  ];

  const selectedEngine = $derived(config.engines.find(e => e.id === config.selected_engine_id));

  // Microwave assignments
  if (!config.microwave) {
    config.microwave = {
      hum_id: null,
      ding_id: null,
      muted: false
    };
  } else if (typeof config.microwave.muted !== 'boolean') {
    config.microwave.muted = false;
  }

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
      light_model: '',
      base_url: '',
      system_prompt: defaultPrompt
    };
    config.engines = [...config.engines, newEngine];
    config.selected_engine_id = id;
    activeSection = 'engines';
  }

  async function refreshMicInputs() {
    if (!navigator?.mediaDevices) return;
    try {
      const temp = await navigator.mediaDevices.getUserMedia({ audio: true });
      temp.getTracks().forEach(track => track.stop());
    } catch (_) {
      // Permission may already be denied or unavailable; continue best-effort enumeration.
    }

    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      micOptions = devices
        .filter(d => d.kind === 'audioinput')
        .map((d, i) => ({
          id: d.deviceId,
          name: d.label || `Microphone ${i + 1}`
        }));
      if (micOptions.length > 0 && !micOptions.some(m => m.id === selectedMicId)) {
        selectedMicId = micOptions[0].id;
      }
    } catch (e) {
      console.warn('Failed to enumerate microphones:', e);
    }
  }

  async function uploadMicrowaveAudio(target) {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: 'Audio Files', extensions: ['mp3', 'wav', 'webm', 'ogg', 'm4a', 'aac', 'flac'] }
        ]
      });
      if (!selected) return;

      const path = selected;
      const name = path.split(/[\/\\]/).pop();
      const ext = (name.split('.').pop() || 'WAV').toUpperCase();

      const asset = await invoke('upload_asset', {
        sourcePath: path,
        name,
        format: ext
      });

      config.assets = [...(config.assets || []), asset];
      if (target === 'hum') config.microwave.hum_id = asset.id;
      if (target === 'ding') config.microwave.ding_id = asset.id;
      message = `Uploaded and assigned ${target.toUpperCase()} sound: ${name}`;
    } catch (e) {
      message = `Upload failed: ${e}`;
    }
  }

  async function startRecording(target) {
    try {
      const constraints = selectedMicId
        ? { audio: { deviceId: { exact: selectedMicId } } }
        : { audio: true };
      const stream = await navigator.mediaDevices.getUserMedia(constraints);
      mediaRecorder = new MediaRecorder(stream);
      audioChunks = [];
      recordingTarget = target;
      recordingTimer = 0;

      mediaRecorder.ondataavailable = (event) => {
        audioChunks.push(event.data);
      };

      mediaRecorder.onstop = async () => {
        const audioBlob = new Blob(audioChunks, { type: 'audio/webm' });
        const reader = new FileReader();
        reader.readAsDataURL(audioBlob);
        reader.onloadend = async () => {
          const base64data = reader.result.split(',')[1];
          const name = `Recording: ${target.toUpperCase()} (${new Date().toLocaleTimeString()})`;
          
          try {
            const asset = await invoke('save_recorded_audio', {
              base64Data: base64data,
              name
            });
            config.assets = [...(config.assets || []), asset];
            if (target === 'hum') config.microwave.hum_id = asset.id;
            if (target === 'ding') config.microwave.ding_id = asset.id;
            message = `Recorded ${target} saved and assigned.`;
          } catch (e) {
            message = `Failed to save recording: ${e}`;
          }
        };
      };

      mediaRecorder.start();
      isRecording = true;
      timerInterval = setInterval(() => {
        recordingTimer++;
      }, 1000);
    } catch (e) {
      message = `Microphone error: ${e}`;
    }
  }

  function stopRecording() {
    if (mediaRecorder) {
      mediaRecorder.stop();
      mediaRecorder.stream.getTracks().forEach(track => track.stop());
    }
    isRecording = false;
    clearInterval(timerInterval);
  }

  function removeEngine(id) {
    config.engines = config.engines.filter(e => e.id !== id);
    if (config.selected_engine_id === id) {
      config.selected_engine_id = config.engines.length > 0 ? config.engines[0].id : '';
    }
  }

  async function addAsset() {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: 'All Assets', extensions: ['stl', 'step', 'stp', 'png', 'jpg', 'jpeg', 'json'] }
        ]
      });

      if (selected) {
        const path = selected;
        const ext = path.split('.').pop().toUpperCase();
        const name = path.split(/[\/\\]/).pop();
        
        const asset = await invoke('upload_asset', {
          sourcePath: path,
          name,
          format: ext
        });

        config.assets = [...(config.assets || []), asset];
        message = `Asset ${name} added.`;
      }
    } catch (e) {
      message = `Upload failed: ${e}`;
    }
  }

  function removeAsset(id) {
    config.assets = config.assets.filter(a => a.id !== id);
  }

  function handleProviderChange() {
    if (selectedEngine) {
      selectedEngine.model = '';
      selectedEngine.light_model = '';
    }
  }
  async function resetPrompt() {
    if (selectedEngine) {
      selectedEngine.system_prompt = await invoke('get_system_prompt');
    }
  }

  onMount(() => {
    void refreshMicInputs();
    const mediaDevices = navigator?.mediaDevices;
    const onDeviceChange = () => { void refreshMicInputs(); };
    mediaDevices?.addEventListener?.('devicechange', onDeviceChange);
    return () => mediaDevices?.removeEventListener?.('devicechange', onDeviceChange);
  });
</script>

<div class="config-container">
  <aside class="config-sidebar">
    <div class="sidebar-group">
      <div class="list-header">
        <label>ENGINES</label>
        <button class="btn btn-xs" onclick={addEngine}>+ ADD</button>
      </div>
      <div class="list-content">
        {#each config.engines as engine}
          <button 
            class="engine-item {activeSection === 'engines' && config.selected_engine_id === engine.id ? 'active' : ''}"
            onclick={() => { config.selected_engine_id = engine.id; activeSection = 'engines'; }}
          >
            <span class="engine-name">{engine.name || '(unnamed)'}</span>
            <span class="engine-provider">{engine.provider}</span>
          </button>
        {/each}
      </div>
    </div>

    <div class="sidebar-group">
      <div class="list-header">
        <label>GLOBAL MEDIA / SOUNDS</label>
        <button class="btn btn-xs" onclick={addAsset}>+ UPLOAD</button>
      </div>
      <div class="list-content">
        {#each (config.assets || []) as asset}
          <button 
            class="engine-item {activeSection === 'assets' && config.selected_asset_id === asset.id ? 'active' : ''}"
            onclick={() => { config.selected_asset_id = asset.id; activeSection = 'assets'; }}
          >
            <span class="engine-name">{asset.name}</span>
            <span class="engine-provider">{asset.format}</span>
          </button>
        {/each}
        {#if !config.assets || config.assets.length === 0}
          <div class="empty-sidebar-msg">No media uploaded.</div>
        {/if}
      </div>
    </div>
    <div class="sidebar-group">
      <div class="list-header">
        <label>MICROWAVE SOUNDS</label>
      </div>
      <div class="list-content microwave-assignments">
        {#if micOptions.length > 1}
          <div class="mic-device-block">
            <span class="role-label">MIC INPUT</span>
            <div class="mic-device-row">
              <Dropdown
                options={micOptions}
                value={selectedMicId}
                onchange={(val) => selectedMicId = val}
                placeholder="Select microphone..."
                disabled={isRecording}
              />
              <button class="btn btn-xs btn-ghost" onclick={refreshMicInputs} disabled={isRecording}>↻ RESCAN</button>
            </div>
          </div>
        {/if}

        <div class="sound-role">
          <span class="role-label">COOKING HUM</span>
          <div class="role-actions">
            {#if isRecording && recordingTarget === 'hum'}
              <button class="btn btn-xs btn-danger pulse" onclick={stopRecording}>⏹ STOP ({recordingTimer}s)</button>
            {:else}
              <button class="btn btn-xs" onclick={() => startRecording('hum')} disabled={isRecording}>🎤 RECORD</button>
            {/if}
            <button class="btn btn-xs btn-ghost" onclick={() => uploadMicrowaveAudio('hum')} disabled={isRecording}>📁 UPLOAD HUM</button>
            {#if config.microwave?.hum_id}
              <button class="btn btn-xs btn-ghost" onclick={() => config.microwave.hum_id = null}>✕ CLEAR</button>
            {/if}
          </div>
          {#if config.microwave?.hum_id}
            {@const asset = config.assets?.find(a => a.id === config.microwave.hum_id)}
            <span class="assigned-name">{asset?.name || 'Assigned'}</span>
          {/if}
        </div>

        <div class="sound-role">
          <span class="role-label">DONE DING</span>
          <div class="role-actions">
            {#if isRecording && recordingTarget === 'ding'}
              <button class="btn btn-xs btn-danger pulse" onclick={stopRecording}>⏹ STOP ({recordingTimer}s)</button>
            {:else}
              <button class="btn btn-xs" onclick={() => startRecording('ding')} disabled={isRecording}>🎤 RECORD</button>
            {/if}
            {#if config.microwave?.ding_id}
              <button class="btn btn-xs btn-ghost" onclick={() => config.microwave.ding_id = null}>✕ CLEAR</button>
            {/if}
          </div>
          {#if config.microwave?.ding_id}
            {@const asset = config.assets?.find(a => a.id === config.microwave.ding_id)}
            <span class="assigned-name">{asset?.name || 'Assigned'}</span>
          {/if}
        </div>
      </div>
    </div>
  </aside>

  <main class="engine-details">
    <div class="details-scrollable">
      {#if activeSection === 'engines' && selectedEngine}
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
            <div class="field flex-1">
              <label for="e-model">RENDER AND HEAVY REASONING</label>
              <Dropdown 
                options={availableModels.length > 0 ? availableModels : (selectedEngine.model ? [selectedEngine.model] : [])} 
                value={selectedEngine.model} 
                placeholder={isLoadingModels ? "Fetching..." : "Fetch models first..."} 
                onchange={(val) => selectedEngine.model = val}
              />
            </div>
            <div class="field flex-1">
              <label for="e-light-model">LIGHT REASONING</label>
              <Dropdown
                options={availableModels.length > 0 ? availableModels : (selectedEngine.light_model ? [selectedEngine.light_model] : (selectedEngine.model ? [selectedEngine.model] : []))}
                value={selectedEngine.light_model}
                placeholder={isLoadingModels ? "Fetching..." : "Optional (falls back to heavy model)"}
                onchange={(val) => selectedEngine.light_model = val}
              />
            </div>
          </div>

          <div class="field">
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
      {:else if activeSection === 'assets'}
        {@const selectedAsset = config.assets?.find(a => a.id === config.selected_asset_id)}
        {#if selectedAsset}
          <div class="details-content">
            <div class="field">
              <label>ASSET NAME</label>
              <input type="text" bind:value={selectedAsset.name} class="input-mono" />
            </div>
            <div class="field">
              <label>FORMAT</label>
              <Dropdown options={formats} bind:value={selectedAsset.format} />
            </div>
            
            <div class="field">
              <label>ASSIGN TO MICROWAVE</label>
              <div class="assignment-buttons">
                <button 
                  class="btn btn-xs {config.microwave?.hum_id === selectedAsset.id ? 'btn-primary' : 'btn-ghost'}"
                  onclick={() => config.microwave.hum_id = selectedAsset.id}
                >
                  ASSIGN AS HUM (COOKING)
                </button>
                <button 
                  class="btn btn-xs {config.microwave?.ding_id === selectedAsset.id ? 'btn-primary' : 'btn-ghost'}"
                  onclick={() => config.microwave.ding_id = selectedAsset.id}
                >
                  ASSIGN AS DING (DONE)
                </button>
              </div>
            </div>

            <div class="field">
              <label>LOCAL PATH</label>
              <div class="path-display">{selectedAsset.path}</div>
            </div>
            <div class="danger-zone">
              <button class="btn btn-xs btn-ghost" onclick={() => removeAsset(selectedAsset.id)}>REMOVE ASSET</button>
            </div>
          </div>
        {:else}
          <div class="no-engine">Select media to view details.</div>
        {/if}
      {:else}
        <div class="no-engine">
          <p>No engine selected. Add one to begin.</p>
          <button class="btn btn-primary" onclick={addEngine}>ADD FIRST ENGINE</button>
        </div>
      {/if}
    </div>

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
    overflow: hidden;
  }

  .config-sidebar {
    width: 240px;
    flex-shrink: 0;
    border-right: 1px solid var(--bg-300);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .sidebar-group {
    display: flex;
    flex-direction: column;
    max-height: 50%;
    border-bottom: 2px solid var(--bg-300);
  }

  .sidebar-group:last-child {
    flex: 1;
    border-bottom: none;
  }

  .list-header {
    padding: 12px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .list-content {
    flex: 1;
    overflow-y: auto;
  }

  .engine-item {
    width: 100%;
    padding: 10px 12px;
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
    font-size: 0.75rem;
    font-weight: bold;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .engine-provider {
    font-size: 0.6rem;
    color: var(--secondary);
    text-transform: uppercase;
  }

  .empty-sidebar-msg {
    padding: 20px;
    font-size: 0.7rem;
    color: var(--text-dim);
    text-align: center;
    font-style: italic;
  }

  .engine-details {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  .details-scrollable {
    flex: 1;
    overflow-y: auto;
  }

  .details-content {
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
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

  .path-display {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--text-dim);
    background: var(--bg-200);
    padding: 8px;
    border: 1px solid var(--bg-300);
    word-break: break-all;
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
    min-height: 300px;
    display: flex;
    flex-direction: column;
  }

  .prompt-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 6px;
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
    background: var(--bg-100);
  }

  .no-engine {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    color: var(--text-dim);
    font-size: 0.8rem;
  }

  .btn-xs {
    padding: 2px 6px;
    font-size: 0.6rem;
  }

  .status-msg {
    font-size: 0.75rem;
    color: var(--secondary);
  }

  .microwave-assignments {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .sound-role {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .mic-device-block {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--bg-300);
  }

  .mic-device-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .mic-device-row :global(.custom-select) {
    flex: 1;
  }

  .role-label {
    font-size: 0.6rem;
    font-weight: bold;
    color: var(--text-dim);
    letter-spacing: 0.05em;
  }

  .role-actions {
    display: flex;
    gap: 6px;
  }

  .assigned-name {
    font-size: 0.6rem;
    color: var(--secondary);
    font-family: var(--font-mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .btn-danger {
    background: var(--red);
    color: white;
    border: none;
  }

  .pulse {
    animation: pulse-red 1.5s infinite;
  }

  @keyframes pulse-red {
    0% { opacity: 1; }
    50% { opacity: 0.6; }
    100% { opacity: 1; }
  }
</style>
