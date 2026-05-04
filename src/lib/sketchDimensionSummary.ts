import type { SketchView } from './tauri/contracts';
import type { SketchPoint } from './sketchWorkspaceState';

export type SketchDimensionStroke = {
  view: SketchView;
  kind?: 'polyline' | 'circle';
  points: SketchPoint[];
  closed: boolean;
  radius?: number;
};

export type SketchDimensionSummary = {
  view: SketchView;
  width: number;
  height: number;
  depth: number;
  pointCount: number;
  constraints: string[];
  evidence: string[];
};

export function buildSketchDimensionSummary(
  stroke: SketchDimensionStroke,
  extrudeDepth: number,
): SketchDimensionSummary | null {
  if (!stroke.closed) return null;

  const kind = stroke.kind ?? 'polyline';
  const profilePoints = kind === 'circle' ? stroke.points : trimClosingPoint(stroke.points);
  if (kind !== 'circle' && profilePoints.length < 3) return null;
  if (!Number.isFinite(extrudeDepth)) return null;

  const bounds = boundsFromStroke(stroke, profilePoints, kind);
  const depth = formatDimension(extrudeDepth);

  return {
    view: stroke.view,
    width: bounds.width,
    height: bounds.height,
    depth,
    pointCount: profilePoints.length,
    constraints: ['CLOSED PROFILE'],
    evidence: [
      `${stroke.view} view`,
      `bounds ${formatMillimeters(bounds.width)}mm x ${formatMillimeters(bounds.height)}mm`,
      `extrude depth ${formatMillimeters(depth)}mm`,
      `${profilePoints.length} profile ${profilePoints.length === 1 ? 'point' : 'points'}`,
    ],
  };
}

function boundsFromPoints(points: SketchPoint[]): { width: number; height: number } {
  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);

  return {
    width: formatDimension(maxX - minX),
    height: formatDimension(maxY - minY),
  };
}

function boundsFromStroke(
  stroke: SketchDimensionStroke,
  points: SketchPoint[],
  kind: SketchDimensionStroke['kind'] | 'polyline',
): { width: number; height: number } {
  if (kind === 'circle') {
    const radius = stroke.radius ?? 0;
    return {
      width: formatDimension(radius * 2),
      height: formatDimension(radius * 2),
    };
  }
  return boundsFromPoints(points);
}

function trimClosingPoint(points: SketchPoint[]): SketchPoint[] {
  if (points.length < 2) return points;

  const first = points[0];
  const last = points[points.length - 1];
  if (first[0] === last[0] && first[1] === last[1]) {
    return points.slice(0, -1);
  }
  return points;
}

function formatDimension(value: number): number {
  return Number(value.toFixed(2));
}

function formatMillimeters(value: number): string {
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(2)));
}
