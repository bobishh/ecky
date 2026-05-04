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
import { buildSketchDocumentFromBrepProjection } from './sketchBrepDerivedSketch';

type Point2d = [number, number];

type Bounds2d = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
};

export type SketchTopologyRepairProposalKind = 'topology' | 'concavity' | 'manual-redraw';

export type SketchTopologyRepairProposal = {
  proposalId: string;
  kind: SketchTopologyRepairProposalKind;
  sketchId: string;
  primitiveId: string | null;
  view: SketchView | null;
  issue: SketchValidationIssue;
  reason: string;
};

export type SketchTopologyRepairEvidence = {
  primitiveId: string;
  detail: string;
};

export type SketchTopologyRepairResult =
  | { document: SketchDocument; evidence: SketchTopologyRepairEvidence }
  | { error: string };

const BOUNDS_TOLERANCE = 1e-6;

export function buildSketchTopologyRepairProposals(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse | null | undefined,
): SketchTopologyRepairProposal[] {
  const validation = projection?.validation;
  if (!validation || validation.passed) return [];

  return (validation.issues ?? [])
    .map((issue, index) => proposalFromIssue(document, projection, issue, index))
    .filter((proposal): proposal is SketchTopologyRepairProposal => proposal !== null);
}

export function applySketchTopologyRepairProposal(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse,
  proposalId: string,
): SketchTopologyRepairResult {
  const proposal = buildSketchTopologyRepairProposals(document, projection).find(
    (candidate) => candidate.proposalId === proposalId,
  );
  if (!proposal) return { error: 'Topology repair proposal missing.' };
  if (!proposal.view) return { error: 'Topology repair view missing.' };
  if (!proposal.primitiveId) return { error: 'Topology repair primitive missing.' };

  const projectionView = (projection.views ?? []).find((view) => view.view === proposal.view);
  if (!projectionView) return { error: 'Topology repair projection view missing.' };

  const derived = buildSketchDocumentFromBrepProjection({
    ...projection,
    views: [projectionView],
  });
  if ('error' in derived) return { error: 'Topology repair projection has no drawable loop.' };

  const derivedPrimitive = derived.document.sketches?.[0]?.primitives?.[0];
  if (!derivedPrimitive?.points?.length) return { error: 'Topology repair projection has no drawable loop.' };

  let replaced = false;
  const sketches = (document.sketches ?? []).map((sketch) => {
    if (sketch.sketchId !== proposal.sketchId) return copySketch(sketch);
    return {
      ...sketch,
      primitives: (sketch.primitives ?? []).map((primitive) => {
        if (primitive.primitiveId !== proposal.primitiveId) return { ...primitive };
        replaced = true;
        return {
          primitiveId: proposal.primitiveId,
          kind: 'polyline' as const,
          points: derivedPrimitive.points?.map(copyPoint) ?? [],
          closed: true,
        };
      }),
    };
  });

  if (!replaced) return { error: 'Topology repair primitive missing.' };

  return {
    document: {
      ...document,
      sketches,
      metadata: {
        ...(document.metadata ?? {}),
        lastRepair: 'topologyRedrawFromBRep',
      },
    },
    evidence: {
      primitiveId: proposal.primitiveId,
      detail: `TOPOLOGY REDRAW ${proposal.view.toUpperCase()} ${proposal.primitiveId} / derived from BRep projection; not authoring history`,
    },
  };
}

function proposalFromIssue(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse,
  issue: SketchValidationIssue,
  index: number,
): SketchTopologyRepairProposal | null {
  const kind = proposalKindFromIssue(document, projection, issue);
  if (!kind) return null;

  const match = findIssueTarget(document, issue);
  const sketchId = match?.sketch.sketchId ?? issue.sketchId;
  const primitiveId = match?.primitive?.primitiveId ?? issue.primitiveId ?? null;
  const view = match?.sketch.view ?? inferViewFromText(issue.message);

  return {
    proposalId: `topology-repair-${sketchId}-${primitiveId ?? 'unbound'}-${index}`,
    kind,
    sketchId,
    primitiveId,
    view,
    issue,
    reason: proposalReason(kind),
  };
}

function proposalKindFromIssue(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse,
  issue: SketchValidationIssue,
): SketchTopologyRepairProposalKind | null {
  const message = issue.message.toLowerCase();
  if (message.includes('topology mismatch')) return 'topology';
  if (message.includes('concavity mismatch')) return 'concavity';
  if (message.includes('containment mismatch') && hasMatchingSketchAndProjectionBounds(document, projection, issue)) {
    return 'manual-redraw';
  }
  return null;
}

function proposalReason(kind: SketchTopologyRepairProposalKind): string {
  if (kind === 'topology') return 'Topology mismatch needs explicit sketch topology repair.';
  if (kind === 'concavity') return 'Concavity mismatch needs explicit profile redraw.';
  return 'Containment mismatch with matching projection bounds needs topology redraw.';
}

function hasMatchingSketchAndProjectionBounds(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse,
  issue: SketchValidationIssue,
): boolean {
  const match = findIssueTarget(document, issue);
  if (!match?.primitive) return false;

  const projectionView = (projection.views ?? []).find((view) => view.view === match.sketch.view);
  if (!projectionView) return false;

  const sketchBounds = boundsFromPrimitive(match.primitive);
  const projectionBounds = boundsFromProjectionView(projectionView);
  return !!sketchBounds && !!projectionBounds && boundsEqual(sketchBounds, projectionBounds);
}

function findIssueTarget(
  document: SketchDocument,
  issue: SketchValidationIssue,
): { sketch: SketchDefinition; primitive: SketchPrimitive | null } | null {
  const sketches = document.sketches ?? [];
  const sketch =
    sketches.find((candidate) => candidate.sketchId === issue.sketchId) ??
    sketches.find((candidate) => candidate.view === inferViewFromText(issue.message));
  if (!sketch) return null;

  if (!issue.primitiveId) return { sketch, primitive: null };

  return {
    sketch,
    primitive: (sketch.primitives ?? []).find((primitive) => primitive.primitiveId === issue.primitiveId) ?? null,
  };
}

function boundsFromPrimitive(primitive: SketchPrimitive): Bounds2d | null {
  return boundsFromPoints((primitive.points ?? []).filter(isPoint2d));
}

function boundsFromProjectionView(view: BrepHiddenLineProjectionView): Bounds2d | null {
  const edges = [...(view.visibleEdges ?? []), ...(view.hiddenEdges ?? [])];
  return boundsFromPoints(edges.flatMap((edge) => edgePoints(edge)));
}

function edgePoints(edge: BrepProjectedEdge2d): Point2d[] {
  return (edge.points ?? []).filter(isPoint2d);
}

function copySketch(sketch: SketchDefinition): SketchDefinition {
  return {
    ...sketch,
    primitives: sketch.primitives?.map((primitive) => ({ ...primitive, points: primitive.points?.map(copyPoint) })),
    constraints: sketch.constraints?.map((constraint) => ({ ...constraint, targetIds: constraint.targetIds ? [...constraint.targetIds] : undefined })),
  };
}

function copyPoint(point: Point2d): Point2d {
  return [point[0], point[1]];
}

function boundsFromPoints(points: Point2d[]): Bounds2d | null {
  if (points.length === 0) return null;

  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  return {
    minX: Math.min(...xs),
    minY: Math.min(...ys),
    maxX: Math.max(...xs),
    maxY: Math.max(...ys),
  };
}

function boundsEqual(left: Bounds2d, right: Bounds2d): boolean {
  return (
    close(left.minX, right.minX) &&
    close(left.minY, right.minY) &&
    close(left.maxX, right.maxX) &&
    close(left.maxY, right.maxY)
  );
}

function close(left: number, right: number): boolean {
  return Math.abs(left - right) <= BOUNDS_TOLERANCE;
}

function inferViewFromText(text: string): SketchView | null {
  const lower = text.toLowerCase();
  if (lower.includes('front')) return 'front';
  if (lower.includes('top')) return 'top';
  if (lower.includes('side')) return 'side';
  if (lower.includes('custom')) return 'custom';
  return null;
}

function isPoint2d(value: unknown): value is Point2d {
  return Array.isArray(value) && value.length === 2 && Number.isFinite(value[0]) && Number.isFinite(value[1]);
}
