import type { DesignOutput } from './types/domain';
import type { WorkingCopyState } from './stores/workingCopy';

type ManualDraftSeedInput = Pick<
  WorkingCopyState,
  | 'title'
  | 'versionName'
  | 'macroDialect'
  | 'engineKind'
  | 'sourceLanguage'
  | 'geometryBackend'
  | 'uiSpec'
  | 'params'
  | 'postProcessing'
>;

export function buildFailedDraftSeed(
  failedDesign: DesignOutput,
  workingDraft: ManualDraftSeedInput,
): DesignOutput {
  return {
    ...failedDesign,
    title: failedDesign.title || workingDraft.title || 'Manual Edit',
    versionName: failedDesign.versionName || workingDraft.versionName || 'Draft',
    macroDialect: failedDesign.macroDialect ?? workingDraft.macroDialect,
    engineKind: failedDesign.engineKind ?? workingDraft.engineKind,
    sourceLanguage: failedDesign.sourceLanguage ?? workingDraft.sourceLanguage,
    geometryBackend: failedDesign.geometryBackend ?? workingDraft.geometryBackend,
    uiSpec: failedDesign.uiSpec ?? workingDraft.uiSpec,
    initialParams: failedDesign.initialParams ?? workingDraft.params,
    postProcessing: failedDesign.postProcessing ?? workingDraft.postProcessing ?? null,
  };
}
