import type {
  BrepHiddenLineProjectionResponse,
  BrepHiddenLineProjectionView,
  BrepProjectedEdge2d,
  SketchDefinition,
  SketchDocument,
  SketchPrimitive,
  SketchValidationIssue,
  SketchView,
} from './tauri/contracts';

export type SketchBrepProjectionValidationStatus = 'pass' | 'pending' | 'fail';

export type SketchBrepProjectionValidationRow = {
  label: string;
  status: SketchBrepProjectionValidationStatus;
  evidence: string;
  issue?: string;
};

export type SketchBrepProjectionBounds = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  width: number;
  height: number;
};

export type SketchBrepProjectionBoundsComparisonViewSeed = {
  view: SketchView;
  sketchBounds: SketchBrepProjectionBounds;
  projectionBounds: SketchBrepProjectionBounds;
  visibleEdgeCount: number;
  hiddenEdgeCount: number;
  edgeCount: number;
};

export type SketchBrepProjectionBoundsComparisonSeed = {
  documentId: string;
  units: string | null;
  views: SketchBrepProjectionBoundsComparisonViewSeed[];
};

export type SketchBrepProjectionViewSummary = {
  view: SketchView;
  visibleEdgeCount: number;
  hiddenEdgeCount: number;
  edgeCount: number;
  boundsMatched: boolean;
};

export type SketchBrepProjectionValidationSummary = {
  rows: SketchBrepProjectionValidationRow[];
  viewSummaries: SketchBrepProjectionViewSummary[];
  boundsComparisonSeed: SketchBrepProjectionBoundsComparisonSeed;
};

export type SketchBrepProjectionRepairTarget = {
  targetId: string;
  sketchId: string;
  primitiveId: string | null;
  view: SketchView | null;
  severity: SketchValidationIssue['severity'];
  label: string;
  reason: string;
  evidence: string;
};

const boundsTolerance = 0.01;

export function buildSketchBrepProjectionValidationSummary(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse | null | undefined,
): SketchBrepProjectionValidationSummary {
  if (!projection) {
    return {
      rows: [
        {
          label: 'BRep projection',
          status: 'pending',
          evidence: 'Waiting for hidden-line projection evidence.',
          issue: 'missing brep projection',
        },
      ],
      viewSummaries: [],
      boundsComparisonSeed: emptyBoundsSeed(document),
    };
  }

  const boundsComparisonSeed = sketchBrepProjectionBoundsSeed(document, projection);
  const sketchViews = sketchViewsWithBounds(document);
  const viewSummaries = boundsComparisonSeed.views.map((view) => ({
    view: view.view,
    visibleEdgeCount: view.visibleEdgeCount,
    hiddenEdgeCount: view.hiddenEdgeCount,
    edgeCount: view.edgeCount,
    boundsMatched: boundsMatch(view.sketchBounds, view.projectionBounds),
  }));
  const rows = [
    projectionRow(projection),
    ...boundsComparisonSeed.views.map((view) => boundsRow(view)),
    ...missingProjectionRows(sketchViews, boundsComparisonSeed.views),
  ];

  return {
    rows,
    viewSummaries,
    boundsComparisonSeed,
  };
}

export function sketchBrepProjectionBoundsSeed(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse,
): SketchBrepProjectionBoundsComparisonSeed {
  const projectionViews = new Map((projection.views ?? []).map((view) => [view.view, view]));
  const views = sketchViewsWithBounds(document).flatMap((sketchView) => {
    const projectionView = projectionViews.get(sketchView.view);
    if (!projectionView) return [];

    const projectionBounds = boundsFromProjectionView(projectionView);
    if (!projectionBounds) return [];

    const visibleEdgeCount = projectionView.visibleEdges?.length ?? 0;
    const hiddenEdgeCount = projectionView.hiddenEdges?.length ?? 0;

    return [
      {
        view: sketchView.view,
        sketchBounds: sketchView.bounds,
        projectionBounds,
        visibleEdgeCount,
        hiddenEdgeCount,
        edgeCount: visibleEdgeCount + hiddenEdgeCount,
      },
    ];
  });

  return {
    documentId: document.documentId,
    units: document.units ?? null,
    views,
  };
}

export function buildSketchBrepProjectionRepairTargets(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse | null | undefined,
): SketchBrepProjectionRepairTarget[] {
  const validation = projection?.validation;
  if (!validation || validation.passed) return [];

  return (validation.issues ?? []).map((issue, index) => repairTargetFromIssue(document, issue, index));
}

function repairTargetFromIssue(
  document: SketchDocument,
  issue: SketchValidationIssue,
  index: number,
): SketchBrepProjectionRepairTarget {
  const match = sketchIssueMatch(document, issue);
  const sketchId = issue.sketchId || match?.sketch.sketchId || 'model';
  const primitiveId = issue.primitiveId ?? match?.primitive?.primitiveId ?? null;
  const view = match?.sketch.view ?? inferViewFromText(issue.message);
  const targetName = primitiveId ?? sketchId;
  const labelPrefix = view ? view.toUpperCase() : 'MODEL';

  return {
    targetId: `brep-repair-${slugPart(sketchId)}-${slugPart(primitiveId ?? 'sketch')}-${index}`,
    sketchId,
    primitiveId,
    view,
    severity: issue.severity,
    label: `${labelPrefix} / ${targetName}`,
    reason: issue.message,
    evidence: [sketchId, primitiveId, issue.message].filter(Boolean).join(' / '),
  };
}

function sketchIssueMatch(
  document: SketchDocument,
  issue: SketchValidationIssue,
): { sketch: SketchDefinition; primitive: SketchPrimitive | null } | null {
  const sketches = document.sketches ?? [];
  for (const sketch of sketches) {
    const primitive = (sketch.primitives ?? []).find((item) => item.primitiveId === issue.primitiveId) ?? null;
    if (primitive) return { sketch, primitive };
  }
  const sketch = sketches.find((item) => item.sketchId === issue.sketchId) ?? null;
  if (sketch) return { sketch, primitive: null };
  const view = inferViewFromText([issue.sketchId, issue.primitiveId ?? '', issue.message].join(' '));
  const viewSketch = view ? sketches.find((item) => item.view === view) ?? null : null;
  return viewSketch ? { sketch: viewSketch, primitive: null } : null;
}

function inferViewFromText(text: string): SketchView | null {
  const lower = text.toLowerCase();
  if (lower.includes('front')) return 'front';
  if (lower.includes('top')) return 'top';
  if (lower.includes('side')) return 'side';
  return null;
}

function missingProjectionRows(
  sketchViews: Array<{ view: SketchView; bounds: SketchBrepProjectionBounds }>,
  comparisonViews: SketchBrepProjectionBoundsComparisonViewSeed[],
): SketchBrepProjectionValidationRow[] {
  const presentViews = new Set(comparisonViews.map((view) => view.view));
  return sketchViews
    .filter((view) => !presentViews.has(view.view))
    .map((view) => ({
      label: `${view.view.toUpperCase()} bounds`,
      status: 'fail',
      evidence: `${view.view} sketch has no matching BRep projection edges.`,
      issue: 'missing brep projection bounds',
    }));
}

function projectionRow(projection: BrepHiddenLineProjectionResponse): SketchBrepProjectionValidationRow {
  const viewCount = projection.views?.length ?? 0;

  if (viewCount === 0) {
    return {
      label: 'BRep projection',
      status: 'pending',
      evidence: `${projection.modelId}; no hidden-line projection views.`,
      issue: 'missing projection views',
    };
  }

  return {
    label: 'BRep projection',
    status: 'pass',
    evidence: `${projection.modelId}; ${basename(projection.sourceArtifactPath)}; ${formatCount(viewCount, 'view')}.`,
  };
}

function boundsRow(view: SketchBrepProjectionBoundsComparisonViewSeed): SketchBrepProjectionValidationRow {
  const matched = boundsMatch(view.sketchBounds, view.projectionBounds);

  return {
    label: `${view.view.toUpperCase()} bounds`,
    status: matched ? 'pass' : 'fail',
    evidence: [
      `sketch ${formatSize(view.sketchBounds)}`,
      `projection ${formatSize(view.projectionBounds)}`,
      `${view.visibleEdgeCount} visible / ${view.hiddenEdgeCount} hidden.`,
    ].join('; '),
    ...(matched ? {} : { issue: 'bounds mismatch' }),
  };
}

function sketchViewsWithBounds(
  document: SketchDocument,
): Array<{ view: SketchView; bounds: SketchBrepProjectionBounds }> {
  return (document.sketches ?? []).flatMap((sketch) => {
    const bounds = boundsFromSketch(sketch);
    return bounds ? [{ view: sketch.view, bounds }] : [];
  });
}

function boundsFromSketch(sketch: SketchDefinition): SketchBrepProjectionBounds | null {
  return boundsFromPoints((sketch.primitives ?? []).flatMap((primitive) => primitivePoints(primitive)));
}

function primitivePoints(primitive: SketchPrimitive): Array<[number, number]> {
  return (primitive.points ?? []).filter(isPointTuple);
}

function boundsFromProjectionView(view: BrepHiddenLineProjectionView): SketchBrepProjectionBounds | null {
  const edges = [...(view.visibleEdges ?? []), ...(view.hiddenEdges ?? [])];
  return boundsFromPoints(edges.flatMap((edge) => edgePoints(edge)));
}

function edgePoints(edge: BrepProjectedEdge2d): Array<[number, number]> {
  return (edge.points ?? []).filter(isPointTuple);
}

function boundsFromPoints(points: Array<[number, number]>): SketchBrepProjectionBounds | null {
  if (points.length === 0) return null;

  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  const minX = Math.min(...xs);
  const minY = Math.min(...ys);
  const maxX = Math.max(...xs);
  const maxY = Math.max(...ys);

  return {
    minX: formatNumber(minX),
    minY: formatNumber(minY),
    maxX: formatNumber(maxX),
    maxY: formatNumber(maxY),
    width: formatNumber(maxX - minX),
    height: formatNumber(maxY - minY),
  };
}

function boundsMatch(a: SketchBrepProjectionBounds, b: SketchBrepProjectionBounds): boolean {
  return (
    close(a.minX, b.minX) &&
    close(a.minY, b.minY) &&
    close(a.maxX, b.maxX) &&
    close(a.maxY, b.maxY) &&
    close(a.width, b.width) &&
    close(a.height, b.height)
  );
}

function close(a: number, b: number): boolean {
  return Math.abs(a - b) <= boundsTolerance;
}

function isPointTuple(value: unknown): value is [number, number] {
  return Array.isArray(value) && value.length === 2 && Number.isFinite(value[0]) && Number.isFinite(value[1]);
}

function emptyBoundsSeed(document: SketchDocument): SketchBrepProjectionBoundsComparisonSeed {
  return {
    documentId: document.documentId,
    units: document.units ?? null,
    views: [],
  };
}

function basename(path: string): string {
  return path.split('/').filter(Boolean).at(-1) ?? path;
}

function formatCount(count: number, noun: string): string {
  return `${count} ${count === 1 ? noun : `${noun}s`}`;
}

function formatSize(bounds: SketchBrepProjectionBounds): string {
  return `${formatNumber(bounds.width)} x ${formatNumber(bounds.height)}`;
}

function formatNumber(value: number): number {
  return Number(value.toFixed(4));
}

function slugPart(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '') || 'target';
}
