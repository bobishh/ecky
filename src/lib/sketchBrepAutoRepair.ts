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
import { findSketchIssueMatch } from './sketchIssueLocator';

type Point2d = [number, number];

type Bounds2d = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  width: number;
  height: number;
};

export type SketchBrepAutoRepairEvidence = {
  primitiveId: string;
  view: SketchView;
  detail: string;
};

export type SketchBrepAutoRepairResult = {
  document: SketchDocument;
  evidence: SketchBrepAutoRepairEvidence[];
  repaired: boolean;
};

const MIN_BOUNDS_SIZE = 1e-6;
const MAX_CONTAINMENT_EXPANSION_RATIO = 2;

export function autoRepairSketchDocumentFromBrepProjection(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse | null | undefined,
): SketchBrepAutoRepairResult {
  const nextDocument = cloneDocument(document);
  const validation = projection?.validation;
  if (!validation || validation.passed) {
    return { document: nextDocument, evidence: [], repaired: false };
  }
  const issues = validation.issues ?? [];

  const evidence: SketchBrepAutoRepairEvidence[] = [];
  const repairedPrimitiveIds = new Set<string>();

  for (const issue of issues) {
    const repair = repairProjectionMismatch(nextDocument, projection, issue, repairedPrimitiveIds);
    if (!repair) continue;
    evidence.push(repair);
    repairedPrimitiveIds.add(repair.primitiveId);
  }

  return {
    document: nextDocument,
    evidence,
    repaired: evidence.length > 0,
  };
}

function repairProjectionMismatch(
  document: SketchDocument,
  projection: BrepHiddenLineProjectionResponse,
  issue: SketchValidationIssue,
  repairedPrimitiveIds: Set<string>,
): SketchBrepAutoRepairEvidence | null {
  const repairKind = repairKindFromIssue(issue);
  if (!repairKind) return null;

  const match = findPrimitive(document, issue);
  if (!match) return null;
  const { sketch, primitive } = match;
  if (repairedPrimitiveIds.has(primitive.primitiveId)) return null;
  if (primitive.kind !== 'polyline' || primitive.closed !== true) return null;

  const projectionView = (projection.views ?? []).find((view) => view.view === sketch.view);
  if (!projectionView) return null;

  const sourcePoints = primitive.points?.filter(isPoint2d) ?? [];
  const logicalPoints = stripDuplicateClosingPoint(sourcePoints);
  const sourceBounds = boundsFromPoints(logicalPoints);
  const localizedProjectionBounds = targetedProjectionBounds(projectionView, issue, primitive);
  const projectionBounds =
    repairKind === 'containment'
      ? localizedProjectionBounds ?? boundsFromProjectionView(projectionView)
      : localizedProjectionBounds && boundsAreScalable(localizedProjectionBounds)
        ? localizedProjectionBounds
        : boundsFromProjectionView(projectionView);
  if (!sourceBounds || !projectionBounds) return null;

  const targetBounds = repairKind === 'bounds' ? projectionBounds : unionBounds(sourceBounds, projectionBounds);
  if (!boundsAreScalable(sourceBounds) || !boundsAreScalable(targetBounds)) return null;
  if (boundsEqual(sourceBounds, targetBounds)) return null;
  if (repairKind === 'containment' && !containmentExpansionAllowed(sourceBounds, targetBounds)) return null;

  const repairedPoints = logicalPoints.map((point) => remapPointToBounds(point, sourceBounds, targetBounds));
  if (primitive.closed && repairedPoints.length > 0) {
    repairedPoints.push(copyPoint(repairedPoints[0]));
  }

  primitive.points = repairedPoints;

  return {
    primitiveId: primitive.primitiveId,
    view: sketch.view,
    detail: `${repairKind === 'bounds' ? 'BREP AUTO SNAP' : 'BREP AUTO CONTAIN'} ${sketch.view.toUpperCase()} ${primitive.primitiveId} bounds ${formatNumber(sourceBounds.width)}x${formatNumber(sourceBounds.height)} -> ${formatNumber(targetBounds.width)}x${formatNumber(targetBounds.height)}`,
  };
}

function repairKindFromIssue(issue: SketchValidationIssue): 'bounds' | 'containment' | null {
  const kind = issue.kind;
  if (kind === 'boundsMismatch') return 'bounds';
  if (kind === 'containmentMismatch') return 'containment';
  return null;
}

function findPrimitive(
  document: SketchDocument,
  issue: SketchValidationIssue,
): { sketch: SketchDefinition; primitive: SketchPrimitive } | null {
  const exactMatch = findSketchIssueMatch(document, issue);
  if (exactMatch?.primitive) return exactMatch as { sketch: SketchDefinition; primitive: SketchPrimitive };
  return null;
}

function boundsFromProjectionView(view: BrepHiddenLineProjectionView): Bounds2d | null {
  const edges = [...(view.visibleEdges ?? []), ...(view.hiddenEdges ?? [])];
  return boundsFromPoints(edges.flatMap((edge) => edgePoints(edge)));
}

function edgePoints(edge: BrepProjectedEdge2d): Point2d[] {
  return (edge.points ?? []).filter(isPoint2d);
}

function targetedProjectionBounds(
  view: BrepHiddenLineProjectionView,
  issue: SketchValidationIssue,
  primitive: SketchPrimitive,
): Bounds2d | null {
  const topology = issue.topology ?? primitive.topology ?? null;
  const loops = view.loops ?? [];
  const matchedLoop =
    (topology?.loopId ? loops.find((loop) => loop.loopId === topology.loopId) : null) ??
    (topology?.edgeIds?.length
      ? loops.find((loop) => sameEdgeIds(loop.edgeIds, topology.edgeIds))
      : null);
  const loopBounds = matchedLoop ? boundsFromPoints((matchedLoop.points ?? []).filter(isPoint2d)) : null;
  if (loopBounds) return loopBounds;

  const edges = [...(view.visibleEdges ?? []), ...(view.hiddenEdges ?? [])];
  if (topology?.edgeIds?.length) {
    const targetEdgeIds = new Set(normalizedEdgeIds(topology.edgeIds));
    const matchingPoints = edges
      .filter((edge) => targetEdgeIds.has(edge.edgeId.trim()))
      .flatMap((edge) => edgePoints(edge));
    const edgeBounds = boundsFromPoints(matchingPoints);
    if (edgeBounds) return edgeBounds;
  }

  if (issue.edgeId) {
    const edgeBounds = boundsFromPoints(
      edges.filter((edge) => edge.edgeId.trim() === issue.edgeId?.trim()).flatMap((edge) => edgePoints(edge)),
    );
    if (edgeBounds) return edgeBounds;
  }

  return null;
}

function boundsFromPoints(points: Point2d[]): Bounds2d | null {
  if (points.length === 0) return null;

  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  const minX = Math.min(...xs);
  const minY = Math.min(...ys);
  const maxX = Math.max(...xs);
  const maxY = Math.max(...ys);

  return {
    minX,
    minY,
    maxX,
    maxY,
    width: maxX - minX,
    height: maxY - minY,
  };
}

function unionBounds(sourceBounds: Bounds2d, projectionBounds: Bounds2d): Bounds2d {
  const minX = Math.min(sourceBounds.minX, projectionBounds.minX);
  const minY = Math.min(sourceBounds.minY, projectionBounds.minY);
  const maxX = Math.max(sourceBounds.maxX, projectionBounds.maxX);
  const maxY = Math.max(sourceBounds.maxY, projectionBounds.maxY);
  return {
    minX,
    minY,
    maxX,
    maxY,
    width: maxX - minX,
    height: maxY - minY,
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
  return Math.abs(left - right) <= 1e-6;
}

function sameEdgeIds(left: string[] | undefined, right: string[] | undefined): boolean {
  const a = normalizedEdgeIds(left);
  const b = normalizedEdgeIds(right);
  return a.length > 0 && a.length === b.length && a.every((edgeId, index) => edgeId === b[index]);
}

function normalizedEdgeIds(edgeIds: string[] | undefined): string[] {
  return [...(edgeIds ?? [])].map((edgeId) => edgeId.trim()).filter(Boolean).sort();
}

function remapPointToBounds(point: Point2d, sourceBounds: Bounds2d, targetBounds: Bounds2d): Point2d {
  const xRatio = (point[0] - sourceBounds.minX) / sourceBounds.width;
  const yRatio = (point[1] - sourceBounds.minY) / sourceBounds.height;
  return [
    roundSketchCoordinate(targetBounds.minX + xRatio * targetBounds.width),
    roundSketchCoordinate(targetBounds.minY + yRatio * targetBounds.height),
  ];
}

function stripDuplicateClosingPoint(points: Point2d[]): Point2d[] {
  if (points.length < 2) return points.map(copyPoint);
  const first = points[0];
  const last = points[points.length - 1];
  const logicalPoints = first[0] === last[0] && first[1] === last[1] ? points.slice(0, -1) : points;
  return logicalPoints.map(copyPoint);
}

function boundsAreScalable(bounds: Bounds2d): boolean {
  return bounds.width > MIN_BOUNDS_SIZE && bounds.height > MIN_BOUNDS_SIZE;
}

function containmentExpansionAllowed(sourceBounds: Bounds2d, targetBounds: Bounds2d): boolean {
  return (
    targetBounds.width / sourceBounds.width <= MAX_CONTAINMENT_EXPANSION_RATIO &&
    targetBounds.height / sourceBounds.height <= MAX_CONTAINMENT_EXPANSION_RATIO
  );
}

function cloneDocument(document: SketchDocument): SketchDocument {
  return JSON.parse(JSON.stringify(document)) as SketchDocument;
}

function copyPoint(point: Point2d): Point2d {
  return [point[0], point[1]];
}

function isPoint2d(value: unknown): value is Point2d {
  return Array.isArray(value) && value.length === 2 && Number.isFinite(value[0]) && Number.isFinite(value[1]);
}

function roundSketchCoordinate(value: number): number {
  return Number(value.toFixed(4));
}

function formatNumber(value: number): string {
  const rounded = roundSketchCoordinate(value);
  return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(4).replace(/\.?0+$/, '');
}
