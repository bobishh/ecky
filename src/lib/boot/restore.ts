import { get } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { session } from '../stores/sessionStore';
import { workingCopy } from '../stores/workingCopy';
import { handleParamChange } from '../controllers/manualController';
import { paramPanelState } from '../stores/paramPanelState';
import { 
  history, 
  activeThreadId, 
  activeVersionId, 
  config,
  availableModels,
  isLoadingModels
} from '../stores/domainState';
import {
  formatBackendError,
  getConfig,
  getDefaultMacro,
  getHistory,
  getLastDesign,
  listModels,
  saveConfig as persistConfig,
} from '../tauri/client';

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

function toAssetUrl(path: string | null | undefined): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
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
    activeThreadId.set(null);
    activeVersionId.set(null);
  }
}

async function restoreLastDesign() {
  try {
    const last = await getLastDesign();
    if (last) {
      const { design, threadId, messageId, artifactBundle, modelManifest, selectedPartId } = last;

      activeThreadId.set(threadId);
      activeVersionId.set(messageId);

      if (design) {
        workingCopy.loadVersion(design, messageId);
        paramPanelState.hydrateFromVersion(design, messageId);
      } else {
        workingCopy.reset();
        paramPanelState.reset();
      }

      if (artifactBundle) {
        session.setStlUrl(toAssetUrl(artifactBundle.previewStlPath));
        session.setModelRuntime(artifactBundle, modelManifest);
        session.setSelectedPartId(selectedPartId);
      } else {
        session.clearModelRuntime();
        if (design) {
          const panel = get(paramPanelState);
          await handleParamChange(panel.params, panel.macroCode, false);
        } else {
          session.setStlUrl(null);
        }
      }
    } else {
      await fetchDefaultMacro();
    }
  } catch (e) {
    console.error("[Boot] Failed to restore last design:", e);
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
