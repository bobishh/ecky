import { get } from 'svelte/store';
import { session } from '../stores/sessionStore';
import { workingCopy } from '../stores/workingCopy';
import { paramPanelState } from '../stores/paramPanelState';
import { clearLastSessionSnapshot, persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import {
  historyStore as history,
  activeThreadIdStore as activeThreadId,
  activeVersionId,
  config,
  availableModels,
  isLoadingModels,
  runtimeCapabilities,
} from '../stores/domainState';
import { repairDefaultAuthoringContext } from '../runtimeCapabilities';
import {
  formatBackendError,
  getConfig,
  getDefaultMacro,
  getHistory,
  getLastDesign,
  getRuntimeCapabilities,
  getThreadLatestVersion,
  getThreadMessageVersion,
  getThreadMessagesPage,
  listModels,
  saveConfig as persistConfig,
} from '../tauri/client';
import { activeThreadMessagesLoading, loadVersion, threadMessagePageState } from '../stores/history';
import { isRenderableVersionTimelineMessage } from '../threadTimeline';
import type { LastDesignSnapshot, Message, Thread, ThreadMessagesPage } from '../types/domain';

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
          console.warn('[Boot] restore is still running.');
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
    const loadedConfig = await loadConfig();

    // 2. Probe runtime capabilities and repair invalid defaults if needed.
    const capabilities = await getRuntimeCapabilities();
    runtimeCapabilities.set(capabilities);

    const repaired = repairDefaultAuthoringContext(loadedConfig, capabilities);
    if (repaired.repaired) {
      config.set(repaired.config);
      await persistConfig(repaired.config);
    }

    // 3. Load History
    await loadHistory();

    // 4. Restore Last Design
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

  if (!loadedConfig.voice || !loadedConfig.voice.sttLanguageCode?.trim()) {
    loadedConfig.voice = { sttLanguageCode: 'en-US' };
    needsSave = true;
  } else {
    const normalizedSttLanguageCode = loadedConfig.voice.sttLanguageCode.trim();
    if (normalizedSttLanguageCode !== loadedConfig.voice.sttLanguageCode) {
      loadedConfig.voice.sttLanguageCode = normalizedSttLanguageCode;
      needsSave = true;
    }
  }

  if (typeof loadedConfig.freecadCmd !== 'string') {
    loadedConfig.freecadCmd = '';
    needsSave = true;
  }

  if (!loadedConfig.defaultEngineKind) {
    loadedConfig.defaultEngineKind = 'freecad';
    needsSave = true;
  }

  if (!loadedConfig.defaultSourceLanguage) {
    loadedConfig.defaultSourceLanguage = 'legacyPython';
    needsSave = true;
  }

  if (!loadedConfig.defaultGeometryBackend) {
    loadedConfig.defaultGeometryBackend = 'freecad';
    needsSave = true;
  }

  if (!loadedConfig.mcp) {
    loadedConfig.mcp = {
      port: null,
      maxSessions: null,
      mode: loadedConfig.connectionType === 'mcp' ? 'active' : 'passive',
      primaryAgentId: null,
      promptTimeoutSecs: 1800,
      autoAgents: [],
    };
    needsSave = true;
  } else {
    if (!loadedConfig.mcp.mode) {
      loadedConfig.mcp.mode = loadedConfig.mcp.autoAgents.length > 0 ? 'active' : 'passive';
      needsSave = true;
    }
    const nextPrimary =
      loadedConfig.mcp.autoAgents.find((agent) => agent.enabled)?.id ?? null;
    if (
      loadedConfig.mcp.mode === 'active' &&
      (!loadedConfig.mcp.primaryAgentId ||
        !loadedConfig.mcp.autoAgents.some(
          (agent) => agent.enabled && agent.id === loadedConfig.mcp.primaryAgentId,
        ))
    ) {
      loadedConfig.mcp.primaryAgentId = nextPrimary;
      needsSave = true;
    }
    if (loadedConfig.mcp.mode === 'passive' && loadedConfig.mcp.primaryAgentId === undefined) {
      loadedConfig.mcp.primaryAgentId = nextPrimary;
      needsSave = true;
    }
    if (
      !Number.isFinite(loadedConfig.mcp.promptTimeoutSecs) ||
      loadedConfig.mcp.promptTimeoutSecs < 10 ||
      loadedConfig.mcp.promptTimeoutSecs > 1800
    ) {
      loadedConfig.mcp.promptTimeoutSecs = 1800;
      needsSave = true;
    }
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

  return loadedConfig;
}

export async function saveConfig() {
  const currentConfig = get(config);
  try {
    await persistConfig(currentConfig);
    try {
      runtimeCapabilities.set(await getRuntimeCapabilities());
    } catch (refreshError) {
      console.warn('[Config] Failed to refresh runtime capabilities:', refreshError);
    }
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

    activeThreadId.set(last.threadId);
    const pointedMessage = await getThreadMessageVersion(last.threadId, last.messageId);
    const latestMessage = pointedMessage ? null : await getThreadLatestVersion(last.threadId);
    const targetMessage = pointedMessage ?? latestMessage ?? snapshotToMessage(last);

    if (!targetMessage) {
      await resetToBlankSession(true);
      await fetchDefaultMacro();
      return;
    }

    upsertRestoredMessage(last.threadId, targetMessage);
    await loadVersion(targetMessage, last.threadId, { rebuildMissingRuntime: true });
    void loadRestoredThreadPage(last.threadId);

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

function snapshotToMessage(snapshot: LastDesignSnapshot): Message | null {
  if (
    !snapshot.messageId ||
    !snapshot.artifactBundle ||
    !snapshot.modelManifest ||
    snapshot.modelManifest.modelId !== snapshot.artifactBundle.modelId
  ) {
    return null;
  }
  return {
    id: snapshot.messageId,
    role: 'assistant',
    content:
      snapshot.design?.title ||
      snapshot.modelManifest?.document?.documentLabel ||
      snapshot.modelManifest?.document?.documentName ||
      snapshot.artifactBundle.modelId,
    status: 'success',
    output: snapshot.design,
    artifactBundle: snapshot.artifactBundle,
    modelManifest: snapshot.modelManifest,
    usage: null,
    agentOrigin: null,
    imageData: null,
    visualKind: null,
    attachmentImages: [],
    timestamp: Date.now() / 1000,
  };
}

function upsertRestoredMessage(threadId: string, message: Message) {
  history.update((items) =>
    items.some((item) => item.id === threadId)
      ? items.map((item) =>
          item.id === threadId
            ? {
                ...item,
                messages: mergeRestoredMessage(item.messages ?? [], message),
              }
            : item,
        )
      : [
          {
            id: threadId,
            title: message.output?.title ?? message.modelManifest?.document?.documentLabel ?? 'Restored Thread',
            summary: '',
            messages: [message],
            updatedAt: message.timestamp,
            versionCount: isRenderableVersionTimelineMessage(message) ? 1 : 0,
            pendingCount: 0,
            queuedCount: 0,
            errorCount: 0,
            status: 'active',
            engineKind: message.artifactBundle?.engineKind ?? 'freecad',
            sourceLanguage: message.artifactBundle?.sourceLanguage ?? message.output?.sourceLanguage ?? 'legacyPython',
            geometryBackend: message.artifactBundle?.geometryBackend ?? message.output?.geometryBackend ?? 'freecad',
          },
          ...items,
        ],
  );
}

function mergeRestoredMessage(messages: Message[], message: Message): Message[] {
  const existingIndex = messages.findIndex((candidate) => candidate.id === message.id);
  if (existingIndex >= 0) {
    return messages.map((candidate, index) => (index === existingIndex ? { ...candidate, ...message } : candidate));
  }
  return [...messages, message];
}

async function loadRestoredThreadPage(threadId: string) {
  activeThreadMessagesLoading.set(true);
  threadMessagePageState.update((state) => ({
    ...state,
    [threadId]: {
      isLoading: true,
      hasMore: state[threadId]?.hasMore ?? false,
      nextBefore: state[threadId]?.nextBefore ?? null,
      error: null,
    },
  }));
  try {
    const page = await getThreadMessagesPage(threadId, null, 50, false);
    if (get(activeThreadId) !== threadId) return;
    mergeRestoredThreadPage(threadId, page);
  } catch (e) {
    if (get(activeThreadId) !== threadId) return;
    threadMessagePageState.update((state) => ({
      ...state,
      [threadId]: {
        isLoading: false,
        hasMore: state[threadId]?.hasMore ?? false,
        nextBefore: state[threadId]?.nextBefore ?? null,
        error: formatBackendError(e),
      },
    }));
    session.setError(`Thread Messages Error: ${formatBackendError(e)}`);
  } finally {
    if (get(activeThreadId) === threadId) {
      activeThreadMessagesLoading.set(false);
    }
  }
}

function mergeRestoredThreadPage(threadId: string, page: ThreadMessagesPage) {
  const activeMessageId = get(activeVersionId);
  history.update((items) =>
    items.map((thread) =>
      thread.id === threadId
        ? {
            ...thread,
            messages: mergeRestoredThreadMessages(thread.messages ?? [], page.messages, activeMessageId),
          }
        : thread,
    ),
  );
  threadMessagePageState.update((state) => ({
    ...state,
    [threadId]: {
      isLoading: false,
      hasMore: page.hasMore,
      nextBefore: page.nextBefore,
      error: null,
    },
  }));
}

function mergeRestoredThreadMessages(
  existingMessages: Message[],
  incomingMessages: Message[],
  activeMessageId: string | null,
): Message[] {
  const existingById = new Map(existingMessages.map((message) => [message.id, message]));
  const incomingIds = new Set(incomingMessages.map((message) => message.id));
  const mergedIncoming = incomingMessages.map((message) =>
    mergeRestoredMessagePayload(existingById.get(message.id), message),
  );

  if (!activeMessageId || incomingIds.has(activeMessageId)) {
    return mergedIncoming;
  }

  const restoredActive = existingById.get(activeMessageId);
  return restoredActive ? [restoredActive, ...mergedIncoming] : mergedIncoming;
}

function mergeRestoredMessagePayload(existing: Message | undefined, incoming: Message): Message {
  if (!existing) return incoming;
  return {
    ...existing,
    ...incoming,
    output: incoming.output ?? existing.output,
    artifactBundle: incoming.artifactBundle ?? existing.artifactBundle,
    modelManifest: incoming.modelManifest ?? existing.modelManifest,
  };
}

export function mergeRestoredThreadMessagesForTests(
  existingMessages: Message[],
  incomingMessages: Message[],
  activeMessageId: string | null,
): Message[] {
  return mergeRestoredThreadMessages(existingMessages, incomingMessages, activeMessageId);
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
