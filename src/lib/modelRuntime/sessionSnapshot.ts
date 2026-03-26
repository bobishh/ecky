import { get } from 'svelte/store';

import { activeThreadId, activeVersionId } from '../stores/domainState';
import { paramPanelState } from '../stores/paramPanelState';
import { session } from '../stores/sessionStore';
import { workingCopy } from '../stores/workingCopy';
import { buildImportedSyntheticDesign } from './importedRuntime';
import { saveLastDesign } from '../tauri/client';
import type { DesignOutput, LastDesignSnapshot } from '../types/domain';

function buildWorkingDesign(): DesignOutput | null {
  const current = get(workingCopy);
  if (!current.macroCode.trim()) {
    const currentSession = get(session);
    const panel = get(paramPanelState);
    return buildImportedSyntheticDesign(currentSession.modelManifest, panel.params, panel.uiSpec);
  }

  return {
    title: current.title,
    versionName: current.versionName,
    response: '',
    interactionMode: 'design',
    macroCode: current.macroCode,
    macroDialect: current.macroDialect ?? 'legacy',
    uiSpec: current.uiSpec,
    initialParams: current.params,
    postProcessing: current.postProcessing ?? null,
  };
}

export async function persistLastSessionSnapshot(
  overrides: Partial<LastDesignSnapshot> = {},
): Promise<void> {
  const currentSession = get(session);
  const baseManifest = overrides.modelManifest ?? currentSession.modelManifest;
  const candidateSelectedPartId = overrides.selectedPartId ?? currentSession.selectedPartId;
  const selectedPartId =
    candidateSelectedPartId &&
    baseManifest?.parts?.some((part) => part.partId === candidateSelectedPartId)
      ? candidateSelectedPartId
      : null;

  const snapshot: LastDesignSnapshot = {
    design: overrides.design !== undefined ? overrides.design : buildWorkingDesign(),
    threadId: overrides.threadId !== undefined ? overrides.threadId : get(activeThreadId),
    messageId: overrides.messageId !== undefined ? overrides.messageId : get(activeVersionId),
    artifactBundle:
      overrides.artifactBundle !== undefined
        ? overrides.artifactBundle
        : currentSession.artifactBundle,
    modelManifest: baseManifest ?? null,
    selectedPartId,
  };

  if (!snapshot.design && !snapshot.artifactBundle && !snapshot.modelManifest) {
    await clearLastSessionSnapshot();
    return;
  }

  try {
    await saveLastDesign(snapshot);
  } catch (error) {
    console.warn('[SessionSnapshot] Failed to persist last snapshot:', error);
  }
}

export async function clearLastSessionSnapshot(): Promise<void> {
  try {
    await saveLastDesign(null);
  } catch (error) {
    console.warn('[SessionSnapshot] Failed to clear last snapshot:', error);
  }
}
