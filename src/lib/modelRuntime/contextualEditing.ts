import type {
  Advisory,
  ArtifactBundle,
  MeasurementAnnotation,
  MeasurementBasis,
  MeasurementGuideKind,
  DesignParams,
  ModelManifest,
  SelectionTarget,
  SelectionTargetKind,
} from '../types/domain';
import type {
  MaterializedSemanticControl,
  MaterializedSemanticSection,
  MaterializedSemanticView,
} from './semanticControls';

export type ContextSelectionKind = SelectionTargetKind | 'global';

export type ContextSelectionTarget = {
  targetId: string;
  kind: ContextSelectionKind;
  partId: string | null;
  label: string;
  editable: boolean;
  viewerNodeId: string | null;
  parameterKeys: string[];
  primitiveIds: string[];
  viewIds: string[];
};

export type MeasurementControlFocus = {
  primitiveId: string | null;
  parameterKey: string | null;
};

export type ResolvedMeasurementGuide = {
  guideId: string;
  kind: MeasurementGuideKind;
  points: [number, number, number][];
  labelPoint: [number, number, number] | null;
  targetIds: string[];
};

export type ResolvedMeasurementCallout = {
  annotationId: string;
  label: string;
  badgeLabel: string;
  explanation: string | null;
  formulaHint: string | null;
  basis: MeasurementBasis;
  guide: ResolvedMeasurementGuide | null;
  targetIds: string[];
  partIds: string[];
  primitiveIds: string[];
  parameterKeys: string[];
};

const TARGET_KIND_PRIORITY: Record<SelectionTargetKind, number> = {
  edge: 0,
  object: 1,
  group: 2,
  part: 3,
};

function normalizeList(values: string[] | null | undefined): string[] {
  return Array.isArray(values) ? values.filter((value) => typeof value === 'string' && value.trim().length > 0) : [];
}

function defaultTargetId(target: SelectionTarget): string {
  const explicit = target.targetId?.trim();
  if (explicit) return explicit;
  return `${target.kind}:${target.partId}:${target.viewerNodeId}`;
}

function normalizeSelectionTarget(target: SelectionTarget): ContextSelectionTarget {
  return {
    targetId: defaultTargetId(target),
    kind: target.kind,
    partId: target.partId,
    label: target.label,
    editable: target.editable,
    viewerNodeId: target.viewerNodeId || null,
    parameterKeys: normalizeList(target.parameterKeys),
    primitiveIds: normalizeList(target.primitiveIds),
    viewIds: normalizeList(target.viewIds),
  };
}

function syntheticPartTarget(manifest: ModelManifest, partId: string): ContextSelectionTarget | null {
  const part = (manifest.parts || []).find((candidate) => candidate.partId === partId);
  if (!part) return null;
  return {
    targetId: `part:${part.partId}`,
    kind: 'part',
    partId: part.partId,
    label: part.label,
    editable: part.editable,
    viewerNodeId: part.viewerNodeIds?.[0] || null,
    parameterKeys: normalizeList(part.parameterKeys),
    primitiveIds: [],
    viewIds: [],
  };
}

export function buildContextSelectionTargets(manifest: ModelManifest | null): ContextSelectionTarget[] {
  if (!manifest) return [];

  const targets = (manifest.selectionTargets || []).map(normalizeSelectionTarget);
  const seenIds = new Set(targets.map((target) => target.targetId));

  for (const part of manifest.parts || []) {
    const hasPartTarget = targets.some(
      (target) => target.kind === 'part' && target.partId === part.partId,
    );
    if (hasPartTarget) continue;
    const fallback = syntheticPartTarget(manifest, part.partId);
    if (!fallback || seenIds.has(fallback.targetId)) continue;
    targets.push(fallback);
    seenIds.add(fallback.targetId);
  }

  return targets.sort((left, right) => {
    const kindDelta =
      (left.kind === 'global' ? 99 : TARGET_KIND_PRIORITY[left.kind]) -
      (right.kind === 'global' ? 99 : TARGET_KIND_PRIORITY[right.kind]);
    if (kindDelta !== 0) return kindDelta;
    return left.label.localeCompare(right.label);
  });
}

export function createGlobalContextTarget(manifest: ModelManifest | null): ContextSelectionTarget | null {
  if (!manifest) return null;
  const label =
    manifest.document?.documentLabel?.trim() ||
    manifest.document?.documentName?.trim() ||
    'Model';
  const editable =
    (manifest.parts || []).some((part) => part.editable) ||
    (manifest.controlPrimitives || []).some((primitive) => primitive.editable);
  return {
    targetId: 'global',
    kind: 'global',
    partId: null,
    label,
    editable,
    viewerNodeId: null,
    parameterKeys: [],
    primitiveIds: [],
    viewIds: [],
  };
}

export function deriveSelectedPartId(target: ContextSelectionTarget | null): string | null {
  return target?.partId ?? null;
}

export function shouldDisplayViewportControlList(target: ContextSelectionTarget | null): boolean {
  return target?.kind !== 'global';
}

export function resolveContextSelectionTarget(
  manifest: ModelManifest | null,
  targets: ContextSelectionTarget[],
  requestedTargetId: string | null,
  fallbackPartId: string | null,
): ContextSelectionTarget | null {
  if (!manifest) return null;

  if (requestedTargetId) {
    const exact = targets.find((target) => target.targetId === requestedTargetId) ?? null;
    if (exact) return exact;
  }

  if (fallbackPartId) {
    const exactPart = targets.find(
      (target) => target.kind === 'part' && target.partId === fallbackPartId,
    );
    if (exactPart) return exactPart;
    const synthetic = syntheticPartTarget(manifest, fallbackPartId);
    if (synthetic) return synthetic;
  }

  return createGlobalContextTarget(manifest);
}

export function resolveViewerNodeTarget(
  targets: ContextSelectionTarget[],
  viewerNodeId: string | null,
  partId: string | null,
): ContextSelectionTarget | null {
  const byNode = viewerNodeId
    ? targets.filter((target) => target.viewerNodeId === viewerNodeId)
    : [];
  if (byNode.length > 0) {
    const targetPriority = (kind: ContextSelectionKind) =>
      kind === 'global' ? 99 : TARGET_KIND_PRIORITY[kind];
    return [...byNode].sort((left, right) => targetPriority(left.kind) - targetPriority(right.kind))[0] ?? null;
  }
  if (!partId) return null;
  return (
    targets.find((target) => target.kind === 'part' && target.partId === partId) ??
    targets.find((target) => target.partId === partId) ??
    null
  );
}

function sourcePriority(source: MaterializedSemanticView['source']): number {
  switch (source) {
    case 'generated':
      return 0;
    case 'manual':
      return 1;
    case 'inherited':
      return 2;
    case 'llm':
      return 3;
    default:
      return 4;
  }
}

function viewPriority(view: MaterializedSemanticView, target: ContextSelectionTarget | null): number {
  if (target && target.kind !== 'global') {
    if (target.viewIds.includes(view.viewId)) return 0;
    if (target.partId && view.scope === 'part' && (view.partIds || []).includes(target.partId)) {
      return 10 + sourcePriority(view.source);
    }
    if (view.scope === 'global') return 20;
    return 30;
  }

  if (view.scope === 'global' && view.isDefault) return 0;
  if (view.scope === 'global') return 1;
  return 10;
}

export function resolveActiveContextViewId(
  views: MaterializedSemanticView[],
  target: ContextSelectionTarget | null,
  requestedViewId: string | null,
): string | null {
  if (views.length === 0) return null;

  if (requestedViewId) {
    const requested = views.find((view) => view.viewId === requestedViewId) ?? null;
    if (requested) {
      if (!target || target.kind === 'global') return requested.viewId;
      if (target.viewIds.includes(requested.viewId)) return requested.viewId;
      if (requested.scope === 'global') return requested.viewId;
      if (target.partId && (requested.partIds || []).includes(target.partId)) {
        return requested.viewId;
      }
    }
  }

  return [...views]
    .sort((left, right) => {
      const priorityDelta = viewPriority(left, target) - viewPriority(right, target);
      if (priorityDelta !== 0) return priorityDelta;
      return (left.order ?? 0) - (right.order ?? 0) || left.label.localeCompare(right.label);
    })[0]?.viewId ?? null;
}

function uniqueControls(controls: MaterializedSemanticControl[]): MaterializedSemanticControl[] {
  const seen = new Set<string>();
  const next: MaterializedSemanticControl[] = [];
  for (const control of controls) {
    if (seen.has(control.primitiveId)) continue;
    seen.add(control.primitiveId);
    next.push(control);
  }
  return next;
}

function controlMatchesTarget(
  control: MaterializedSemanticControl,
  target: ContextSelectionTarget,
): boolean {
  if (target.primitiveIds.includes(control.primitiveId)) return true;
  if (target.parameterKeys.length === 0) return false;
  if (control.rawField && target.parameterKeys.includes(control.rawField.key)) return true;
  return (control.bindings || []).some((binding) => target.parameterKeys.includes(binding.parameterKey));
}

export function pickContextControls(
  view: MaterializedSemanticView | null,
  target: ContextSelectionTarget | null,
): MaterializedSemanticControl[] {
  if (!view) return [];

  const visibleControls = view.sections
    .filter((section) => !section.collapsed)
    .flatMap((section) => section.controls);

  if (!target || target.kind === 'global') {
    return visibleControls;
  }

  const exactControls = visibleControls.filter((control) => controlMatchesTarget(control, target));
  const partScoped = target.partId
    ? visibleControls.filter(
        (control) =>
          !exactControls.some((exact) => exact.primitiveId === control.primitiveId) &&
          (control.partIds || []).includes(target.partId as string),
      )
    : [];
  const globalControls = visibleControls.filter(
    (control) =>
      !exactControls.some((exact) => exact.primitiveId === control.primitiveId) &&
      !partScoped.some((scoped) => scoped.primitiveId === control.primitiveId) &&
      (control.partIds || []).length === 0,
  );

  const ordered = uniqueControls([...exactControls, ...partScoped, ...globalControls]);
  return ordered.length > 0 ? ordered : visibleControls;
}

function matchesControlQuery(control: MaterializedSemanticControl, query: string): boolean {
  const signature = `${control.label} ${control.rawField?.key ?? ''}`.toLowerCase();
  return signature.includes(query);
}

export function resolveContextSections(
  view: MaterializedSemanticView | null,
  target: ContextSelectionTarget | null,
  searchQuery: string,
): MaterializedSemanticSection[] {
  if (!view) return [];
  const orderedControls = pickContextControls(view, target);
  const order = new Map(orderedControls.map((control, index) => [control.primitiveId, index]));
  const allowedIds = new Set(orderedControls.map((control) => control.primitiveId));
  const query = searchQuery.trim().toLowerCase();

  return view.sections
    .map((section) => ({
      ...section,
      controls: section.controls
        .filter((control) => allowedIds.has(control.primitiveId))
        .filter((control) => !query || matchesControlQuery(control, query))
        .sort(
          (left, right) =>
            (order.get(left.primitiveId) ?? Number.MAX_SAFE_INTEGER) -
              (order.get(right.primitiveId) ?? Number.MAX_SAFE_INTEGER) ||
            left.label.localeCompare(right.label),
        ),
    }))
    .filter((section) => section.controls.length > 0);
}

export function pickContextAdvisories(
  view: MaterializedSemanticView | null,
  target: ContextSelectionTarget | null,
): Advisory[] {
  if (!view) return [];
  if (!target || target.kind === 'global') return view.advisories;

  const visiblePrimitiveIds = new Set(
    pickContextControls(view, target).map((control) => control.primitiveId),
  );
  return view.advisories.filter((advisory) => {
    if ((advisory.viewIds || []).some((viewId) => target.viewIds.includes(viewId))) return true;
    return (advisory.primitiveIds || []).some((primitiveId) => visiblePrimitiveIds.has(primitiveId));
  });
}

export function resolveTargetParameterKeys(
  manifest: ModelManifest | null,
  target: ContextSelectionTarget | null,
): Set<string> {
  const keys = new Set<string>();
  if (!manifest || !target || target.kind === 'global') return keys;

  for (const key of target.parameterKeys) {
    keys.add(key);
  }
  if (keys.size > 0) return keys;

  if (target.primitiveIds.length > 0) {
    for (const primitive of manifest.controlPrimitives || []) {
      if (!target.primitiveIds.includes(primitive.primitiveId)) continue;
      for (const binding of primitive.bindings || []) {
        keys.add(binding.parameterKey);
      }
    }
  }
  if (keys.size > 0) return keys;

  if (target.partId) {
    for (const group of manifest.parameterGroups || []) {
      if (!(group.partIds || []).includes(target.partId)) continue;
      for (const key of group.parameterKeys || []) {
        keys.add(key);
      }
    }
    if (keys.size === 0) {
      const part = (manifest.parts || []).find((candidate) => candidate.partId === target.partId);
      for (const key of part?.parameterKeys || []) {
        keys.add(key);
      }
    }
  }

  return keys;
}

export function filterFieldsBySearch<T extends { key: string; label: string }>(
  fields: T[],
  searchQuery: string,
): T[] {
  const query = searchQuery.trim().toLowerCase();
  if (!query) return fields;
  return fields.filter((field) => {
    const label = typeof field.label === 'string' ? field.label : '';
    return field.key.toLowerCase().includes(query) || label.toLowerCase().includes(query);
  });
}

export function buildContextMetrics(
  manifest: ModelManifest | null,
  target: ContextSelectionTarget | null,
  params: DesignParams,
): { parameterCount: number; advisoryCount: number; editable: boolean; hasValue: boolean } {
  const parameterCount = resolveTargetParameterKeys(manifest, target).size;
  const hasValue = parameterCount > 0 && Object.keys(params || {}).length > 0;
  return {
    parameterCount,
    advisoryCount: 0,
    editable: Boolean(target?.editable),
    hasValue,
  };
}

function measurementAxisLabel(axis: MeasurementAnnotation['axis']): string {
  switch (axis) {
    case 'x':
      return 'width';
    case 'y':
      return 'depth';
    case 'z':
      return 'height';
    case 'radial':
      return 'radius';
    case 'normal':
      return 'thickness';
    case 'path':
      return 'path';
    default:
      return 'measurement';
  }
}

function measurementBasisLabel(basis: MeasurementBasis): string {
  switch (basis) {
    case 'outer':
      return 'outer';
    case 'inner':
      return 'inner';
    case 'wall':
      return 'wall';
    case 'clearance':
      return 'clearance';
    case 'centerline':
      return 'centerline';
    case 'pitch':
      return 'pitch';
    default:
      return 'custom';
  }
}

function measurementBadgeLabel(annotation: MeasurementAnnotation): string {
  const label = annotation.label?.trim();
  if (label) return label;
  return `${measurementBasisLabel(annotation.basis)} ${measurementAxisLabel(annotation.axis)}`.trim();
}

function annotationTargetMatchesSelection(
  annotation: MeasurementAnnotation,
  target: ContextSelectionTarget | null,
  targetsById: Map<string, ContextSelectionTarget>,
): boolean {
  const targetIds = normalizeList(annotation.targetIds);
  if (!target || target.kind === 'global' || targetIds.length === 0) return false;
  if (targetIds.includes(target.targetId)) return true;
  if (!target.partId) return false;
  return targetIds.some(
    (targetId) => targetsById.get(targetId)?.partId === target.partId,
  );
}

function annotationRank(
  annotation: MeasurementAnnotation,
  focus: MeasurementControlFocus | null,
  target: ContextSelectionTarget | null,
  targetsById: Map<string, ContextSelectionTarget>,
): number | null {
  const parameterKeys = normalizeList(annotation.parameterKeys);
  const primitiveIds = normalizeList(annotation.primitiveIds);
  if (focus?.parameterKey && parameterKeys.includes(focus.parameterKey)) return 0;
  if (focus?.primitiveId && primitiveIds.includes(focus.primitiveId)) return 1;
  if (annotationTargetMatchesSelection(annotation, target, targetsById)) return 2;
  return null;
}

function deriveAnnotationPartIds(
  annotation: MeasurementAnnotation,
  manifest: ModelManifest,
  targetsById: Map<string, ContextSelectionTarget>,
  selectedTarget: ContextSelectionTarget | null,
): string[] {
  const partIds = new Set<string>();
  for (const targetId of normalizeList(annotation.targetIds)) {
    const partId = targetsById.get(targetId)?.partId;
    if (partId) partIds.add(partId);
  }
  for (const primitiveId of normalizeList(annotation.primitiveIds)) {
    const primitive = (manifest.controlPrimitives || []).find(
      (entry) => entry.primitiveId === primitiveId,
    );
    for (const partId of primitive?.partIds || []) {
      if (partId) partIds.add(partId);
    }
  }
  if (partIds.size === 0 && selectedTarget?.partId) {
    partIds.add(selectedTarget.partId);
  }
  return [...partIds];
}

export function resolveMeasurementCallout(
  manifest: ModelManifest | null,
  artifactBundle: ArtifactBundle | null,
  targets: ContextSelectionTarget[],
  focus: MeasurementControlFocus | null,
  selectedTarget: ContextSelectionTarget | null,
): ResolvedMeasurementCallout | null {
  const annotations = manifest?.measurementAnnotations || [];
  if (!manifest || annotations.length === 0) return null;

  const targetsById = new Map(targets.map((target) => [target.targetId, target] as const));
  type RankedAnnotation = {
    annotation: MeasurementAnnotation;
    rank: number;
    index: number;
  };
  let best: RankedAnnotation | null = null;

  for (const [index, annotation] of annotations.entries()) {
    const rank = annotationRank(annotation, focus, selectedTarget, targetsById);
    if (rank === null) continue;
    if (!best) {
      best = { annotation, rank, index };
      continue;
    }
    if (rank < best.rank) {
      best = { annotation, rank, index };
      continue;
    }
    if (rank > best.rank) continue;

    const currentHasGuide = Boolean(annotation.guideId);
    const bestHasGuide = Boolean(best.annotation.guideId);
    if (currentHasGuide && !bestHasGuide) {
      best = { annotation, rank, index };
      continue;
    }
    if (currentHasGuide === bestHasGuide && index < best.index) {
      best = { annotation, rank, index };
    }
  }

  if (!best) return null;

  const bestAnnotation = best.annotation;
  const guide = bestAnnotation.guideId
    ? (artifactBundle?.measurementGuides || []).find(
        (entry) => entry.guideId === bestAnnotation.guideId,
      ) ?? null
    : null;
  const anchorsById = new Map(
    (artifactBundle?.calloutAnchors || []).map((anchor) => [anchor.anchorId, anchor] as const),
  );
  const resolvedGuide = guide
    ? {
        guideId: guide.guideId,
        kind: guide.kind,
        points: normalizeList(guide.anchorIds)
          .map((anchorId) => anchorsById.get(anchorId)?.position || null)
          .filter((point): point is [number, number, number] => Array.isArray(point)),
        labelPoint: guide.labelAnchorId
          ? anchorsById.get(guide.labelAnchorId)?.position || null
          : null,
        targetIds: guide.targetIds || [],
      }
    : null;

  return {
    annotationId: bestAnnotation.annotationId,
    label: bestAnnotation.label,
    badgeLabel: measurementBadgeLabel(bestAnnotation),
    explanation: bestAnnotation.explanation ?? null,
    formulaHint: bestAnnotation.formulaHint ?? null,
    basis: bestAnnotation.basis,
    guide: resolvedGuide,
    targetIds:
      normalizeList(bestAnnotation.targetIds).length > 0
        ? normalizeList(bestAnnotation.targetIds)
        : selectedTarget
          ? [selectedTarget.targetId]
          : [],
    partIds: deriveAnnotationPartIds(bestAnnotation, manifest, targetsById, selectedTarget),
    primitiveIds: normalizeList(bestAnnotation.primitiveIds),
    parameterKeys: normalizeList(bestAnnotation.parameterKeys),
  };
}
