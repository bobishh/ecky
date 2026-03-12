import { get } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { workingCopy } from '../stores/workingCopy';
import { activeThreadId, activeVersionId, config } from '../stores/domainState';
import { refreshHistory } from '../stores/history';
import { showCodeModal } from '../stores/viewState';
import { session, setManualRenderActive } from '../stores/sessionStore';
import { startMicrowaveHum, stopMicrowaveHum, ensureContext } from '../audio/microwave';
import { paramPanelState } from '../stores/paramPanelState';
import { persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import { buildImportedSyntheticDesign } from '../modelRuntime/importedRuntime';
import { ensureSemanticManifest } from '../modelRuntime/semanticControls';
import type { DesignOutput, DesignParams, ParamValue, UiField, UiSpec } from '../types/domain';
import {
  addManualVersion,
  applyImportedModel,
  formatBackendError,
  getModelManifest,
  parseMacroParams,
  renderModel,
  saveModelManifest,
  updateParameters,
  updateVersionRuntime,
} from '../tauri/client';

let latestParamRenderSeq = 0;

type ManualCommitOptions = {
  targetThreadId?: string | null;
  activateTargetOnSuccess?: boolean;
  successStatus?: string;
};

function toAssetUrl(path: string | null | undefined): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

function fallbackParamValue(field: UiField): ParamValue {
  switch (field.type) {
    case 'checkbox':
      return false;
    case 'select':
      return field.options[0]?.value ?? '';
    case 'range':
    case 'number':
      return typeof field.min === 'number' ? field.min : 0;
  }
}

function mergeFieldWithExisting(parsedField: UiField, existingField: UiField | undefined): UiField {
  if (!existingField || existingField.type !== parsedField.type) {
    return parsedField;
  }

  switch (parsedField.type) {
    case 'checkbox':
      {
        const existing = existingField;
        return {
          ...parsedField,
          label: existing.label || parsedField.label,
          frozen: existing.frozen ?? parsedField.frozen,
        };
      }
    case 'select':
      {
        const existing = existingField as Extract<UiField, { type: 'select' }>;
        return {
          ...parsedField,
          label: existing.label || parsedField.label,
          frozen: existing.frozen ?? parsedField.frozen,
          options:
            existing.options?.length > 0 ? existing.options : parsedField.options,
        };
      }
    case 'range':
    case 'number':
      {
        const existing = existingField as Extract<UiField, { type: 'range' | 'number' }>;
        return {
          ...parsedField,
          label: existing.label || parsedField.label,
          frozen: existing.frozen ?? parsedField.frozen,
          min: existing.min ?? parsedField.min,
          max: existing.max ?? parsedField.max,
          step: existing.step ?? parsedField.step,
          minFrom: existing.minFrom ?? parsedField.minFrom,
          maxFrom: existing.maxFrom ?? parsedField.maxFrom,
        };
      }
  }
}

function coerceParamValue(field: UiField, currentValue: ParamValue | undefined, parsedValue: ParamValue | undefined): ParamValue {
  const candidate = currentValue ?? parsedValue;

  switch (field.type) {
    case 'checkbox':
      if (typeof candidate === 'boolean') return candidate;
      return typeof parsedValue === 'boolean' ? parsedValue : false;
    case 'select': {
      const optionValues = new Set((field.options || []).map((option) => option.value));
      if (typeof candidate === 'string' && optionValues.has(candidate)) return candidate;
      if (typeof parsedValue === 'string' && optionValues.has(parsedValue)) return parsedValue;
      return field.options[0]?.value ?? '';
    }
    case 'range':
    case 'number':
      if (typeof candidate === 'number' && Number.isFinite(candidate)) return candidate;
      if (typeof parsedValue === 'number' && Number.isFinite(parsedValue)) return parsedValue;
      return fallbackParamValue(field);
  }
}

async function reconcileManualControls(
  editedCode: string,
  currentUiSpec: UiSpec,
  currentParams: DesignParams,
): Promise<{ uiSpec: UiSpec; params: DesignParams; parserMatched: boolean }> {
  try {
    const parsed = await parseMacroParams(editedCode);
    if (!parsed.fields.length) {
      return { uiSpec: currentUiSpec, params: currentParams, parserMatched: false };
    }

    const existingByKey = new Map(currentUiSpec.fields.map((field) => [field.key, field]));
    const nextFields = parsed.fields.map((field) =>
      mergeFieldWithExisting(field, existingByKey.get(field.key)),
    );
    const nextParams: DesignParams = {};
    for (const field of nextFields) {
      nextParams[field.key] = coerceParamValue(field, currentParams[field.key], parsed.params[field.key]);
    }

    return {
      uiSpec: { fields: nextFields },
      params: nextParams,
      parserMatched: true,
    };
  } catch (error) {
    console.warn('[ManualController] Failed to reconcile controls from edited macro:', error);
    return { uiSpec: currentUiSpec, params: currentParams, parserMatched: false };
  }
}

export async function handleParamChange(
  newParams: DesignParams,
  forcedCode: string | null = null,
  persist: boolean = true
) {
  console.log('[ManualController] handleParamChange start', { newParams, persist });
  session.setError(null);
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  const snapshotThreadId = get(activeThreadId);
  const targetVersionId = panel.versionId || wc.sourceVersionId || get(activeVersionId);
  const currentParams = { ...panel.params, ...newParams };
  const renderSeq = ++latestParamRenderSeq;
  
  // 1. Update workingCopy immediately for UI responsiveness
  paramPanelState.setParams(currentParams);
  workingCopy.patch({ params: currentParams });

  const codeToUse = forcedCode || panel.macroCode || wc.macroCode;
  if (!codeToUse) {
    const currentSession = get(session);
    const importedDesign = buildImportedSyntheticDesign(
      currentSession.modelManifest,
      currentParams,
      panel.uiSpec,
    );

    if (importedDesign) {
      const sourceBundle = currentSession.artifactBundle;
      const sourceManifest = currentSession.modelManifest;
      paramPanelState.setParams(importedDesign.initialParams);
      paramPanelState.setUiSpec(importedDesign.uiSpec);
      workingCopy.patch({
        title: importedDesign.title,
        versionName: importedDesign.versionName,
        uiSpec: importedDesign.uiSpec,
        params: importedDesign.initialParams,
      });

      if (!sourceBundle || !sourceManifest) {
        if (get(activeThreadId) === snapshotThreadId) {
          await persistLastSessionSnapshot({
            design: importedDesign,
            artifactBundle: currentSession.artifactBundle,
            modelManifest: currentSession.modelManifest,
          });
        }
        session.setStatus('Imported model controls updated.');
        return;
      }

      try {
        setManualRenderActive(true);
        const currentConfig = get(config);
        startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);
        session.setStatus('Applying imported FCStd bindings...');

        const nextBundle = await applyImportedModel(
          sourceBundle,
          sourceManifest,
          importedDesign.initialParams,
          persist ? targetVersionId : null,
        );
        const rawNextManifest = await getModelManifest(nextBundle.modelId);
        const nextManifest =
          ensureSemanticManifest(
            rawNextManifest,
            importedDesign.uiSpec,
            importedDesign.initialParams,
            sourceManifest,
          ) ?? rawNextManifest;
        if (JSON.stringify(nextManifest) !== JSON.stringify(rawNextManifest)) {
          await saveModelManifest(nextBundle.modelId, nextManifest, persist ? targetVersionId : null);
        }

        if (renderSeq !== latestParamRenderSeq) {
          return;
        }

        if (get(activeThreadId) === snapshotThreadId) {
          session.setStlUrl(toAssetUrl(nextBundle.previewStlPath));
          session.setModelRuntime(nextBundle, nextManifest);
          await persistLastSessionSnapshot({
            design: importedDesign,
            artifactBundle: nextBundle,
            modelManifest: nextManifest,
          });
          session.setStatus('Imported model updated.');
        }

        if (persist && targetVersionId && get(activeThreadId) === snapshotThreadId) {
          await refreshHistory();
        }
      } catch (e) {
        console.error(
          '[ManualController] apply_imported_model error:',
          formatBackendError(e),
          e,
        );
        if (get(activeThreadId) === snapshotThreadId) {
          session.setError(`Imported Apply Failed: ${formatBackendError(e)}`);
        }
      } finally {
        if (renderSeq === latestParamRenderSeq) {
          stopMicrowaveHum('__manual__');
          setManualRenderActive(false);
        }
      }
      return;
    }

    console.warn('[ManualController] No macroCode to execute');
    if (get(activeThreadId) === snapshotThreadId) {
      session.setError('Apply Failed: no macro or imported model is available for this version.');
    }
    return;
  }

  ensureContext();

  session.setStatus('Executing FreeCAD engine...');
  try {
    setManualRenderActive(true);
    const currentConfig = get(config);
    startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);

    console.log('[ManualController] Invoking render_model with', { parameters: currentParams });
    const bundle = await renderModel(codeToUse, currentParams);
    const rawManifest = await getModelManifest(bundle.modelId);
    const previousManifest = get(session).modelManifest;
    const manifest =
      ensureSemanticManifest(rawManifest, panel.uiSpec, currentParams, previousManifest) ??
      rawManifest;
    if (JSON.stringify(manifest) !== JSON.stringify(rawManifest)) {
      await saveModelManifest(bundle.modelId, manifest, persist ? targetVersionId : null);
    }

    if (renderSeq !== latestParamRenderSeq) {
      return;
    }

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(toAssetUrl(bundle.previewStlPath));
      session.setModelRuntime(bundle, manifest);
    }

    if (get(activeThreadId) === snapshotThreadId) {
      await persistLastSessionSnapshot({
        artifactBundle: bundle,
        modelManifest: manifest,
      });
    }

    const sourceVersionId = targetVersionId;
    if (persist && sourceVersionId) {
      console.log('[ManualController] Persisting parameters to messageId:', sourceVersionId);
      try {
        await updateVersionRuntime(sourceVersionId, bundle, manifest);
      } catch (e) {
        console.error('[ManualController] Failed to update version runtime:', formatBackendError(e), e);
      }
      try {
        await updateParameters(sourceVersionId, currentParams);
        console.log('[ManualController] update_parameters success');
        if (renderSeq === latestParamRenderSeq && get(activeThreadId) === snapshotThreadId) {
          await refreshHistory();
        }
      } catch (e) {
        console.error('[ManualController] Failed to persist parameters:', formatBackendError(e), e);
      }
    }
  } catch (e) {
    console.error('[ManualController] render_model error:', formatBackendError(e), e);
    if (renderSeq === latestParamRenderSeq && get(activeThreadId) === snapshotThreadId) {
      session.setError(`Render Error: ${formatBackendError(e)}`);
    }
  } finally {
    if (renderSeq === latestParamRenderSeq) {
      stopMicrowaveHum('__manual__');
      setManualRenderActive(false);
    }
  }
}

export function stageParamChange(newParams: DesignParams) {
  const panel = get(paramPanelState);
  const currentParams = { ...panel.params, ...newParams };
  paramPanelState.setParams(currentParams);
  workingCopy.patch({ params: currentParams });
  session.setStatus('Parameters staged. Apply to rerender.');
}

export async function commitManualVersion(
  editedCode: string,
  titleOverride: string | null = null,
  options: ManualCommitOptions = {},
) {
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  const previousThreadId = get(activeThreadId);
  const snapshotThreadId = options.targetThreadId || previousThreadId || crypto.randomUUID();
  const activateTargetOnSuccess =
    options.activateTargetOnSuccess ??
    (!previousThreadId || snapshotThreadId !== previousThreadId);

  session.setStatus("Validating and committing manual edit...");
  session.setError(null);
  try {
    setManualRenderActive(true);
    const currentConfig = get(config);
    startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);
    const reconciled = await reconcileManualControls(editedCode, panel.uiSpec, panel.params);
    const nextUiSpec = reconciled.uiSpec;
    const nextParams = reconciled.params;

    const bundle = await renderModel(editedCode, nextParams);
    const rawManifest = await getModelManifest(bundle.modelId);
    const previousManifest = get(session).modelManifest;
    const manifest =
      ensureSemanticManifest(rawManifest, nextUiSpec, nextParams, previousManifest) ??
      rawManifest;

    const newMsgId = await addManualVersion({
      threadId: snapshotThreadId,
      title: titleOverride || wc.title || "Manual Edit",
      versionName: "V-manual",
      macroCode: editedCode,
      parameters: nextParams,
      uiSpec: nextUiSpec,
      artifactBundle: bundle,
      modelManifest: manifest,
    });
    if (JSON.stringify(manifest) !== JSON.stringify(rawManifest)) {
      await saveModelManifest(bundle.modelId, manifest, newMsgId);
    }

    const committedDesign: DesignOutput = {
      title: titleOverride || wc.title || "Manual Edit",
      versionName: "V-manual",
      response: "Manual edit committed as new version.",
      interactionMode: "design",
      macroCode: editedCode,
      uiSpec: nextUiSpec,
      initialParams: nextParams
    };

    if (activateTargetOnSuccess) {
      activeThreadId.set(snapshotThreadId);
      activeVersionId.set(null);
    }

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(toAssetUrl(bundle.previewStlPath));
      session.setModelRuntime(bundle, manifest);
      workingCopy.loadVersion(committedDesign, newMsgId);
      paramPanelState.hydrateFromVersion(committedDesign, newMsgId);
      activeVersionId.set(newMsgId);
      showCodeModal.set(false);
      session.setStatus(
        options.successStatus ||
          (reconciled.parserMatched
            ? "Manual version committed. Controls resynced from macro."
            : "Manual version committed."),
      );
    }
    await refreshHistory();

    if (get(activeThreadId) === snapshotThreadId) {
      await persistLastSessionSnapshot({
        design: committedDesign,
        threadId: snapshotThreadId,
        messageId: newMsgId,
        artifactBundle: bundle,
        modelManifest: manifest,
        selectedPartId: null,
      });
    }
    
    stopMicrowaveHum('__manual__');
    setManualRenderActive(false);
  } catch (e) {
    console.error('[ManualController] commitManualVersion error:', formatBackendError(e), e);
    session.setError(`Manual Commit Failed: ${formatBackendError(e)}`);
    stopMicrowaveHum('__manual__');
    setManualRenderActive(false);
    throw e;
  }
}

export async function forkManualVersion(
  editedCode: string,
  titleOverride: string | null = null,
) {
  const wc = get(workingCopy);
  const label = titleOverride || wc.title || 'Manual Edit';
  const confirmed =
    typeof window === 'undefined'
      ? true
      : window.confirm(`Fork "${label}" into a new thread with this code?`);
  if (!confirmed) return;

  await commitManualVersion(editedCode, titleOverride, {
    targetThreadId: crypto.randomUUID(),
    activateTargetOnSuccess: true,
    successStatus: 'Forked into a new thread.',
  });
}
