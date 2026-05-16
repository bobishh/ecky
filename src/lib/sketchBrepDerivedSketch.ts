import type {
  BrepHiddenLineProjectionResponse,
  BrepProjectedLoop2d,
  BrepProjectedLoopRole,
  BrepHiddenLineProjectionView,
  BrepProjectedEdge2d,
  SketchDefinition,
  SketchDocument,
  SketchPrimitive,
  SketchPrimitiveTopology,
  SketchView,
} from './tauri/contracts';

type Point2d = [number, number];

type Bounds2d = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
};

type DerivedLoop = {
  loopId?: string;
  points: Point2d[];
  topology?: SketchPrimitiveTopology;
};

type LoopSegment = {
  edgeId: string;
  sourceClass: string;
  a: Point2d;
  b: Point2d;
};

export type SketchBrepDerivedSketchResult =
  | {
      document: SketchDocument;
      evidence: string;
      views: SketchView[];
    }
  | { error: string };

export function buildSketchDocumentFromBrepProjection(
  projection: BrepHiddenLineProjectionResponse,
): SketchBrepDerivedSketchResult {
  const sketches = (projection.views ?? [])
    .map((view) => sketchFromProjectionView(view))
    .filter((sketch): sketch is SketchDefinition => sketch !== null);

  if (sketches.length === 0) {
    return { error: 'BRep projection has no drawable sketch bounds.' };
  }

  const views = sketches.map((sketch) => sketch.view);
  return {
    document: {
      documentId: `derived-brep-${safeId(projection.modelId || 'model')}`,
      activeSketchId: sketches[0]?.sketchId ?? null,
      units: 'mm',
      metadata: {
        provenance: 'derivedFromBRep',
        sourceArtifactPath: projection.sourceArtifactPath,
        sourceModelId: projection.modelId,
      },
      sketches,
    },
    evidence: `DERIVED FROM BREP / NOT AUTHORING HISTORY / ${views.map((view) => view.toUpperCase()).join(', ')}`,
    views,
  };
}

function sketchFromProjectionView(view: BrepHiddenLineProjectionView): SketchDefinition | null {
  const bounds = boundsFromProjectionView(view);
  if (!bounds) return null;
  const loopPrimitives = loopPrimitivesFromProjectionView(view);

  return {
    sketchId: `derived-brep-${view.view}`,
    view: view.view,
    primitives: loopPrimitives.length
      ? loopPrimitives
      : [
          {
            primitiveId: `derived-brep-${view.view}`,
            kind: 'polyline',
            closed: true,
            points: rectanglePoints(bounds),
          },
        ],
  };
}

function loopPrimitivesFromProjectionView(view: BrepHiddenLineProjectionView): SketchPrimitive[] {
  const loops = closedLoopsFromProjectionView(view);
  if (loops.length === 0) return [];

  const usedPrimitiveIds = new Set<string>();
  return loops.map((loop, index) => ({
    primitiveId: uniquePrimitiveId(
      usedPrimitiveIds,
      loop.loopId ? `derived-brep-${view.view}-${safeId(loop.loopId)}` : defaultPrimitiveId(view.view, index),
    ),
    kind: 'polyline' as const,
    closed: true,
    points: loop.points,
    ...(loop.topology ? { topology: copyTopology(loop.topology) } : {}),
  }));
}

function closedLoopsFromProjectionView(view: BrepHiddenLineProjectionView): DerivedLoop[] {
  const projectedLoops = closedProjectedLoops(view.loops ?? []);
  if (projectedLoops.length > 0) return projectedLoops;

  const edges = [...(view.visibleEdges ?? []), ...(view.hiddenEdges ?? [])];
  const segments = edges.flatMap(loopSegmentsForEdge);
  if (segments.length === 0) return [];
  const remaining = segments.map((segment) => ({ ...segment, a: copyPoint(segment.a), b: copyPoint(segment.b) }));
  const loops: DerivedLoop[] = [];

  while (remaining.length > 0) {
    const loop = consumeClosedLoop(remaining);
    if (loop) loops.push(loop);
  }

  return classifyDerivedLoops(loops);
}

function closedProjectedLoops(loops: BrepProjectedLoop2d[]): { loopId?: string; points: Point2d[]; topology?: SketchPrimitiveTopology }[] {
  return loops
    .map((loop) => ({
      loopId: loop.loopId,
      points: (loop.points ?? []).filter(isPoint2d),
      topology: {
        loopId: loop.loopId,
        edgeIds: [...(loop.edgeIds ?? [])],
        loopRole: loop.role ?? null,
        sourceClass: loop.sourceClass,
      },
    }))
    .filter((loop) => loop.points.length >= 4 && samePoint(loop.points[0], loop.points.at(-1)));
}

function consumeClosedLoop(remaining: LoopSegment[]): DerivedLoop | null {
  const first = remaining.shift();
  if (!first) return null;

  const loop = [copyPoint(first.a), copyPoint(first.b)];
  const edgeIds = [first.edgeId];
  const sourceClasses = [first.sourceClass];
  while (remaining.length > 0) {
    if (loop.length >= 4 && samePoint(loop[0], loop.at(-1))) break;

    const current = loop.at(-1);
    if (!current) return null;

    const nextIndex = remaining.findIndex(
      (segment) => samePoint(segment.a, current) || samePoint(segment.b, current),
    );
    if (nextIndex < 0) break;

    const [segment] = remaining.splice(nextIndex, 1);
    const oriented = samePoint(segment.a, current)
      ? { point: segment.b, edgeId: segment.edgeId, sourceClass: segment.sourceClass }
      : { point: segment.a, edgeId: segment.edgeId, sourceClass: segment.sourceClass };
    loop.push(copyPoint(oriented.point));
    edgeIds.push(oriented.edgeId);
    sourceClasses.push(oriented.sourceClass);
  }

  if (!(loop.length >= 4 && samePoint(loop[0], loop.at(-1)))) return null;
  return {
    points: loop,
    topology: {
      loopId: `derived-${safeId([...edgeIds].sort().join('-'))}`,
      edgeIds,
      sourceClass: dominantSourceClass(sourceClasses),
    },
  };
}

function loopSegmentsForEdge(edge: BrepProjectedEdge2d): LoopSegment[] {
  return (edge.points ?? [])
    .filter(isPoint2d)
    .slice(0)
    .reduce<LoopSegment[]>((segments, point, index, points) => {
      const next = points[index + 1];
      if (!next || samePoint(point, next)) return segments;
      segments.push({
        edgeId: edge.edgeId,
        sourceClass: edge.sourceClass,
        a: copyPoint(point),
        b: copyPoint(next),
      });
      return segments;
    }, []);
}

function classifyDerivedLoops(loops: DerivedLoop[]): DerivedLoop[] {
  const areas = loops.map((loop) => polygonAreaAbs(loop.points));
  const classified = loops.map((loop, index) => {
    const sample = representativeLoopPoint(loop.points);
    const containingLargerCount =
      sample === null
        ? 0
        : loops.filter(
            (other, otherIndex) =>
              otherIndex !== index && areas[otherIndex] > areas[index] && polygonContainsPoint(other.points, sample),
          ).length;
    const loopRole: BrepProjectedLoopRole = containingLargerCount % 2 === 1 ? 'hole' : 'outer';
    return {
      ...loop,
      topology: {
        ...(loop.topology ?? {}),
        edgeIds: [...(loop.topology?.edgeIds ?? [])],
        loopRole,
      },
    };
  });

  return classified.sort((left, right) => {
    const leftRank = loopRoleRank(left.topology?.loopRole);
    const rightRank = loopRoleRank(right.topology?.loopRole);
    return leftRank - rightRank || (left.topology?.loopId ?? '').localeCompare(right.topology?.loopId ?? '');
  });
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

function rectanglePoints(bounds: Bounds2d): Point2d[] {
  return [
    [bounds.minX, bounds.minY],
    [bounds.maxX, bounds.minY],
    [bounds.maxX, bounds.maxY],
    [bounds.minX, bounds.maxY],
    [bounds.minX, bounds.minY],
  ];
}

function copyPoint(point: Point2d): Point2d {
  return [point[0], point[1]];
}

function defaultPrimitiveId(view: SketchView, index: number): string {
  return index === 0 ? `derived-brep-${view}` : `derived-brep-${view}-${index + 1}`;
}

function uniquePrimitiveId(usedPrimitiveIds: Set<string>, candidate: string): string {
  if (!usedPrimitiveIds.has(candidate)) {
    usedPrimitiveIds.add(candidate);
    return candidate;
  }

  let suffix = 2;
  while (usedPrimitiveIds.has(`${candidate}-${suffix}`)) {
    suffix += 1;
  }
  const uniqueId = `${candidate}-${suffix}`;
  usedPrimitiveIds.add(uniqueId);
  return uniqueId;
}

function samePoint(left: Point2d | undefined, right: Point2d | undefined): boolean {
  return Boolean(left && right && Math.abs(left[0] - right[0]) <= 1e-6 && Math.abs(left[1] - right[1]) <= 1e-6);
}

function copyTopology(topology: SketchPrimitiveTopology | null | undefined): SketchPrimitiveTopology | null | undefined {
  if (!topology) return topology;
  return {
    ...topology,
    edgeIds: topology.edgeIds ? [...topology.edgeIds] : undefined,
  };
}

function dominantSourceClass(sourceClasses: string[]): string {
  const normalized = sourceClasses.map((sourceClass) => sourceClass.trim()).filter(Boolean);
  if (normalized.length === 0) return 'derived';
  return normalized.every((sourceClass) => sourceClass === normalized[0]) ? normalized[0] : 'derived';
}

function polygonAreaAbs(points: Point2d[]): number {
  if (points.length < 3) return 0;
  let area = 0;
  for (let index = 0; index < points.length; index += 1) {
    const next = (index + 1) % points.length;
    area += points[index][0] * points[next][1] - points[next][0] * points[index][1];
  }
  return Math.abs(area * 0.5);
}

function representativeLoopPoint(points: Point2d[]): Point2d | null {
  const last = points.at(-1);
  if (!last) return null;
  return points.find((point) => !samePoint(point, last)) ?? null;
}

function polygonContainsPoint(points: Point2d[], sample: Point2d): boolean {
  let inside = false;
  for (let index = 0, previous = points.length - 1; index < points.length; previous = index++) {
    const [xi, yi] = points[index];
    const [xj, yj] = points[previous];
    const intersects = yi > sample[1] !== yj > sample[1] && sample[0] < ((xj - xi) * (sample[1] - yi)) / (yj - yi || Number.EPSILON) + xi;
    if (intersects) inside = !inside;
  }
  return inside;
}

function loopRoleRank(role: SketchPrimitiveTopology['loopRole']): number {
  if (role === 'outer') return 0;
  if (role === 'hole') return 1;
  return 2;
}

function isPoint2d(value: unknown): value is Point2d {
  return Array.isArray(value) && value.length === 2 && Number.isFinite(value[0]) && Number.isFinite(value[1]);
}

function safeId(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/^-+|-+$/g, '') || 'model';
}
