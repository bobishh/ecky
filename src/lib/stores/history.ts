import { get } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { history, activeThreadId, activeVersionId } from './domainState';
import { workingCopy } from './workingCopy';
import { session } from './sessionStore';
import { handleParamChange, commitManualVersion } from '../controllers/manualController';
import { paramPanelState } from './paramPanelState';
import { estimateBase64Bytes, profileLog } from '../debug/profiler';
import { clearLastSessionSnapshot, persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import type { Message, Thread } from '../types/domain';
import {
  deleteThread as deleteThreadCommand,
  deleteVersion as deleteVersionCommand,
  formatBackendError,
  getHistory,
  getMessStlPath,
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

export async function loadVersion(msg: Message | null | undefined) {
  if (!isVersionCandidate(msg)) return;
  activeVersionId.set(msg.id);
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
  // Lazy load messages if they aren't present
  if (!thread.messages || thread.messages.length === 0) {
    try {
      freshThread = await getThread(targetThreadId);
      // Update the thread in the history store list so we don't fetch it again
      history.update(h => h.map(t => t.id === targetThreadId ? { ...t, messages: freshThread.messages } : t));
    } catch (e) {
      console.error("[History] Failed to lazy-load thread:", e);
    }
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

export function forkDesign() {
  const newId = crypto.randomUUID();
  activeThreadId.set(newId);
  activeVersionId.set(null);
  paramPanelState.setVersionId(null);
  workingCopy.patch({
    versionName: 'Forked',
    sourceVersionId: null
  });
  session.setStatus('Design forked. Next generation will create a new thread.');
  void clearLastSessionSnapshot();
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
