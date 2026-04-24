import { buildImportedParams, buildImportedPreviewTransforms, buildImportedUiSpec } from '../modelRuntime/importedRuntime';
import {
  buildContextSelectionTargets,
  deriveSelectedPartId,
  pickContextAdvisories,
  pickContextControls,
  resolveActiveContextViewId,
  resolveContextSelectionTarget,
  resolveMeasurementCallout,
} from '../modelRuntime/contextualEditing';
import {
  ensureSemanticManifest,
  materializeControlViews,
  type MaterializedSemanticControl,
  type MaterializedSemanticView,
} from '../modelRuntime/semanticControls';
import type {
  ArtifactBundle,
  DesignParams,
  ModelManifest,
  UiSpec,
  Advisory,
} from '../types/domain';
import type {
  ContextSelectionTarget,
  MeasurementControlFocus,
  ResolvedMeasurementCallout,
} from '../modelRuntime/contextualEditing';
import type { ImportedPreviewTransform } from '../modelRuntime/importedRuntime';

export type ContextStateInput = {
  activeArtifactBundle?: ArtifactBundle | null;
  activeControlViewId: string | null;
  focusedMeasurementControl: MeasurementControlFocus | null;
  paramUiSpec: UiSpec | null;
  paramValues: DesignParams;
  selectedContextTargetId: string | null;
  selectedPartId: string | null;
  sessionModelManifest: ModelManifest | null;
};

export type ContextState = {
  effectiveUiSpec: UiSpec;
  effectiveParameters: DesignParams;
  activeModelManifest: ModelManifest | null;
  contextSelectionTargets: ContextSelectionTarget[];
  selectedTarget: ContextSelectionTarget | null;
  selectedPartId: string | null;
  importedPreviewTransforms: Record<string, ImportedPreviewTransform>;
  overlaySelectedPart: NonNullable<ModelManifest['parts']>[number] | null;
  overlayPreviewOnly: boolean;
  availableControlViews: MaterializedSemanticView[];
  resolvedActiveControlViewId: string | null;
  activeControlView: MaterializedSemanticView | null;
  overlayControls: MaterializedSemanticControl[];
  overlayAdvisories: Advisory[];
  activeMeasurementCallout: ResolvedMeasurementCallout | null;
};

export function deriveContextState(input: ContextStateInput): ContextState {
  const importedUiSpec = (buildImportedUiSpec(input.sessionModelManifest) ?? { fields: [] }) as UiSpec;
  const effectiveUiSpec: UiSpec =
    (input.paramUiSpec?.fields || []).length > 0
      ? (input.paramUiSpec as UiSpec)
      : importedUiSpec;
  const effectiveParameters = buildImportedParams(
    input.sessionModelManifest,
    input.paramValues || {},
    effectiveUiSpec,
  );
  const activeModelManifest = ensureSemanticManifest(
    input.sessionModelManifest,
    effectiveUiSpec,
    effectiveParameters,
  );
  const contextSelectionTargets = buildContextSelectionTargets(activeModelManifest);
  const selectedTarget = resolveContextSelectionTarget(
    activeModelManifest,
    contextSelectionTargets,
    input.selectedContextTargetId,
    input.selectedPartId,
  );
  const resolvedSelectedPartId = deriveSelectedPartId(selectedTarget);
  const importedPreviewTransforms = buildImportedPreviewTransforms(
    activeModelManifest,
    effectiveParameters,
  );
  const overlaySelectedPart =
    resolvedSelectedPartId && activeModelManifest?.parts?.length
      ? activeModelManifest.parts.find((part) => part.partId === resolvedSelectedPartId) ?? null
      : null;
  const overlayPreviewOnly =
    !!(
      activeModelManifest?.sourceKind === 'importedFcstd' &&
      overlaySelectedPart?.editable &&
      resolvedSelectedPartId &&
      importedPreviewTransforms[resolvedSelectedPartId] &&
      (
        Math.abs(importedPreviewTransforms[resolvedSelectedPartId].scale.x - 1) > 0.001 ||
        Math.abs(importedPreviewTransforms[resolvedSelectedPartId].scale.y - 1) > 0.001 ||
        Math.abs(importedPreviewTransforms[resolvedSelectedPartId].scale.z - 1) > 0.001
      )
    );
  const availableControlViews = materializeControlViews(
    activeModelManifest,
    effectiveUiSpec,
    effectiveParameters,
  );
  const resolvedActiveControlViewId = resolveActiveContextViewId(
    availableControlViews,
    selectedTarget,
    input.activeControlViewId,
  );
  const activeControlView =
    availableControlViews.find((view) => view.viewId === resolvedActiveControlViewId) ??
    availableControlViews[0] ??
    null;
  const overlayControls = pickContextControls(activeControlView, selectedTarget);
  const overlayAdvisories = pickContextAdvisories(activeControlView, selectedTarget);
  const activeMeasurementCallout = resolveMeasurementCallout(
    activeModelManifest,
    input.activeArtifactBundle ?? null,
    contextSelectionTargets,
    input.focusedMeasurementControl,
    selectedTarget,
  );

  return {
    effectiveUiSpec,
    effectiveParameters,
    activeModelManifest,
    contextSelectionTargets,
    selectedTarget,
    selectedPartId: resolvedSelectedPartId,
    importedPreviewTransforms,
    overlaySelectedPart,
    overlayPreviewOnly,
    availableControlViews,
    resolvedActiveControlViewId,
    activeControlView,
    overlayControls,
    overlayAdvisories,
    activeMeasurementCallout,
  };
}
