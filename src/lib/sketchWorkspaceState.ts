import type { SketchConstraint, SketchDraftRequest, SketchPrimitive, SketchView } from './tauri/contracts';

export type SketchPoint = [number, number];

export type SketchDimensionLocks = {
  width?: boolean;
  height?: boolean;
};

export type SketchStroke = {
  primitiveId: string;
  view: SketchView;
  points: SketchPoint[];
  closed: boolean;
  dimensionLocks?: SketchDimensionLocks;
};

export type PaneBounds = {
  left: number;
  top: number;
  width: number;
  height: number;
};

export type SvgPointLike = {
  x: number;
  y: number;
  matrixTransform(matrix: unknown): SvgPointLike;
};

export type SvgCoordinateSpace = {
  createSVGPoint(): SvgPointLike;
  getScreenCTM(): { inverse(): unknown } | null;
};

const CLOSE_DISTANCE = 8;
const MIN_CLOSED_POINTS = 4;
const DEFAULT_EXTRUDE_DEPTH = 12;
const DIMENSION_TOLERANCE = 1e-6;

export function normalizePanePoint(clientX: number, clientY: number, bounds: PaneBounds): SketchPoint {
  const x = bounds.width > 0 ? ((clientX - bounds.left) / bounds.width) * 100 : 0;
  const y = bounds.height > 0 ? ((clientY - bounds.top) / bounds.height) * 100 : 0;
  return [clamp(x), clamp(y)];
}

export function clientPointToSvgPoint(clientX: number, clientY: number, svg: SvgCoordinateSpace): SketchPoint {
  const screenCtm = svg.getScreenCTM();
  if (!screenCtm) return [0, 0];

  const point = svg.createSVGPoint();
  point.x = clientX;
  point.y = clientY;
  const svgPoint = point.matrixTransform(screenCtm.inverse());
  return [clamp(svgPoint.x), clamp(svgPoint.y)];
}

export function pointsToSvg(points: SketchPoint[]): string {
  return points.map(([x, y]) => `${x},${y}`).join(' ');
}

export function sourceLineCount(source: string): number {
  if (!source) return 0;
  return source.split(/\r\n|\r|\n/).length;
}

export function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export function finishStroke(stroke: SketchStroke): SketchStroke {
  if (stroke.points.length < 2) return stroke;

  const first = stroke.points[0];
  const last = stroke.points[stroke.points.length - 1];
  const closed = stroke.points.length >= MIN_CLOSED_POINTS && distance(first, last) <= CLOSE_DISTANCE;
  const points = closed ? [...stroke.points.slice(0, -1), first] : stroke.points;

  return {
    ...stroke,
    points,
    closed,
  };
}

export function closeStroke(stroke: SketchStroke): SketchStroke {
  if (stroke.closed || stroke.points.length < 3) return stroke;

  return {
    ...stroke,
    points: [...stroke.points, stroke.points[0]],
    closed: true,
  };
}

export function buildSketchDraftRequest(strokes: SketchStroke[]): SketchDraftRequest | { error: string } {
  const profile = primaryClosedProfile(strokes);
  if (!profile) return { error: 'Close profile before preview.' };

  const depthResult = extrudeDepthForProfile(strokes, profile);
  if ('error' in depthResult) return depthResult;

  const primitive: SketchPrimitive = {
    primitiveId: profile.primitiveId,
    kind: 'polyline',
    points: profile.points,
    closed: true,
  };

  return {
    partId: 'sketch-draft-part',
    sketch: {
      sketchId: `sketch-${profile.view}`,
      view: profile.view,
      primitives: [primitive],
      constraints: constraintsForStroke(profile),
    },
    operation: 'extrude',
    amount: depthResult.amount,
    symmetric: false,
  };
}

export type SketchDraftModeSummary = {
  mode: 'single-view' | 'multi-view';
  label: string;
  detail: string;
};

export function summarizeSketchDraftMode(strokes: SketchStroke[]): SketchDraftModeSummary {
  const profile = primaryClosedProfile(strokes);
  if (!profile) {
    return { mode: 'single-view', label: 'NO CLOSED PROFILE', detail: 'Close a profile before preview.' };
  }

  const depthResult = extrudeDepthForProfile(strokes, profile);
  if ('error' in depthResult) {
    return { mode: 'single-view', label: 'MULTI-VIEW BLOCKED', detail: depthResult.error };
  }

  if (depthResult.source === 'top' || depthResult.source === 'side') {
    return {
      mode: 'multi-view',
      label: 'MULTI-VIEW CONSTRAINED',
      detail: `DEPTH ${formatDimension(depthResult.amount)}MM from ${depthResult.source.toUpperCase()} view.`,
    };
  }

  return {
    mode: 'single-view',
    label: 'SINGLE-VIEW EXTRUDE',
    detail: `DEPTH ${formatDimension(depthResult.amount)}MM default.`,
  };
}

export function constraintsForStroke(stroke: SketchStroke): SketchConstraint[] {
  const constraints: SketchConstraint[] = [
    { constraintId: `${stroke.primitiveId}-closed`, kind: 'closed', targetIds: [stroke.primitiveId] },
  ];

  if (!stroke.closed || !stroke.dimensionLocks) return constraints;

  const bounds = strokeBounds(stroke);
  if (stroke.dimensionLocks.width) {
    constraints.push({
      constraintId: `${stroke.primitiveId}-width-dimension`,
      kind: 'dimension',
      targetIds: [stroke.primitiveId],
      value: formatConstraintValue(bounds.width),
    });
  }
  if (stroke.dimensionLocks.height) {
    constraints.push({
      constraintId: `${stroke.primitiveId}-height-dimension`,
      kind: 'dimension',
      targetIds: [stroke.primitiveId],
      value: formatConstraintValue(bounds.height),
    });
  }

  return constraints;
}

function strokeBounds(stroke: SketchStroke): { width: number; height: number } {
  const points = stroke.closed ? stroke.points.slice(0, -1) : stroke.points;
  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  return {
    width: Math.max(...xs) - Math.min(...xs),
    height: Math.max(...ys) - Math.min(...ys),
  };
}

function primaryClosedProfile(strokes: SketchStroke[]): SketchStroke | undefined {
  return strokes.find((stroke) => stroke.closed && stroke.view === 'front') ?? strokes.find((stroke) => stroke.closed);
}

function extrudeDepthForProfile(
  strokes: SketchStroke[],
  profile: SketchStroke,
): { amount: number; source: 'default' | 'top' | 'side' } | { error: string } {
  if (profile.view !== 'front') return { amount: DEFAULT_EXTRUDE_DEPTH, source: 'default' };

  const frontBounds = strokeBounds(profile);
  const topProfile = strokes.find((stroke) => stroke.closed && stroke.view === 'top');
  const sideProfile = strokes.find((stroke) => stroke.closed && stroke.view === 'side');
  const topBounds = topProfile ? strokeBounds(topProfile) : null;
  const sideBounds = sideProfile ? strokeBounds(sideProfile) : null;

  if (topBounds && !sameDimension(topBounds.width, frontBounds.width)) {
    return {
      error: `Top view width ${formatDimension(topBounds.width)}mm must match Front view width ${formatDimension(frontBounds.width)}mm.`,
    };
  }

  if (sideBounds && !sameDimension(sideBounds.height, frontBounds.height)) {
    return {
      error: `Side view height ${formatDimension(sideBounds.height)}mm must match Front view height ${formatDimension(frontBounds.height)}mm.`,
    };
  }

  if (topBounds && sideBounds && !sameDimension(topBounds.height, sideBounds.width)) {
    return {
      error: `Top view depth ${formatDimension(topBounds.height)}mm must match Side view depth ${formatDimension(sideBounds.width)}mm.`,
    };
  }

  if (topBounds) return { amount: formatConstraintValue(topBounds.height), source: 'top' };
  if (sideBounds) return { amount: formatConstraintValue(sideBounds.width), source: 'side' };
  return { amount: DEFAULT_EXTRUDE_DEPTH, source: 'default' };
}

function sameDimension(left: number, right: number): boolean {
  return Math.abs(left - right) <= DIMENSION_TOLERANCE;
}

function formatDimension(value: number): string {
  if (Number.isInteger(value)) return String(value);
  return value.toFixed(2).replace(/\.?0+$/, '');
}

function formatConstraintValue(value: number): number {
  return Number(value.toFixed(4));
}

function clamp(value: number): number {
  return Math.max(0, Math.min(100, Number(value.toFixed(2))));
}

function distance(a: SketchPoint, b: SketchPoint): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1]);
}
