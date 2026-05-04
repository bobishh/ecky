import type {
  BrepHiddenLineProjectionResponse,
  BrepHiddenLineProjectionView,
  BrepProjectedEdge2d,
  SketchDefinition,
  SketchDocument,
  SketchView,
} from './tauri/contracts';

type Point2d = [number, number];

type Bounds2d = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
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
  const loops = closedLoopsFromProjectionView(view);
  const loopPrimitives = loops.map((points, index) => ({
    primitiveId: index === 0 ? `derived-brep-${view.view}` : `derived-brep-${view.view}-${index + 1}`,
    kind: 'polyline' as const,
    closed: true,
    points,
  }));

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

function closedLoopsFromProjectionView(view: BrepHiddenLineProjectionView): Point2d[][] {
  const edges = [...(view.visibleEdges ?? []), ...(view.hiddenEdges ?? [])];
  const segments = edges.map((edge) => edgePoints(edge)).filter((points) => points.length >= 2);
  if (segments.length === 0) return [];
  const remaining = segments.map((segment) => segment.map(copyPoint));
  const loops: Point2d[][] = [];

  while (remaining.length > 0) {
    const loop = consumeClosedLoop(remaining);
    if (loop) loops.push(loop);
  }

  return loops;
}

function consumeClosedLoop(remaining: Point2d[][]): Point2d[] | null {
  const loop = remaining.shift() ?? [];
  while (remaining.length > 0) {
    if (loop.length >= 4 && samePoint(loop[0], loop.at(-1))) return loop;

    const current = loop.at(-1);
    if (!current) return null;

    const nextIndex = remaining.findIndex(
      (segment) => samePoint(segment[0], current) || samePoint(segment.at(-1), current),
    );
    if (nextIndex < 0) break;

    const [segment] = remaining.splice(nextIndex, 1);
    const oriented = samePoint(segment[0], current) ? segment : [...segment].reverse();
    loop.push(...oriented.slice(1));
  }

  return loop.length >= 4 && samePoint(loop[0], loop.at(-1)) ? loop : null;
}

function boundsFromProjectionView(view: BrepHiddenLineProjectionView): Bounds2d | null {
  const edges = [...(view.visibleEdges ?? []), ...(view.hiddenEdges ?? [])];
  return boundsFromPoints(edges.flatMap((edge) => edgePoints(edge)));
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

function samePoint(left: Point2d | undefined, right: Point2d | undefined): boolean {
  return Boolean(left && right && Math.abs(left[0] - right[0]) <= 1e-6 && Math.abs(left[1] - right[1]) <= 1e-6);
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
