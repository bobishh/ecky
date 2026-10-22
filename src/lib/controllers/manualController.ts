import { get } from 'svelte/store';
import { writable } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { workingCopy } from '../stores/workingCopy';
import { activeThreadIdStore as activeThreadId, activeVersionId, config } from '../stores/domainState';
import { activateWorkspaceProjection, rememberCommittedVersionMessage } from '../stores/history';
import { session } from '../stores/sessionStore';
import { startMicrowaveHum, stopMicrowaveHum, ensureContext } from '../audio/microwave';
import { paramPanelState } from '../stores/paramPanelState';
import { resolveParamApplySource } from './paramApplySource';
import { recordSessionActivityEvent } from '../stores/sessionActivityStore';
import { confirmAction } from '../ui/confirmAction';
import type {
  DesignOutput,
  DesignParams,
  PostProcessingSpec,
  SourceLanguage,
  GeometryBackend,
  UiSpec,
} from '../types/domain';
import {
  applyManualCode,
  applyManualParameters,
  applyImportedParameters,
  createDesignThreadIntent,
  formatBackendError,
  type ManualCodeApplyResult,
} from '../tauri/client';
import { closeWindow as closeWindowStore } from '../stores/windowStore';
import type { WorkingCopyState } from '../stores/workingCopy';
import { pendingImageGeometry, pendingImageGeometryStatus } from '../imageGeometryPending';
import { LatestTaskGate } from './latestTaskGate';
import { activeRenderSnapshot, hydrateActiveRenderSnapshot } from '../stores/activeRenderSnapshot';
import { manualVersionLabel } from '../manualVersionLabels';

const latestParamRenderGate = new LatestTaskGate();
const manualApplyBusy = writable(false);
let inFlightManualApplies = 0;
const updateManualApplyBusy = () => manualApplyBusy.set(inFlightManualApplies > 0);

export const manualApplyBusyStore = manualApplyBusy;

async function trackManualApply<T>(task: () => Promise<T>): Promise<T> {
  inFlightManualApplies += 1;
  updateManualApplyBusy();
  try {
    return await task();
  } finally {
    inFlightManualApplies = Math.max(0, inFlightManualApplies - 1);
    updateManualApplyBusy();
  }
}

type ManualCommitOptions = {
  successStatus?: string;
  versionName?: string | null;
};

export type ManualVersionCommitInput = {
  code: string;
  title?: string | null;
  versionName?: string | null;
};

export function shouldPreserveWorkingCopyMacroDraft(
  workingCopyState: Pick<WorkingCopyState, 'macroCode' | 'dirty'>,
  committedMacroCode: string,
): boolean {
  return workingCopyState.dirty && workingCopyState.macroCode !== committedMacroCode;
}

function restoreWorkingCopyMacroDraftIfNeeded(
  previousWorkingCopy: Pick<WorkingCopyState, 'macroCode' | 'dirty'>,
  committedMacroCode: string,
) {
  if (!shouldPreserveWorkingCopyMacroDraft(previousWorkingCopy, committedMacroCode)) return;
  workingCopy.patch({
    macroCode: previousWorkingCopy.macroCode,
    dirty: true,
  });
}

function changedParamKeys(before: DesignParams, after: DesignParams): string[] {
  const keys = new Set([...Object.keys(before), ...Object.keys(after)]);
  return [...keys].filter((key) => stableJson(before[key]) !== stableJson(after[key])).sort();
}

// Exported for the once-per-action emission tests (session-collab-visibility 5.7).
export function recordParamsChanged(input: {
  threadId: string | null;
  versionId: string | null;
  before: DesignParams;
  after: DesignParams;
  persist: boolean;
}) {
  const keys = changedParamKeys(input.before, input.after);
  if (keys.length === 0) return;
  recordSessionActivityEvent({
    threadId: input.threadId,
    versionId: input.versionId,
    kind: 'params_changed',
    title: input.persist ? 'Parameter commit requested' : 'Parameters applied',
    summary: `${keys.length} parameter${keys.length === 1 ? '' : 's'} changed: ${keys.join(', ')}`,
    severity: 'info',
    diffs: keys.map((key) => ({
      kind: 'params',
      key,
      before: stableJson(input.before[key]),
      after: stableJson(input.after[key]),
    })),
  });
}

function recordRenderEvent(input: {
  threadId: string | null;
  versionId: string | null;
  kind: 'render_started' | 'render_succeeded' | 'render_failed';
  title: string;
  summary: string;
  severity: 'info' | 'success' | 'error';
  raw?: unknown;
}) {
  recordSessionActivityEvent(input);
}

function stableJson(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(stableJson).join(',')}]`;
  }
  if (value && typeof value === 'object') {
    return `{${Object.entries(value as Record<string, unknown>)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, entry]) => `${JSON.stringify(key)}:${stableJson(entry)}`)
      .join(',')}}`;
  }
  return JSON.stringify(value);
}

function toAssetUrl(path: string | null | undefined): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

async function applyStoredTargetParameters(input: {
  threadId: string;
  targetVersionId: string | null;
  params: DesignParams;
  persist: boolean;
  title: string | null;
  versionName: string | null;
  renderToken: ReturnType<LatestTaskGate['reserve']>;
  imported?: boolean;
  source?: string;
  uiSpec?: UiSpec;
  postProcessing?: PostProcessingSpec | null;
  sourceLanguage?: SourceLanguage | null;
  geometryBackend?: GeometryBackend | null;
}): Promise<boolean> {
  ensureContext();
  const currentConfig = get(config);
  startMicrowaveHum('__manual__', currentConfig, input.threadId);
  session.setStatus(input.persist ? 'Appending parameter version...' : 'Applying parameter preview...');
  recordRenderEvent({
    threadId: input.threadId,
    versionId: input.targetVersionId,
    kind: 'render_started',
    title: 'Parameter apply started',
    summary: 'Rust controller is rendering parameter state.',
    severity: 'info',
  });

  try {
    const result = input.source
      ? await applyManualCode({
          threadId: input.threadId,
          baseMessageId: input.targetVersionId,
          source: input.source,
          persist: input.persist,
          title: input.title,
          versionName: input.versionName,
          uiSpec: input.uiSpec ?? { fields: [] },
          parameters: input.params,
          postProcessing: input.postProcessing ?? null,
          sourceLanguage: input.sourceLanguage ?? null,
          geometryBackend: input.geometryBackend ?? null,
        })
      : await (input.imported ? applyImportedParameters : applyManualParameters)({
          threadId: input.threadId,
          targetMessageId: input.targetVersionId!,
          parameters: input.params,
          persist: input.persist,
          title: input.title,
          versionName: input.versionName,
        });
    const isCurrent =
      latestParamRenderGate.isCurrent(input.renderToken) &&
      get(activeThreadId) === input.threadId;
    if (!isCurrent) return false;

    if (result.status === 'working' && result.messageId) {
      rememberCommittedVersionMessage(input.threadId, result.designOutput.title, {
        id: result.messageId,
        role: 'assistant',
        content: 'Parameter version pending render.',
        status: 'working',
        output: result.designOutput,
        usage: null,
        artifactBundle: null,
        modelManifest: null,
        agentOrigin: null,
        imageData: null,
        visualKind: null,
        attachmentImages: [],
        timestamp: Date.now() / 1000,
      });
      workingCopy.loadVersion(result.designOutput, result.messageId);
      paramPanelState.hydrateFromVersion(result.designOutput, result.messageId);
      session.setStatus('Parameter version appended. Rendering...');
      recordSessionActivityEvent({
        threadId: input.threadId,
        versionId: result.messageId,
        kind: 'version_committed',
        title: 'Parameter version appended',
        summary: 'Render continues in background.',
        severity: 'info',
      });
      return true;
    }

    if (result.status === 'error' || result.error) {
      if (input.persist && result.messageId) {
        rememberCommittedVersionMessage(input.threadId, result.designOutput.title, {
          id: result.messageId,
          role: 'assistant',
          content: result.designOutput.response,
          status: 'error',
          output: result.designOutput,
          usage: null,
          artifactBundle: null,
          modelManifest: null,
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: Date.now() / 1000,
        });
      }
      const error = result.error ?? `Parameter apply failed with status ${result.status}.`;
      recordRenderEvent({
        threadId: input.threadId,
        versionId: result.messageId ?? input.targetVersionId,
        kind: 'render_failed',
        title: 'Parameter apply failed',
        summary: formatBackendError(error),
        severity: 'error',
        raw: error,
      });
      session.setError(error);
      return false;
    }

    if (!result.artifactBundle || !result.modelManifest || !result.snapshotId) {
      throw new Error('Parameter apply succeeded without complete runtime payload.');
    }

    const messageId = result.messageId ?? input.targetVersionId;
    const successStatus = input.persist
      ? 'Parameter version appended.'
      : 'Parameter preview applied.';
    if (input.persist && result.messageId) {
      rememberCommittedVersionMessage(input.threadId, result.designOutput.title, {
        id: result.messageId,
        role: 'assistant',
        content: result.designOutput.response,
        status: result.status,
        output: result.designOutput,
        usage: null,
        artifactBundle: result.artifactBundle,
        modelManifest: result.modelManifest,
        agentOrigin: null,
        imageData: null,
        visualKind: null,
        attachmentImages: [],
        timestamp: Date.now() / 1000,
      });
      activeVersionId.set(result.messageId);
      workingCopy.loadVersion(result.designOutput, result.messageId);
      paramPanelState.hydrateFromVersion(result.designOutput, result.messageId);
    }
    hydrateActiveRenderSnapshot({
      snapshotId: result.snapshotId,
      threadId: input.threadId,
      messageId,
      design: result.designOutput,
      artifactBundle: result.artifactBundle,
      modelManifest: result.modelManifest,
      selectedPartId: null,
      stlUrl: toAssetUrl(result.artifactBundle.modelStlPath),
      status: successStatus,
      targetRef: result.messageId
        ? { kind: 'savedVersion', threadId: input.threadId, messageId: result.messageId }
        : null,
    });
    recordRenderEvent({
      threadId: input.threadId,
      versionId: messageId,
      kind: 'render_succeeded',
      title: 'Parameter apply succeeded',
      summary: successStatus,
      severity: 'success',
      raw: {
        modelId: result.artifactBundle.modelId,
        modelStlPath: result.artifactBundle.modelStlPath,
      },
    });
    if (input.persist && result.messageId) {
      recordSessionActivityEvent({
        threadId: input.threadId,
        versionId: result.messageId,
        kind: 'version_committed',
        title: 'Parameter version appended',
        summary: successStatus,
        severity: 'success',
      });
    }
    return true;
  } catch (error) {
    if (
      latestParamRenderGate.isCurrent(input.renderToken) &&
      get(activeThreadId) === input.threadId
    ) {
      recordRenderEvent({
        threadId: input.threadId,
        versionId: input.targetVersionId,
        kind: 'render_failed',
        title: 'Parameter apply failed',
        summary: formatBackendError(error),
        severity: 'error',
        raw: error,
      });
      session.setError(
        error && typeof error === 'object' && 'code' in error && 'message' in error
          ? (error as import('../tauri/contracts').AppError)
          : `Apply Failed: ${formatBackendError(error)}`,
      );
    }
    return false;
  } finally {
    if (latestParamRenderGate.isCurrent(input.renderToken)) stopMicrowaveHum('__manual__');
  }
}

export async function handleParamChange(
  newParams: DesignParams,
  forcedCode: string | null = null,
  persist: boolean = false
): Promise<boolean> {
  return trackManualApply(() => doHandleParamChange(newParams, forcedCode, persist));
}

async function doHandleParamChange(
  newParams: DesignParams,
  forcedCode: string | null = null,
  persist: boolean = false
): Promise<boolean> {
  console.log('[ManualController] handleParamChange start', { newParams, persist });
  session.setError(null);
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  const snapshotThreadId = get(activeThreadId);
  const applySource = resolveParamApplySource({
    forcedCode,
    workingMacroCode: wc.macroCode,
    panelVersionId: panel.versionId,
    sourceVersionId: wc.sourceVersionId,
    activeVersionId: get(activeVersionId),
  });
  const targetVersionId =
    applySource.ok || applySource.reason === 'missing-macro-code'
      ? applySource.targetVersionId
      : wc.sourceVersionId || get(activeVersionId) || panel.versionId;
  if (!applySource.ok && applySource.reason === 'stale-panel-source-version-mismatch') {
    console.warn('[ManualController] Stale parameter panel source mismatch', applySource);
    if (get(activeThreadId) === snapshotThreadId) {
      session.setError(
        `Apply Failed: parameter panel is stale for active source (${applySource.panelVersionId} != ${applySource.sourceVersionId}). Reload the active version.`,
      );
    }
    return false;
  }

  const currentParams = forcedCode ? { ...newParams } : { ...panel.params, ...newParams };
  const renderToken = latestParamRenderGate.reserve(snapshotThreadId ?? '__detached__');
  recordParamsChanged({
    threadId: snapshotThreadId,
    versionId: targetVersionId,
    before: panel.params,
    after: currentParams,
    persist,
  });
  
  // 1. Update workingCopy immediately for UI responsiveness
  paramPanelState.setParams(currentParams);
  workingCopy.patch({ params: currentParams });

  const codeToUse = applySource.ok ? applySource.code : '';
  const pendingImages = pendingImageGeometry(codeToUse, panel.uiSpec, currentParams);
  if (pendingImages.length > 0) {
    if (get(activeThreadId) === snapshotThreadId) {
      session.setStatus(pendingImageGeometryStatus(pendingImages));
    }
    return true;
  }
  const renderSnapshot = get(activeRenderSnapshot);
  const storedTargetMatches =
    !forcedCode &&
    Boolean(snapshotThreadId) &&
    Boolean(targetVersionId) &&
    renderSnapshot?.threadId === snapshotThreadId &&
    renderSnapshot.messageId === targetVersionId &&
    renderSnapshot.design.macroCode === codeToUse;
  if (storedTargetMatches && snapshotThreadId && targetVersionId) {
    return applyStoredTargetParameters({
      threadId: snapshotThreadId,
      targetVersionId,
      params: currentParams,
      persist,
      title: wc.title || null,
      versionName: wc.versionName || null,
      renderToken,
    });
  }
  if (!codeToUse) {
    if (!snapshotThreadId || !targetVersionId) {
      console.warn('[ManualController] No macroCode or imported component runtime');
      if (get(activeThreadId) === snapshotThreadId) {
        session.setError('Apply Failed: imported parameters require a bound target version.');
      }
      return false;
    }
    return applyStoredTargetParameters({
      threadId: snapshotThreadId,
      targetVersionId,
      params: currentParams,
      persist,
      title: wc.title || null,
      versionName: wc.versionName || null,
      renderToken,
      imported: true,
    });
  }

  if (!snapshotThreadId) {
    session.setError('Apply Failed: source parameters require a bound design thread.');
    return false;
  }
  return applyStoredTargetParameters({
    threadId: snapshotThreadId,
    targetVersionId: targetVersionId ?? null,
    params: currentParams,
    persist,
    title: wc.title || null,
    versionName: wc.versionName || null,
    renderToken,
    source: codeToUse,
    uiSpec: panel.uiSpec,
    postProcessing: wc.postProcessing ?? null,
    sourceLanguage: wc.sourceLanguage ?? null,
    geometryBackend: wc.geometryBackend ?? null,
  });

}

export function stageParamChange(newParams: DesignParams) {
  const panel = get(paramPanelState);
  const currentParams = { ...panel.params, ...newParams };
  paramPanelState.setParams(currentParams);
  workingCopy.patch({ params: currentParams });
  session.setStatus('Parameters staged. Apply to rerender.');
}

export function projectManualCodeDraftResult(
  result: ManualCodeApplyResult,
  editedCode: string,
  beforeSource = get(workingCopy).macroCode,
) {
  if (result.status === 'error' || !result.artifactBundle || !result.modelManifest || !result.snapshotId) {
    throw result.error ?? new Error('Manual code preview returned no renderable runtime.');
  }

  if (get(activeThreadId) === result.threadId) {
    hydrateActiveRenderSnapshot({
      snapshotId: result.snapshotId,
      threadId: result.threadId,
      messageId: result.baseMessageId,
      design: result.designOutput,
      artifactBundle: result.artifactBundle,
      modelManifest: result.modelManifest,
      selectedPartId: null,
      stlUrl: toAssetUrl(result.artifactBundle.modelStlPath),
      status: 'Code applied; watcher will append its version.',
    });
    recordSessionActivityEvent({
      threadId: result.threadId,
      versionId: result.baseMessageId,
      kind: 'macro_patch_applied',
      title: 'Code draft applied',
      summary: result.parserMatched
        ? 'Code draft applied. Controls resynced from macro.'
        : 'Code draft applied.',
      severity: 'success',
      diffs: [
        {
          kind: 'text',
          label: 'Macro source',
          path: 'macro',
          before: beforeSource,
          after: editedCode,
        },
      ],
    });
    recordRenderEvent({
      threadId: result.threadId,
      versionId: result.baseMessageId,
      kind: 'render_succeeded',
      title: 'Code draft render succeeded',
      summary: 'Edited macro draft rendered.',
      severity: 'success',
      raw: { modelId: result.artifactBundle.modelId, modelStlPath: result.artifactBundle.modelStlPath },
    });
  }

  return {
    design: result.designOutput,
    artifactBundle: result.artifactBundle,
    modelManifest: result.modelManifest,
    parserMatched: result.parserMatched,
  };
}

export async function applyManualCodeDraft(editedCode: string) {
  const wc = get(workingCopy);
  return commitManualVersion({
    code: editedCode,
    title: wc.title || null,
    versionName: manualVersionLabel(wc.versionName),
  });
}

export async function commitManualVersion(
  editedCodeOrInput: string | ManualVersionCommitInput,
  titleOverride: string | null = null,
  options: ManualCommitOptions = {},
) {
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  const editedCode = typeof editedCodeOrInput === 'string' ? editedCodeOrInput : editedCodeOrInput.code;
  const inputTitle =
    typeof editedCodeOrInput === 'string' ? titleOverride : editedCodeOrInput.title ?? titleOverride;
  const inputVersionName =
    typeof editedCodeOrInput === 'string' ? options.versionName : editedCodeOrInput.versionName ?? options.versionName;
  const previousThreadId = get(activeThreadId);
  const committedTitle = inputTitle?.trim() || wc.title?.trim() || null;
  const committedVersionName = manualVersionLabel(inputVersionName || wc.versionName);

  if (!previousThreadId) {
    session.setStatus('Creating a thread for the manual version...');
    session.setError(null);
    try {
      const created = await createDesignThreadIntent({
        mode: 'macro',
        title: committedTitle || 'Untitled model',
        source: editedCode,
      });
      await activateWorkspaceProjection(created.workspace);
      closeWindowStore('code');
      if (created.initialVersionError) {
        throw created.initialVersionError;
      }
      session.setStatus(options.successStatus || 'Manual version created in a new thread.');
      return;
    } catch (error) {
      const rawError = formatBackendError(error);
      session.setError(`Manual Apply Failed: ${rawError}`);
      throw error;
    }
  }

  const snapshotThreadId = previousThreadId;

  session.setStatus("Applying manual edit as a new version...");
  session.setError(null);
  try {
    const currentConfig = get(config);
    startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);
    recordRenderEvent({
      threadId: snapshotThreadId,
      versionId: panel.versionId || wc.sourceVersionId || get(activeVersionId),
      kind: 'render_started',
      title: 'Manual version render started',
      summary: 'Rendering appended manual edit.',
      severity: 'info',
    });
    const result = await applyManualCode({
      threadId: snapshotThreadId,
      baseMessageId: panel.versionId || wc.sourceVersionId || get(activeVersionId),
      source: editedCode,
      persist: true,
      title: committedTitle,
      versionName: committedVersionName,
      sourceLanguage: wc.sourceLanguage || null,
      geometryBackend: wc.geometryBackend || null,
      parameters: panel.params,
      uiSpec: panel.uiSpec,
      postProcessing: wc.postProcessing ?? null,
    });
    if (!result.messageId) throw new Error('Manual code commit returned no version id.');
    if (result.status === 'error' || !result.artifactBundle || !result.modelManifest || !result.snapshotId) {
      rememberCommittedVersionMessage(snapshotThreadId, result.designOutput.title, {
        id: result.messageId,
        role: 'assistant',
        content: result.designOutput.response,
        status: 'error',
        output: result.designOutput,
        usage: null,
        artifactBundle: null,
        modelManifest: null,
        agentOrigin: null,
        imageData: null,
        visualKind: null,
        attachmentImages: [],
        timestamp: Date.now() / 1000,
      });
      throw result.error ?? new Error('Manual code commit failed.');
    }
    const newMsgId = result.messageId;
    const committedDesign = result.designOutput;
    rememberCommittedVersionMessage(snapshotThreadId, committedTitle || committedDesign.title, {
      id: newMsgId,
      role: 'assistant',
      content: committedDesign.response,
      status: 'success',
      output: committedDesign,
      usage: null,
      artifactBundle: result.artifactBundle,
      modelManifest: result.modelManifest,
      agentOrigin: null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
      timestamp: Date.now() / 1000,
    });

    if (get(activeThreadId) === snapshotThreadId) {
      const previousWorkingCopy = get(workingCopy);
      hydrateActiveRenderSnapshot({
        snapshotId: result.snapshotId,
        threadId: snapshotThreadId,
        messageId: newMsgId,
        design: committedDesign,
        artifactBundle: result.artifactBundle,
        modelManifest: result.modelManifest,
        selectedPartId: null,
        stlUrl: toAssetUrl(result.artifactBundle.modelStlPath),
        status: options.successStatus || 'Manual version appended.',
      });
      restoreWorkingCopyMacroDraftIfNeeded(previousWorkingCopy, committedDesign.macroCode);
      closeWindowStore('code');
      recordRenderEvent({
        threadId: snapshotThreadId,
        versionId: newMsgId,
        kind: 'render_succeeded',
        title: 'Manual version render succeeded',
        summary: 'Appended manual edit rendered.',
        severity: 'success',
        raw: { modelId: result.artifactBundle.modelId, modelStlPath: result.artifactBundle.modelStlPath },
      });
      recordSessionActivityEvent({
        threadId: snapshotThreadId,
        versionId: newMsgId,
        kind: 'macro_patch_applied',
        title: 'Manual source applied',
        summary: 'Manual source applied and committed.',
        severity: 'success',
        diffs: [
          {
            kind: 'text',
            label: 'Macro source',
            path: 'macro',
            before: wc.macroCode,
            after: editedCode,
          },
        ],
      });
      recordSessionActivityEvent({
        threadId: snapshotThreadId,
        versionId: newMsgId,
        kind: 'version_committed',
        title: 'Manual version appended',
        summary: committedDesign.response,
        severity: 'success',
        diffs: [
          {
            kind: 'text',
            label: 'Macro source',
            path: 'macro',
            before: wc.macroCode,
            after: editedCode,
          },
        ],
      });
      session.setStatus(
        options.successStatus ||
          (result.parserMatched
            ? "Manual version appended. Controls resynced from macro."
            : "Manual version appended."),
      );
    }
    stopMicrowaveHum('__manual__');

  } catch (e) {
    const rawError = formatBackendError(e);
    console.error('[ManualController] appendManualVersion error:', rawError, e);
    recordRenderEvent({
      threadId: snapshotThreadId,
      versionId: panel.versionId || wc.sourceVersionId || get(activeVersionId),
      kind: 'render_failed',
      title: 'Manual version render failed',
      summary: rawError,
      severity: 'error',
      raw: e,
    });
    session.setError(`Manual Apply Failed: ${rawError}`);
    stopMicrowaveHum('__manual__');
    throw e;
  }
}

export async function forkManualVersion(
  editedCodeOrInput: string | ManualVersionCommitInput,
  titleOverride: string | null = null,
) {
  const wc = get(workingCopy);
  const label =
    typeof editedCodeOrInput === 'string'
      ? titleOverride || wc.title || 'Untitled model'
      : editedCodeOrInput.title || titleOverride || wc.title || 'Untitled model';
  const confirmed = await confirmAction(`Fork "${label}" into a new thread with this code?`);
  if (!confirmed) return;

  const editedCode =
    typeof editedCodeOrInput === 'string' ? editedCodeOrInput : editedCodeOrInput.code;
  const baseThreadId = get(activeThreadId);
  const panel = get(paramPanelState);
  const baseMessageId = panel.versionId || wc.sourceVersionId || get(activeVersionId);
  session.setStatus('Forking edited code into a new thread...');
  session.setError(null);
  try {
    const created = await createDesignThreadIntent({
      mode: 'macro',
      title: label,
      source: editedCode,
      ...(baseThreadId && baseMessageId ? { baseThreadId, baseMessageId } : {}),
    });
    await activateWorkspaceProjection(created.workspace);
    closeWindowStore('code');
    if (created.initialVersionError) {
      throw created.initialVersionError;
    }
    session.setStatus('Forked into a new thread.');
  } catch (error) {
    const rawError = formatBackendError(error);
    session.setError(`Manual Fork Failed: ${rawError}`);
    throw error;
  }
}
