<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Dropdown from './Dropdown.svelte';
  import VertexGenie from './VertexGenie.svelte';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { derivePrimaryAgentId, normalizeMcpMode } from './agents/state';
  import { inferModelCapabilities } from './modelRuntime/modelCapabilities';
  import {
    formatBackendError,
    getAppLogs,
    getDesignSystemPrompt,
    getMcpServerStatus,
    exportEckyMcpSkillZip,
    listAgentModels,
    saveRecordedAudio,
    uploadAsset,
    type AppLogEntry,
  } from './tauri/client';
  import type {
    AppConfig,
    AutoAgent,
    McpServerStatus,
    RuntimeCapabilities,
    GenieTraits,
  } from './types/domain';

  type ActiveSection = 'app' | 'agents' | 'engines';
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
    runtimeCapabilities = null,
    eckyTraits = null,
    onRerollEcky,
    onfetch,
    onsave,
  }: {
    config: AppConfig;
    availableModels?: string[];
    isLoadingModels?: boolean;
    runtimeCapabilities?: RuntimeCapabilities | null;
    eckyTraits?: Partial<GenieTraits> | null;
    onRerollEcky?: (() => void) | null;
    onfetch?: () => Promise<void> | void;
    onsave?: () => Promise<void> | void;
  } = $props();

  let isSaving = $state(false);
  let message = $state('');
  let activeSection = $state<ActiveSection>('agents');

  // Eagerly initialize mcp config so derived below is always non-null
  if (!config.mcp) config.mcp = { port: null, maxSessions: null, mode: 'passive', primaryAgentId: null, promptTimeoutSecs: 1800, eckyAstAuthoring: false, autoAgents: [] };
  if (!Array.isArray(config.mcp.autoAgents)) config.mcp.autoAgents = [];
  if (typeof config.mcp.eckyAstAuthoring !== 'boolean') config.mcp.eckyAstAuthoring = false;
  if (!config.mcp.mode) config.mcp.mode = normalizeMcpMode(config.mcp.mode, config.mcp.autoAgents);
  if (config.mcp.primaryAgentId === undefined) {
    config.mcp.primaryAgentId = derivePrimaryAgentId(config.mcp.autoAgents, config.mcp.primaryAgentId);
  }
  if (!Number.isFinite(config.mcp.promptTimeoutSecs)) {
    config.mcp.promptTimeoutSecs = 1800;
  }

  function deriveConnectionType(): ConnectionType {
    if (config.connectionType === 'mcp') return 'mcp';
    if (config.connectionType === 'api_key') return 'api_key';
    // fallback heuristic for configs without persisted connectionType
    if (config.engines.some(e => e.enabled)) return 'api_key';
    if ((config.mcp?.autoAgents ?? []).length > 0) return 'mcp';
    return null;
  }

  function ensurePrimaryAutoAgent() {
    if (!config.mcp) return;
    const nextPrimaryAgentId = derivePrimaryAgentId(config.mcp.autoAgents ?? [], config.mcp.primaryAgentId);
    if (nextPrimaryAgentId === config.mcp.primaryAgentId) return;
    config = {
      ...config,
      mcp: {
        ...config.mcp,
        primaryAgentId: nextPrimaryAgentId,
      },
    };
  }

  const connectionType = $derived.by<ConnectionType>(() => deriveConnectionType());
  const mcpMode = $derived.by<McpMode>(() =>
    normalizeMcpMode(config.mcp?.mode, config.mcp?.autoAgents ?? []),
  );

  function setConnectionType(type: ConnectionType) {
    config = {
      ...config,
      connectionType: type,
    };
  }

  function setMcpMode(mode: McpMode) {
    const currentMcp = getMcpConfig();
    config = {
      ...config,
      mcp: {
        ...currentMcp,
        mode,
      },
    };
    ensurePrimaryAutoAgent();
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
  let appLogs = $state<AppLogEntry[]>([]);
  let logsLoading = $state(false);
  let skillExporting = $state(false);
  let designSystemPrompt = $state('');
  let designSystemPromptError = $state('');

  type McpAgentSnippet = {
    id: string;
    label: string;
    location: string;
    snippet: string;
  };
  type AutoAgentPreset = {
    id: string;
    label: string;
    cmd: string;
    model: string | null;
    args: string[];
  };

  const eckyMcpToolNames = [
    'bootstrap_ecky',
    'health_check',
    'workspace_overview',
    'session_log_in',
    'session_log_out',
    'resume_session',
    'thread_list',
    'thread_create',
    'thread_borrow',
    'thread_get',
    'agent_identity_set',
    'target_meta_get',
    'target_macro_get',
    'macro_buffer_get',
    'macro_buffer_replace_range',
    'macro_buffer_apply_patch',
    'macro_buffer_preview_render',
    'target_detail_get',
    'target_get',
    'get_model_screenshot',
    'params_preview_render',
    'macro_preview_render',
    'macro_buffer_replace_and_preview',
    'semantic_manifest_get',
    'control_primitive_save',
    'control_primitive_delete',
    'control_view_save',
    'control_view_delete',
    'measurement_annotation_save',
    'measurement_annotation_delete',
    'commit_preview_version',
    'thread_fork_from_target',
    'version_restore',
    'user_confirm_request',
    'request_user_prompt',
    'long_action_notice',
    'long_action_clear',
    'finalize_thread',
  ];

  const providers = [
    { id: 'gemini', name: 'Google Gemini' },
    { id: 'openai', name: 'OpenAI (or Compatible)' },
    { id: 'ollama', name: 'Ollama (Local)' }
  ];

  // Per-agent fetched model lists (keyed by agent id)
  let agentModelLists = $state<Record<string, string[]>>({});
  let agentModelIsLive = $state<Record<string, boolean>>({});
  let agentModelFetching = $state<Record<string, boolean>>({});

  async function fetchAgentModels(agent: AutoAgent) {
    if (!agent.cmd.trim()) return;
    agentModelFetching = { ...agentModelFetching, [agent.id]: true };
    try {
      const result = await listAgentModels(agent.cmd);
      agentModelLists = { ...agentModelLists, [agent.id]: result.models };
      agentModelIsLive = { ...agentModelIsLive, [agent.id]: result.isLive };
      if (result.models.length > 0 && !agent.model) agent.model = result.models[0];
    } catch {
      // ignore — user can retry
    } finally {
      agentModelFetching = { ...agentModelFetching, [agent.id]: false };
    }
  }

  function syncAgentModelsFromApiConfig(agent: AutoAgent) {
    if (!availableModels.length) return;
    agentModelLists = { ...agentModelLists, [agent.id]: availableModels };
    agentModelIsLive = { ...agentModelIsLive, [agent.id]: true };
    if (!agent.model && availableModels.length > 0) agent.model = availableModels[0];
  }

  // Auto-fetch models for all agents when the agents section is shown
  let autoFetchDone = false;
  $effect(() => {
    if (activeSection !== 'agents' || autoFetchDone) return;
    if (mcpMode !== 'active') return;
    autoFetchDone = true;
    const agents = config.mcp?.autoAgents ?? [];
    for (const agent of agents) {
      if (agent.cmd.trim() && !agentModelLists[agent.id]) {
        void fetchAgentModels(agent);
      }
    }
  });

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
  const selectedEngineCapabilities = $derived.by(() => {
    if (!selectedEngine) return null;
    return inferModelCapabilities(
      selectedEngine.provider,
      selectedEngine.baseUrl,
      selectedEngine.model,
    );
  });
  const freecadCapability = $derived(runtimeCapabilities?.freecad ?? null);
  const build123dCapability = $derived(runtimeCapabilities?.build123d ?? null);
  const directOcctCapability = $derived(runtimeCapabilities?.directOcct ?? null);
  const selectedEnginePromptCarrier = $derived.by(() => {
    switch (selectedEngine?.provider) {
      case 'gemini':
        return 'GEMINI SYSTEM INSTRUCTION';
      case 'ollama':
      case 'openai':
      default:
        return 'OPENAI / OLLAMA SYSTEM MESSAGE';
    }
  });

  function setDefaultFreecadContext() {
    if (!freecadCapability?.available) return;
    config.defaultSourceLanguage = 'legacyPython';
    config.defaultGeometryBackend = 'freecad';
    config.defaultEngineKind = 'freecad';
  }

  function setDefaultBuild123dContext() {
    if (!build123dCapability?.available) return;
    config.defaultSourceLanguage = 'build123d';
    config.defaultGeometryBackend = 'build123d';
    config.defaultEngineKind = 'build123d';
  }

  function setDefaultEckyIrContext() {
    config.defaultSourceLanguage = 'ecky';
    if (config.defaultGeometryBackend === 'build123d' && !build123dCapability?.available) {
      config.defaultGeometryBackend = freecadCapability?.available ? 'freecad' : 'mesh';
    } else if (config.defaultGeometryBackend === 'freecad' && !freecadCapability?.available) {
      config.defaultGeometryBackend = build123dCapability?.available ? 'build123d' : 'mesh';
    } else if (!config.defaultGeometryBackend) {
      config.defaultGeometryBackend = build123dCapability?.available
        ? 'build123d'
        : freecadCapability?.available
          ? 'freecad'
          : 'mesh';
    }
    config.defaultEngineKind = 'ecky';
  }

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

  function getVoiceConfig(): AppConfig['voice'] {
    if (!config.voice || !config.voice.sttLanguageCode?.trim()) {
      config.voice = { sttLanguageCode: 'en-US' };
    }
    return config.voice;
  }
  const voice = $derived.by(() => getVoiceConfig());

  if (typeof config.freecadCmd !== 'string') {
    config.freecadCmd = '';
  }

  if (typeof config.cadTextFontPath !== 'string') {
    config.cadTextFontPath = '';
  }

  function getMcpConfig(): NonNullable<AppConfig['mcp']> {
    return config.mcp!;
  }

  async function fetchLogs() {
    logsLoading = true;
    try {
      appLogs = await getAppLogs();
    } catch {
      // ignore
    } finally {
      logsLoading = false;
    }
  }

  $effect(() => {
    if (activeSection === 'app') {
      void fetchLogs();
    }
  });

  function addAutoAgent() {
    const cur = config.mcp!;
    const nextAgent = { id: `agent-${Date.now()}`, label: '', cmd: '', model: null, args: [], enabled: true, startOnDemand: false };
    config.mcp = { ...cur, autoAgents: [...cur.autoAgents, nextAgent] };
    if (!config.mcp.primaryAgentId) {
      config.mcp.primaryAgentId = nextAgent.id;
    }
  }

  function addAutoAgentPreset(preset: AutoAgentPreset) {
    const cur = config.mcp!;
    const nextAgent = {
      id: `agent-${Date.now()}`,
      label: preset.label,
      cmd: preset.cmd,
      model: preset.model,
      args: [...preset.args],
      enabled: true,
      startOnDemand: false,
    };
    config.mcp = { ...cur, autoAgents: [...cur.autoAgents, nextAgent] };
    if (!config.mcp.primaryAgentId) {
      config.mcp.primaryAgentId = nextAgent.id;
    }
  }

  function removeAutoAgent(id: string) {
    const cur = config.mcp!;
    config.mcp = { ...cur, autoAgents: cur.autoAgents.filter(a => a.id !== id) };
    ensurePrimaryAutoAgent();
  }

  function getAgentArgsString(args: string[]): string {
    return args.join(' ');
  }

  function setAgentArgsFromString(agent: AutoAgent, value: string) {
    agent.args = value.trim() ? value.trim().split(/\s+/) : [];
  }
  const mcpConfig = $derived(config.mcp!);
  const primaryAgentOptions = $derived((mcpConfig.autoAgents ?? []).filter((agent) => agent.enabled));

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
            tools: {
              allowed: eckyMcpToolNames.map((tool) => `mcp_ecky_mcp_${tool}`),
            },
            mcpServers: {
              ecky_mcp: {
                httpUrl: endpoint,
                trust: true,
                includeTools: eckyMcpToolNames,
              },
            },
          },
          null,
          2,
        ),
      },
      {
        id: 'amp',
        label: 'AMP',
        location: '~/.config/amp/settings.json',
        snippet: JSON.stringify(
          {
            'amp.mcpServers': {
              ecky_mcp: {
                url: endpoint,
              },
            },
            'amp.tools.enable': ['mcp__ecky_mcp__*'],
          },
          null,
          2,
        ),
      },
      {
        id: 'codex',
        label: 'CODEX',
        location: '~/.codex/config.toml',
        snippet: `model = "gpt-5.4"\n\n[mcp_servers.ecky_mcp]\nenabled = true\nurl = "${endpoint}"\n`,
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
      {
        id: 'opencode',
        label: 'OPENCODE',
        location: '~/.config/opencode/opencode.json',
        snippet: JSON.stringify(
          {
            $schema: 'https://opencode.ai/config.json',
            mcp: {
              ecky_mcp: {
                type: 'remote',
                enabled: true,
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

  const autoAgentPresets: AutoAgentPreset[] = [
    {
      id: 'claude',
      label: 'CLAUDE',
      cmd: 'claude',
      model: null,
      args: ['--allowedTools', 'Read', 'LS', 'Glob', 'Grep', 'mcp__ecky_mcp__*'],
    },
    { id: 'gemini', label: 'GEMINI', cmd: 'gemini', model: null, args: [] },
    { id: 'codex', label: 'CODEX', cmd: 'codex', model: 'gpt-5.4', args: [] },
    {
      id: 'amp',
      label: 'AMP',
      cmd: 'amp',
      model: null,
      args: ['--settings-file', '/Users/bogdan/.config/amp/settings.json'],
    },
    { id: 'opencode', label: 'OPENCODE', cmd: 'opencode', model: null, args: [] },
  ];

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

  async function handleExportEckyMcpSkillZip() {
    skillExporting = true;
    message = 'Exporting Ecky MCP skill...';
    try {
      const targetPath = await save({
        defaultPath: 'ecky-mcp-skill.zip',
        filters: [{ name: 'Zip Archive', extensions: ['zip'] }],
      });
      if (!targetPath) {
        message = 'Skill export cancelled.';
        return;
      }
      await exportEckyMcpSkillZip(targetPath);
      message = 'Ecky MCP skill exported.';
    } catch (e: unknown) {
      message = `Skill export failed: ${formatBackendError(e)}`;
    } finally {
      skillExporting = false;
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
      const voiceConfig = getVoiceConfig();
      voiceConfig.sttLanguageCode = voiceConfig.sttLanguageCode.trim() || 'en-US';
      if (onsave) await onsave();
      message = 'Registry saved successfully.';
    } catch (e: unknown) {
      message = `Error: ${formatBackendError(e)}`;
    } finally {
      isSaving = false;
    }
  }

  async function toggleAudioMute() {
    const currentMicrowave = getMicrowaveConfig();
    const nextMuted = !currentMicrowave.muted;
    config = {
      ...config,
      microwave: {
        ...currentMicrowave,
        muted: nextMuted,
      },
    };
    if (onsave) {
      await onsave();
    }
  }

  async function refreshDesignSystemPrompt(provider: string | null | undefined) {
    designSystemPromptError = '';
    try {
      designSystemPrompt = (await getDesignSystemPrompt(provider ?? null)) || '';
    } catch (e: unknown) {
      designSystemPrompt = '';
      designSystemPromptError = `Prompt load failed: ${formatBackendError(e)}`;
    }
  }

  async function copyDesignSystemPrompt() {
    try {
      await navigator.clipboard.writeText(designSystemPrompt);
      message = `Copied ${selectedEngine?.name || 'engine'} system prompt.`;
    } catch (e: unknown) {
      message = `Copy failed: ${formatBackendError(e)}`;
    }
  }

  $effect(() => {
    if (!selectedEngine) {
      designSystemPrompt = '';
      designSystemPromptError = '';
      return;
    }
    void refreshDesignSystemPrompt(selectedEngine.provider);
  });

  function addEngine() {
    const id = `engine-${Date.now()}`;
    const newEngine = {
      id,
      name: 'New Engine',
      provider: 'gemini',
      apiKey: '',
      model: '',
      lightModel: '',
      baseUrl: '',
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
          { name: 'All Assets', extensions: ['stl', 'step', 'stp', 'png', 'jpg', 'jpeg', 'webp', 'svg', 'json'] }
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

  async function pickCadTextFont() {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: 'Font Files', extensions: ['ttf', 'otf', 'ttc'] }
        ]
      });
      if (typeof selected === 'string') {
        config.cadTextFontPath = selected;
      }
    } catch (e: unknown) {
      message = `Font picker failed: ${formatBackendError(e)}`;
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
</script>

<div class="config-container">
  <aside class="config-sidebar">
    <button class="nav-item {activeSection === 'app' ? 'active' : ''}" onclick={() => activeSection = 'app'}>APP</button>
    <button class="nav-item {activeSection === 'agents' || activeSection === 'engines' ? 'active' : ''}" onclick={() => activeSection = 'agents'}>AGENTS</button>
  </aside>

  <main class="engine-details">
    <div class="details-scrollable">
      <div class="details-content">
      {#if activeSection === 'app'}
        <section class="ecky-settings-card" aria-label="Ecky settings preview">
          <div class="ecky-settings-card__copy">
            <div class="field-title">ECKY PREVIEW</div>
            <div class="field-help">Seed override lives here now. Workbench corner stays clean.</div>
            <div class="ecky-settings-card__actions">
              <button
                class="btn btn-xs"
                type="button"
                aria-label="Reroll Ecky seed"
                onclick={() => onRerollEcky?.()}
              >
                REROLL
              </button>
            </div>
          </div>
          <div class="ecky-settings-card__preview" data-testid="settings-ecky-preview">
            <VertexGenie mode="idle" bubble="" traits={eckyTraits} safeRightInset={0} />
          </div>
        </section>
        {#if runtimeCapabilities}
          <div class="field">
            <div class="field-title">RUNTIME STATUS</div>
            <div class="field-help">BUILD123D: {runtimeCapabilities.build123d.detail}</div>
            <div class="field-help">FREECAD: {runtimeCapabilities.freecad.detail}</div>
            <div class="field-help">NATIVE: {runtimeCapabilities.mesh.detail}</div>
          </div>
        {/if}

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

        <div class="field">
          <div class="prompt-header">
            <label for="cad-text-font-path">CAD TEXT FONT</label>
            <div class="button-row">
              <button class="btn btn-xs btn-ghost" type="button" onclick={pickCadTextFont}>CHOOSE</button>
              <button class="btn btn-xs btn-ghost" type="button" onclick={() => config.cadTextFontPath = ''}>AUTO</button>
            </div>
          </div>
          <input
            id="cad-text-font-path"
            type="text"
            class="input-mono"
            placeholder="/System/Library/Fonts/Supplemental/Arial Black.ttf"
            bind:value={config.cadTextFontPath}
          />
          <div class="field-help">
            Used for Ecky <code>text</code> geometry in FreeCAD renders. Blank uses the built-in bold fallback list.
          </div>
        </div>

        <div class="field">
          <label for="max-generation-attempts">MAX GENERATION ATTEMPTS</label>
          <input
            id="max-generation-attempts"
            type="number"
            class="input-mono"
            min="1"
            max="10"
            bind:value={config.maxGenerationAttempts}
          />
          <div class="field-help">
            Max LLM repair retries per request. Default: 3.
          </div>
        </div>

        <div class="field">
          <label for="max-verify-attempts">MAX SCREENSHOT VERIFY ATTEMPTS</label>
          <input
            id="max-verify-attempts"
            type="number"
            class="input-mono"
            min="0"
            max="5"
            bind:value={config.maxVerifyAttempts}
          />
          <div class="field-help">
            VLM screenshot verification rounds after render. 0 = disabled (structural check always runs). Default: 2.
          </div>
        </div>

        <div class="field">
          <div class="field-title">AUDIO</div>
          <div class="conn-type-row">
            <button
              class="conn-type-btn {microwave.muted ? 'active' : ''}"
              type="button"
              aria-pressed={microwave.muted}
              onclick={toggleAudioMute}
            >
              {microwave.muted ? 'AUDIO OFF' : 'AUDIO ON'}
            </button>
          </div>
          <div class="field-help">Controls Ecky speech and microwave playback. Save registry to persist.</div>
        </div>

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
          <label for="stt-language-code">STT LANGUAGE CODE</label>
          <input
            id="stt-language-code"
            type="text"
            class="input-mono"
            spellcheck="false"
            bind:value={voice.sttLanguageCode}
          />
          <div class="field-help">BCP-47 code passed to speech backend. Default: en-US.</div>
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

        <div class="field">
          <div class="prompt-header">
            <div class="field-title">SUPERVISOR LOGS</div>
            <button class="btn btn-xs btn-ghost" onclick={fetchLogs} disabled={logsLoading}>
              {logsLoading ? '…' : '↻ REFRESH'}
            </button>
          </div>
          <div class="field-help">Runtime logs from agent supervisor. Shows last 200 entries.</div>
          {#if appLogs.length === 0}
            <div class="field-note">{logsLoading ? 'Loading...' : 'No log entries.'}</div>
          {:else}
            <div class="log-list">
              {#each [...appLogs].reverse() as entry}
                <div class="log-entry">
                  <span class="log-ts">{new Date(entry.tsMs).toLocaleTimeString()}</span>
                  <span class="log-msg">{entry.message}</span>
                </div>
              {/each}
            </div>
          {/if}
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

        <div class="field">
          <div class="field-title">DEFAULT AUTHORING CONTEXT</div>
          <div class="field-help" style="margin-bottom: 6px;">SOURCE</div>
          <div class="conn-type-row">
            <button
              class="conn-type-btn {config.defaultSourceLanguage === 'legacyPython' ? 'active' : ''}"
              onclick={setDefaultFreecadContext}
              disabled={runtimeCapabilities ? !runtimeCapabilities.freecad.available : false}
              title={runtimeCapabilities && !runtimeCapabilities.freecad.available ? runtimeCapabilities.freecad.detail : undefined}
            >FREECAD PYTHON</button>
            <button
              class="conn-type-btn {config.defaultSourceLanguage === 'build123d' ? 'active' : ''}"
              onclick={setDefaultBuild123dContext}
              disabled={runtimeCapabilities ? !runtimeCapabilities.build123d.available : false}
              title={runtimeCapabilities && !runtimeCapabilities.build123d.available ? runtimeCapabilities.build123d.detail : undefined}
            >BUILD123D PYTHON</button>
            <button
              class="conn-type-btn {config.defaultSourceLanguage === 'ecky' ? 'active' : ''}"
              onclick={setDefaultEckyIrContext}
            >ECKY</button>
          </div>
          {#if config.defaultSourceLanguage === 'ecky'}
            <div class="field-help" style="margin-top: 8px; margin-bottom: 6px;">BACKEND FOR ECKY</div>
            <div class="conn-type-row" style="margin-top: 6px;">
              <button
                class="conn-type-btn {config.defaultGeometryBackend === 'freecad' ? 'active' : ''}"
                onclick={() => { config.defaultGeometryBackend = 'freecad'; }}
                disabled={runtimeCapabilities ? !runtimeCapabilities.freecad.available : false}
                title={runtimeCapabilities && !runtimeCapabilities.freecad.available ? runtimeCapabilities.freecad.detail : undefined}
              >FREECAD</button>
              <button
                class="conn-type-btn {config.defaultGeometryBackend === 'build123d' ? 'active' : ''}"
                onclick={() => { config.defaultGeometryBackend = 'build123d'; }}
                disabled={runtimeCapabilities ? !runtimeCapabilities.build123d.available : false}
                title={runtimeCapabilities && !runtimeCapabilities.build123d.available ? runtimeCapabilities.build123d.detail : undefined}
              >BUILD123D</button>
              <button
                class="conn-type-btn {config.defaultGeometryBackend === 'mesh' ? 'active' : ''}"
                onclick={() => { config.defaultGeometryBackend = 'mesh'; }}
              >NATIVE</button>
            </div>
            {#if directOcctCapability}
              <div class="direct-occt-fastpath" aria-label="Direct OCCT STEP fast path">
                <div class="direct-occt-fastpath__head">
                  <span>DIRECT OCCT STEP FAST PATH</span>
                  <strong class:direct-occt-fastpath__state--ready={directOcctCapability.available}>
                    {directOcctCapability.available ? 'READY' : 'BLOCKED'}
                  </strong>
                </div>
                <div class="direct-occt-fastpath__detail">{directOcctCapability.detail}</div>
              </div>
            {/if}
          {/if}
          <div class="field-help">
            New generated threads inherit this source and backend by default. Imported FCStd threads stay FreeCAD Python.
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
                onclick={() => setMcpMode('passive')}
              >PASSIVE</button>
              <button
                class="conn-type-btn {mcpMode === 'active' ? 'active' : ''}"
                onclick={() => setMcpMode('active')}
              >ACTIVE</button>
            </div>
            <div class="field-help">
              {mcpMode === 'passive'
                ? 'External agents (Claude Code, Gemini CLI, Codex) connect to Ecky\'s MCP server.'
                : 'Ecky wakes the primary agent when a queued message arrives, then hibernates it between turns.'}
            </div>
          </div>

          <div class="field">
            <label for="mcp-prompt-timeout">PROMPT TIMEOUT (SECONDS)</label>
            <input
              id="mcp-prompt-timeout"
              type="number"
              class="input-mono"
              min="10"
              max="1800"
              value={mcpConfig.promptTimeoutSecs ?? 1800}
              oninput={(e) => {
                const v = (e.currentTarget as HTMLInputElement).valueAsNumber;
                getMcpConfig().promptTimeoutSecs = !Number.isFinite(v)
                  ? 1800
                  : Math.min(1800, Math.max(10, v));
              }}
            />
            <div class="field-help">
              Default wait used by <code>request_user_prompt</code> when the agent does not pass <code>timeoutSecs</code>.
            </div>
          </div>

          <div class="field mcp-ast-authoring-field">
            <label class="mcp-ast-authoring-toggle" title="Expose experimental Ecky AST authoring MCP tools">
              <input
                aria-label="ECKY AST AUTHORING"
                type="checkbox"
                bind:checked={mcpConfig.eckyAstAuthoring}
              />
              <span class="tgl-track"></span>
              <span class="toggle-label">ECKY AST AUTHORING</span>
            </label>
            <div class="field-help">
              Enables AST MCP tools for Ecky source and disables macro buffer edits while active.
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
                <div class="prompt-actions">
                  <button class="btn btn-xs" onclick={() => copyMcpSnippet(genericMcpSnippet, 'generic JSON')}>COPY GENERIC JSON</button>
                  <button class="btn btn-xs btn-ghost" onclick={handleExportEckyMcpSkillZip} disabled={skillExporting}>
                    {skillExporting ? 'EXPORTING...' : 'EXPORT SKILL ZIP'}
                  </button>
                </div>
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
              <div class="field-help">Processes Ecky can wake on demand (e.g. Codex, Gemini CLI). Requires restart to take effect.</div>
              <div class="auto-agent-presets">
                {#each autoAgentPresets as preset (preset.id)}
                  <button class="btn btn-xs btn-ghost" onclick={() => addAutoAgentPreset(preset)}>
                    + {preset.label}
                  </button>
                {/each}
              </div>
              <div class="field-help">Presets fill safe cmd/args for Ecky. Gemini/Amp home-config snippets below scope MCP access; keep AMP model empty because its CLI uses mode flags instead of <code>--model</code>.</div>
              <div class="field-row">
                <div class="field flex-1">
                  <label for="mcp-primary-agent">PRIMARY AGENT</label>
                  <select
                    id="mcp-primary-agent"
                    class="input-mono"
                    value={mcpConfig.primaryAgentId ?? ''}
                    onchange={(e) => {
                      getMcpConfig().primaryAgentId = (e.currentTarget as HTMLSelectElement).value || null;
                    }}
                    disabled={primaryAgentOptions.length === 0}
                  >
                    <option value="">No enabled agents</option>
                    {#each primaryAgentOptions as agent}
                      <option value={agent.id}>{agent.label || agent.cmd || agent.id}</option>
                    {/each}
                  </select>
                  <div class="field-help">
                    After you save, only the selected primary agent will receive the next queued turn.
                  </div>
                </div>
              </div>
              {#if mcpConfig.autoAgents && mcpConfig.autoAgents.length > 0}
                <div class="auto-agent-list">
                  {#each mcpConfig.autoAgents as agent (agent.id)}
                    {@const modelOpts = agentModelLists[agent.id] ?? []}
                    {@const isLive = agentModelIsLive[agent.id]}
                    {@const isFetching = !!agentModelFetching[agent.id]}
                    <div class="auto-agent-card">
                      <!-- Row 1: label / cmd / toggles / remove -->
                      <div class="aac-row aac-row-top">
                        <input type="text" class="input-mono aac-label" placeholder="Label" bind:value={agent.label} />
                        <input type="text" class="input-mono aac-cmd" placeholder="Command (e.g. gemini)" bind:value={agent.cmd} />
                        <label class="aac-toggle" title="Enabled for active MCP wake">
                          <input type="checkbox" bind:checked={agent.enabled} onchange={ensurePrimaryAutoAgent} />
                          <span class="tgl-track"></span>
                          <span class="tgl-label">ON</span>
                        </label>
                        <button class="btn btn-xs btn-ghost aac-remove" onclick={() => removeAutoAgent(agent.id)} title="Remove">✕</button>
                      </div>
                      <!-- Row 2: model select + fetch + sync + fallback badge -->
                      <div class="aac-row aac-row-model">
                        <span class="aac-field-label">MODEL</span>
                        {#if modelOpts.length > 0}
                          <select
                            class="input-mono aac-model-select"
                            value={agent.model ?? ''}
                            onchange={(e) => { agent.model = (e.currentTarget as HTMLSelectElement).value || null; }}
                          >
                            <option value="">auto</option>
                            {#each modelOpts as m}
                              <option value={m}>{m}</option>
                            {/each}
                          </select>
                        {/if}
                        <input
                          type="text"
                          class="input-mono aac-model-input"
                          placeholder={modelOpts.length > 0 ? 'or type custom model ID' : 'Model ID (optional)'}
                          value={agent.model ?? ''}
                          oninput={(e) => { agent.model = (e.currentTarget as HTMLInputElement).value || null; }}
                        />
                        <button
                          class="btn btn-xs btn-ghost"
                          onclick={() => fetchAgentModels(agent)}
                          disabled={isFetching}
                          title="Fetch live models from CLI tool env (reads GEMINI_API_KEY / ANTHROPIC_API_KEY / OPENAI_API_KEY)"
                        >{isFetching ? '…' : '↻ Fetch'}</button>
                        {#if availableModels.length > 0}
                          <button
                            class="btn btn-xs btn-ghost"
                            onclick={() => syncAgentModelsFromApiConfig(agent)}
                            title="Copy models from the currently configured API engine"
                          >Sync API</button>
                        {/if}
                        {#if isLive === false}
                          <span class="aac-fallback-badge" title="These are static fallback models — no API key found in env">fallback</span>
                        {/if}
                      </div>
                      <!-- Row 3: args only -->
                      <div class="aac-row aac-row-args">
                        <span class="aac-field-label">ARGS</span>
                        <input
                          type="text"
                          class="input-mono aac-args"
                          placeholder="Extra args (e.g. -y --sandbox)"
                          value={getAgentArgsString(agent.args)}
                          oninput={(e) => setAgentArgsFromString(agent, (e.currentTarget as HTMLInputElement).value)}
                        />
                      </div>
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
              <span class="tgl-track"></span>
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

          {#if selectedEngineCapabilities && selectedEngineCapabilities.reason}
            <div class="engine-capability-hint" data-testid="engine-vision-warning">
              {selectedEngineCapabilities.reason}
            </div>
          {/if}

          <div class="field engine-system-prompt" data-testid="engine-system-prompt">
            <div class="prompt-header">
              <div class="field-title">SYSTEM PROMPT</div>
              <button
                class="btn btn-xs btn-ghost"
                onclick={copyDesignSystemPrompt}
                disabled={!designSystemPrompt}
              >
                COPY SYSTEM PROMPT
              </button>
            </div>
            <div class="field-note" data-testid="engine-system-prompt-carrier">
              {selectedEnginePromptCarrier}
            </div>
            <pre
              class="input-mono system-prompt-preview"
              aria-label="SYSTEM PROMPT"
              aria-readonly="true"
              data-testid="engine-system-prompt-code"
              tabindex="0"
            ><code>{designSystemPrompt}</code></pre>
            {#if designSystemPromptError}
              <div class="field-note">{designSystemPromptError}</div>
            {/if}
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

  .toggle-label {
    font-size: 0.68rem;
    font-weight: bold;
    letter-spacing: 0.06em;
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

  .ecky-settings-card {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 150px;
    gap: 18px;
    align-items: center;
    padding: 14px 16px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    overflow: hidden;
  }

  .ecky-settings-card__copy {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .ecky-settings-card__actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .ecky-settings-card__preview {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 150px;
    overflow: hidden;
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

  @media (max-width: 860px) {
    .ecky-settings-card {
      grid-template-columns: 1fr;
      justify-items: start;
    }
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

  .engine-capability-hint {
    padding: 10px 12px;
    border: 1px solid var(--primary);
    border-left-width: 2px;
    background: var(--bg-200);
    color: var(--text);
    font-size: 0.65rem;
    line-height: 1.45;
    overflow: hidden;
  }

  .system-prompt-preview {
    width: 100%;
    min-height: 220px;
    max-height: 360px;
    margin: 0;
    padding: 12px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text);
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
    overflow: auto;
    outline: none;
  }

  .system-prompt-preview:focus {
    border-color: var(--primary);
  }

  .system-prompt-preview code {
    font: inherit;
    color: inherit;
  }

  .direct-occt-fastpath {
    margin-top: 8px;
    padding: 9px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    overflow: hidden;
  }

  .direct-occt-fastpath__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    color: var(--text-dim);
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.08em;
  }

  .direct-occt-fastpath__head strong {
    color: var(--red);
    white-space: nowrap;
  }

  .direct-occt-fastpath__head strong.direct-occt-fastpath__state--ready {
    color: var(--secondary);
  }

  .direct-occt-fastpath__detail {
    margin-top: 5px;
    color: var(--text-dim);
    font-size: 0.62rem;
    line-height: 1.35;
    overflow-wrap: anywhere;
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

  input {
    padding: 8px 12px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.8rem;
    outline: none;
    font-family: var(--font-mono);
    width: 100%;
  }

  input:focus {
    border-color: var(--primary);
  }

  .prompt-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    margin-bottom: 6px;
    overflow: hidden;
  }

  .prompt-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
    min-width: 0;
    overflow: hidden;
  }

  .button-row {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    min-width: 0;
    overflow: hidden;
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
    gap: 14px;
  }

  .auto-agent-presets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 8px 0 10px;
  }

  .auto-agent-card {
    display: flex;
    flex-direction: column;
    gap: 0;
    border: 1px solid var(--bg-300);
    border-left: 2px solid var(--primary);
    background: var(--bg-100);
    overflow: hidden;
  }

  .aac-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    border-bottom: 1px solid var(--bg-300);
    flex-wrap: nowrap;
    min-width: 0;
  }

  .aac-row:last-child {
    border-bottom: none;
  }

  .aac-row input[type="text"],
  .aac-row select {
    padding: 5px 9px;
    font-size: 0.75rem;
    min-width: 0;
  }

  .aac-label { width: 110px; flex-shrink: 0; }
  .aac-cmd   { flex: 1; min-width: 80px; }
  .aac-remove { flex-shrink: 0; }

  .aac-field-label {
    font-size: 0.55rem;
    font-weight: bold;
    letter-spacing: 0.07em;
    color: var(--text-dim);
    flex-shrink: 0;
    width: 38px;
  }

  .aac-model-select {
    width: 160px;
    flex-shrink: 0;
    cursor: pointer;
  }

  .aac-model-input {
    flex: 1;
    min-width: 80px;
  }

  .aac-args {
    flex: 1;
    min-width: 0;
  }

  .aac-toggle,
  .mcp-ast-authoring-toggle,
  .engine-enabled-toggle {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    cursor: pointer;
    user-select: none;
  }

  .aac-toggle input[type="checkbox"],
  .mcp-ast-authoring-toggle input[type="checkbox"],
  .engine-enabled-toggle input[type="checkbox"] {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
    pointer-events: none;
  }

  .tgl-track {
    position: relative;
    width: 30px;
    height: 16px;
    background: var(--bg-300);
    border: 1px solid #3a3a5a;
    border-radius: 9px;
    flex-shrink: 0;
    transition: background 0.15s, border-color 0.15s;
  }

  .tgl-track::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 10px;
    height: 10px;
    background: var(--text-dim);
    border-radius: 50%;
    transition: transform 0.15s, background 0.15s;
  }

  .aac-toggle:has(input:checked) .tgl-track,
  .mcp-ast-authoring-toggle:has(input:checked) .tgl-track,
  .engine-enabled-toggle:has(input:checked) .tgl-track {
    background: var(--primary);
    border-color: var(--primary);
  }

  .aac-toggle:has(input:checked) .tgl-track::after,
  .mcp-ast-authoring-toggle:has(input:checked) .tgl-track::after,
  .engine-enabled-toggle:has(input:checked) .tgl-track::after {
    transform: translateX(14px);
    background: #fff;
  }

  .tgl-label {
    font-size: 0.6rem;
    font-weight: bold;
    letter-spacing: 0.05em;
    color: var(--text-dim);
    white-space: nowrap;
  }

  .aac-toggle:has(input:checked) .tgl-label,
  .mcp-ast-authoring-toggle:has(input:checked) .toggle-label {
    color: var(--primary);
  }

  .aac-fallback-badge {
    font-size: 0.55rem;
    font-weight: bold;
    letter-spacing: 0.06em;
    color: var(--text-dim);
    border: 1px solid var(--bg-400);
    padding: 1px 5px;
    flex-shrink: 0;
  }

  .log-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 400px;
    overflow-y: auto;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    padding: 6px 8px;
  }

  .log-entry {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-family: var(--font-mono);
    font-size: 0.65rem;
    line-height: 1.5;
    min-width: 0;
  }

  .log-ts {
    color: var(--text-dim);
    flex-shrink: 0;
    font-size: 0.6rem;
  }

  .log-msg {
    color: var(--text);
    word-break: break-word;
    min-width: 0;
  }

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

  /* .auto-agent-toggle replaced by .aac-toggle */

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
