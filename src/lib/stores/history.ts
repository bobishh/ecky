import { get, writable } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { historyStore as history, activeThreadIdStore as activeThreadId, activeVersionId, config } from './domainState';
import { workingCopy, isDirty } from './workingCopy';
import { session } from './sessionStore';
import { handleParamChange, commitManualVersion } from '../controllers/manualController';
import { paramPanelState } from './paramPanelState';
import { profileLog } from '../debug/profiler';
import { clearLastSessionSnapshot, persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import { inspectRuntimeBundle } from '../modelRuntime/runtimeBundle';
import type { GeometryBackend, Message, SourceLanguage, Thread } from '../types/domain';
import { isCurrentThreadLoad as isCurrentThreadLoadState, shouldSkipThreadSelect } from '../threadLoading';
import { isRenderableVersionTimelineMessage } from '../threadTimeline';
import {
  addImportedModelVersion,
  addManualVersion,
  deleteThread as deleteThreadCommand,
  deleteVersion as deleteVersionCommand,
  finalizeThread as finalizeThreadCommand,
  reopenThread as reopenThreadCommand,
  getInventory as getInventoryCommand,
  formatBackendError,
  getHistory,
  getMessStlPath,
  getModelManifest,
  getThreadLatestVersion,
  getThreadMessagesPage,
  renameThread as renameThreadCommand,
  setThreadAuthoringContext as setThreadAuthoringContextCommand,
  setThreadEngineKind as setThreadEngineKindCommand,
  getThread,
  restoreVersion as restoreVersionCommand,
  updateVersionRuntime,
} from '../tauri/client';

type NewThreadPayload = {
  mode?: 'blank' | 'macro';
  code?: string;
  title?: string;
};

type ThreadMessagePageState = {
  isLoading: boolean;
  hasMore: boolean;
  nextBefore: number | null;
  error: string | null;
};

let latestLoadVersionToken = 0;
let latestThreadSwitchToken = 0;
const versionRuntimePayloadCache = new Map<
  string,
  { artifactBundle: Message['artifactBundle']; modelManifest: Message['modelManifest'] }
>();

export const activeThreadMessagesLoading = writable(false);
export const activeThreadVersionLoading = writable(false);
export const activeThreadLoadingId = writable<string | null>(null);
export const threadMessagePageState = writable<Record<string, ThreadMessagePageState>>({});

const DEFAULT_MESSAGE_PAGE_LIMIT = 50;

function isCurrentThreadLoad(token: number, threadId: string): boolean {
  return isCurrentThreadLoadState(token, latestThreadSwitchToken, get(activeThreadId), threadId);
}

function setThreadPageState(threadId: string, patch: Partial<ThreadMessagePageState>) {
  const defaults: ThreadMessagePageState = {
    isLoading: false,
    hasMore: false,
    nextBefore: null,
    error: null,
  };
  threadMessagePageState.update((state) => ({
    ...state,
    [threadId]: {
      ...defaults,
      ...(state[threadId] ?? {}),
      ...patch,
    },
  }));
}

function mergeThreadMessages(existing: Message[], incoming: Message[]): Message[] {
  const seen = new Set<string>();
  return [...incoming, ...existing].filter((message) => {
    if (seen.has(message.id)) return false;
    seen.add(message.id);
    return true;
  });
}

function toAssetUrl(path: string | null | undefined): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

function isVersionCandidate(message: Message | null | undefined): message is Message {
  return Boolean(message && isRenderableVersionTimelineMessage(message));
}

function versionLabel(message: Message): string {
  return (
    message.output?.title ||
    message.modelManifest?.document?.documentLabel ||
    message.modelManifest?.document?.documentName ||
    message.artifactBundle?.modelId ||
    'Imported Model'
  );
}

function hasConsistentRuntimePayload(
  bundle: Message['artifactBundle'] | null | undefined,
  manifest: Message['modelManifest'] | null | undefined,
): boolean {
  return Boolean(bundle && manifest && bundle.modelId === manifest.modelId);
}

function rememberVersionRuntimePayload(
  messageId: string,
  artifactBundle: Message['artifactBundle'] | null | undefined,
  modelManifest: Message['modelManifest'] | null | undefined,
) {
  if (!hasConsistentRuntimePayload(artifactBundle, modelManifest)) return;
  versionRuntimePayloadCache.set(messageId, {
    artifactBundle: artifactBundle ?? null,
    modelManifest: modelManifest ?? null,
  });
}

function resolveVersionRuntimePayload(message: Message): {
  artifactBundle: Message['artifactBundle'] | null;
  modelManifest: Message['modelManifest'] | null;
} {
  const cached = versionRuntimePayloadCache.get(message.id);
  if (cached && hasConsistentRuntimePayload(cached.artifactBundle, cached.modelManifest)) {
    return cached;
  }

  const currentThreadId = get(activeThreadId);
  const currentVersionId = get(activeVersionId);
  const currentSession = get(session);
  if (
    message.id === currentVersionId &&
    currentThreadId &&
    hasConsistentRuntimePayload(currentSession.artifactBundle, currentSession.modelManifest)
  ) {
    return {
      artifactBundle: currentSession.artifactBundle,
      modelManifest: currentSession.modelManifest,
    };
  }

  return {
    artifactBundle: message.artifactBundle ?? null,
    modelManifest: message.modelManifest ?? null,
  };
}

export function resetVersionRuntimePayloadCacheForTests() {
  versionRuntimePayloadCache.clear();
}

export function rememberVersionRuntimePayloadForTests(
  messageId: string,
  artifactBundle: Message['artifactBundle'] | null | undefined,
  modelManifest: Message['modelManifest'] | null | undefined,
) {
  rememberVersionRuntimePayload(messageId, artifactBundle, modelManifest);
}

export function resolveVersionRuntimePayloadForTests(message: Message) {
  return resolveVersionRuntimePayload(message);
}

async function persistVersionRuntimePayload(
  messageId: string,
  artifactBundle: Message['artifactBundle'] | null | undefined,
  modelManifest: Message['modelManifest'] | null | undefined,
  persistRuntime: typeof updateVersionRuntime = updateVersionRuntime,
): Promise<boolean> {
  if (!hasConsistentRuntimePayload(artifactBundle, modelManifest)) return false;
  await persistRuntime(messageId, artifactBundle!, modelManifest!);
  return true;
}

export async function persistVersionRuntimePayloadForTests(
  messageId: string,
  artifactBundle: Message['artifactBundle'] | null | undefined,
  modelManifest: Message['modelManifest'] | null | undefined,
  persistRuntime?: typeof updateVersionRuntime,
) {
  return persistVersionRuntimePayload(messageId, artifactBundle, modelManifest, persistRuntime);
}

async function resolveForkRuntimePayload(message: Message): Promise<{
  artifactBundle: Message['artifactBundle'] | null;
  modelManifest: Message['modelManifest'] | null;
}> {
  const runtimePayload = resolveVersionRuntimePayload(message);
  if (hasConsistentRuntimePayload(runtimePayload.artifactBundle, runtimePayload.modelManifest)) {
    return runtimePayload;
  }

  if (message.artifactBundle) {
    try {
      const refreshedManifest = await getModelManifest(message.artifactBundle.modelId);
      if (message.artifactBundle.modelId === refreshedManifest.modelId) {
        rememberVersionRuntimePayload(message.id, message.artifactBundle, refreshedManifest);
        return {
          artifactBundle: message.artifactBundle,
          modelManifest: refreshedManifest,
        };
      }
    } catch (e) {
      console.warn('[History] Failed to refresh manifest for fork:', e);
    }
  }

  return { artifactBundle: null, modelManifest: null };
}

export async function loadVersion(msg: Message | null | undefined, expectedThreadId: string | null = get(activeThreadId)) {
  if (!isVersionCandidate(msg)) return;
  const loadToken = ++latestLoadVersionToken;
  activeVersionId.set(msg.id);
  let rebuiltRuntime = false;
  const isStale = () =>
    loadToken !== latestLoadVersionToken ||
    get(activeVersionId) !== msg.id ||
    (expectedThreadId !== null && get(activeThreadId) !== expectedThreadId);

  if (msg.output) {
    workingCopy.loadVersion(msg.output, msg.id);
    paramPanelState.hydrateFromVersion(msg.output, msg.id);
  } else {
    workingCopy.reset();
    paramPanelState.reset();
  }

  const runtimePayload = resolveVersionRuntimePayload(msg);
  const runtime = await inspectRuntimeBundle(
    runtimePayload.artifactBundle ?? null,
    undefined,
    undefined,
    msg.output?.postProcessing ?? null,
    msg.output?.initialParams ?? {},
  );
  if (isStale()) return;
  if (runtime.bundle) {
    session.setStlUrl(toAssetUrl(runtime.bundle.previewStlPath));
    session.setModelRuntime(runtime.bundle, runtimePayload.modelManifest ?? msg.modelManifest ?? null);
    session.setSelectedPartId(null);
    rememberVersionRuntimePayload(
      msg.id,
      runtime.bundle,
      runtimePayload.modelManifest ?? msg.modelManifest ?? null,
    );
  } else if (runtime.skippedOversizedPreview) {
    session.setStlUrl(null);
    session.clearModelRuntime();
  } else if (msg.output) {
    session.clearModelRuntime();
    session.setStatus('Cached runtime missing on disk. Rebuilding preview...');
    await handleParamChange(msg.output.initialParams || {}, msg.output.macroCode, false);
    if (isStale()) return;
    rebuiltRuntime = true;
    rememberVersionRuntimePayload(
      msg.id,
      get(session).artifactBundle,
      get(session).modelManifest,
    );
    try {
      await persistVersionRuntimePayload(
        msg.id,
        get(session).artifactBundle,
        get(session).modelManifest,
      );
    } catch (error) {
      console.warn('[History] Failed to persist rebuilt runtime bundle:', error);
    }
  } else {
    session.setStlUrl(null);
    session.clearModelRuntime();
  }

  if (runtime.skippedOversizedPreview) {
    session.setStatus(
      runtime.bundle
        ? `Loaded Version: ${versionLabel(msg)} (lithophane preview skipped; using base part geometry to keep the viewer responsive).`
        : `Loaded Version: ${versionLabel(msg)} (lithophane preview was too large to load safely).`,
    );
  } else if (runtime.degradedToPreview) {
    session.setStatus(`Loaded Version: ${versionLabel(msg)} (preview only; part geometry was evicted).`);
  } else if (rebuiltRuntime) {
    session.setStatus(`Loaded Version: ${versionLabel(msg)} (runtime rebuilt from macro).`);
  } else if (runtime.bundle || msg.output || !msg.artifactBundle) {
    session.setStatus(`Loaded Version: ${versionLabel(msg)}`);
  }

  if (isStale()) return;
  await persistLastSessionSnapshot({
    design: msg.output ?? null,
    threadId: expectedThreadId ?? get(activeThreadId),
    messageId: msg.id,
    artifactBundle: runtime.bundle ?? runtimePayload.artifactBundle ?? msg.artifactBundle ?? null,
    modelManifest: runtimePayload.modelManifest ?? msg.modelManifest ?? null,
    selectedPartId: null,
  });
}

export async function loadFromHistory(thread: Thread) {
  const targetThreadId = thread.id;
  const existingThread = get(history).find((candidate) => candidate.id === targetThreadId);
  const existingPageState = get(threadMessagePageState)[targetThreadId];
  if (
    shouldSkipThreadSelect(targetThreadId, {
      activeThreadId: get(activeThreadId),
      loadingThreadId: get(activeThreadLoadingId),
      threadHasMessages: Boolean(existingThread?.messages?.length),
      threadMessagesLoading: Boolean(existingPageState?.isLoading),
    })
  ) return;

  const switchToken = ++latestThreadSwitchToken;
  activeThreadId.set(targetThreadId);
  activeThreadLoadingId.set(targetThreadId);
  activeThreadMessagesLoading.set(true);
  activeThreadVersionLoading.set(true);
  setThreadPageState(targetThreadId, { isLoading: true, error: null });

  history.update((items) => {
    const preservedMessages = existingThread?.messages ?? thread.messages ?? [];
    const summaryThread = { ...thread, messages: preservedMessages };
    return items.some((candidate) => candidate.id === targetThreadId)
      ? items.map((candidate) =>
          candidate.id === targetThreadId ? { ...candidate, ...summaryThread } : candidate,
        )
      : [summaryThread, ...items];
  });

  const latestVersionPromise = getThreadLatestVersion(targetThreadId);
  const messagesPromise = getThreadMessagesPage(
    targetThreadId,
    null,
    DEFAULT_MESSAGE_PAGE_LIMIT,
    false,
  );

  try {
    const latestVersion = await latestVersionPromise;
    if (!isCurrentThreadLoad(switchToken, targetThreadId)) {
      void messagesPromise.catch(() => undefined);
      return;
    }

    if (latestVersion) {
      await loadVersion(latestVersion, targetThreadId);
    } else {
      activeVersionId.set(null);
      workingCopy.reset();
      paramPanelState.reset();
      const hasFailed = thread.errorCount > 0;
      if (hasFailed) {
        try {
          const messPath = await getMessStlPath();
          if (!isCurrentThreadLoad(switchToken, targetThreadId)) {
            void messagesPromise.catch(() => undefined);
            return;
          }
          session.setStlUrl(toAssetUrl(messPath));
          session.clearModelRuntime();
        } catch {
          if (isCurrentThreadLoad(switchToken, targetThreadId)) session.setStlUrl(null);
        }
      } else {
        session.setStlUrl(null);
      }
      await clearLastSessionSnapshot();
    }
  } catch (e) {
    if (isCurrentThreadLoad(switchToken, targetThreadId)) {
      console.error('[History] Failed to load latest thread version:', e);
      session.setError(`Thread Load Error: ${formatBackendError(e)}`);
    }
  } finally {
    if (isCurrentThreadLoad(switchToken, targetThreadId)) {
      activeThreadVersionLoading.set(false);
    }
  }

  try {
    const page = await messagesPromise;
    if (!isCurrentThreadLoad(switchToken, targetThreadId)) return;
    history.update((items) =>
      items.map((candidate) =>
        candidate.id === targetThreadId ? { ...candidate, messages: page.messages } : candidate,
      ),
    );
    setThreadPageState(targetThreadId, {
      isLoading: false,
      hasMore: page.hasMore,
      nextBefore: page.nextBefore,
      error: null,
    });
    profileLog('history.load_thread_page', {
      threadId: targetThreadId,
      messages: page.messages.length,
      hasMore: page.hasMore,
    });
  } catch (e) {
    if (isCurrentThreadLoad(switchToken, targetThreadId)) {
      console.error('[History] Failed to load thread messages:', e);
      setThreadPageState(targetThreadId, {
        isLoading: false,
        error: formatBackendError(e),
      });
      session.setError(`Thread Messages Error: ${formatBackendError(e)}`);
    }
  } finally {
    if (isCurrentThreadLoad(switchToken, targetThreadId)) {
      activeThreadMessagesLoading.set(false);
      activeThreadLoadingId.set(null);
    }
  }
}

export async function deleteThread(id: string) {
  try {
    await deleteThreadCommand(id);
    if (get(activeThreadId) === id) {
      activeThreadId.set(null);
      activeVersionId.set(null);
      workingCopy.reset();
      paramPanelState.reset();
      session.setStlUrl(null);
      await clearLastSessionSnapshot();
    }
    const freshHistory = await getHistory();
    history.set(freshHistory);
  } catch (e) {
    session.setError(`Delete Error: ${formatBackendError(e)}`);
  }
}

export async function renameThread(id: string, title: string) {
  const trimmed = title.trim();
  if (!trimmed) {
    session.setError('Rename Error: Thread title cannot be empty.');
    return;
  }

  try {
    await renameThreadCommand(id, trimmed);
    history.update((items) =>
      items.map((thread) => (thread.id === id ? { ...thread, title: trimmed } : thread)),
    );
    await refreshHistory();
    if (get(activeThreadId) === id) {
      session.setStatus(`Thread renamed to ${trimmed}.`);
    }
  } catch (e) {
    session.setError(`Rename Error: ${formatBackendError(e)}`);
  }
}

function resolveInternalGeometryBackend(
  thread: Thread | undefined,
  sourceLanguage: SourceLanguage,
): GeometryBackend {
  if (sourceLanguage === 'legacyPython') return 'freecad';
  if (thread?.geometryBackend && thread.geometryBackend !== 'freecad') return thread.geometryBackend;
  const defaultGeometryBackend = get(config).defaultGeometryBackend;
  if (defaultGeometryBackend && defaultGeometryBackend !== 'freecad') return defaultGeometryBackend;
  return 'build123d';
}

export async function setThreadAuthoringContext(id: string, sourceLanguage: SourceLanguage) {
  try {
    const existingThread = get(history).find((thread) => thread.id === id);
    const geometryBackend = resolveInternalGeometryBackend(existingThread, sourceLanguage);
    await setThreadAuthoringContextCommand(id, sourceLanguage, geometryBackend);
    history.update((items) => {
      if (items.some((thread) => thread.id === id)) {
        return items.map((thread) =>
          thread.id === id ? { ...thread, sourceLanguage, geometryBackend } : thread,
        );
      }
      return items;
    });
    await refreshHistory();
    if (get(activeThreadId) === id) {
      session.setStatus(`Authoring context updated.`);
    }
  } catch (e) {
    session.setError(`Update Error: ${formatBackendError(e)}`);
  }
}

export async function setThreadEngineKind(id: string, engineKind: Thread['engineKind']) {
  try {
    await setThreadEngineKindCommand(id, engineKind);
    history.update((items) => {
      if (items.some((thread) => thread.id === id)) {
        return items.map((thread) => (thread.id === id ? { ...thread, engineKind } : thread));
      }
      return items;
    });
    await refreshHistory();
    if (get(activeThreadId) === id) {
      session.setStatus(
        engineKind === 'ecky'
          ? 'Thread engine set to Ecky.'
          : engineKind === 'build123d'
            ? 'Thread engine set to build123d Python.'
            : 'Thread engine set to FreeCAD.',
      );
    }
  } catch (e) {
    session.setError(`Engine Error: ${formatBackendError(e)}`);
  }
}

export async function deleteVersion(messageId: string) {
  try {
    await deleteVersionCommand(messageId);
    const currentThreadId = get(activeThreadId);
    if (!currentThreadId) return;

    await refreshHistory();

    // Update active version if we deleted the current one
    if (get(activeVersionId) === messageId) {
      const latestVersion = await getThreadLatestVersion(currentThreadId);
      if (!latestVersion) {
        activeVersionId.set(null);
        workingCopy.reset();
        paramPanelState.reset();
        session.setStlUrl(null);
        session.clearModelRuntime();
        await clearLastSessionSnapshot();
      } else {
        await loadVersion(latestVersion, currentThreadId);
      }
    }
    const page = await getThreadMessagesPage(currentThreadId, null, DEFAULT_MESSAGE_PAGE_LIMIT, false);
    history.update((items) =>
      items.map((thread) => (thread.id === currentThreadId ? { ...thread, messages: page.messages } : thread)),
    );
    setThreadPageState(currentThreadId, {
      isLoading: false,
      hasMore: page.hasMore,
      nextBefore: page.nextBefore,
      error: null,
    });
    session.setStatus('Version removed from the carousel.');
  } catch (e) {
    session.setError(`Failed to delete version: ${formatBackendError(e)}`);
  }
}

export async function restoreVersion(messageId: string) {
  try {
    await restoreVersionCommand(messageId);
    await refreshHistory();
    session.setStatus('Version returned to the carousel.');
  } catch (e) {
    session.setError(`Restore Error: ${formatBackendError(e)}`);
  }
}

export function createNewThread(payload: NewThreadPayload | null | undefined) {
  const newId = crypto.randomUUID();
  activeThreadId.set(newId);
  activeVersionId.set(null);
  workingCopy.reset();
  paramPanelState.reset();
  session.setStlUrl(null);
  void clearLastSessionSnapshot();
  
  if (payload?.mode === 'macro' && payload.code) {
    session.setStatus(`Initializing thread with macro: ${payload.title}...`);
    // We'll call a special commit function for the initial macro
    commitInitialMacro(payload.code, payload.title);
  } else {
    session.setStatus('New design session started.');
  }
}

async function commitInitialMacro(code: string, title: string | undefined) {
  try {
    // Ensure the manual controller knows which thread to commit to
    // Since activeThreadId was just set, commitManualVersion should pick it up.
    await commitManualVersion(code, title);
  } catch (e) {
    console.error("[History] Failed to commit initial macro:", e);
    session.setError(`Initial Macro Error: ${e}`);
  }
}

async function resolveActiveVersionMessage(): Promise<Message | null> {
  const threadId = get(activeThreadId);
  if (!threadId) return null;

  const thread = get(history).find((candidate) => candidate.id === threadId) ?? null;

  const messages = thread?.messages || [];
  const selectedVersionId = get(activeVersionId);
  const selectedMessage = selectedVersionId
    ? messages.find((message) => message.id === selectedVersionId)
    : null;
  if (isVersionCandidate(selectedMessage)) return selectedMessage;
  const loadedVersion = [...messages].reverse().find(isVersionCandidate) ?? null;
  if (loadedVersion) return loadedVersion;

  try {
    return await getThreadLatestVersion(threadId);
  } catch (e) {
    console.warn('[History] Failed to load active version for fork:', e);
    return null;
  }
}

export async function forkDesign() {
  try {
    const sourceMessage = await resolveActiveVersionMessage();
    if (!sourceMessage) {
      session.setError('Fork Error: No active version is loaded.');
      return;
    }

    const label = versionLabel(sourceMessage);
    const confirmed =
      typeof window === 'undefined'
        ? true
        : window.confirm(`Fork "${label}" into a new thread now?`);
    if (!confirmed) return;

    const newThreadId = crypto.randomUUID();
    let newMessageId = '';
    const runtimePayload = await resolveForkRuntimePayload(sourceMessage);

    if (sourceMessage.output) {
      newMessageId = await addManualVersion({
        threadId: newThreadId,
        title: sourceMessage.output.title || label,
        versionName: sourceMessage.output.versionName || 'Forked',
        macroCode: sourceMessage.output.macroCode,
        parameters: sourceMessage.output.initialParams || {},
        uiSpec: sourceMessage.output.uiSpec,
        artifactBundle: runtimePayload.artifactBundle ?? null,
        modelManifest: runtimePayload.modelManifest ?? null,
      });
    } else if (runtimePayload.artifactBundle && runtimePayload.modelManifest) {
      newMessageId = await addImportedModelVersion({
        threadId: newThreadId,
        title: label,
        artifactBundle: runtimePayload.artifactBundle,
        modelManifest: runtimePayload.modelManifest,
      });
    } else {
      session.setError('Fork Error: Active version has no reusable payload to fork.');
      return;
    }

    const forkedThread = await getThread(newThreadId);
    history.update((items) => {
      const nextItems = items.filter((item) => item.id !== newThreadId);
      return [forkedThread, ...nextItems];
    });

    activeThreadId.set(newThreadId);
    const forkedMessage =
      forkedThread.messages.find((message) => message.id === newMessageId) ??
      [...forkedThread.messages].reverse().find(isVersionCandidate) ??
      null;

    if (forkedMessage) {
      await loadVersion(forkedMessage);
    } else {
      activeVersionId.set(newMessageId || null);
    }

    session.setStatus('Design forked into a new thread.');
    paramPanelState.setVersionId(newMessageId || null);
    await refreshHistory();
  } catch (e) {
    session.setError(`Fork Error: ${formatBackendError(e)}`);
  }
}

export async function finalizeThread(id: string) {
  try {
    const selectedMessageId =
      get(activeThreadId) === id ? get(activeVersionId) : null;
    await finalizeThreadCommand(id, selectedMessageId);
    if (get(activeThreadId) === id) {
      activeThreadId.set(null);
      activeVersionId.set(null);
      workingCopy.reset();
      paramPanelState.reset();
      session.setStlUrl(null);
      await clearLastSessionSnapshot();
    }
    await refreshHistory();
    session.setStatus('Selected model promoted to inventory.');
  } catch (e) {
    session.setError(`Finalize Error: ${formatBackendError(e)}`);
  }
}

export async function reopenThread(id: string) {
  try {
    await reopenThreadCommand(id);
    await refreshHistory();
    session.setStatus('Thread reopened from inventory.');
  } catch (e) {
    session.setError(`Reopen Error: ${formatBackendError(e)}`);
  }
}

export async function openInventoryThread(id: string): Promise<boolean> {
  try {
    const thread = await getThread(id);
    await loadFromHistory(thread);
    session.setStatus('Opened final model from inventory.');
    return true;
  } catch (e) {
    session.setError(`Open Inventory Model Error: ${formatBackendError(e)}`);
    return false;
  }
}

export async function loadOlderThreadMessages(threadId: string) {
  const state = get(threadMessagePageState)[threadId];
  if (!state?.hasMore || state.isLoading || state.nextBefore === null) return;

  setThreadPageState(threadId, { isLoading: true, error: null });
  try {
    const page = await getThreadMessagesPage(
      threadId,
      state.nextBefore,
      DEFAULT_MESSAGE_PAGE_LIMIT,
      false,
    );
    history.update((items) =>
      items.map((thread) =>
        thread.id === threadId
          ? { ...thread, messages: mergeThreadMessages(thread.messages ?? [], page.messages) }
          : thread,
      ),
    );
    setThreadPageState(threadId, {
      isLoading: false,
      hasMore: page.hasMore,
      nextBefore: page.nextBefore,
      error: null,
    });
  } catch (e) {
    setThreadPageState(threadId, {
      isLoading: false,
      error: formatBackendError(e),
    });
    if (get(activeThreadId) === threadId) {
      session.setError(`Thread Messages Error: ${formatBackendError(e)}`);
    }
  }
}

export async function loadInventory(): Promise<Thread[]> {
  try {
    return await getInventoryCommand();
  } catch (e) {
    console.error('[History] Failed to load inventory:', e);
    return [];
  }
}

export async function refreshHistory() {
  try {
    const freshHistory = await getHistory();
    const loadedMessages = new Map(get(history).map((thread) => [thread.id, thread.messages ?? []]));
    history.set(
      freshHistory.map((thread) => ({
        ...thread,
        messages: loadedMessages.get(thread.id) ?? [],
      })),
    );
  } catch (e) {
    console.error("[History] Failed to refresh history:", e);
  }
}
