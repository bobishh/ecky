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
import {
  findSketchIssueMatch,
  primitiveMatchesIssueEdgeId,
  primitiveMatchesIssueTopology,
} from './sketchIssueLocator';

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

  const targetPrimitive = (document.sketches ?? [])
    .find((sketch) => sketch.sketchId === proposal.sketchId)
    ?.primitives?.find((primitive) => primitive.primitiveId === proposal.primitiveId);
  const targetPrimitiveIndex = closedPrimitiveIndexForProposal(document, proposal);
  const derivedPrimitives = derived.document.sketches?.[0]?.primitives ?? [];
  const derivedPrimitive =
    findDerivedPrimitiveForRepair(targetPrimitive ?? null, proposal.issue, derivedPrimitives) ??
    derivedPrimitives[targetPrimitiveIndex] ??
    derivedPrimitives[0];
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
          ...(derivedPrimitive.topology || primitive.topology
            ? { topology: copyTopology(derivedPrimitive.topology ?? primitive.topology) }
            : {}),
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

function closedPrimitiveIndexForProposal(document: SketchDocument, proposal: SketchTopologyRepairProposal): number {
  if (!proposal.primitiveId) return 0;

  const sketch = (document.sketches ?? []).find((candidate) => candidate.sketchId === proposal.sketchId);
  if (!sketch) return 0;

  const closedPrimitiveIds = (sketch.primitives ?? [])
    .filter((primitive) => primitive.closed)
    .map((primitive) => primitive.primitiveId);
  const primitiveIndex = closedPrimitiveIds.findIndex((primitiveId) => primitiveId === proposal.primitiveId);
  return primitiveIndex >= 0 ? primitiveIndex : 0;
}

function findDerivedPrimitiveForRepair(
  targetPrimitive: SketchPrimitive | null,
  issue: SketchValidationIssue,
  derivedPrimitives: SketchPrimitive[],
): SketchPrimitive | null {
  const targetLoopId = targetPrimitive?.topology?.loopId;
  if (targetLoopId) {
    const byLoopId = derivedPrimitives.find((primitive) => primitive.topology?.loopId === targetLoopId);
    if (byLoopId) return byLoopId;
  }

  const byIssueTopology = derivedPrimitives.find((primitive) => primitiveMatchesIssueTopology(primitive, issue.topology));
  if (byIssueTopology) return byIssueTopology;

  const targetEdgeIds = targetPrimitive?.topology?.edgeIds ?? [];
  if (targetEdgeIds.length > 0) {
    const byEdgeIds = derivedPrimitives.find((primitive) => primitiveMatchesIssueTopology(primitive, { edgeIds: targetEdgeIds }));
    if (byEdgeIds) return byEdgeIds;
  }

  if (issue.edgeId) {
    const byIssueEdgeId = derivedPrimitives.filter((primitive) => primitiveMatchesIssueEdgeId(primitive, issue.edgeId));
    if (byIssueEdgeId.length === 1) return byIssueEdgeId[0];
  }

  const targetLoopRole = targetPrimitive?.topology?.loopRole ?? issue.topology?.loopRole;
  if (targetLoopRole) {
    const roleMatches = derivedPrimitives.filter((primitive) => primitive.topology?.loopRole === targetLoopRole);
    if (roleMatches.length === 1) return roleMatches[0];
    const closestRoleMatch = pickClosestPrimitiveByBounds(targetPrimitive, roleMatches);
    if (closestRoleMatch) return closestRoleMatch;
  }

  return null;
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
  if (!match) return null;

  const sketchId = match.sketch.sketchId;
  const primitiveId = match.primitive?.primitiveId ?? null;
  const view = match.sketch.view;

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
  const kind = issue.kind;
  if (kind === 'topologyMismatch') return 'topology';
  if (kind === 'concavityMismatch') return 'concavity';
  if (kind === 'containmentMismatch' && hasMatchingSketchAndProjectionBounds(document, projection, issue)) {
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
  return findSketchIssueMatch(document, issue);
}

function boundsFromPrimitive(primitive: SketchPrimitive): Bounds2d | null {
  return boundsFromPoints((primitive.points ?? []).filter(isPoint2d));
}

function boundsFromProjectionView(view: BrepHiddenLineProjectionView): Bounds2d | null {
  const edges = [...(view.visibleEdges ?? []), ...(view.hiddenEdges ?? [])];
  const edgeBounds = boundsFromPoints(edges.flatMap((edge) => edgePoints(edge)));
  if (edgeBounds) return edgeBounds;
  return boundsFromPoints((view.loops ?? []).flatMap((loop) => (loop.points ?? []).filter(isPoint2d)));
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

function copyTopology(topology: SketchPrimitive['topology']): SketchPrimitive['topology'] {
  if (!topology) return topology;
  return {
    ...topology,
    edgeIds: topology.edgeIds ? [...topology.edgeIds] : undefined,
  };
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

function pickClosestPrimitiveByBounds(
  targetPrimitive: SketchPrimitive | null,
  candidates: SketchPrimitive[],
): SketchPrimitive | null {
  if (!targetPrimitive || candidates.length < 2) return null;

  const targetBounds = boundsFromPrimitive(targetPrimitive);
  if (!targetBounds) return null;

  const ranked = candidates
    .map((candidate) => {
      const candidateBounds = boundsFromPrimitive(candidate);
      if (!candidateBounds) return null;
      return {
        primitive: candidate,
        distance: boundsDistance(targetBounds, candidateBounds),
      };
    })
    .filter((candidate): candidate is { primitive: SketchPrimitive; distance: number } => candidate !== null)
    .sort((left, right) => left.distance - right.distance);

  if (ranked.length < 2) return ranked[0]?.primitive ?? null;
  if (close(ranked[0].distance, ranked[1].distance)) return null;
  return ranked[0].primitive;
}

function boundsDistance(left: Bounds2d, right: Bounds2d): number {
  return (
    Math.abs(left.minX - right.minX) +
    Math.abs(left.minY - right.minY) +
    Math.abs(left.maxX - right.maxX) +
    Math.abs(left.maxY - right.maxY)
  );
}

function isPoint2d(value: unknown): value is Point2d {
  return Array.isArray(value) && value.length === 2 && Number.isFinite(value[0]) && Number.isFinite(value[1]);
}
