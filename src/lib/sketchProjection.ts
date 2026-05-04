import type { SketchView } from './tauri/contracts';
import { strokeKind, type SketchPoint, type SketchStroke } from './sketchWorkspaceState';

type OrthographicSketchView = Extract<SketchView, 'front' | 'top' | 'side'>;
type ProjectionRole = 'source' | 'derived';
type ProjectionAxis = 'x' | 'y' | 'z';

type AxisPair = {
  horizontal: ProjectionAxis;
  vertical: ProjectionAxis;
};

export type SketchProjectionBounds = {
  left: number;
  top: number;
  width: number;
  height: number;
  depth: number;
};

export type SketchProjection = {
  view: OrthographicSketchView;
  label: string;
  role: ProjectionRole;
  points?: SketchPoint[];
  path?: string;
  bounds?: SketchProjectionBounds;
  explanation: string;
};

const orthographicViews: OrthographicSketchView[] = ['front', 'top', 'side'];

const viewAxes: Record<OrthographicSketchView, AxisPair> = {
  front: { horizontal: 'x', vertical: 'y' },
  top: { horizontal: 'x', vertical: 'z' },
  side: { horizontal: 'z', vertical: 'y' },
};

export function buildSketchProjections(profile: SketchStroke, extrusionAmount: number): SketchProjection[] {
  if (!profile.closed) {
    throw new Error('Closed sketch profile required for projection.');
  }

  if (!Number.isFinite(extrusionAmount) || extrusionAmount < 0) {
    throw new Error('Extrusion amount must be a finite non-negative number.');
  }

  const sourceView = orthographicView(profile.view);
  const sourceAxes = viewAxes[sourceView];
  const profileBounds = boundsFromStroke(profile);

  return orthographicViews.map((view) => {
    if (view === sourceView) {
      return {
        view,
        label: `${view.toUpperCase()} / SOURCE PROFILE`,
        role: 'source',
        points: profilePoints(profile),
        path: profilePath(profile),
        explanation: `${viewLabel(view)} view shows original closed profile.`,
      };
    }

    return {
      view,
      label: `${view.toUpperCase()} / EXTRUSION DEPTH`,
      role: 'derived',
      bounds: derivedBounds(viewAxes[view], sourceAxes, profileBounds, extrusionAmount),
      explanation: `${viewLabel(view)} view derives source profile extent plus ${formatMillimeters(extrusionAmount)}mm extrusion depth.`,
    };
  });
}

function orthographicView(view: SketchView): OrthographicSketchView {
  if (view === 'front' || view === 'top' || view === 'side') return view;
  throw new Error('Orthographic sketch view required for projection.');
}

function derivedBounds(
  targetAxes: AxisPair,
  sourceAxes: AxisPair,
  sourceBounds: Omit<SketchProjectionBounds, 'depth'>,
  depth: number,
): SketchProjectionBounds {
  const horizontal = axisExtent(targetAxes.horizontal, sourceAxes, sourceBounds, depth);
  const vertical = axisExtent(targetAxes.vertical, sourceAxes, sourceBounds, depth);

  return {
    left: horizontal.start,
    top: vertical.start,
    width: horizontal.size,
    height: vertical.size,
    depth,
  };
}

function axisExtent(
  axis: ProjectionAxis,
  sourceAxes: AxisPair,
  sourceBounds: Omit<SketchProjectionBounds, 'depth'>,
  depth: number,
): { start: number; size: number } {
  if (axis === sourceAxes.horizontal) {
    return { start: sourceBounds.left, size: sourceBounds.width };
  }
  if (axis === sourceAxes.vertical) {
    return { start: sourceBounds.top, size: sourceBounds.height };
  }
  return { start: 0, size: depth };
}

function boundsFromStroke(stroke: SketchStroke): Omit<SketchProjectionBounds, 'depth'> {
  if (strokeKind(stroke) === 'circle') {
    const center = stroke.points[0];
    const radius = stroke.radius ?? 0;
    return {
      left: center[0] - radius,
      top: center[1] - radius,
      width: radius * 2,
      height: radius * 2,
    };
  }

  const points = stroke.points;
  if (points.length < 3) {
    throw new Error('Closed sketch profile needs at least three points.');
  }

  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);

  return {
    left: minX,
    top: minY,
    width: maxX - minX,
    height: maxY - minY,
  };
}

function profilePath(stroke: SketchStroke): string {
  if (strokeKind(stroke) === 'circle') {
    const center = stroke.points[0];
    const radius = stroke.radius ?? 0;
    return [
      `M${formatNumber(center[0] - radius)} ${formatNumber(center[1])}`,
      `A${formatNumber(radius)} ${formatNumber(radius)} 0 1 0 ${formatNumber(center[0] + radius)} ${formatNumber(center[1])}`,
      `A${formatNumber(radius)} ${formatNumber(radius)} 0 1 0 ${formatNumber(center[0] - radius)} ${formatNumber(center[1])}`,
      'Z',
    ].join(' ');
  }
  const pathPoints = trimClosingPoint(stroke.points);
  const [start, ...segments] = pathPoints;
  if (!start) {
    throw new Error('Closed sketch profile needs at least one point.');
  }

  return [`M${formatPoint(start)}`, ...segments.map((point) => `L${formatPoint(point)}`), 'Z'].join(' ');
}

function profilePoints(stroke: SketchStroke): SketchPoint[] {
  if (strokeKind(stroke) === 'circle') {
    const center = stroke.points[0];
    const radius = stroke.radius ?? 0;
    return [
      [center[0] - radius, center[1]],
      [center[0], center[1] - radius],
      [center[0] + radius, center[1]],
      [center[0], center[1] + radius],
      [center[0] - radius, center[1]],
    ];
  }
  return clonePoints(stroke.points);
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

function clonePoints(points: SketchPoint[]): SketchPoint[] {
  return points.map(([x, y]) => [x, y]);
}

function formatPoint([x, y]: SketchPoint): string {
  return `${formatNumber(x)} ${formatNumber(y)}`;
}

function formatMillimeters(value: number): string {
  return formatNumber(value);
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(2)));
}

function viewLabel(view: OrthographicSketchView): string {
  return view.charAt(0).toUpperCase() + view.slice(1);
}
