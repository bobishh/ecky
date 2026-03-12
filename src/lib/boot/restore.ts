import { get } from 'svelte/store';
import { session } from '../stores/sessionStore';
import { workingCopy } from '../stores/workingCopy';
import { paramPanelState } from '../stores/paramPanelState';
import { clearLastSessionSnapshot, persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import {
  history,
  activeThreadId,
  activeVersionId,
  config,
  availableModels,
  isLoadingModels,
  freecadAvailable,
} from '../stores/domainState';
import {
  checkFreecad,
  formatBackendError,
  getConfig,
  getDefaultMacro,
  getHistory,
  getLastDesign,
  getThread,
  listModels,
  saveConfig as persistConfig,
} from '../tauri/client';
import { loadVersion } from '../stores/history';

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

    // 2. Check FreeCAD availability (non-blocking, best-effort)
    checkFreecad().then(available => freecadAvailable.set(available)).catch(() => freecadAvailable.set(false));

    // 3. Load History
    await loadHistory();

    // 4. Restore Last Design (Render preview only, no persistence write)
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
  const loadedConfig = await getConfig();
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

  if (typeof loadedConfig.freecadCmd !== 'string') {
    loadedConfig.freecadCmd = '';
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
    await persistConfig(loadedConfig);
  }
}

export async function saveConfig() {
  const currentConfig = get(config);
  try {
    await persistConfig(currentConfig);
    session.setStatus('Configuration saved.');
  } catch (e) {
    session.setError(`Config Save Error: ${formatBackendError(e)}`);
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
    const models = await listModels(
      selectedEngine.provider,
      selectedEngine.apiKey,
      selectedEngine.baseUrl,
    );
    availableModels.set(models);

    if (models.length > 0 && (!selectedEngine.model || !models.includes(selectedEngine.model))) {
      selectedEngine.model = models[0];
      config.set(currentConfig);
      await persistConfig(currentConfig);
    }
  } catch (e) {
    console.error("[Config] Failed to fetch models:", e);
    availableModels.set([]);
    session.setError(`Engine Error: ${formatBackendError(e)}`); 
  } finally {
    isLoadingModels.set(false);
  }
}

async function loadHistory() {
  const freshHistory = await getHistory();
  history.set(freshHistory);
  
  const tid = get(activeThreadId);
  if (tid && !freshHistory.some(t => t.id === tid)) {
    await resetToBlankSession(true);
  }
}

async function restoreLastDesign() {
  try {
    const last = await getLastDesign();
    if (!last?.threadId || !last?.messageId) {
      await resetToBlankSession(Boolean(last));
      await fetchDefaultMacro();
      return;
    }

    const historyThread = get(history).find((thread) => thread.id === last.threadId);
    if (!historyThread) {
      await resetToBlankSession(true);
      await fetchDefaultMacro();
      return;
    }

    const freshThread = await getThread(last.threadId);
    history.update((items) =>
      items.map((thread) =>
        thread.id === freshThread.id ? { ...thread, messages: freshThread.messages } : thread,
      ),
    );

    const targetMessage = freshThread.messages.find(
      (message) =>
        message.id === last.messageId &&
        message.role === 'assistant' &&
        (message.output || message.artifactBundle),
    );

    if (!targetMessage) {
      await resetToBlankSession(true);
      await fetchDefaultMacro();
      return;
    }

    activeThreadId.set(last.threadId);
    await loadVersion(targetMessage);

    if (last.selectedPartId) {
      session.setSelectedPartId(last.selectedPartId);
      await persistLastSessionSnapshot({ selectedPartId: last.selectedPartId });
    }
  } catch (e) {
    console.error("[Boot] Failed to restore last design:", e);
    await resetToBlankSession(true);
    await fetchDefaultMacro();
  }
}

async function fetchDefaultMacro() {
  try {
    const code = await getDefaultMacro();
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

async function resetToBlankSession(clearSnapshot: boolean) {
  activeThreadId.set(null);
  activeVersionId.set(null);
  workingCopy.reset();
  paramPanelState.reset();
  session.setStlUrl(null);
  if (clearSnapshot) {
    await clearLastSessionSnapshot();
  }
}
