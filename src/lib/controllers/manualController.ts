import { get } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { workingCopy } from '../stores/workingCopy';
import { activeThreadIdStore as activeThreadId, activeVersionId, config } from '../stores/domainState';
import { refreshHistory, rememberCommittedVersionMessage } from '../stores/history';
import { showCodeModal } from '../stores/viewState';
import { session, setManualRenderActive } from '../stores/sessionStore';
import { startMicrowaveHum, stopMicrowaveHum, ensureContext } from '../audio/microwave';
import { paramPanelState } from '../stores/paramPanelState';
import { persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import { buildImportedSyntheticDesign } from '../modelRuntime/importedRuntime';
import { getRenderableRuntimeBundle, inspectRuntimeBundle } from '../modelRuntime/runtimeBundle';
import { ensureSemanticManifest } from '../modelRuntime/semanticControls';
import { confirmAction } from '../ui/confirmAction';
import type {
  ArtifactBundle,
  DesignOutput,
  DesignParams,
  MacroDialect,
  ParamValue,
  PostProcessingSpec,
  UiField,
  UiSpec,
} from '../types/domain';
import {
  addManualVersion,
  applyImportedModel,
  formatBackendError,
  getModelManifest,
  parseMacroParams,
  renderModel,
  saveModelManifest,
} from '../tauri/client';

let latestParamRenderSeq = 0;

type ManualCommitOptions = {
  targetThreadId?: string | null;
  activateTargetOnSuccess?: boolean;
  successStatus?: string;
  versionName?: string | null;
};

export type ManualVersionCommitInput = {
  code: string;
  title?: string | null;
  versionName?: string | null;
};

function toAssetUrl(path: string | null | undefined): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

function workingCopyBackendLabel(design: Pick<DesignOutput, 'geometryBackend' | 'sourceLanguage' | 'macroDialect'>): string {
  if (design.geometryBackend === 'build123d' || design.sourceLanguage === 'build123d') return 'build123d';
  if (design.macroDialect === 'ecky' || design.sourceLanguage === 'ecky') {
    return 'Ecky';
  }
  return 'FreeCAD';
}

function fallbackParamValue(field: UiField): ParamValue {
  switch (field.type) {
    case 'checkbox':
      return false;
    case 'select':
      return field.options[0]?.value ?? '';
    case 'image':
      return '';
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
    case 'image':
      {
        const existing = existingField as Extract<UiField, { type: 'image' }>;
        return {
          ...parsedField,
          label: existing.label || parsedField.label,
          frozen: existing.frozen ?? parsedField.frozen,
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
    case 'image':
      if (typeof candidate === 'string') return candidate;
      return typeof parsedValue === 'string' ? parsedValue : '';
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
  persist: boolean = false
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
        setManualRenderActive(true, {
          threadId: snapshotThreadId,
          messageId: targetVersionId,
        });
        const currentConfig = get(config);
        startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);
        session.setStatus('Applying imported FCStd bindings...');

        const nextBundle = await applyImportedModel(
          sourceBundle,
          sourceManifest,
          importedDesign.initialParams,
          null,
        );
        const rawNextManifest = await getModelManifest(nextBundle.modelId);
        const nextManifest =
          ensureSemanticManifest(
            rawNextManifest,
            importedDesign.uiSpec,
            importedDesign.initialParams,
            sourceManifest,
          ) ?? rawNextManifest;
        const manifestChanged = JSON.stringify(nextManifest) !== JSON.stringify(rawNextManifest);
        if (manifestChanged) {
          await saveModelManifest(nextBundle.modelId, nextManifest, null);
        }

        if (renderSeq !== latestParamRenderSeq) {
          return;
        }

        if (get(activeThreadId) === snapshotThreadId) {
          session.setStlUrl(toAssetUrl(nextBundle.previewStlPath));
          session.setModelRuntime(nextBundle, nextManifest);
        }

        if (persist && snapshotThreadId && get(activeThreadId) === snapshotThreadId) {
          const committedTitle =
            importedDesign.title ||
            sourceManifest.document.documentLabel ||
            sourceManifest.document.documentName ||
            'Imported FreeCAD Model';
          const committedVersionName = importedDesign.versionName || 'Imported';
          const newMsgId = await addManualVersion({
            threadId: snapshotThreadId,
            title: committedTitle,
            versionName: committedVersionName,
            macroCode: importedDesign.macroCode,
            sourceLanguage: importedDesign.sourceLanguage,
            geometryBackend: importedDesign.geometryBackend,
            parameters: importedDesign.initialParams,
            uiSpec: importedDesign.uiSpec,
            postProcessing: importedDesign.postProcessing ?? null,
            artifactBundle: nextBundle,
            modelManifest: nextManifest,
          });
          if (manifestChanged) {
            await saveModelManifest(nextBundle.modelId, nextManifest, newMsgId);
          }
          rememberCommittedVersionMessage(snapshotThreadId, committedTitle, {
            id: newMsgId,
            role: 'assistant',
            content: 'Imported model committed as new version.',
            status: 'success',
            output: importedDesign,
            usage: null,
            artifactBundle: nextBundle,
            modelManifest: nextManifest,
            agentOrigin: null,
            imageData: null,
            visualKind: null,
            attachmentImages: [],
            timestamp: Date.now() / 1000,
          });
          activeVersionId.set(newMsgId);
          workingCopy.loadVersion(importedDesign, newMsgId);
          paramPanelState.hydrateFromVersion(importedDesign, newMsgId);
          await persistLastSessionSnapshot({
            design: importedDesign,
            threadId: snapshotThreadId,
            messageId: newMsgId,
            artifactBundle: nextBundle,
            modelManifest: nextManifest,
            selectedPartId: null,
          });
          await refreshHistory();
          session.setStatus('Imported model committed as new version.');
        } else if (get(activeThreadId) === snapshotThreadId) {
          await persistLastSessionSnapshot({
            design: importedDesign,
            artifactBundle: nextBundle,
            modelManifest: nextManifest,
          });
          session.setStatus('Imported model updated. Commit version to save history.');
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

  session.setStatus(`Executing ${workingCopyBackendLabel(wc)} engine...`);
  try {
    setManualRenderActive(true, {
      threadId: snapshotThreadId,
      messageId: targetVersionId,
    });
    const currentConfig = get(config);
    startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);

    console.log('[ManualController] Invoking render_model with', { parameters: currentParams });
    const bundle = await renderModel(
      codeToUse,
      currentParams,
      wc.macroDialect ?? null,
      wc.geometryBackend ?? null,
      wc.postProcessing ?? null,
    );
    const runtime = await inspectRuntimeBundle(
      bundle,
      undefined,
      undefined,
      wc.postProcessing ?? null,
      currentParams,
    );
    const renderableBundle =
      runtime.bundle ??
      getRenderableRuntimeBundle(bundle, wc.postProcessing ?? null, currentParams) ??
      bundle;
    const rawManifest = await getModelManifest(bundle.modelId);
    const previousManifest = get(session).modelManifest;
    const manifest =
      ensureSemanticManifest(rawManifest, panel.uiSpec, currentParams, previousManifest) ??
      rawManifest;
    const manifestChanged = JSON.stringify(manifest) !== JSON.stringify(rawManifest);
    if (manifestChanged && !persist) {
      await saveModelManifest(bundle.modelId, manifest, null);
    }

    if (renderSeq !== latestParamRenderSeq) {
      return;
    }

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(toAssetUrl(renderableBundle.previewStlPath));
      session.setModelRuntime(renderableBundle, manifest);
      if (runtime.skippedOversizedPreview) {
        session.setStatus(
          'Rendered safely. Lithophane preview was skipped in the viewer; base part meshes are shown instead.',
        );
      }
    }

    if (get(activeThreadId) === snapshotThreadId) {
      await persistLastSessionSnapshot({
        artifactBundle: renderableBundle,
        modelManifest: manifest,
      });
    }

    if (persist && snapshotThreadId) {
      const committedTitle = wc.title || manifest.document?.documentLabel || manifest.document?.documentName || 'Parameter Apply';
      const committedVersionName = wc.versionName || 'Param Apply';
      const committedDesign = buildManualDesign({
        title: committedTitle,
        versionName: committedVersionName,
        response: 'Parameter version committed.',
        macroCode: codeToUse,
        bundle: renderableBundle,
        uiSpec: panel.uiSpec,
        params: currentParams,
        postProcessing: wc.postProcessing ?? null,
        workingMacroDialect: wc.macroDialect,
      });
      const newMsgId = await addManualVersion({
        threadId: snapshotThreadId,
        title: committedTitle,
        versionName: committedVersionName,
        macroCode: codeToUse,
        sourceLanguage: renderableBundle.sourceLanguage || wc.sourceLanguage || null,
        geometryBackend: renderableBundle.geometryBackend || wc.geometryBackend || null,
        parameters: currentParams,
        uiSpec: panel.uiSpec,
        postProcessing: wc.postProcessing ?? null,
        artifactBundle: renderableBundle,
        modelManifest: manifest,
      });
      if (manifestChanged) {
        await saveModelManifest(bundle.modelId, manifest, newMsgId);
      }

      rememberCommittedVersionMessage(snapshotThreadId, committedTitle, {
        id: newMsgId,
        role: 'assistant',
        content: committedDesign.response,
        status: 'success',
        output: committedDesign,
        usage: null,
        artifactBundle: renderableBundle,
        modelManifest: manifest,
        agentOrigin: null,
        imageData: null,
        visualKind: null,
        attachmentImages: [],
        timestamp: Date.now() / 1000,
      });

      if (renderSeq === latestParamRenderSeq && get(activeThreadId) === snapshotThreadId) {
        activeVersionId.set(newMsgId);
        workingCopy.loadVersion(committedDesign, newMsgId);
        paramPanelState.hydrateFromVersion(committedDesign, newMsgId);
        await persistLastSessionSnapshot({
          design: committedDesign,
          threadId: snapshotThreadId,
          messageId: newMsgId,
          artifactBundle: renderableBundle,
          modelManifest: manifest,
          selectedPartId: null,
        });
        await refreshHistory();
        session.setStatus(
          runtime.skippedOversizedPreview
            ? 'Parameter version committed. Lithophane preview was skipped in the viewer; base part meshes are shown instead.'
            : 'Parameter version committed.',
        );
      }
    } else if (renderSeq === latestParamRenderSeq && get(activeThreadId) === snapshotThreadId) {
      session.setStatus(
        runtime.skippedOversizedPreview
          ? 'Parameters applied. Commit version to save history. Lithophane preview was skipped in the viewer; base part meshes are shown instead.'
          : 'Parameters applied. Commit version to save history.',
      );
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

function buildManualDesign(input: {
  title: string;
  versionName: string;
  response: string;
  macroCode: string;
  bundle: ArtifactBundle;
  uiSpec: UiSpec;
  params: DesignParams;
  postProcessing: PostProcessingSpec | null;
  workingMacroDialect: MacroDialect;
}): DesignOutput {
  return {
    title: input.title,
    versionName: input.versionName,
    response: input.response,
    interactionMode: "design",
    macroCode: input.macroCode,
    macroDialect:
      input.bundle.sourceLanguage === 'build123d'
        ? 'build123d'
        : input.bundle.engineKind === 'ecky'
          ? 'ecky'
          : input.workingMacroDialect ?? 'legacy',
    sourceLanguage: input.bundle.sourceLanguage || (input.bundle.engineKind === 'ecky' ? 'ecky' : 'legacyPython'),
    geometryBackend: input.bundle.geometryBackend || (input.bundle.engineKind === 'ecky' ? 'mesh' : 'freecad'),
    engineKind: input.bundle.engineKind,
    uiSpec: input.uiSpec,
    initialParams: input.params,
    postProcessing: input.postProcessing ?? null,
  };
}

export async function applyManualCodeDraft(editedCode: string) {
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  const snapshotThreadId = get(activeThreadId);
  const targetVersionId = panel.versionId || wc.sourceVersionId || get(activeVersionId);

  session.setStatus('Applying code draft...');
  session.setError(null);
  try {
    setManualRenderActive(true, {
      threadId: snapshotThreadId,
      messageId: targetVersionId,
    });
    const currentConfig = get(config);
    startMicrowaveHum('__manual__', currentConfig, snapshotThreadId);

    const reconciled = await reconcileManualControls(editedCode, panel.uiSpec, panel.params);
    const nextUiSpec = reconciled.uiSpec;
    const nextParams = reconciled.params;
    const manualMacroDialect =
      wc.sourceLanguage === 'build123d' || wc.macroDialect === 'build123d' ? wc.macroDialect ?? 'build123d' : null;
    const manualGeometryBackend =
      wc.sourceLanguage === 'build123d' || wc.geometryBackend === 'build123d' ? 'build123d' : null;
    const bundle = await renderModel(
      editedCode,
      nextParams,
      manualMacroDialect,
      manualGeometryBackend,
      wc.postProcessing ?? null,
    );
    const runtime = await inspectRuntimeBundle(
      bundle,
      undefined,
      undefined,
      wc.postProcessing ?? null,
      nextParams,
    );
    const renderableBundle =
      runtime.bundle ??
      getRenderableRuntimeBundle(bundle, wc.postProcessing ?? null, nextParams) ??
      bundle;
    const rawManifest = await getModelManifest(bundle.modelId);
    const previousManifest = get(session).modelManifest;
    const manifest =
      ensureSemanticManifest(rawManifest, nextUiSpec, nextParams, previousManifest) ??
      rawManifest;
    if (JSON.stringify(manifest) !== JSON.stringify(rawManifest)) {
      await saveModelManifest(bundle.modelId, manifest, null);
    }

    const draftDesign = buildManualDesign({
      title: wc.title || 'Manual Edit',
      versionName: wc.versionName || 'Draft',
      response: 'Code draft applied.',
      macroCode: editedCode,
      bundle,
      uiSpec: nextUiSpec,
      params: nextParams,
      postProcessing: wc.postProcessing ?? null,
      workingMacroDialect: wc.macroDialect,
    });

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(toAssetUrl(renderableBundle.previewStlPath));
      session.setModelRuntime(renderableBundle, manifest);
      workingCopy.patch({
        macroCode: editedCode,
        macroDialect: draftDesign.macroDialect ?? wc.macroDialect,
        engineKind: draftDesign.engineKind ?? wc.engineKind,
        sourceLanguage: draftDesign.sourceLanguage,
        geometryBackend: draftDesign.geometryBackend,
        uiSpec: nextUiSpec,
        params: nextParams,
      });
      paramPanelState.hydrate({
        versionId: targetVersionId,
        macroCode: editedCode,
        uiSpec: nextUiSpec,
        params: nextParams,
      });
      await persistLastSessionSnapshot({
        design: draftDesign,
        threadId: snapshotThreadId,
        messageId: targetVersionId,
        artifactBundle: renderableBundle,
        modelManifest: manifest,
        selectedPartId: null,
      });
      session.setStatus(
        runtime.skippedOversizedPreview
          ? 'Code applied. Commit version to save history. Lithophane preview was skipped in the viewer; base part meshes are shown instead.'
          : 'Code applied. Commit version to save history.',
      );
    }

    return {
      design: draftDesign,
      artifactBundle: renderableBundle,
      modelManifest: manifest,
      parserMatched: reconciled.parserMatched,
    };
  } catch (e) {
    console.error('[ManualController] applyManualCodeDraft error:', formatBackendError(e), e);
    if (get(activeThreadId) === snapshotThreadId) {
      session.setError(`Apply Failed: ${formatBackendError(e)}`);
    }
    throw e;
  } finally {
    stopMicrowaveHum('__manual__');
    setManualRenderActive(false);
  }
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

    const manualMacroDialect =
      wc.sourceLanguage === 'build123d' || wc.macroDialect === 'build123d' ? wc.macroDialect ?? 'build123d' : null;
    const manualGeometryBackend =
      wc.sourceLanguage === 'build123d' || wc.geometryBackend === 'build123d' ? 'build123d' : null;
    const bundle = await renderModel(
      editedCode,
      nextParams,
      manualMacroDialect,
      manualGeometryBackend,
      wc.postProcessing ?? null,
    );
    const runtime = await inspectRuntimeBundle(
      bundle,
      undefined,
      undefined,
      wc.postProcessing ?? null,
      nextParams,
    );
    const renderableBundle =
      runtime.bundle ??
      getRenderableRuntimeBundle(bundle, wc.postProcessing ?? null, nextParams) ??
      bundle;
    const rawManifest = await getModelManifest(bundle.modelId);
    const previousManifest = get(session).modelManifest;
    const manifest =
      ensureSemanticManifest(rawManifest, nextUiSpec, nextParams, previousManifest) ??
      rawManifest;

    const committedTitle = inputTitle || wc.title || "Manual Edit";
    const committedVersionName = inputVersionName?.trim() || wc.versionName || "V-manual";
    const newMsgId = await addManualVersion({
      threadId: snapshotThreadId,
      title: committedTitle,
      versionName: committedVersionName,
      macroCode: editedCode,
      sourceLanguage: bundle.sourceLanguage || wc.sourceLanguage || null,
      geometryBackend: bundle.geometryBackend || wc.geometryBackend || null,
      parameters: nextParams,
      uiSpec: nextUiSpec,
      postProcessing: wc.postProcessing ?? null,
      artifactBundle: bundle,
      modelManifest: manifest,
    });
    if (JSON.stringify(manifest) !== JSON.stringify(rawManifest)) {
      await saveModelManifest(bundle.modelId, manifest, newMsgId);
    }

    const committedDesign: DesignOutput = {
      title: committedTitle,
      versionName: committedVersionName,
      response: "Manual edit committed as new version.",
      interactionMode: "design",
      macroCode: editedCode,
      macroDialect:
        bundle.sourceLanguage === 'build123d'
          ? 'build123d'
          : bundle.engineKind === 'ecky'
            ? 'ecky'
            : wc.macroDialect ?? 'legacy',
      sourceLanguage: bundle.sourceLanguage || (bundle.engineKind === 'ecky' ? 'ecky' : 'legacyPython'),
      geometryBackend: bundle.geometryBackend || (bundle.engineKind === 'ecky' ? 'mesh' : 'freecad'),
      engineKind: bundle.engineKind,
      uiSpec: nextUiSpec,
      initialParams: nextParams,
      postProcessing: wc.postProcessing ?? null,
    };
    rememberCommittedVersionMessage(snapshotThreadId, committedTitle, {
      id: newMsgId,
      role: 'assistant',
      content: committedDesign.response,
      status: 'success',
      output: committedDesign,
      usage: null,
      artifactBundle: renderableBundle,
      modelManifest: manifest,
      agentOrigin: null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
      timestamp: Date.now() / 1000,
    });

    if (activateTargetOnSuccess) {
      activeThreadId.set(snapshotThreadId);
      activeVersionId.set(null);
    }

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(toAssetUrl(renderableBundle.previewStlPath));
      session.setModelRuntime(renderableBundle, manifest);
      workingCopy.loadVersion(committedDesign, newMsgId);
      paramPanelState.hydrateFromVersion(committedDesign, newMsgId);
      activeVersionId.set(newMsgId);
      showCodeModal.set(false);
      session.setStatus(
        runtime.skippedOversizedPreview
          ? 'Manual version committed. Lithophane preview was skipped in the viewer; base part meshes are shown instead.'
          : options.successStatus ||
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
        artifactBundle: renderableBundle,
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
  editedCodeOrInput: string | ManualVersionCommitInput,
  titleOverride: string | null = null,
) {
  const wc = get(workingCopy);
  const label =
    typeof editedCodeOrInput === 'string'
      ? titleOverride || wc.title || 'Manual Edit'
      : editedCodeOrInput.title || titleOverride || wc.title || 'Manual Edit';
  const confirmed = await confirmAction(`Fork "${label}" into a new thread with this code?`);
  if (!confirmed) return;

  await commitManualVersion(editedCodeOrInput, titleOverride, {
    targetThreadId: crypto.randomUUID(),
    activateTargetOnSuccess: true,
    successStatus: 'Forked into a new thread.',
  });
}
