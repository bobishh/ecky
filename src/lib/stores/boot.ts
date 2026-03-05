import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { session, handleParamChange } from './sessionFlow';
import { workingCopy } from './workingCopy';
import { 
  history, 
  activeThreadId, 
  activeVersionId, 
  config,
  availableModels,
  isLoadingModels
} from './domainState';

export async function boot(appState) {
  session.setPhase('booting');
  session.setStatus('Restoring config, history, and active workspace...');
  
  try {
    // 1. Load Config
    await loadConfig();
    
    // 2. Load History
    await loadHistory();
    
    // 3. Restore Last Design
    await restoreLastDesign(appState);
    
    session.setPhase('idle');
    session.setStatus('System ready.');
  } catch (e) {
    console.error('Boot failed:', e);
    session.setPhase('error');
    session.setError('Boot failed: ' + e);
  }
}

export async function loadConfig() {
  try {
    const loadedConfig = await invoke('get_config');
    let configPatched = false;

    if (loadedConfig.engines?.length > 0) {
      const hasSelectedEngine = loadedConfig.engines.some(e => e.id === loadedConfig.selected_engine_id);
      if (!hasSelectedEngine) {
        loadedConfig.selected_engine_id = loadedConfig.engines[0].id;
        configPatched = true;
      }
    }

    if (!loadedConfig.microwave || typeof loadedConfig.microwave.muted !== 'boolean') {
      loadedConfig.microwave = {
        hum_id: loadedConfig.microwave?.hum_id ?? null,
        ding_id: loadedConfig.microwave?.ding_id ?? null,
        muted: false
      };
      configPatched = true;
    }

    config.set(loadedConfig);
    if (loadedConfig.selected_engine_id) {
      await fetchModels();
    }

    if (configPatched) {
      await invoke('save_config', { config: loadedConfig });
    }
  } catch (e) {
    session.setError(`Config Load Error: ${e}`);
    session.setPhase('error');
  }
}

export async function saveConfig() {
  const currentConfig = get(config);
  try {
    await invoke('save_config', { config: currentConfig });
    session.setStatus('Configuration saved.');
  } catch (e) {
    session.setError(`Config Save Error: ${e}`);
    session.setPhase('error');
  }
}

export async function fetchModels() {
  const currentConfig = get(config);
  const selectedEngine = currentConfig.engines.find(e => e.id === currentConfig.selected_engine_id);
  
  if (!selectedEngine) return;
  if (!selectedEngine.api_key && selectedEngine.provider !== 'ollama') {
    availableModels.set([]);
    return;
  }
  
  isLoadingModels.set(true);
  
  try {
    const models = await invoke('list_models', {
      provider: selectedEngine.provider,
      apiKey: selectedEngine.api_key,
      baseUrl: selectedEngine.base_url
    });
    availableModels.set(models);

    if (models.length > 0 && (!selectedEngine.model || !models.includes(selectedEngine.model))) {
      selectedEngine.model = models[0];
      config.set(currentConfig);
      await invoke('save_config', { config: currentConfig });
    }
  } catch (e) {
    console.error("Failed to fetch models:", e);
    availableModels.set([]);
    session.setError(`Engine Error: ${e}`); 
  } finally {
    isLoadingModels.set(false);
  }
}

async function loadHistory() {
  try {
    const freshHistory = await invoke('get_history');
    history.set(freshHistory);
    const tid = get(activeThreadId);
    if (tid && !freshHistory.some(t => t.id === tid)) {
      activeThreadId.set(null);
      activeVersionId.set(null);
    }
  } catch (e) {
    console.error("Failed to load history:", e);
  }
}

async function restoreLastDesign(appState) {
  try {
    const last = await invoke('get_last_design');
    if (last) {
      const [design, threadId] = last;
      let restoredFromThread = false;

      if (threadId) {
        try {
          const thread = await invoke('get_thread', { id: threadId });
          const lastAssistantMsg = thread
            ? [...thread.messages].reverse().find(m => m.role === 'assistant' && m.output)
            : null;

          if (lastAssistantMsg) {
            activeThreadId.set(threadId);
            activeVersionId.set(lastAssistantMsg.id);
            workingCopy.loadVersion(design, lastAssistantMsg.id);
            restoredFromThread = true;
          }
        } catch (e) {
          console.warn("Could not load full thread during boot:", e);
        }
      }

      if (!restoredFromThread) {
        workingCopy.loadVersion(design, null);
        activeThreadId.set(threadId);
      }

      const wc = get(workingCopy);
      await handleParamChange(wc.params, wc.macroCode);
    } else {
      await fetchDefaultMacro();
    }
  } catch (e) {
    console.error("Failed to restore last design:", e);
    await fetchDefaultMacro();
  }
}

async function fetchDefaultMacro() {
  try {
    const code = await invoke('get_default_macro');
    if (!get(workingCopy).macroCode) {
      workingCopy.patch({ macroCode: code });
    }
  } catch (e) {
    console.error("Failed to load default macro:", e);
  }
}
