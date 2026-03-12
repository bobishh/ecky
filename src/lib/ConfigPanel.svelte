<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Dropdown from './Dropdown.svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import {
    formatBackendError,
    getMcpServerStatus,
    getSystemPrompt,
    saveRecordedAudio,
    uploadAsset,
  } from './tauri/client';
  import type { AppConfig, AutoAgent, McpServerStatus } from './types/domain';

  type ActiveSection = 'agents' | 'engines' | 'freecad' | 'sounds' | 'prompts';
  type ConnectionType = 'api_key' | 'mcp' | null;
  type McpMode = 'passive' | 'active';
  type RecordingTarget = 'hum' | 'ding';
  type MicLoadState = 'idle' | 'loading' | 'ready' | 'error';
  type MicrophoneOption = {
    id: string;
    name: string;
  };

  let {
    config = $bindable(),
    availableModels = [],
    isLoadingModels = false,
    onfetch,
    onsave,
  }: {
    config: AppConfig;
    availableModels?: string[];
    isLoadingModels?: boolean;
    onfetch?: () => Promise<void> | void;
    onsave?: () => Promise<void> | void;
  } = $props();

  let isSaving = $state(false);
  let message = $state('');
  let activeSection = $state<ActiveSection>('agents');

  // Eagerly initialize mcp config so derived below is always non-null
  if (!config.mcp) config.mcp = { port: null, maxSessions: null, autoAgents: [] };
  if (!Array.isArray(config.mcp.autoAgents)) config.mcp.autoAgents = [];

  function deriveConnectionType(): ConnectionType {
    if (config.connectionType === 'mcp') return 'mcp';
    if (config.connectionType === 'api_key') return 'api_key';
    // fallback heuristic for configs without persisted connectionType
    if (config.engines.some(e => e.enabled)) return 'api_key';
    if ((config.mcp?.autoAgents ?? []).length > 0) return 'mcp';
    return null;
  }

  let connectionType = $state<ConnectionType>(deriveConnectionType());
  let mcpMode = $state<McpMode>((config.mcp?.autoAgents ?? []).length > 0 ? 'active' : 'passive');

  function setConnectionType(type: ConnectionType) {
    connectionType = type;
    config.connectionType = type;
  }

  // Recording state
  let isRecording = $state(false);
  let recordingTarget = $state<RecordingTarget | null>(null);
  let mediaRecorder: MediaRecorder | null = null;
  let audioChunks: Blob[] = [];
  let recordingTimer = $state(0);
  let timerInterval: ReturnType<typeof setInterval> | null = null;
  let micOptions = $state<MicrophoneOption[]>([]);
  let selectedMicId = $state('');
  let micLoadState = $state<MicLoadState>('idle');
  let micStatusMessage = $state('');
  let selectedAssetId = $state('');
  let mcpStatus = $state<McpServerStatus | null>(null);
  let mcpStatusMessage = $state('');

  type McpAgentSnippet = {
    id: string;
    label: string;
    location: string;
    snippet: string;
  };

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

  const selectedEngine = $derived(config.engines.find(e => e.id === config.selectedEngineId));

  function asString(value: string | number | null | undefined): string {
    if (typeof value === 'string') return value;
    if (typeof value === 'number') return String(value);
    return '';
  }

  function getMicrowaveConfig(): NonNullable<AppConfig['microwave']> {
    if (!config.microwave) {
      config.microwave = {
        humId: null,
        dingId: null,
        muted: false
      };
    } else if (typeof config.microwave.muted !== 'boolean') {
      config.microwave.muted = false;
    }
    return config.microwave;
  }
  const microwave = $derived.by(() => getMicrowaveConfig());

  if (typeof config.freecadCmd !== 'string') {
    config.freecadCmd = '';
  }

  function getMcpConfig(): NonNullable<AppConfig['mcp']> {
    return config.mcp!;
  }

  function addAutoAgent() {
    const cur = config.mcp!;
    config.mcp = { ...cur, autoAgents: [...cur.autoAgents, { id: `agent-${Date.now()}`, label: '', cmd: '', args: [], enabled: true }] };
  }

  function removeAutoAgent(id: string) {
    const cur = config.mcp!;
    config.mcp = { ...cur, autoAgents: cur.autoAgents.filter(a => a.id !== id) };
  }

  function getAgentArgsString(args: string[]): string {
    return args.join(' ');
  }

  function setAgentArgsFromString(agent: AutoAgent, value: string) {
    agent.args = value.trim() ? value.trim().split(/\s+/) : [];
  }
  const mcpConfig = $derived(config.mcp!);

  const mcpEndpoint = $derived(mcpStatus?.endpointUrl || 'http://127.0.0.1:39249/mcp');

  const mcpAgentSnippets = $derived.by<McpAgentSnippet[]>(() => {
    const endpoint = mcpEndpoint;
    return [
      {
        id: 'gemini',
        label: 'GEMINI CLI',
        location: '~/.gemini/settings.json',
        snippet: JSON.stringify(
          {
            mcpServers: {
              ecky_mcp: {
                httpUrl: endpoint,
              },
            },
          },
          null,
          2,
        ),
      },
      {
        id: 'codex',
        label: 'CODEX',
        location: '~/.codex/config.toml',
        snippet: `[mcp_servers.ecky_mcp]\nenabled = true\nurl = "${endpoint}"\n`,
      },
      {
        id: 'claude',
        label: 'CLAUDE CODE',
        location: '.mcp.json or ~/.claude.json',
        snippet: JSON.stringify(
          {
            mcpServers: {
              ecky_mcp: {
                type: 'http',
                url: endpoint,
              },
            },
          },
          null,
          2,
        ),
      },
    ];
  });

  const genericMcpSnippet = $derived.by(() => {
    const endpoint = mcpStatus?.endpointUrl || 'http://127.0.0.1:39249/mcp';
    return JSON.stringify(
      {
        mcpServers: {
          ecky_mcp: {
            httpUrl: endpoint
          }
        }
      },
      null,
      2,
    );
  });

  async function refreshMcpStatus() {
    try {
      mcpStatus = await getMcpServerStatus();
      mcpStatusMessage = mcpStatus.running ? '' : (mcpStatus.lastStartupError || '');
    } catch (e: unknown) {
      mcpStatusMessage = `Status error: ${formatBackendError(e)}`;
    }
  }

  async function copyMcpSnippet(snippet: string, label: string) {
    try {
      await navigator.clipboard.writeText(snippet);
      mcpStatusMessage = `Copied ${label} MCP snippet.`;
    } catch (e: unknown) {
      mcpStatusMessage = `Copy failed: ${formatBackendError(e)}`;
    }
  }

  let mcpPollInterval: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    void refreshMcpStatus();
    mcpPollInterval = setInterval(() => void refreshMcpStatus(), 3000);
  });

  onDestroy(() => {
    if (mcpPollInterval) clearInterval(mcpPollInterval);
  });

  async function handleSave() {
    isSaving = true;
    message = 'Saving registry...';
    try {
      if (onsave) await onsave();
      message = 'Registry saved successfully.';
    } catch (e: unknown) {
      message = `Error: ${formatBackendError(e)}`;
    } finally {
      isSaving = false;
    }
  }

  async function addEngine() {
    const id = `engine-${Date.now()}`;
    const defaultPrompt = await getSystemPrompt();
    const newEngine = {
      id,
      name: 'New Engine',
      provider: 'gemini',
      apiKey: '',
      model: '',
      lightModel: '',
      baseUrl: '',
      systemPrompt: defaultPrompt,
      enabled: false,
    };
    config.engines = [...config.engines, newEngine];
    config.selectedEngineId = id;
    setConnectionType('api_key');
    activeSection = 'engines';
  }

  async function refreshMicInputs(requestPermission = true) {
    if (!navigator?.mediaDevices) {
      micLoadState = 'error';
      micStatusMessage = 'Media devices are unavailable in this webview.';
      micOptions = [];
      selectedMicId = '';
      return;
    }

    micLoadState = 'loading';
    micStatusMessage = '';

    if (requestPermission) {
      try {
        const temp = await navigator.mediaDevices.getUserMedia({ audio: true });
        temp.getTracks().forEach(track => track.stop());
      } catch (e: unknown) {
        micLoadState = 'error';
        micStatusMessage = `Microphone access failed: ${formatBackendError(e)}`;
        micOptions = [];
        selectedMicId = '';
        return;
      }
    }

    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      micOptions = devices
        .filter(d => d.kind === 'audioinput')
        .map((d, i) => ({
          id: d.deviceId,
          name: d.label || `Microphone ${i + 1}`
        }));
      selectedMicId = micOptions.some(m => m.id === selectedMicId) ? selectedMicId : (micOptions[0]?.id ?? '');
      micLoadState = 'ready';
      micStatusMessage = micOptions.length === 0
        ? 'No named microphone inputs were returned. Recording will use the system default input if available.'
        : '';
    } catch (e: unknown) {
      micLoadState = 'error';
      micStatusMessage = `Failed to enumerate microphones: ${formatBackendError(e)}`;
      micOptions = [];
      selectedMicId = '';
      console.warn('Failed to enumerate microphones:', e);
    }
  }

  async function loadMicInputs() {
    await refreshMicInputs(true);
  }

  async function uploadMicrowaveAudio(target: RecordingTarget) {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: 'Audio Files', extensions: ['mp3', 'wav', 'webm', 'ogg', 'm4a', 'aac', 'flac'] }
        ]
      });
      if (typeof selected !== 'string') return;

      const path = selected;
      const name = path.split(/[\/\\]/).pop() || path;
      const ext = (name.split('.').pop() || 'WAV').toUpperCase();

      const asset = await uploadAsset({
        sourcePath: path,
        name,
        format: ext
      });

      const microwaveConfig = getMicrowaveConfig();
      config.assets = [...(config.assets || []), asset];
      if (target === 'hum') microwaveConfig.humId = asset.id;
      if (target === 'ding') microwaveConfig.dingId = asset.id;
      message = `Uploaded and assigned ${target.toUpperCase()} sound: ${name}`;
    } catch (e: unknown) {
      message = `Upload failed: ${formatBackendError(e)}`;
    }
  }

  async function startRecording(target: RecordingTarget) {
    try {
      const constraints: MediaStreamConstraints = selectedMicId
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
          if (typeof reader.result !== 'string') {
            message = 'Failed to read recording buffer.';
            return;
          }
          const base64data = reader.result.split(',')[1] ?? '';
          const name = `Recording: ${target.toUpperCase()} (${new Date().toLocaleTimeString()})`;
          
          try {
            const asset = await saveRecordedAudio({
              base64Data: base64data,
              name
            });
            const microwaveConfig = getMicrowaveConfig();
            config.assets = [...(config.assets || []), asset];
            if (target === 'hum') microwaveConfig.humId = asset.id;
            if (target === 'ding') microwaveConfig.dingId = asset.id;
            message = `Recorded ${target} saved and assigned.`;
          } catch (e: unknown) {
            message = `Failed to save recording: ${formatBackendError(e)}`;
          }
        };
      };

      mediaRecorder.start();
      isRecording = true;
      timerInterval = setInterval(() => {
        recordingTimer++;
      }, 1000);
    } catch (e: unknown) {
      message = `Microphone error: ${formatBackendError(e)}`;
    }
  }

  function stopRecording() {
    if (mediaRecorder) {
      mediaRecorder.stop();
      mediaRecorder.stream.getTracks().forEach(track => track.stop());
    }
    isRecording = false;
    if (timerInterval) {
      clearInterval(timerInterval);
      timerInterval = null;
    }
  }

  function removeEngine(id: string) {
    config.engines = config.engines.filter(e => e.id !== id);
    if (config.selectedEngineId === id) {
      config.selectedEngineId = config.engines.length > 0 ? config.engines[0].id : '';
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

      if (typeof selected === 'string') {
        const path = selected;
        const ext = (path.split('.').pop() || '').toUpperCase();
        const name = path.split(/[\/\\]/).pop() || path;
        
        const asset = await uploadAsset({
          sourcePath: path,
          name,
          format: ext
        });

        config.assets = [...(config.assets || []), asset];
        selectedAssetId = asset.id;
        message = `Asset ${name} added.`;
      }
    } catch (e: unknown) {
      message = `Upload failed: ${formatBackendError(e)}`;
    }
  }

  function removeAsset(id: string) {
    config.assets = config.assets.filter(a => a.id !== id);
    if (selectedAssetId === id) {
      selectedAssetId = '';
    }
  }

  async function refreshModels() {
    if (!onfetch) return;
    try {
      await onfetch();
    } catch (e: unknown) {
      message = `Model fetch failed: ${formatBackendError(e)}`;
    }
  }

  async function handleProviderChange() {
    if (selectedEngine) {
      selectedEngine.model = '';
      selectedEngine.lightModel = '';
    }
    await refreshModels();
  }
  async function resetPrompt() {
    if (selectedEngine) {
      selectedEngine.systemPrompt = await getSystemPrompt();
    }
  }
</script>

<div class="config-container">
  <aside class="config-sidebar">
    <button class="nav-item {activeSection === 'prompts' ? 'active' : ''}" onclick={() => activeSection = 'prompts'}>PROMPTS</button>
    <button class="nav-item {activeSection === 'freecad' ? 'active' : ''}" onclick={() => activeSection = 'freecad'}>FREECAD</button>
    <button class="nav-item {activeSection === 'sounds' ? 'active' : ''}" onclick={() => activeSection = 'sounds'}>SOUNDS</button>
    <button class="nav-item {activeSection === 'agents' || activeSection === 'engines' ? 'active' : ''}" onclick={() => activeSection = 'agents'}>AGENTS</button>
  </aside>

  <main class="engine-details">
    <div class="details-scrollable">
      <div class="details-content">

      {#if activeSection === 'prompts'}
        {#if config.engines.length === 0}
          <div class="no-engine">
            <p>No engines configured. Add an engine in AGENTS first.</p>
          </div>
        {:else}
          {#if selectedEngine}
            <div class="field prompt-field">
              <div class="prompt-header">
                <label for="e-prompt-top">SYSTEM PROMPT — {selectedEngine.name || 'selected engine'}</label>
                <button class="btn btn-xs" onclick={resetPrompt}>RESET TO DEFAULT</button>
              </div>
              <textarea
                id="e-prompt-top"
                class="input-mono system-prompt-input"
                spellcheck="false"
                bind:value={selectedEngine.systemPrompt}
                placeholder="Template for LLM. Use $USER_PROMPT as placeholder for user intent."
              ></textarea>
            </div>
          {/if}
        {/if}

      {:else if activeSection === 'freecad'}
        <div class="field">
          <div class="prompt-header">
            <label for="freecad-cmd">FREECAD COMMAND / APP</label>
            <button class="btn btn-xs btn-ghost" onclick={() => config.freecadCmd = ''}>AUTO DISCOVER</button>
          </div>
          <input
            id="freecad-cmd"
            type="text"
            class="input-mono"
            placeholder="/Applications/FreeCAD.app or /Applications/FreeCAD.app/Contents/Resources/bin/freecadcmd"
            bind:value={config.freecadCmd}
          />
          <div class="field-help">
            Leave blank to auto-detect via `FREECAD_CMD`, PATH, or standard macOS FreeCAD locations.
          </div>
        </div>

      {:else if activeSection === 'sounds'}
        <div class="field">
          <div class="field-title">MIC INPUT</div>
          <div class="mic-device-block">
            <div class="mic-device-header">
              <button
                class="btn btn-xs btn-ghost"
                onclick={loadMicInputs}
                disabled={isRecording || micLoadState === 'loading'}
              >
                {micLoadState === 'idle' ? 'LOAD INPUTS' : '↻ RESCAN'}
              </button>
            </div>
            {#if micLoadState === 'idle'}
              <div class="mic-status">Load inputs only when needed. Recording uses system default if not loaded.</div>
            {:else if micLoadState === 'loading'}
              <div class="mic-status">Scanning microphones...</div>
            {:else if micLoadState === 'error'}
              <div class="mic-status mic-status-error">{micStatusMessage}</div>
            {:else if micOptions.length > 1}
              <div class="mic-device-row">
                <Dropdown options={micOptions} value={selectedMicId} onchange={(val) => selectedMicId = asString(val)} placeholder="Select microphone..." disabled={isRecording} />
              </div>
            {:else if micOptions.length === 1}
              <div class="mic-status">Using `{micOptions[0].name}`.</div>
            {:else}
              <div class="mic-status">{micStatusMessage}</div>
            {/if}
          </div>
        </div>

        <div class="field">
          <div class="prompt-header">
            <div class="field-title">AUDIO LIBRARY</div>
            <button class="btn btn-xs" onclick={addAsset}>+ UPLOAD</button>
          </div>
          {#if (config.assets || []).length > 0}
            <div class="asset-list">
              {#each (config.assets || []) as asset}
                <div class="asset-row">
                  <span class="asset-name">{asset.name}</span>
                  <span class="asset-fmt">{asset.format}</span>
                  <button class="btn btn-xs btn-ghost" onclick={() => removeAsset(asset.id)} title="Remove">✕</button>
                </div>
              {/each}
            </div>
          {:else}
            <div class="field-help">No audio files uploaded yet.</div>
          {/if}
        </div>

        <div class="field">
          <div class="field-title">COOKING HUM</div>
          <div class="sound-role">
            <div class="role-actions">
              {#if isRecording && recordingTarget === 'hum'}
                <button class="btn btn-xs btn-danger pulse" onclick={stopRecording}>⏹ STOP ({recordingTimer}s)</button>
              {:else}
                <button class="btn btn-xs" onclick={() => startRecording('hum')} disabled={isRecording}>🎤 RECORD</button>
              {/if}
              <button class="btn btn-xs btn-ghost" onclick={() => uploadMicrowaveAudio('hum')} disabled={isRecording}>📁 UPLOAD</button>
              {#if microwave.humId}
                <button class="btn btn-xs btn-ghost" onclick={() => microwave.humId = null}>✕ CLEAR</button>
              {/if}
            </div>
            {#if microwave.humId}
              {@const asset = config.assets?.find(a => a.id === microwave.humId)}
              <span class="assigned-name">{asset?.name || 'Assigned'}</span>
            {:else}
              <span class="field-help">Plays while a generation is in progress.</span>
            {/if}
          </div>
        </div>

        <div class="field">
          <div class="field-title">DONE DING</div>
          <div class="sound-role">
            <div class="role-actions">
              {#if isRecording && recordingTarget === 'ding'}
                <button class="btn btn-xs btn-danger pulse" onclick={stopRecording}>⏹ STOP ({recordingTimer}s)</button>
              {:else}
                <button class="btn btn-xs" onclick={() => startRecording('ding')} disabled={isRecording}>🎤 RECORD</button>
              {/if}
              <button class="btn btn-xs btn-ghost" onclick={() => uploadMicrowaveAudio('ding')} disabled={isRecording}>📁 UPLOAD</button>
              {#if microwave.dingId}
                <button class="btn btn-xs btn-ghost" onclick={() => microwave.dingId = null}>✕ CLEAR</button>
              {/if}
            </div>
            {#if microwave.dingId}
              {@const asset = config.assets?.find(a => a.id === microwave.dingId)}
              <span class="assigned-name">{asset?.name || 'Assigned'}</span>
            {:else}
              <span class="field-help">Plays when a render completes.</span>
            {/if}
          </div>
        </div>

      {:else if activeSection === 'agents'}
        <div class="field">
          <div class="field-title">CONNECTION TYPE</div>
          <div class="conn-type-row">
            <button
              class="conn-type-btn {connectionType === 'api_key' ? 'active' : ''}"
              onclick={() => setConnectionType('api_key')}
            >API KEY</button>
            <button
              class="conn-type-btn {connectionType === 'mcp' ? 'active' : ''}"
              onclick={() => setConnectionType('mcp')}
            >MCP</button>
          </div>
        </div>

        {#if connectionType === 'api_key'}
          <div class="field">
            <div class="prompt-header">
              <div class="field-title">API ENGINES</div>
              <button class="btn btn-xs" onclick={addEngine}>+ ADD</button>
            </div>
            {#if config.engines.length === 0}
              <div class="no-engine">
                <p>No engines configured yet.</p>
                <button class="btn btn-primary" onclick={addEngine}>ADD FIRST MODEL</button>
              </div>
            {:else}
              <div class="engine-list">
                {#each config.engines as engine}
                  <button
                    class="engine-card {config.selectedEngineId === engine.id ? 'selected' : ''} {engine.enabled ? '' : 'disabled'}"
                    onclick={() => { config.selectedEngineId = engine.id; activeSection = 'engines'; }}
                  >
                    <span class="engine-card__name">{engine.name || '(unnamed)'}</span>
                    <span class="engine-card__meta">
                      {engine.provider}{engine.model ? ' · ' + engine.model : ''}
                      {#if !engine.enabled}<span class="engine-card__off">OFF</span>{/if}
                    </span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>

        {:else if connectionType === 'mcp'}
          <div class="field">
            <div class="field-title">MODE</div>
            <div class="conn-type-row">
              <button
                class="conn-type-btn {mcpMode === 'passive' ? 'active' : ''}"
                onclick={() => mcpMode = 'passive'}
              >PASSIVE</button>
              <button
                class="conn-type-btn {mcpMode === 'active' ? 'active' : ''}"
                onclick={() => mcpMode = 'active'}
              >ACTIVE</button>
            </div>
            <div class="field-help">
              {mcpMode === 'passive'
                ? 'External agents (Claude Code, Gemini CLI, Codex) connect to Ecky\'s MCP server.'
                : 'Ecky launches agents automatically on startup.'}
            </div>
          </div>

          {#if mcpMode === 'passive'}
            <div class="field">
              <div class="field-row">
                <div class="field flex-1">
                  <label for="mcp-port">PORT</label>
                  <input
                    id="mcp-port"
                    type="number"
                    class="input-mono"
                    min="1024"
                    max="65535"
                    placeholder="39249 (default)"
                    value={mcpConfig.port ?? ''}
                    oninput={(e) => {
                      const v = (e.currentTarget as HTMLInputElement).valueAsNumber;
                      getMcpConfig().port = isNaN(v) ? null : v;
                    }}
                  />
                </div>
                <div class="field flex-1">
                  <label for="mcp-max-sessions">MAX SESSIONS</label>
                  <input
                    id="mcp-max-sessions"
                    type="number"
                    class="input-mono"
                    min="1"
                    max="16"
                    placeholder="Unlimited"
                    value={mcpConfig.maxSessions ?? ''}
                    oninput={(e) => {
                      const v = (e.currentTarget as HTMLInputElement).valueAsNumber;
                      getMcpConfig().maxSessions = isNaN(v) ? null : v;
                    }}
                  />
                </div>
              </div>
              <div class="field-help">Server starts on launch. Requires restart to take effect.</div>
            </div>

            <div class="field">
              <div class="prompt-header">
                <div class="field-title">CONNECT YOUR AGENT</div>
                <button class="btn btn-xs" onclick={() => copyMcpSnippet(genericMcpSnippet, 'generic JSON')}>COPY GENERIC JSON</button>
              </div>
              <div class="mcp-status-row">
                <span class:mcp-running={mcpStatus?.running} class:mcp-stopped={!mcpStatus?.running}>
                  {mcpStatus?.running ? 'RUNNING' : 'STOPPED'}
                </span>
                <span class="mcp-endpoint">{mcpStatus?.endpointUrl || 'http://127.0.0.1:39249/mcp'}</span>
              </div>
              <div class="mcp-agent-grid">
                {#each mcpAgentSnippets as agent (agent.id)}
                  <div class="mcp-agent-card">
                    <div class="mcp-agent-card__head">
                      <span class="mcp-agent-card__label">{agent.label}</span>
                      <button class="btn btn-xs" onclick={() => copyMcpSnippet(agent.snippet, agent.label)}>COPY</button>
                    </div>
                    <div class="mcp-agent-card__path">{agent.location}</div>
                  </div>
                {/each}
              </div>
              {#if mcpStatusMessage}
                <div class="field-note">{mcpStatusMessage}</div>
              {/if}
              {#if mcpStatus?.lastStartupError}
                <div class="field-note">Last startup error: {mcpStatus.lastStartupError}</div>
              {/if}
            </div>

          {:else}
            <div class="field">
              <div class="prompt-header">
                <div class="field-title">AUTO-AGENTS</div>
                <button class="btn btn-xs" onclick={addAutoAgent}>+ ADD</button>
              </div>
              <div class="field-help">Processes Ecky launches on startup (e.g. Codex, Gemini CLI). Requires restart.</div>
              {#if mcpConfig.autoAgents && mcpConfig.autoAgents.length > 0}
                <div class="auto-agent-list">
                  {#each mcpConfig.autoAgents as agent (agent.id)}
                    <div class="auto-agent-row">
                      <input type="text" class="input-mono auto-agent-label" placeholder="Label" bind:value={agent.label} />
                      <input type="text" class="input-mono auto-agent-cmd" placeholder="Command (e.g. codex)" bind:value={agent.cmd} />
                      <input
                        type="text"
                        class="input-mono auto-agent-args"
                        placeholder="Args (space-separated)"
                        value={getAgentArgsString(agent.args)}
                        oninput={(e) => setAgentArgsFromString(agent, (e.currentTarget as HTMLInputElement).value)}
                      />
                      <label class="auto-agent-toggle" title="Enabled">
                        <input type="checkbox" bind:checked={agent.enabled} />
                        ON
                      </label>
                      <button class="btn btn-xs btn-ghost" onclick={() => removeAutoAgent(agent.id)} title="Remove">✕</button>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="field-note">No auto-agents configured.</div>
              {/if}
            </div>
          {/if}

        {:else}
          <div class="no-engine">
            <p>Choose how this instance connects to an AI.</p>
          </div>
        {/if}

      {:else if activeSection === 'engines' && selectedEngine}
          <button class="back-link" onclick={() => activeSection = 'agents'}>← AGENTS</button>
          <div class="engine-enabled-row">
            <label class="engine-enabled-toggle">
              <input type="checkbox" bind:checked={selectedEngine.enabled} />
              <span class="toggle-label">{selectedEngine.enabled ? 'LIVE — API calls enabled' : 'DISABLED — no API calls will be made'}</span>
            </label>
            {#if !selectedEngine.enabled}
              <div class="field-help">Enable to allow this engine to send requests to the provider API.</div>
            {/if}
          </div>
          <div class="field-row">
            <div class="field flex-2">
              <label for="e-name">DISPLAY NAME</label>
              <input 
                id="e-name" 
                type="text" 
                class="input-mono" 
                placeholder="e.g. My Gemini" 
                bind:value={selectedEngine.name}
              />
            </div>
            <div class="field flex-1">
              <label for="e-provider">PROVIDER</label>
              <Dropdown 
                options={providers} 
                value={selectedEngine.provider} 
                onchange={async (val) => { selectedEngine.provider = asString(val); await handleProviderChange(); }} 
              />
            </div>
          </div>

          <div class="field">
            <label for="e-key">API KEY</label>
            <input 
              id="e-key" 
              type="password" 
              class="input-mono" 
              placeholder="Enter API key..." 
              bind:value={selectedEngine.apiKey}
              onblur={refreshModels}
            />
          </div>

          <div class="field-row">
            <div class="field flex-1">
              <div class="prompt-header">
                <label for="e-model">RENDER AND HEAVY REASONING</label>
                <button class="btn btn-xs btn-ghost" onclick={refreshModels} disabled={isLoadingModels}>
                  ↻ FETCH MODELS
                </button>
              </div>
              <Dropdown 
                options={availableModels.length > 0 ? availableModels : (selectedEngine.model ? [selectedEngine.model] : [])} 
                value={selectedEngine.model} 
                placeholder={isLoadingModels ? "Fetching..." : "Fetch models first..."} 
                onchange={(val) => selectedEngine.model = asString(val)}
              />
            </div>
            <div class="field flex-1">
              <label for="e-light-model">LIGHT REASONING</label>
              <Dropdown
                options={availableModels.length > 0 ? availableModels : (selectedEngine.lightModel ? [selectedEngine.lightModel] : (selectedEngine.model ? [selectedEngine.model] : []))}
                value={selectedEngine.lightModel}
                placeholder={isLoadingModels ? "Fetching..." : "Optional (falls back to heavy model)"}
                onchange={(val) => selectedEngine.lightModel = asString(val)}
              />
              <div class="field-note">Used for text-only intent checks. Image-bearing requests fall back to the main model.</div>
            </div>
          </div>

          <div class="field">
            <label for="e-baseurl">BASE URL (OPTIONAL)</label>
            <input 
              id="e-baseurl" 
              type="text" 
              class="input-mono" 
              placeholder="Default" 
              bind:value={selectedEngine.baseUrl}
              onblur={refreshModels}
            />
          </div>

          <div class="field prompt-field">
            <div class="prompt-header">
              <label for="e-prompt">SYSTEM PROMPT (template with $USER_PROMPT)</label>
              <button class="btn btn-xs" onclick={resetPrompt}>RESET TO DEFAULT</button>
            </div>
            <textarea 
              id="e-prompt" 
              class="input-mono system-prompt-input" 
              spellcheck="false"
              bind:value={selectedEngine.systemPrompt}
              placeholder="Template for LLM. Use $USER_PROMPT as placeholder for user intent."
            ></textarea>
          </div>

          <div class="danger-zone">
            <button class="btn btn-xs btn-ghost" onclick={() => removeEngine(selectedEngine.id)}>REMOVE ENGINE</button>
          </div>
      {:else if activeSection === 'engines'}
        <div class="no-engine">
          <p>No engine selected. Add one to begin.</p>
          <button class="btn btn-primary" onclick={addEngine}>ADD FIRST ENGINE</button>
        </div>
      {/if}
      </div>
    </div>

    <div class="config-footer">
      <span class="status-msg">{message}</span>
      <button class="btn btn-primary" onclick={handleSave} disabled={isSaving}>
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

  .nav-item {
    width: 100%;
    padding: 10px 12px;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--bg-300);
    color: var(--text-dim);
    font-size: 0.65rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    cursor: pointer;
  }

  .nav-item:hover {
    background: var(--bg-200);
    color: var(--text);
  }

  .nav-item.active {
    background: var(--bg-200);
    color: var(--primary);
    border-left: 2px solid var(--primary);
    padding-left: 10px;
  }

  .engine-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .engine-card {
    width: 100%;
    padding: 10px 12px;
    text-align: left;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    cursor: pointer;
  }

  .engine-card:hover {
    border-color: var(--primary);
  }

  .engine-card.selected {
    border-color: var(--primary);
    background: var(--bg-300);
  }

  .engine-card__name {
    font-size: 0.75rem;
    font-weight: bold;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .engine-card__meta {
    font-size: 0.6rem;
    color: var(--text-dim);
    font-family: var(--font-mono);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .engine-enabled-row {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 12px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .engine-enabled-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-size: 0.68rem;
    font-weight: bold;
    letter-spacing: 0.06em;
  }

  .toggle-label {
    color: var(--text);
  }

  .engine-card.disabled {
    opacity: 0.5;
  }

  .engine-card__off {
    margin-left: 6px;
    font-size: 0.55rem;
    font-weight: bold;
    color: var(--text-dim);
    letter-spacing: 0.08em;
    border: 1px solid var(--bg-400);
    padding: 1px 4px;
  }

  .back-link {
    background: none;
    border: none;
    color: var(--text-dim);
    font-size: 0.62rem;
    font-weight: bold;
    letter-spacing: 0.06em;
    cursor: pointer;
    padding: 0;
    text-align: left;
  }

  .back-link:hover {
    color: var(--primary);
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

  .field-help {
    font-size: 0.65rem;
    color: var(--text-dim);
    line-height: 1.4;
  }

  .field-note {
    font-size: 0.62rem;
    color: var(--text-dim);
    line-height: 1.35;
  }

  .flex-1 { flex: 1; }
  .flex-2 { flex: 2; }

  label {
    font-size: 0.65rem;
    color: var(--text-dim);
    font-weight: bold;
    letter-spacing: 0.05em;
  }

  .field-title {
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

  .mcp-status-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .mcp-running,
  .mcp-stopped {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 74px;
    padding: 2px 8px;
    border: 1px solid var(--bg-300);
    font-size: 0.62rem;
    font-weight: bold;
    letter-spacing: 0.06em;
    background: var(--bg-200);
  }

  .mcp-running {
    border-color: var(--secondary);
    color: var(--secondary);
  }

  .mcp-stopped {
    border-color: var(--primary);
    color: var(--primary);
  }

  .mcp-endpoint {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 0.68rem;
    color: var(--text-dim);
  }

  .mcp-agent-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 10px;
  }

  .mcp-agent-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    overflow: hidden;
  }

  .mcp-agent-card__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .mcp-agent-card__label {
    font-size: 0.66rem;
    font-weight: bold;
    letter-spacing: 0.06em;
    color: var(--text);
  }

  .mcp-agent-card__path {
    font-family: var(--font-mono);
    font-size: 0.62rem;
    color: var(--text-dim);
    word-break: break-word;
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

  .mic-device-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .mic-device-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .mic-device-row :global(.custom-select) {
    flex: 1;
  }

  .asset-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .asset-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
  }

  .asset-name {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .asset-fmt {
    font-size: 0.6rem;
    font-weight: bold;
    color: var(--secondary);
    letter-spacing: 0.05em;
    flex-shrink: 0;
  }

  .mic-status {
    font-size: 0.65rem;
    line-height: 1.4;
    color: var(--text-dim);
  }

  .mic-status-error {
    color: var(--red);
    white-space: pre-wrap;
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

  .auto-agent-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .auto-agent-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .auto-agent-label { width: 110px; flex-shrink: 0; }
  .auto-agent-cmd   { width: 140px; flex-shrink: 0; }
  .auto-agent-args  { flex: 1; min-width: 0; }

  .conn-type-row {
    display: flex;
    gap: 0;
  }

  .conn-type-btn {
    flex: 1;
    padding: 7px 12px;
    font-size: 0.65rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    border: 1px solid var(--bg-400);
    background: var(--bg-200);
    color: var(--text-dim);
    cursor: pointer;
  }

  .conn-type-btn + .conn-type-btn {
    border-left: none;
  }

  .conn-type-btn:hover {
    background: var(--bg-300);
    color: var(--text);
  }

  .conn-type-btn.active {
    background: var(--bg-300);
    color: var(--primary);
    border-color: var(--primary);
  }

  .auto-agent-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 0.6rem;
    font-weight: bold;
    color: var(--text-dim);
    letter-spacing: 0.05em;
    flex-shrink: 0;
    cursor: pointer;
    white-space: nowrap;
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
