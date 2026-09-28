import { get, writable } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { historyStore as history, activeThreadIdStore as activeThreadId, activeVersionId } from './domainState';
import { workingCopy, isDirty } from './workingCopy';
import { session } from './sessionStore';
import { paramPanelState } from './paramPanelState';
import { profileLog } from '../debug/profiler';
import { clearLastSessionSnapshot, persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import { getRenderableRuntimeBundle } from '../modelRuntime/runtimeBundle';
import { confirmAction } from '../ui/confirmAction';
import { createRevisionedSingleflight } from './historyProjectionRefresh';
import type { Message, Thread } from '../types/domain';
import { isCurrentThreadLoad as isCurrentThreadLoadState, shouldSkipThreadSelect } from '../threadLoading';
import {
  activeVersionTimelineIndex,
  versionTimelineMessages,
} from '../threadTimeline';
import { sameArtifactVersion } from '../versionPreviewPersistence';
import {
  deleteThreadIntent,
  deleteVersionIntent,
  finalizeThreadIntent,
  reopenThreadIntent,
  getInventory as getInventoryCommand,
  formatBackendError,
  getHistory,
  getMessStlPath,
  createDesignThreadIntent,
  getThreadLatestVersion,
  getThreadMessageVersion,
  getThreadMessagesPage,
  getWorkspaceProjection,
  openInventoryThreadIntent,
  forkDesignIntent,
  renameThread as renameThreadCommand,
  restoreVersionIntent,
} from '../tauri/client';

type NewThreadPayload = {
  mode?: 'blank' | 'macro';
  code?: string;
  title?: string;
};

type ThreadMessagePageState = {
  isLoading: boolean;
  hasMore: boolean;
  nextBefore: string | null;
  error: string | null;
};

export function resolveProjectedPageState(
  existing: ThreadMessagePageState | undefined,
  incoming: Pick<ThreadMessagePageState, 'hasMore' | 'nextBefore'>,
): Pick<ThreadMessagePageState, 'hasMore' | 'nextBefore'> {
  return existing?.nextBefore
    ? { hasMore: existing.hasMore, nextBefore: existing.nextBefore }
    : incoming;
}

let latestLoadVersionToken = 0;
let latestThreadSwitchToken = 0;
export const activeThreadMessagesLoading = writable(false);
export const activeThreadVersionLoading = writable(false);
export const activeThreadLoadingId = writable<string | null>(null);
export const threadMessagePageState = writable<Record<string, ThreadMessagePageState>>({});

const INITIAL_THREAD_MESSAGE_PAGE_LIMIT = 20;
const OLDER_THREAD_MESSAGE_PAGE_LIMIT = 50;
const THREAD_MESSAGES_PAGE_TIMEOUT_MS = 8000;
const RESTORED_OLDER_PAGE_LIMIT = 10;
const HISTORY_WINDOW_STORAGE_KEY = 'ecky.historyProjectionWindows.v1';
const historyProjectionRefreshes = createRevisionedSingleflight<{
  freshHistory: Thread[];
  page: Awaited<ReturnType<typeof getThreadMessagesPage>>;
}>();
let localHistoryProjectionRevision = 0;

function storedOlderPageCount(threadId: string): number {
  try {
    const raw = JSON.parse(localStorage.getItem(HISTORY_WINDOW_STORAGE_KEY) ?? '{}') as Record<string, number>;
    return Math.max(0, Math.min(RESTORED_OLDER_PAGE_LIMIT, Math.floor(raw[threadId] ?? 0)));
  } catch {
    return 0;
  }
}

function persistOlderPageCount(threadId: string, count: number): void {
  try {
    const raw = JSON.parse(localStorage.getItem(HISTORY_WINDOW_STORAGE_KEY) ?? '{}') as Record<string, number>;
    raw[threadId] = Math.max(0, Math.min(RESTORED_OLDER_PAGE_LIMIT, Math.floor(count)));
    localStorage.setItem(HISTORY_WINDOW_STORAGE_KEY, JSON.stringify(raw));
  } catch {
    // Projection-window persistence is best effort; durable history remains canonical.
  }
}

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

export function projectHistorySummaries(freshHistory: Thread[]): void {
  const loadedMessages = new Map(get(history).map((thread) => [thread.id, thread.messages ?? []]));
  history.set(
    freshHistory.map((thread) => ({
      ...thread,
      messages: loadedMessages.get(thread.id) ?? [],
    })),
  );
}

export function evictSupersededVersionDetails(
  threads: Thread[],
  selectedThreadId: string,
  selected: Message,
): Thread[] {
  return threads.map((thread) => ({
    ...thread,
    messages: (thread.messages ?? []).map((message) => {
      if (thread.id === selectedThreadId && message.id === selected.id) {
        return mergeThreadMessagePayload(message, selected);
      }
      if (!message.output && !message.artifactBundle && !message.modelManifest && !message.imageData) {
        return message;
      }
      return {
        ...message,
        output: null,
        artifactBundle: null,
        modelManifest: null,
        structuralVerification: null,
        imageData: null,
        attachmentImages: [],
      };
    }),
  }));
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
  // Count every authored attempt. Failed, pending, and artifactless versions
  // remain history rows and must not disappear from the count.
  return Math.max(fallback, versionTimelineMessages(messages).length);
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
  activeVersionId.set(message.id);
}

/** Project one canonical backend workspace without follow-up history reads. */
export async function projectWorkspaceProjection(
  projection: Awaited<ReturnType<typeof getWorkspaceProjection>>,
): Promise<Message | null> {
  const threadId = projection.thread.id;
  const selectedVersion = projection.selectedVersion;
  history.update((threads) => {
    const existing = threads.find((thread) => thread.id === threadId) ?? null;
    const projectedMessages = selectedVersion
      ? [...projection.messagesPage.messages, selectedVersion]
      : projection.messagesPage.messages;
    const messages = mergeThreadMessages(existing?.messages ?? [], projectedMessages);
    const projectedThread = { ...projection.thread, messages };
    return [projectedThread, ...threads.filter((thread) => thread.id !== threadId)];
  });
  const existingPageState = get(threadMessagePageState)[threadId];
  const projectedPageState = resolveProjectedPageState(existingPageState, {
    hasMore: projection.messagesPage.hasMore,
    nextBefore: projection.messagesPage.nextBefore,
  });
  setThreadPageState(threadId, {
    isLoading: false,
    ...projectedPageState,
    error: null,
  });
  if (selectedVersion && get(activeThreadId) === threadId) {
    await loadVersion(selectedVersion, threadId, { persistSnapshot: false });
  }
  return selectedVersion;
}

/** Activate one canonical workspace returned by a Rust-owned intent. */
export async function activateWorkspaceProjection(
  projection: Awaited<ReturnType<typeof getWorkspaceProjection>>,
): Promise<Message | null> {
  const threadId = projection.thread.id;
  latestThreadSwitchToken += 1;
  latestLoadVersionToken += 1;
  beginThreadSwitch(threadId);
  activeThreadLoadingId.set(null);
  activeThreadMessagesLoading.set(false);
  activeThreadVersionLoading.set(false);
  return projectWorkspaceProjection(projection);
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
  activeVersionId.set(message.id);
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
  return Boolean(message && versionTimelineMessages([message]).length > 0);
}

export function resolveCommittedVersionAfterHistoryRefresh(
  currentVersionId: string | null,
  messages: Message[],
): Message | null {
  if (!currentVersionId || messages.some((message) => message.id === currentVersionId)) {
    return null;
  }
  return [...messages].reverse().find(isVersionCandidate) ?? null;
}

function versionLabel(message: Message): string {
  return (
    message.output?.title ||
    message.versionSummary?.title ||
    message.modelManifest?.document?.documentLabel ||
    message.modelManifest?.document?.documentName ||
    message.artifactBundle?.modelId ||
    message.versionSummary?.modelId ||
    'Imported Model'
  );
}

function hasConsistentRuntimePayload(
  bundle: Message['artifactBundle'] | null | undefined,
  manifest: Message['modelManifest'] | null | undefined,
): boolean {
  return Boolean(bundle && manifest && bundle.modelId === manifest.modelId);
}

function resolveVersionRuntimePayload(message: Message): {
  artifactBundle: Message['artifactBundle'] | null;
  modelManifest: Message['modelManifest'] | null;
} {
  const currentThreadId = get(activeThreadId);
  const currentVersionId = get(activeVersionId);
  const currentSession = get(session);
  if (
    message.id === currentVersionId &&
    currentThreadId &&
    hasConsistentRuntimePayload(currentSession.artifactBundle, currentSession.modelManifest) &&
    message.artifactBundle &&
    sameArtifactVersion(message.artifactBundle, currentSession.artifactBundle)
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

export async function loadVersion(
  msg: Message | null | undefined,
  expectedThreadId: string | null = get(activeThreadId),
  options: { persistSnapshot?: boolean } = {},
): Promise<boolean> {
  if (!isVersionCandidate(msg)) return false;
  console.warn('[CAD_FLOW][version.load.start]', {
    expectedThreadId,
    messageId: msg.id,
    sourceKind: msg.artifactBundle?.sourceKind ?? msg.modelManifest?.sourceKind ?? null,
    modelId: msg.artifactBundle?.modelId ?? msg.modelManifest?.modelId ?? null,
    modelStlPath: msg.artifactBundle?.modelStlPath ?? null,
    hasCanonicalSource: Boolean(msg.output?.macroCode),
  });
  const loadToken = ++latestLoadVersionToken;
  let runtimeUnavailable = false;
  const isStale = () =>
    loadToken !== latestLoadVersionToken ||
    (expectedThreadId !== null && get(activeThreadId) !== expectedThreadId);

  const versionMessage = await hydrateVersionCandidate(msg, expectedThreadId);
  if (isStale()) return false;
  if (expectedThreadId) {
    history.update((threads) =>
      evictSupersededVersionDetails(threads, expectedThreadId, versionMessage),
    );
  }

  if (versionMessage.output) {
    workingCopy.loadVersion(versionMessage.output, versionMessage.id);
    paramPanelState.hydrateFromVersion(versionMessage.output, versionMessage.id);
  } else {
    workingCopy.reset();
    paramPanelState.reset();
  }
  activeVersionId.set(versionMessage.id);
  session.setError(
    versionMessage.status === 'error'
      ? versionMessage.content || versionMessage.output?.response || 'Version render failed.'
      : null,
  );

  const runtimePayload = resolveVersionRuntimePayload(versionMessage);
  const renderableBundle = getRenderableRuntimeBundle(
    runtimePayload.artifactBundle,
    versionMessage.output?.postProcessing ?? null,
    versionMessage.output?.initialParams ?? {},
  );
  const runtime = {
    bundle: renderableBundle?.modelStlPath ? renderableBundle : null,
    degradedToModel: Boolean(
      renderableBundle &&
        (runtimePayload.artifactBundle?.viewerAssets?.length ?? 0) > 0 &&
        (renderableBundle.viewerAssets?.length ?? 0) === 0,
    ),
  };
  console.warn('[CAD_FLOW][version.runtime.result]', {
    expectedThreadId,
    messageId: versionMessage.id,
    modelId: runtimePayload.artifactBundle?.modelId ?? null,
    modelStlPath: runtimePayload.artifactBundle?.modelStlPath ?? null,
    hasStoredModel: Boolean(runtime.bundle),
    degradedToModel: runtime.degradedToModel,
  });
  if (isStale()) return false;
  if (runtime.bundle) {
    session.setStlUrl(toAssetUrl(runtime.bundle.modelStlPath));
    session.setModelRuntime(runtime.bundle, runtimePayload.modelManifest ?? versionMessage.modelManifest ?? null);
    session.setSelectedPartId(null);
  } else if (versionMessage.output) {
    session.clearModelRuntime();
    session.setStlUrl(null);
    runtimeUnavailable = true;
  } else {
    session.setStlUrl(null);
    session.clearModelRuntime();
  }

  if (runtime.degradedToModel) {
    session.setStatus(`Loaded Version: ${versionLabel(versionMessage)} (model only; part geometry was evicted).`);
  } else if (runtimeUnavailable) {
    session.setStatus(`Loaded Version: ${versionLabel(versionMessage)} (saved model unavailable).`);
  } else if (runtime.bundle || versionMessage.output || !versionMessage.artifactBundle) {
    session.setStatus(`Loaded Version: ${versionLabel(versionMessage)}`);
  }

  if (isStale()) return false;
  if (options.persistSnapshot !== false) {
    const persistedBundle = runtime.bundle ?? runtimePayload.artifactBundle ?? versionMessage.artifactBundle ?? null;
    const persistedManifest = runtimePayload.modelManifest ?? versionMessage.modelManifest ?? null;
    await persistLastSessionSnapshot({
      design: versionMessage.output ?? null,
      threadId: expectedThreadId ?? get(activeThreadId),
      messageId: versionMessage.id,
      artifactBundle: persistedBundle,
      modelManifest: persistedManifest,
      selectedPartId: null,
    });
  }
  return Boolean(runtime.bundle);
}

/** Project completed backend work only onto its still-selected version. */
export async function projectSelectedVersionOutcome(
  threadId: string,
  messageId: string,
  readVersion: typeof getThreadMessageVersion = getThreadMessageVersion,
): Promise<void> {
  const isSelected = () => get(activeThreadId) === threadId && get(activeVersionId) === messageId;
  if (!isSelected()) return;
  const version = await readVersion(threadId, messageId);
  if (!version || version.id !== messageId || !isSelected()) return;
  history.update((threads) => mergeCommittedVersionMessage(threads, threadId, '', version));
  if (version.status === 'error') {
    session.setError(version.content || version.output?.response || 'Version render failed.');
    return;
  }
  if (
    version.status === 'success' &&
    hasConsistentRuntimePayload(version.artifactBundle, version.modelManifest) &&
    !sameArtifactVersion(version.artifactBundle, get(session).artifactBundle)
  ) {
    await loadVersion(version, threadId, { persistSnapshot: false });
  }
}

export async function loadFromHistory(thread: Thread) {
  const targetThreadId = thread.id;
  const existingThread = get(history).find((candidate) => candidate.id === targetThreadId);
  const existingPageState = get(threadMessagePageState)[targetThreadId];
  const seededMessages = existingThread?.messages ?? thread.messages ?? [];
  const seededLatestVersion = [...seededMessages].reverse().find(isVersionCandidate) ?? null;
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
  activeThreadMessagesLoading.set(true);
  activeThreadVersionLoading.set(true);
  setThreadPageState(targetThreadId, {
    isLoading: true,
    hasMore: existingPageState?.hasMore ?? false,
    nextBefore: existingPageState?.nextBefore ?? null,
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

  const projectionPromise = withBackendTimeout(
    getWorkspaceProjection(targetThreadId, null, INITIAL_THREAD_MESSAGE_PAGE_LIMIT),
    THREAD_MESSAGES_PAGE_TIMEOUT_MS,
    'Thread workspace projection load timed out',
  );
  let bootstrappedVersionId: string | null = null;

  try {
    if (seededLatestVersion) {
      bootstrappedVersionId = seededLatestVersion.id;
      await loadVersion(seededLatestVersion, targetThreadId);
      if (!isCurrentThreadLoad(switchToken, targetThreadId)) {
        void projectionPromise.catch(() => undefined);
        return;
      }
    }

    const projection = await projectionPromise;
    if (!isCurrentThreadLoad(switchToken, targetThreadId)) {
      return;
    }

    history.update((items) =>
      items.map((candidate) =>
        candidate.id === targetThreadId
          ? { ...candidate, ...projection.thread, messages: candidate.messages ?? [] }
          : candidate,
      ),
    );

    const latestVersion = projection.selectedVersion;
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
        await loadVersion(latestVersion, targetThreadId);
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

  void (async () => {
    try {
      const initialPage = (await projectionPromise).messagesPage;
      if (!isCurrentThreadLoad(switchToken, targetThreadId)) return;
      let page = initialPage;
      let restoredMessages = [...page.messages];
      const restoreCount = storedOlderPageCount(targetThreadId);
      for (let index = 0; index < restoreCount && page.hasMore && page.nextBefore; index += 1) {
        page = await getThreadMessagesPage(
          targetThreadId,
          page.nextBefore,
          OLDER_THREAD_MESSAGE_PAGE_LIMIT,
          false,
        );
        if (!isCurrentThreadLoad(switchToken, targetThreadId)) return;
        restoredMessages = mergeThreadMessages(restoredMessages, page.messages);
      }
      history.update((items) =>
        items.map((candidate) =>
          candidate.id === targetThreadId
            ? {
                ...candidate,
                messages: mergeActiveThreadMessages(
                  candidate.messages ?? [],
                  restoredMessages,
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
        messages: restoredMessages.length,
        hasMore: page.hasMore,
        restoredOlderPages: restoreCount,
      });
    } catch (e) {
      if (isCurrentThreadLoad(switchToken, targetThreadId)) {
        console.error('[History] Failed to load thread messages:', e);
        setThreadPageState(targetThreadId, {
          isLoading: false,
          error: formatBackendError(e),
        });
      }
    } finally {
      if (isCurrentThreadLoad(switchToken, targetThreadId)) {
        activeThreadMessagesLoading.set(false);
        activeThreadLoadingId.set(null);
      }
    }
  })();
}

export async function deleteThread(id: string): Promise<boolean> {
  try {
    const projection = await deleteThreadIntent(
      id,
      get(activeThreadId) === id ? get(activeVersionId) : null,
    );
    if (get(activeThreadId) === id) {
      activeThreadId.set(null);
      activeVersionId.set(null);
      workingCopy.reset();
      paramPanelState.reset();
      session.setStlUrl(null);
    }
    projectHistorySummaries(projection.history);
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
    const selectedMessageId = effectiveActiveVersionId(
      currentThread?.messages ?? [],
      get(activeVersionId),
    );
    const result = await deleteVersionIntent({
      messageId,
      selectedThreadId: currentThreadId,
      selectedMessageId,
      messageLimit: INITIAL_THREAD_MESSAGE_PAGE_LIMIT,
    });
    history.update((items) => items.map((thread) => ({
      ...thread,
      messages: (thread.messages ?? []).filter((message) => message.id !== messageId),
    })));
    if (result.threadRemoved) {
      history.update((items) => items.filter((thread) => thread.id !== result.threadId));
      if (currentThreadId === result.threadId) {
        activeThreadId.set(null);
        detachActiveVersionRuntime();
      }
    } else if (result.workspace) {
      if (currentThreadId === result.threadId && !result.workspace.selectedVersion) {
        detachActiveVersionRuntime();
      }
      await projectWorkspaceProjection(result.workspace);
    }
    session.setStatus('Version removed from the carousel.');
  } catch (e) {
    session.setError(`Failed to delete version: ${formatBackendError(e)}`);
  }
}

export async function restoreVersion(messageId: string) {
  try {
    const currentThreadId = get(activeThreadId);
    const currentThread = currentThreadId
      ? get(history).find((thread) => thread.id === currentThreadId)
      : null;
    const result = await restoreVersionIntent({
      messageId,
      selectedThreadId: currentThreadId,
      selectedMessageId: effectiveActiveVersionId(
        currentThread?.messages ?? [],
        get(activeVersionId),
      ),
      messageLimit: INITIAL_THREAD_MESSAGE_PAGE_LIMIT,
    });
    if (result.workspace) {
      await projectWorkspaceProjection(result.workspace);
    }
    session.setStatus('Version returned to the carousel.');
  } catch (e) {
    session.setError(`Restore Error: ${formatBackendError(e)}`);
  }
}

export async function createNewThread(
  payload: NewThreadPayload | null | undefined,
  deps: {
    createThread?: typeof createDesignThreadIntent;
  } = {},
): Promise<string | null> {
  const mode = payload?.mode === 'macro' ? 'macro' : 'blank';
  let created: Awaited<ReturnType<typeof createDesignThreadIntent>>;
  try {
    created = await (deps.createThread ?? createDesignThreadIntent)({
      mode,
      title: payload?.title ?? null,
      source: mode === 'macro' ? payload?.code ?? null : null,
    });
  } catch (error) {
    session.setError(`New Thread Error: ${formatBackendError(error)}`);
    return null;
  }

  const newId = created.threadId;
  try {
    await activateWorkspaceProjection(created.workspace);
  } catch (error) {
    session.setError(`New Thread Projection Error: ${formatBackendError(error)}`);
    return newId;
  }

  if (created.initialVersionError) {
    session.setError(`Initial Macro Error: ${formatBackendError(created.initialVersionError)}`);
  } else if (mode === 'macro') {
    session.setStatus('New design initialized.');
  } else {
    session.setStatus('New design session started.');
  }
  return newId;
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

    const sourceThreadId = get(activeThreadId);
    if (!sourceThreadId) {
      session.setError('Fork Error: No active thread is loaded.');
      return;
    }
    const result = await forkDesignIntent({
      sourceThreadId,
      sourceMessageId: sourceMessage.id,
      title: sourceMessage.output?.title || label,
      versionName: sourceMessage.output?.versionName || 'Forked',
      messageLimit: INITIAL_THREAD_MESSAGE_PAGE_LIMIT,
    });
    const forkedMessage = result.workspace.selectedVersion;
    if (!forkedMessage) {
      throw new Error('Forked thread projection is unavailable.');
    }
    const forkedThread = {
      ...result.workspace.thread,
      messages: result.workspace.messagesPage.messages,
    };
    history.update((items) => {
      const nextItems = items.filter((item) => item.id !== result.threadId);
      return [forkedThread, ...nextItems];
    });

    latestThreadSwitchToken += 1;
    latestLoadVersionToken += 1;
    beginThreadSwitch(result.threadId);
    setThreadPageState(result.threadId, {
      isLoading: false,
      hasMore: result.workspace.messagesPage.hasMore,
      nextBefore: result.workspace.messagesPage.nextBefore,
      error: null,
    });
    await loadVersion(forkedMessage, result.threadId, { persistSnapshot: false });

    session.setStatus('Design forked into a new thread.');
    paramPanelState.setVersionId(result.messageId);
  } catch (e) {
    session.setError(`Fork Error: ${formatBackendError(e)}`);
  }
}

export async function finalizeThread(id: string) {
  try {
    const selectedMessageId =
      get(activeThreadId) === id ? get(activeVersionId) : null;
    const projection = await finalizeThreadIntent(id, selectedMessageId);
    if (get(activeThreadId) === id) {
      activeThreadId.set(null);
      activeVersionId.set(null);
      workingCopy.reset();
      paramPanelState.reset();
      session.setStlUrl(null);
    }
    projectHistorySummaries(projection.history);
    session.setStatus('Project completed.');
  } catch (e) {
    session.setError(`Finalize Error: ${formatBackendError(e)}`);
  }
}

export async function reopenThread(id: string) {
  try {
    const projection = await reopenThreadIntent(id);
    projectHistorySummaries(projection.history);
    session.setStatus('Project reopened.');
  } catch (e) {
    session.setError(`Reopen Error: ${formatBackendError(e)}`);
  }
}

export async function openInventoryThread(id: string): Promise<boolean> {
  try {
    const projection = await openInventoryThreadIntent(
      id,
      INITIAL_THREAD_MESSAGE_PAGE_LIMIT,
    );
    await activateWorkspaceProjection(projection);
    session.setStatus('Viewing completed project.');
    return true;
  } catch (e) {
    session.setError(`Open Completed Project Error: ${formatBackendError(e)}`);
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
    persistOlderPageCount(threadId, storedOlderPageCount(threadId) + 1);
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
    projectHistorySummaries(freshHistory);
  } catch (e) {
    console.error("[History] Failed to refresh history:", e);
  }
}

/**
 * Refreshes only bounded history projections for one thread.
 *
 * Event bursts coalesce behind one in-flight request. Existing older pages and
 * hydrated selected-version payload remain resident; the newest compact page
 * patches matching rows without resetting timeline UI state.
 */
export function refreshThreadHistoryProjection(
  threadId: string,
  revision?: number | null,
): Promise<void> {
  const requestedRevision = Number.isFinite(revision)
    ? Number(revision)
    : ++localHistoryProjectionRevision;

  return historyProjectionRefreshes.request(
    threadId,
    requestedRevision,
    async () => {
      const startedAt = performance.now();
      const [freshHistory, page] = await Promise.all([
        getHistory(),
        getThreadMessagesPage(
          threadId,
          null,
          INITIAL_THREAD_MESSAGE_PAGE_LIMIT,
          false,
        ),
      ]);
      profileLog('history.refresh_projection', {
        projection: 'thread_summary+timeline_page',
        durationMs: Number((performance.now() - startedAt).toFixed(2)),
        rows: page.messages.length,
        bytes: page.observedBytes,
        truncatedFields: page.truncatedFields?.length ?? 0,
      });
      return { freshHistory, page };
    },
    ({ freshHistory, page }) => {
      const freshSummary = freshHistory.find((thread) => thread.id === threadId) ?? null;
      history.update((items) => {
        const existing = items.find((thread) => thread.id === threadId) ?? null;
        const existingMessages = existing?.messages ?? [];
        const patchedMessages = page.messages.map((message) =>
          mergeThreadMessagePayload(
            existingMessages.find((candidate) => candidate.id === message.id),
            message,
          ),
        );
        const messages = mergeThreadMessages(existingMessages, patchedMessages);
        const summary = freshSummary ?? existing;
        if (!summary) return items;
        const nextThread = { ...summary, messages };
        const nextItems = items.some((thread) => thread.id === threadId)
          ? items.map((thread) => (thread.id === threadId ? nextThread : thread))
          : [nextThread, ...items];
        const otherFresh = new Map(
          freshHistory
            .filter((thread) => thread.id !== threadId)
            .map((thread) => [thread.id, thread]),
        );
        return nextItems.map((thread) => {
          const nextSummary = otherFresh.get(thread.id);
          return nextSummary
            ? { ...nextSummary, messages: thread.messages ?? [] }
            : thread;
        });
      });

      const existingPageState = get(threadMessagePageState)[threadId];
      setThreadPageState(threadId, {
        isLoading: false,
        hasMore: existingPageState?.nextBefore !== null && existingPageState?.nextBefore !== undefined
          ? existingPageState.hasMore
          : page.hasMore,
        nextBefore: existingPageState?.nextBefore ?? page.nextBefore,
        error: null,
      });
    },
  ).catch((error) => {
      setThreadPageState(threadId, {
        isLoading: false,
        error: formatBackendError(error),
      });
      throw error;
    });
}
