import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { workingCopy } from '../stores/workingCopy';
import { activeThreadId, activeVersionId, config } from '../stores/domainState';
import { refreshHistory } from '../stores/history';
import { showCodeModal } from '../stores/viewState';
import { session, setManualRenderActive } from '../stores/sessionStore';
import { startMicrowaveHum, stopMicrowaveHum, ensureContext } from '../audio/microwave';
import { paramPanelState } from '../stores/paramPanelState';
import type { DesignOutput, DesignParams } from '../types/domain';

let latestParamRenderSeq = 0;

function toAssetUrl(path: string | null | undefined): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

export async function handleParamChange(
  newParams: DesignParams,
  forcedCode: string | null = null,
  persist: boolean = true
) {
  console.log('[ManualController] handleParamChange start', { newParams, persist });
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  const snapshotThreadId = get(activeThreadId);
  const currentParams = { ...panel.params, ...newParams };
  const renderSeq = ++latestParamRenderSeq;
  
  // 1. Update workingCopy immediately for UI responsiveness
  paramPanelState.setParams(currentParams);
  workingCopy.patch({ params: currentParams });

  const codeToUse = forcedCode || panel.macroCode || wc.macroCode;
  if (!codeToUse) {
    console.warn('[ManualController] No macroCode to execute');
    return;
  }

  ensureContext();

  session.setStatus('Executing FreeCAD engine...');
  try {
    setManualRenderActive(true);
    const currentConfig = get(config);
    startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);

    console.log('[ManualController] Invoking render_stl with', { parameters: currentParams });
    const absolutePath = await invoke<string>('render_stl', {
      macroCode: codeToUse,
      parameters: currentParams
    });

    if (renderSeq !== latestParamRenderSeq) {
      return;
    }

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(toAssetUrl(absolutePath));
    }

    const sourceVersionId = panel.versionId || wc.sourceVersionId;
    if (persist && sourceVersionId) {
      console.log('[ManualController] Persisting parameters to messageId:', sourceVersionId);
      try {
        await invoke('update_parameters', { messageId: sourceVersionId, parameters: currentParams });
        console.log('[ManualController] update_parameters success');
        if (renderSeq === latestParamRenderSeq && get(activeThreadId) === snapshotThreadId) {
          await refreshHistory();
        }
      } catch (e) {
        console.error('[ManualController] Failed to persist parameters:', e);
      }
    }
  } catch (e) {
    console.error('[ManualController] render_stl error:', e);
    if (renderSeq === latestParamRenderSeq && get(activeThreadId) === snapshotThreadId) {
      session.setError(`Render Error: ${e}`);
    }
  } finally {
    if (renderSeq === latestParamRenderSeq) {
      stopMicrowaveHum('__manual__');
      setManualRenderActive(false);
    }
  }
}

export async function commitManualVersion(editedCode: string, titleOverride: string | null = null) {
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  const snapshotThreadId = get(activeThreadId);

  if (!snapshotThreadId) {
    session.setError("Cannot commit manual version: No active thread.");
    return;
  }

  session.setStatus("Validating and committing manual edit...");
  try {
    setManualRenderActive(true);
    const currentConfig = get(config);
    startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);

    const absolutePath = await invoke<string>('render_stl', {
      macroCode: editedCode,
      parameters: panel.params
    });

    // Use camelCase for threadId, macroCode, uiSpec
    const newMsgId = await invoke<string>('add_manual_version', {
      threadId: snapshotThreadId,
      title: titleOverride || wc.title || "Manual Edit",
      versionName: "V-manual",
      macroCode: editedCode,
      parameters: panel.params,
      uiSpec: panel.uiSpec
    });

    const committedDesign: DesignOutput = {
      title: titleOverride || wc.title || "Manual Edit",
      versionName: "V-manual",
      response: "Manual edit committed as new version.",
      interactionMode: "design",
      macroCode: editedCode,
      uiSpec: panel.uiSpec,
      initialParams: panel.params
    };

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(toAssetUrl(absolutePath));
      workingCopy.loadVersion(committedDesign, newMsgId);
      paramPanelState.hydrateFromVersion(committedDesign, newMsgId);
      activeVersionId.set(newMsgId);
      showCodeModal.set(false);
      session.setStatus("Manual version committed.");
      await refreshHistory();
    }
    
    stopMicrowaveHum('__manual__');
    setManualRenderActive(false);
  } catch (e) {
    console.error('[ManualController] commitManualVersion error:', e);
    if (get(activeThreadId) === snapshotThreadId) {
      session.setError(`Manual Commit Failed: ${e}`);
    }
    stopMicrowaveHum('__manual__');
    setManualRenderActive(false);
  }
}
