import { get } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { history, activeThreadId, activeVersionId } from './domainState';
import { workingCopy, isDirty } from './workingCopy';
import { session } from './sessionStore';
import { handleParamChange, commitManualVersion } from '../controllers/manualController';
import { paramPanelState } from './paramPanelState';
import { estimateBase64Bytes, profileLog } from '../debug/profiler';
import { clearLastSessionSnapshot, persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import type { AgentDraft, Message, Thread } from '../types/domain';
import {
  addImportedModelVersion,
  addManualVersion,
  deleteAgentDraft,
  deleteThread as deleteThreadCommand,
  deleteVersion as deleteVersionCommand,
  finalizeThread as finalizeThreadCommand,
  reopenThread as reopenThreadCommand,
  getInventory as getInventoryCommand,
  formatBackendError,
  getHistory,
  getMessStlPath,
  getModelManifest,
  renameThread as renameThreadCommand,
  getThread,
  restoreVersion as restoreVersionCommand,
} from '../tauri/client';

type NewThreadPayload = {
  mode?: 'blank' | 'macro';
  code?: string;
  title?: string;
};

function toAssetUrl(path: string | null | undefined): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

function isVersionCandidate(message: Message | null | undefined): message is Message {
  return Boolean(message && message.role === 'assistant' && (message.output || message.artifactBundle));
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

async function resolveForkRuntimePayload(message: Message): Promise<{
  artifactBundle: Message['artifactBundle'] | null;
  modelManifest: Message['modelManifest'] | null;
}> {
  if (hasConsistentRuntimePayload(message.artifactBundle, message.modelManifest)) {
    return {
      artifactBundle: message.artifactBundle ?? null,
      modelManifest: message.modelManifest ?? null,
    };
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

  if (message.artifactBundle) {
    try {
      const refreshedManifest = await getModelManifest(message.artifactBundle.modelId);
      if (message.artifactBundle.modelId === refreshedManifest.modelId) {
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

export async function applyAgentDraft(draft: AgentDraft) {
  const { designOutput, artifactBundle, modelManifest, baseMessageId } = draft;
  workingCopy.loadVersion(designOutput, baseMessageId);
  paramPanelState.hydrateFromVersion(designOutput, baseMessageId);
  if (artifactBundle) {
    session.setStlUrl(toAssetUrl(artifactBundle.previewStlPath));
    session.setModelRuntime(artifactBundle, modelManifest ?? null);
  }
  session.setAgentDraft(null);
  session.setStatus('Agent draft loaded. Review and SAVE VERSION to persist.');
}

export async function loadVersion(msg: Message | null | undefined) {
  if (!isVersionCandidate(msg)) return;
  activeVersionId.set(msg.id);
  
  session.setAgentDraft(null);

  if (msg.output) {
    workingCopy.loadVersion(msg.output, msg.id);
    paramPanelState.hydrateFromVersion(msg.output, msg.id);
  } else {
    workingCopy.reset();
    paramPanelState.reset();
  }

  if (msg.artifactBundle) {
    session.setStlUrl(toAssetUrl(msg.artifactBundle.previewStlPath));
    session.setModelRuntime(msg.artifactBundle, msg.modelManifest ?? null);
    session.setSelectedPartId(null);
  } else if (msg.output) {
    session.clearModelRuntime();
    await handleParamChange(msg.output.initialParams || {}, msg.output.macroCode, false);
  } else {
    session.setStlUrl(null);
    session.clearModelRuntime();
  }

  session.setStatus(`Loaded Version: ${versionLabel(msg)}`);

  await persistLastSessionSnapshot({
    design: msg.output ?? null,
    threadId: get(activeThreadId),
    messageId: msg.id,
    artifactBundle: msg.artifactBundle ?? null,
    modelManifest: msg.modelManifest ?? null,
    selectedPartId: null,
  });
}

export async function loadFromHistory(thread: Thread) {
  const targetThreadId = thread.id;
  activeThreadId.set(targetThreadId);
  
  let freshThread: Thread = thread;
  try {
    freshThread = await getThread(targetThreadId);
    history.update((items) =>
      items.map((candidate) =>
        candidate.id === targetThreadId ? { ...candidate, messages: freshThread.messages } : candidate,
      ),
    );
  } catch (e) {
    console.error('[History] Failed to load thread:', e);
  }
  
  const lastAssistantMsg = [...freshThread.messages].reverse().find(isVersionCandidate);
  const imagePayloadBytes = (freshThread.messages || []).reduce((sum, m) => sum + estimateBase64Bytes(m.imageData), 0);
  profileLog('history.load_thread', {
    threadId: targetThreadId,
    messages: freshThread.messages?.length || 0,
    images: (freshThread.messages || []).filter(m => !!m.imageData).length,
    imagePayloadMb: Number((imagePayloadBytes / (1024 * 1024)).toFixed(2)),
  });
  
  if (lastAssistantMsg) {
    await loadVersion(lastAssistantMsg);
  } else {
    // Thread has no successful versions (failed or pending)
    activeVersionId.set(null);
    workingCopy.reset();
    paramPanelState.reset();
    
    // Show mess if there are failed attempts
    const hasFailed = freshThread.messages?.some(m => m.status === 'error') ?? false;
    if (hasFailed) {
      try {
        const messPath = await getMessStlPath();
        session.setStlUrl(toAssetUrl(messPath));
        session.clearModelRuntime();
      } catch (e) {
        session.setStlUrl(null);
      }
    } else {
      session.setStlUrl(null);
    }
    await clearLastSessionSnapshot();
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

export async function deleteVersion(messageId: string) {
  try {
    await deleteVersionCommand(messageId);
    const currentThreadId = get(activeThreadId);
    if (!currentThreadId) return;

    // Use refreshHistory to correctly fetch thread messages
    await refreshHistory();

    // Update active version if we deleted the current one
    if (get(activeVersionId) === messageId) {
      const currentHistory = get(history);
      const updatedThread = currentHistory.find(t => t.id === currentThreadId);
      if (!updatedThread) {
        activeThreadId.set(null);
        activeVersionId.set(null);
        workingCopy.reset();
        paramPanelState.reset();
        session.setStlUrl(null);
        session.clearModelRuntime();
        await clearLastSessionSnapshot();
        return;
      }
      const remainingVersions = updatedThread?.messages
        ? updatedThread.messages.filter(isVersionCandidate)
        : [];
      
      if (remainingVersions.length > 0) {
        // Load the last available version
        await loadVersion(remainingVersions[remainingVersions.length - 1]);
      } else {
        // No versions left, reset working copy
        activeVersionId.set(null);
        workingCopy.reset();
        paramPanelState.reset();
        session.setStlUrl(null);
        session.clearModelRuntime();
        await clearLastSessionSnapshot();
      }
    }
  } catch (e) {
    session.setError(`Failed to delete version: ${formatBackendError(e)}`);
  }
}

export async function restoreVersion(messageId: string) {
  try {
    await restoreVersionCommand(messageId);
    await refreshHistory();
    session.setStatus('Version restored.');
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

  let thread = get(history).find((candidate) => candidate.id === threadId) ?? null;
  if (!thread?.messages?.length) {
    try {
      thread = await getThread(threadId);
      history.update((items) =>
        items.some((candidate) => candidate.id === threadId)
          ? items.map((candidate) =>
              candidate.id === threadId ? { ...candidate, messages: thread?.messages ?? [] } : candidate,
            )
          : items,
      );
    } catch (e) {
      console.warn('[History] Failed to load active thread for fork:', e);
    }
  }

  const messages = thread?.messages || [];
  const selectedVersionId = get(activeVersionId);
  const selectedMessage = selectedVersionId
    ? messages.find((message) => message.id === selectedVersionId)
    : null;
  if (isVersionCandidate(selectedMessage)) return selectedMessage;
  return [...messages].reverse().find(isVersionCandidate) ?? null;
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
    await finalizeThreadCommand(id);
    if (get(activeThreadId) === id) {
      activeThreadId.set(null);
      activeVersionId.set(null);
      workingCopy.reset();
      paramPanelState.reset();
      session.setStlUrl(null);
      await clearLastSessionSnapshot();
    }
    await refreshHistory();
    session.setStatus('Thread finalized and moved to inventory.');
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
    const tid = get(activeThreadId);
    
    if (tid) {
      try {
        const fullThread = await getThread(tid);
        const imagePayloadBytes = (fullThread.messages || []).reduce((sum, m) => sum + estimateBase64Bytes(m.imageData), 0);
        profileLog('history.refresh_active_thread', {
          threadId: tid,
          messages: fullThread.messages?.length || 0,
          images: (fullThread.messages || []).filter(m => !!m.imageData).length,
          imagePayloadMb: Number((imagePayloadBytes / (1024 * 1024)).toFixed(2)),
        });
        const updatedHistory = freshHistory.map(t => 
          t.id === tid ? { ...t, messages: fullThread.messages } : t
        );
        history.set(updatedHistory);
      } catch (e) {
        console.warn("[History] Failed to refresh full active thread:", e);
        history.set(freshHistory);
      }
    } else {
      history.set(freshHistory);
    }
  } catch (e) {
    console.error("[History] Failed to refresh history:", e);
  }
}
