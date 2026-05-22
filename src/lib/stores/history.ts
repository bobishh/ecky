import { get, writable } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { historyStore as history, activeThreadIdStore as activeThreadId, activeVersionId } from './domainState';
import { workingCopy, isDirty } from './workingCopy';
import { session } from './sessionStore';
import { handleParamChange, commitManualVersion } from '../controllers/manualController';
import { paramPanelState } from './paramPanelState';
import { profileLog } from '../debug/profiler';
import { clearLastSessionSnapshot, persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import { getRenderableRuntimeBundle, inspectRuntimeBundle } from '../modelRuntime/runtimeBundle';
import { confirmAction } from '../ui/confirmAction';
import type { Message, Thread } from '../types/domain';
import { isCurrentThreadLoad as isCurrentThreadLoadState, shouldSkipThreadSelect } from '../threadLoading';
import {
  activeVersionTimelineIndex,
  isRenderableVersionTimelineMessage,
  versionTimelineMessages,
} from '../threadTimeline';
import { sameArtifactVersion } from '../versionPreviewPersistence';
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
  getThreadMessageVersion,
  getThreadMessagesPage,
  renameThread as renameThreadCommand,
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

const INITIAL_THREAD_MESSAGE_PAGE_LIMIT = 20;
const OLDER_THREAD_MESSAGE_PAGE_LIMIT = 50;
const THREAD_MESSAGES_PAGE_TIMEOUT_MS = 8000;
const THREAD_LATEST_VERSION_TIMEOUT_MS = 8000;

export type LoadVersionOptions = {
  rebuildMissingRuntime?: boolean;
};

async function hydrateVersionCandidate(
  message: Message,
  threadId: string | null,
): Promise<Message> {
  if (!threadId) return message;
  if (message.output && message.artifactBundle && message.modelManifest) return message;
  const hydrated = await getThreadMessageVersion(threadId, message.id);
  return hydrated && isVersionCandidate(hydrated) ? hydrated : message;
}

function isCurrentThreadLoad(token: number, threadId: string): boolean {
  return isCurrentThreadLoadState(token, latestThreadSwitchToken, get(activeThreadId), threadId);
}

function withBackendTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | null = null;
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(message)), timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timeoutId) clearTimeout(timeoutId);
  });
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

function mergeThreadMessagePayload(existing: Message | undefined, incoming: Message): Message {
  if (!existing) return incoming;
  return {
    ...existing,
    ...incoming,
    output: incoming.output ?? existing.output,
    artifactBundle: incoming.artifactBundle ?? existing.artifactBundle,
    modelManifest: incoming.modelManifest ?? existing.modelManifest,
  };
}

function mergeActiveThreadMessages(
  existingMessages: Message[],
  incomingMessages: Message[],
  activeMessageId: string | null,
): Message[] {
  const existingById = new Map(existingMessages.map((message) => [message.id, message]));
  const incomingIds = new Set(incomingMessages.map((message) => message.id));
  const mergedIncoming = incomingMessages.map((message) =>
    mergeThreadMessagePayload(existingById.get(message.id), message),
  );

  if (!activeMessageId || incomingIds.has(activeMessageId)) {
    return mergedIncoming;
  }

  const restoredActive = existingById.get(activeMessageId);
  return restoredActive ? [restoredActive, ...mergedIncoming] : mergedIncoming;
}

function versionCountForMessages(messages: Message[], fallback: number): number {
  return Math.max(fallback, messages.filter(isRenderableVersionTimelineMessage).length);
}

function mergeCommittedVersionMessage(
  threads: Thread[],
  threadId: string,
  title: string,
  message: Message,
): Thread[] {
  const existing = threads.find((thread) => thread.id === threadId) ?? null;
  const nextMessages = mergeThreadMessages(existing?.messages ?? [], [message]);
  const nextThread: Thread = existing
    ? {
        ...existing,
        title: title || existing.title,
        messages: nextMessages,
        updatedAt: Math.max(existing.updatedAt ?? 0, message.timestamp),
        versionCount: versionCountForMessages(nextMessages, existing.versionCount ?? 0),
      }
    : {
        id: threadId,
        title,
        summary: '',
        messages: nextMessages,
        updatedAt: message.timestamp,
        versionCount: versionCountForMessages(nextMessages, 0),
        pendingCount: 0,
        queuedCount: 0,
        errorCount: 0,
        status: 'active',
      };

  return [nextThread, ...threads.filter((thread) => thread.id !== threadId)];
}

export function rememberCommittedVersionMessage(threadId: string, title: string, message: Message) {
  history.update((threads) => mergeCommittedVersionMessage(threads, threadId, title, message));
}

export function rememberLatestThreadVersion(threadId: string, message: Message) {
  history.update((threads) =>
    threads.map((thread) => {
      if (thread.id !== threadId) return thread;
      const nextMessages = mergeThreadMessages(thread.messages ?? [], [message]);
      return {
        ...thread,
        messages: nextMessages,
        updatedAt: Math.max(thread.updatedAt ?? 0, message.timestamp),
        versionCount: versionCountForMessages(nextMessages, thread.versionCount ?? 0),
      };
    }),
  );
}

function beginThreadSwitch(targetThreadId: string) {
  activeVersionId.set(null);
  workingCopy.reset();
  paramPanelState.reset();
  session.setError(null);
  session.setStlUrl(null);
  session.clearModelRuntime();
  activeThreadId.set(targetThreadId);
}

function detachActiveVersionRuntime() {
  latestLoadVersionToken++;
  activeVersionId.set(null);
  workingCopy.reset();
  paramPanelState.reset();
  session.setStlUrl(null);
  session.clearModelRuntime();
}

function effectiveActiveVersionId(messages: Message[], currentVersionId: string | null): string | null {
  const versions = versionTimelineMessages(messages);
  const index = activeVersionTimelineIndex(versions, currentVersionId);
  return index >= 0 ? versions[index]?.id ?? null : null;
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

function isDurableRuntimePath(path: string | null | undefined): boolean {
  if (!path) return false;
  return path.includes('/model-runtime/') || path.includes('\\model-runtime\\');
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
    hasConsistentRuntimePayload(currentSession.artifactBundle, currentSession.modelManifest) &&
    (!message.artifactBundle || sameArtifactVersion(message.artifactBundle, currentSession.artifactBundle))
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

export async function loadVersion(
  msg: Message | null | undefined,
  expectedThreadId: string | null = get(activeThreadId),
  options: LoadVersionOptions = {},
) {
  if (!isVersionCandidate(msg)) return;
  const rebuildMissingRuntime = options.rebuildMissingRuntime ?? true;
  const loadToken = ++latestLoadVersionToken;
  let rebuiltRuntime = false;
  const isStale = () =>
    loadToken !== latestLoadVersionToken ||
    (expectedThreadId !== null && get(activeThreadId) !== expectedThreadId);

  const versionMessage = await hydrateVersionCandidate(msg, expectedThreadId);
  if (isStale()) return;

  if (versionMessage.output) {
    workingCopy.loadVersion(versionMessage.output, versionMessage.id);
    paramPanelState.hydrateFromVersion(versionMessage.output, versionMessage.id);
  } else {
    workingCopy.reset();
    paramPanelState.reset();
  }
  activeVersionId.set(versionMessage.id);

  const runtimePayload = resolveVersionRuntimePayload(versionMessage);
  const trustedRuntimeBundle = getRenderableRuntimeBundle(
    runtimePayload.artifactBundle ?? null,
    versionMessage.output?.postProcessing ?? null,
    versionMessage.output?.initialParams ?? {},
  );
  if (trustedRuntimeBundle?.previewStlPath && isDurableRuntimePath(trustedRuntimeBundle.previewStlPath)) {
    session.setStlUrl(toAssetUrl(trustedRuntimeBundle.previewStlPath));
    session.setModelRuntime(trustedRuntimeBundle, runtimePayload.modelManifest ?? versionMessage.modelManifest ?? null);
    session.setSelectedPartId(null);
    rememberVersionRuntimePayload(
      versionMessage.id,
      trustedRuntimeBundle,
      runtimePayload.modelManifest ?? versionMessage.modelManifest ?? null,
    );
    session.setStatus(`Loaded Version: ${versionLabel(versionMessage)}`);
    if (isStale()) return;
    await persistLastSessionSnapshot({
      design: versionMessage.output ?? null,
      threadId: expectedThreadId ?? get(activeThreadId),
      messageId: versionMessage.id,
      artifactBundle: trustedRuntimeBundle,
      modelManifest: runtimePayload.modelManifest ?? versionMessage.modelManifest ?? null,
      selectedPartId: null,
    });
    return;
  }

  const runtime = await inspectRuntimeBundle(
    runtimePayload.artifactBundle ?? null,
    undefined,
    undefined,
    versionMessage.output?.postProcessing ?? null,
    versionMessage.output?.initialParams ?? {},
  );
  if (isStale()) return;
  if (runtime.bundle) {
    session.setStlUrl(toAssetUrl(runtime.bundle.previewStlPath));
    session.setModelRuntime(runtime.bundle, runtimePayload.modelManifest ?? versionMessage.modelManifest ?? null);
    session.setSelectedPartId(null);
    rememberVersionRuntimePayload(
      versionMessage.id,
      runtime.bundle,
      runtimePayload.modelManifest ?? versionMessage.modelManifest ?? null,
    );
  } else if (runtime.skippedOversizedPreview) {
    session.setStlUrl(null);
    session.clearModelRuntime();
  } else if (versionMessage.output) {
    session.clearModelRuntime();
    if (!rebuildMissingRuntime) {
      session.setStlUrl(null);
      session.setStatus(`Cached runtime missing for ${versionLabel(versionMessage)}.`);
      return;
    }
    session.setStatus('Cached runtime missing on disk. Rebuilding preview...');
    await handleParamChange(versionMessage.output.initialParams || {}, versionMessage.output.macroCode, false);
    if (isStale()) return;
    rebuiltRuntime = true;
    rememberVersionRuntimePayload(
      versionMessage.id,
      get(session).artifactBundle,
      get(session).modelManifest,
    );
    try {
      await persistVersionRuntimePayload(
        versionMessage.id,
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
        ? `Loaded Version: ${versionLabel(versionMessage)} (lithophane preview skipped; using base part geometry to keep the viewer responsive).`
        : `Loaded Version: ${versionLabel(versionMessage)} (lithophane preview was too large to load safely).`,
    );
  } else if (runtime.degradedToPreview) {
    session.setStatus(`Loaded Version: ${versionLabel(versionMessage)} (preview only; part geometry was evicted).`);
  } else if (rebuiltRuntime) {
    session.setStatus(`Loaded Version: ${versionLabel(versionMessage)} (runtime rebuilt from macro).`);
  } else if (runtime.bundle || versionMessage.output || !versionMessage.artifactBundle) {
    session.setStatus(`Loaded Version: ${versionLabel(versionMessage)}`);
  }

  if (isStale()) return;
  await persistLastSessionSnapshot({
    design: versionMessage.output ?? null,
    threadId: expectedThreadId ?? get(activeThreadId),
    messageId: versionMessage.id,
    artifactBundle: runtime.bundle ?? runtimePayload.artifactBundle ?? versionMessage.artifactBundle ?? null,
    modelManifest: runtimePayload.modelManifest ?? versionMessage.modelManifest ?? null,
    selectedPartId: null,
  });
}

export async function loadFromHistory(thread: Thread) {
  const targetThreadId = thread.id;
  const existingThread = get(history).find((candidate) => candidate.id === targetThreadId);
  const existingPageState = get(threadMessagePageState)[targetThreadId];
  const seededMessages = existingThread?.messages ?? thread.messages ?? [];
  const seededLatestVersion = [...seededMessages].reverse().find(isVersionCandidate) ?? null;
  const skipInitialMessagesFetch =
    thread.status === 'finalized' &&
    seededMessages.length > 0 &&
    seededLatestVersion !== null;
  if (
    shouldSkipThreadSelect(targetThreadId, {
      activeThreadId: get(activeThreadId),
      loadingThreadId: get(activeThreadLoadingId),
      threadHasMessages: Boolean(existingThread?.messages?.length),
      threadMessagesLoading: Boolean(existingPageState?.isLoading),
    })
  ) return;

  const switchToken = ++latestThreadSwitchToken;
  beginThreadSwitch(targetThreadId);
  activeThreadLoadingId.set(targetThreadId);
  activeThreadMessagesLoading.set(!skipInitialMessagesFetch);
  activeThreadVersionLoading.set(true);
  setThreadPageState(targetThreadId, {
    isLoading: !skipInitialMessagesFetch,
    hasMore: skipInitialMessagesFetch ? false : existingPageState?.hasMore ?? false,
    nextBefore: skipInitialMessagesFetch ? null : existingPageState?.nextBefore ?? null,
    error: null,
  });

  history.update((items) => {
    const preservedMessages = existingThread?.messages ?? thread.messages ?? [];
    const summaryThread = { ...thread, messages: preservedMessages };
    return items.some((candidate) => candidate.id === targetThreadId)
      ? items.map((candidate) =>
          candidate.id === targetThreadId ? { ...candidate, ...summaryThread } : candidate,
        )
      : [summaryThread, ...items];
  });

  const latestVersionPromise = withBackendTimeout(
    getThreadLatestVersion(targetThreadId),
    THREAD_LATEST_VERSION_TIMEOUT_MS,
    'Thread latest version load timed out',
  );
  const messagesPromise = skipInitialMessagesFetch
    ? Promise.resolve(null)
    : withBackendTimeout(
        getThreadMessagesPage(
          targetThreadId,
          null,
          INITIAL_THREAD_MESSAGE_PAGE_LIMIT,
          false,
        ),
        THREAD_MESSAGES_PAGE_TIMEOUT_MS,
        'Thread messages load timed out',
      );
  let bootstrappedVersionId: string | null = null;

  try {
    if (seededLatestVersion) {
      bootstrappedVersionId = seededLatestVersion.id;
      await loadVersion(seededLatestVersion, targetThreadId, { rebuildMissingRuntime: true });
      if (!isCurrentThreadLoad(switchToken, targetThreadId)) {
        void messagesPromise.catch(() => undefined);
        return;
      }
    }

    const latestVersion = await latestVersionPromise;
    if (!isCurrentThreadLoad(switchToken, targetThreadId)) {
      void messagesPromise.catch(() => undefined);
      return;
    }

    if (latestVersion) {
      history.update((items) =>
        items.map((candidate) => {
          if (candidate.id !== targetThreadId) return candidate;
          const nextMessages = mergeThreadMessages(candidate.messages ?? [], [latestVersion]);
          return {
            ...candidate,
            messages: nextMessages,
            versionCount: versionCountForMessages(nextMessages, candidate.versionCount ?? 0),
          };
        }),
      );
      if (latestVersion.id !== bootstrappedVersionId) {
        await loadVersion(latestVersion, targetThreadId, { rebuildMissingRuntime: true });
      }
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
    if (!page) {
      setThreadPageState(targetThreadId, {
        isLoading: false,
        hasMore: false,
        nextBefore: null,
        error: null,
      });
      return;
    }
    history.update((items) =>
      items.map((candidate) =>
        candidate.id === targetThreadId
          ? {
              ...candidate,
              messages: mergeActiveThreadMessages(
                candidate.messages ?? [],
                page.messages,
                get(activeVersionId),
              ),
            }
          : candidate,
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

export async function deleteThread(id: string): Promise<boolean> {
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
    history.set(freshHistory.filter((thread) => thread.id !== id));
    return true;
  } catch (e) {
    session.setError(`Delete Error: ${formatBackendError(e)}`);
    return false;
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

export async function deleteVersion(messageId: string) {
  try {
    const currentThreadId = get(activeThreadId);
    const currentThread = currentThreadId
      ? get(history).find((thread) => thread.id === currentThreadId)
      : null;
    const wasActiveVersion =
      get(activeVersionId) === messageId ||
      effectiveActiveVersionId(currentThread?.messages ?? [], get(activeVersionId)) === messageId;
    await deleteVersionCommand(messageId);
    versionRuntimePayloadCache.delete(messageId);
    if (!currentThreadId) return;
    if (wasActiveVersion) {
      detachActiveVersionRuntime();
    }

    await refreshHistory();

    // Update active version if we deleted the current one
    if (wasActiveVersion) {
      const latestVersion = await getThreadLatestVersion(currentThreadId);
      if (!latestVersion) {
        await clearLastSessionSnapshot();
      } else {
        await loadVersion(latestVersion, currentThreadId);
      }
    }
    const page = await getThreadMessagesPage(
      currentThreadId,
      null,
      INITIAL_THREAD_MESSAGE_PAGE_LIMIT,
      false,
    );
    history.update((items) =>
      items.map((thread) =>
        thread.id === currentThreadId
          ? {
              ...thread,
              messages: mergeActiveThreadMessages(
                thread.messages ?? [],
                page.messages,
                get(activeVersionId),
              ),
            }
          : thread,
      ),
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
    const currentThreadId = get(activeThreadId);
    if (currentThreadId) {
      const page = await getThreadMessagesPage(
        currentThreadId,
        null,
        INITIAL_THREAD_MESSAGE_PAGE_LIMIT,
        false,
      );
      history.update((items) =>
        items.map((thread) =>
          thread.id === currentThreadId
            ? {
                ...thread,
                messages: mergeActiveThreadMessages(
                  thread.messages ?? [],
                  page.messages,
                  get(activeVersionId),
                ),
              }
            : thread,
        ),
      );
      setThreadPageState(currentThreadId, {
        isLoading: false,
        hasMore: page.hasMore,
        nextBefore: page.nextBefore,
        error: null,
      });
      const restored = page.messages.find((message) => message.id === messageId);
      if (restored) {
        await loadVersion(restored, currentThreadId);
      }
    }
    session.setStatus('Version returned to the carousel.');
  } catch (e) {
    session.setError(`Restore Error: ${formatBackendError(e)}`);
  }
}

export function createNewThread(payload: NewThreadPayload | null | undefined) {
  const newId = crypto.randomUUID();
  latestThreadSwitchToken += 1;
  latestLoadVersionToken += 1;
  beginThreadSwitch(newId);
  activeThreadLoadingId.set(null);
  activeThreadMessagesLoading.set(false);
  activeThreadVersionLoading.set(false);
  setThreadPageState(newId, {
    isLoading: false,
    hasMore: false,
    nextBefore: null,
    error: null,
  });
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
    const confirmed = await confirmAction(`Fork "${label}" into a new thread now?`);
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
      OLDER_THREAD_MESSAGE_PAGE_LIMIT,
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
  return await getInventoryCommand();
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
