import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { session } from '../stores/sessionStore';
import { workingCopy } from '../stores/workingCopy';
import { handleParamChange } from '../controllers/manualController';
import { paramPanelState } from '../stores/paramPanelState';
import type { AppConfig, DesignOutput, Thread } from '../types/domain';
import { 
  history, 
  activeThreadId, 
  activeVersionId, 
  config,
  availableModels,
  isLoadingModels
} from '../stores/domainState';

type TauriBridgeWindow = Window & typeof globalThis & {
  __TAURI_INTERNALS__?: {
    invoke?: unknown;
  };
};

function hasTauriInvokeBridge(): boolean {
  if (typeof window === 'undefined') return true;
  const bridge = (window as TauriBridgeWindow).__TAURI_INTERNALS__;
  return typeof bridge?.invoke === 'function';
}

/**
 * Main boot sequence for the application.
 * Restores configuration, history, and the last active design.
 */
export async function boot() {
  session.setPhase('booting');
  session.setStatus('Restoring environment...');

  const bootWatchdog = typeof window !== 'undefined'
    ? window.setTimeout(() => {
        if (get(session).phase === 'booting') {
          console.warn('[Boot] watchdog tripped; switching to interactive mode.');
          session.setPhase('idle');
          session.setStatus('System ready.');
        }
      }, 1500)
    : 0;

  if (!hasTauriInvokeBridge()) {
    session.setPhase('idle');
    session.setStatus('System ready.');
    if (bootWatchdog) window.clearTimeout(bootWatchdog);
    return;
  }
  
  try {
    // 1. Load Config (Idempotent)
    await loadConfig();
    
    // 2. Load History
    await loadHistory();
    
    // 3. Restore Last Design (Render preview only, no persistence write)
    await restoreLastDesign();
    
    session.setPhase('idle');
    session.setStatus('System ready.');
  } catch (e) {
    console.error('[Boot] failed:', e);
    session.setPhase('error');
    session.setError('Boot failed: ' + e);
  } finally {
    if (bootWatchdog) window.clearTimeout(bootWatchdog);
  }
}

async function loadConfig() {
  const loadedConfig = await invoke<AppConfig>('get_config');
  let needsSave = false;

  // Normalize engines
  if (loadedConfig.engines?.length > 0) {
    const hasSelectedEngine = loadedConfig.engines.some((e) => e.id === loadedConfig.selectedEngineId);
    if (!hasSelectedEngine) {
      loadedConfig.selectedEngineId = loadedConfig.engines[0].id;
      needsSave = true;
    }
  }

  // Normalize microwave settings
  if (!loadedConfig.microwave || typeof loadedConfig.microwave.muted !== 'boolean') {
    loadedConfig.microwave = {
      humId: loadedConfig.microwave?.humId ?? null,
      dingId: loadedConfig.microwave?.dingId ?? null,
      muted: false
    };
    needsSave = true;
  }

  config.set(loadedConfig);
  
  if (loadedConfig.selectedEngineId) {
    fetchModels().catch((e) => {
      console.warn('[Boot] Deferred model fetch failed:', e);
    });
  }

  // Only write if we actually repaired/normalized something
  if (needsSave) {
    await invoke('save_config', { config: loadedConfig });
  }
}

export async function saveConfig() {
  const currentConfig = get(config);
  try {
    await invoke('save_config', { config: currentConfig });
    session.setStatus('Configuration saved.');
  } catch (e) {
    session.setError(`Config Save Error: ${e}`);
  }
}

export async function fetchModels() {
  const currentConfig = get(config);
  const selectedEngine = currentConfig.engines.find((e) => e.id === currentConfig.selectedEngineId);
  
  if (!selectedEngine) return;
  if (!selectedEngine.apiKey && selectedEngine.provider !== 'ollama') {
    availableModels.set([]);
    return;
  }
  
  isLoadingModels.set(true);
  try {
    const modelsRaw = await invoke<unknown>('list_models', {
      provider: selectedEngine.provider,
      apiKey: selectedEngine.apiKey,
      baseUrl: selectedEngine.baseUrl
    });
    const models = Array.isArray(modelsRaw)
      ? modelsRaw.filter((m): m is string => typeof m === 'string')
      : [];
    availableModels.set(models);

    if (models.length > 0 && (!selectedEngine.model || !models.includes(selectedEngine.model))) {
      selectedEngine.model = models[0];
      config.set(currentConfig);
      await invoke('save_config', { config: currentConfig });
    }
  } catch (e) {
    console.error("[Config] Failed to fetch models:", e);
    availableModels.set([]);
    session.setError(`Engine Error: ${e}`); 
  } finally {
    isLoadingModels.set(false);
  }
}

async function loadHistory() {
  const freshHistory = await invoke<Thread[]>('get_history');
  history.set(freshHistory);
  
  const tid = get(activeThreadId);
  if (tid && !freshHistory.some(t => t.id === tid)) {
    activeThreadId.set(null);
    activeVersionId.set(null);
  }
}

async function restoreLastDesign() {
  try {
    const lastRaw = await invoke<unknown>('get_last_design');
    const last = normalizeLastDesign(lastRaw);
    if (last) {
      const [design, threadId] = last;
      let restoredFromThread = false;

      if (threadId) {
        try {
          const thread = await invoke<Thread>('get_thread', { id: threadId });
          const lastAssistantMsg = thread
            ? [...thread.messages].reverse().find(m => m.role === 'assistant' && m.output)
            : null;

          if (lastAssistantMsg) {
            activeThreadId.set(threadId);
            activeVersionId.set(lastAssistantMsg.id);
            workingCopy.loadVersion(lastAssistantMsg.output, lastAssistantMsg.id);
            paramPanelState.hydrateFromVersion(lastAssistantMsg.output, lastAssistantMsg.id);
            restoredFromThread = true;
          }
        } catch (e) {
          console.warn("[Boot] Could not load full thread during restore:", e);
        }
      }

      if (!restoredFromThread) {
        workingCopy.loadVersion(design, null);
        paramPanelState.hydrateFromVersion(design, null);
        activeThreadId.set(threadId);
      }

      // Render preview (persist=false to avoid boot-time DB writes)
      const panel = get(paramPanelState);
      await handleParamChange(panel.params, panel.macroCode, false);
    } else {
      await fetchDefaultMacro();
    }
  } catch (e) {
    console.error("[Boot] Failed to restore last design:", e);
    await fetchDefaultMacro();
  }
}

function normalizeLastDesign(payload: unknown): [DesignOutput, string | null] | null {
  if (!payload) return null;

  if (Array.isArray(payload)) {
    const design = payload[0] as DesignOutput | undefined;
    if (!design) return null;
    const threadId = payload[1];
    return [
      design,
      typeof threadId === 'string' ? threadId : null,
    ];
  }

  if (typeof payload === 'object') {
    const data = payload as Record<string, unknown>;
    const design = data.design as DesignOutput | undefined;
    if (!design) return null;
    const threadId =
      typeof data.threadId === 'string'
        ? data.threadId
        : typeof data.thread_id === 'string'
          ? data.thread_id
          : null;
    return [design, threadId];
  }

  return null;
}

async function fetchDefaultMacro() {
  try {
    const code = await invoke<string>('get_default_macro');
    if (!get(workingCopy).macroCode) {
      workingCopy.patch({ macroCode: code });
      paramPanelState.hydrate({
        versionId: null,
        macroCode: code,
        uiSpec: { fields: [] },
        params: {}
      });
    }
  } catch (e) {
    console.error("[Boot] Failed to load default macro:", e);
  }
}
