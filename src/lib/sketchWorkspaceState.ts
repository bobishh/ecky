import type { SketchConstraint, SketchDraftRequest, SketchPrimitive, SketchPrimitiveKind, SketchView } from './tauri/contracts';

export type SketchPoint = [number, number];

export type SketchDimensionLocks = {
  width?: boolean;
  height?: boolean;
};

export type SketchStroke = {
  primitiveId: string;
  view: SketchView;
  kind?: Extract<SketchPrimitiveKind, 'polyline' | 'circle'>;
  points: SketchPoint[];
  closed: boolean;
  radius?: number;
  dimensionLocks?: SketchDimensionLocks;
};

export type SketchOrthographicRepairAction =
  | {
      kind: 'scaleViewAxis';
      primitiveId: string;
      view: SketchView;
      axis: 'x' | 'y';
      sourceView: SketchView;
      current: number;
      target: number;
      message: string;
    }
  | {
      kind: 'translateViewAxisRange';
      primitiveId: string;
      view: SketchView;
      axis: 'x' | 'y';
      sourceView: SketchView;
      currentMin: number;
      currentMax: number;
      targetMin: number;
      targetMax: number;
      message: string;
    };

export type SketchDraftError = {
  error: string;
  repairAction?: SketchOrthographicRepairAction;
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
  if (strokeKind(stroke) !== 'polyline') {
    return {
      ...stroke,
      closed: true,
    };
  }
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
  if (strokeKind(stroke) !== 'polyline') {
    return {
      ...stroke,
      closed: true,
    };
  }
  if (stroke.closed || stroke.points.length < 3) return stroke;

  return {
    ...stroke,
    points: [...stroke.points, stroke.points[0]],
    closed: true,
  };
}

export function buildSketchDraftRequest(strokes: SketchStroke[]): SketchDraftRequest | SketchDraftError {
  const profile = primaryClosedProfile(strokes);
  if (!profile) return { error: 'Close profile before preview.' };

  const depthResult = extrudeDepthForProfile(strokes, profile);
  if ('error' in depthResult) return depthResult;

  const primitive: SketchPrimitive = {
    primitiveId: profile.primitiveId,
    kind: strokeKind(profile),
    points: profile.points.map(copyPoint),
    closed: true,
    radius: strokeKind(profile) === 'circle' ? profile.radius ?? null : null,
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

  if (strokeKind(stroke) !== 'polyline' || !stroke.closed || !stroke.dimensionLocks) return constraints;

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

function strokeBounds(stroke: SketchStroke): { minX: number; minY: number; maxX: number; maxY: number; width: number; height: number } {
  if (strokeKind(stroke) === 'circle') {
    const center = stroke.points[0];
    const radius = stroke.radius;
    if (!center || typeof radius !== 'number' || !Number.isFinite(radius) || radius <= 0) {
      throw new Error('Circle radius must be positive and finite.');
    }
    return {
      minX: center[0] - radius,
      minY: center[1] - radius,
      maxX: center[0] + radius,
      maxY: center[1] + radius,
      width: radius * 2,
      height: radius * 2,
    };
  }
  const points = stroke.closed ? stroke.points.slice(0, -1) : stroke.points;
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

function primaryClosedProfile(strokes: SketchStroke[]): SketchStroke | undefined {
  return strokes.find((stroke) => stroke.closed && stroke.view === 'front') ?? strokes.find((stroke) => stroke.closed);
}

export function strokeKind(stroke: SketchStroke): Extract<SketchPrimitiveKind, 'polyline' | 'circle'> {
  return stroke.kind === 'circle' ? 'circle' : 'polyline';
}

function copyPoint(point: SketchPoint): SketchPoint {
  return [point[0], point[1]];
}

function extrudeDepthForProfile(
  strokes: SketchStroke[],
  profile: SketchStroke,
): { amount: number; source: 'default' | 'top' | 'side' } | SketchDraftError {
  if (profile.view !== 'front') return { amount: DEFAULT_EXTRUDE_DEPTH, source: 'default' };

  const frontBounds = strokeBounds(profile);
  const topProfile = strokes.find((stroke) => stroke.closed && stroke.view === 'top');
  const sideProfile = strokes.find((stroke) => stroke.closed && stroke.view === 'side');
  const topBounds = topProfile ? strokeBounds(topProfile) : null;
  const sideBounds = sideProfile ? strokeBounds(sideProfile) : null;

  if (topProfile && topBounds && !sameDimension(topBounds.width, frontBounds.width)) {
    const error = `Top view width ${formatDimension(topBounds.width)}mm must match Front view width ${formatDimension(frontBounds.width)}mm.`;
    return {
      error,
      repairAction: {
        kind: 'scaleViewAxis',
        primitiveId: topProfile.primitiveId,
        view: 'top',
        axis: 'x',
        sourceView: 'front',
        current: formatConstraintValue(topBounds.width),
        target: formatConstraintValue(frontBounds.width),
        message: error,
      },
    };
  }

  if (topProfile && topBounds && !sameRange(topBounds.minX, topBounds.maxX, frontBounds.minX, frontBounds.maxX)) {
    const error = `Top view x range ${formatRange(topBounds.minX, topBounds.maxX)}mm must match Front view x range ${formatRange(frontBounds.minX, frontBounds.maxX)}mm.`;
    return {
      error,
      repairAction: {
        kind: 'translateViewAxisRange',
        primitiveId: topProfile.primitiveId,
        view: 'top',
        axis: 'x',
        sourceView: 'front',
        currentMin: formatConstraintValue(topBounds.minX),
        currentMax: formatConstraintValue(topBounds.maxX),
        targetMin: formatConstraintValue(frontBounds.minX),
        targetMax: formatConstraintValue(frontBounds.maxX),
        message: error,
      },
    };
  }

  if (sideProfile && sideBounds && !sameDimension(sideBounds.height, frontBounds.height)) {
    const error = `Side view height ${formatDimension(sideBounds.height)}mm must match Front view height ${formatDimension(frontBounds.height)}mm.`;
    return {
      error,
      repairAction: {
        kind: 'scaleViewAxis',
        primitiveId: sideProfile.primitiveId,
        view: 'side',
        axis: 'y',
        sourceView: 'front',
        current: formatConstraintValue(sideBounds.height),
        target: formatConstraintValue(frontBounds.height),
        message: error,
      },
    };
  }

  if (sideProfile && sideBounds && !sameRange(sideBounds.minY, sideBounds.maxY, frontBounds.minY, frontBounds.maxY)) {
    const error = `Side view y range ${formatRange(sideBounds.minY, sideBounds.maxY)}mm must match Front view y range ${formatRange(frontBounds.minY, frontBounds.maxY)}mm.`;
    return {
      error,
      repairAction: {
        kind: 'translateViewAxisRange',
        primitiveId: sideProfile.primitiveId,
        view: 'side',
        axis: 'y',
        sourceView: 'front',
        currentMin: formatConstraintValue(sideBounds.minY),
        currentMax: formatConstraintValue(sideBounds.maxY),
        targetMin: formatConstraintValue(frontBounds.minY),
        targetMax: formatConstraintValue(frontBounds.maxY),
        message: error,
      },
    };
  }

  if (sideProfile && topBounds && sideBounds && !sameDimension(topBounds.height, sideBounds.width)) {
    const error = `Top view depth ${formatDimension(topBounds.height)}mm must match Side view depth ${formatDimension(sideBounds.width)}mm.`;
    return {
      error,
      repairAction: {
        kind: 'scaleViewAxis',
        primitiveId: sideProfile.primitiveId,
        view: 'side',
        axis: 'x',
        sourceView: 'top',
        current: formatConstraintValue(sideBounds.width),
        target: formatConstraintValue(topBounds.height),
        message: error,
      },
    };
  }

  if (sideProfile && topBounds && sideBounds && !sameRange(topBounds.minY, topBounds.maxY, sideBounds.minX, sideBounds.maxX)) {
    const error = `Top view depth range ${formatRange(topBounds.minY, topBounds.maxY)}mm must match Side view depth range ${formatRange(sideBounds.minX, sideBounds.maxX)}mm.`;
    return {
      error,
      repairAction: {
        kind: 'translateViewAxisRange',
        primitiveId: sideProfile.primitiveId,
        view: 'side',
        axis: 'x',
        sourceView: 'top',
        currentMin: formatConstraintValue(sideBounds.minX),
        currentMax: formatConstraintValue(sideBounds.maxX),
        targetMin: formatConstraintValue(topBounds.minY),
        targetMax: formatConstraintValue(topBounds.maxY),
        message: error,
      },
    };
  }

  if (topBounds) return { amount: formatConstraintValue(topBounds.height), source: 'top' };
  if (sideBounds) return { amount: formatConstraintValue(sideBounds.width), source: 'side' };
  return { amount: DEFAULT_EXTRUDE_DEPTH, source: 'default' };
}

function sameDimension(left: number, right: number): boolean {
  return Math.abs(left - right) <= DIMENSION_TOLERANCE;
}

function sameRange(leftMin: number, leftMax: number, rightMin: number, rightMax: number): boolean {
  return sameDimension(leftMin, rightMin) && sameDimension(leftMax, rightMax);
}

function formatDimension(value: number): string {
  if (Number.isInteger(value)) return String(value);
  return value.toFixed(2).replace(/\.?0+$/, '');
}

function formatRange(min: number, max: number): string {
  return `${formatDimension(min)}..${formatDimension(max)}`;
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
