import type { SketchView } from './tauri/contracts';

export type SketchGhostPreviewPoint = [number, number];

export type SketchGhostPreviewStroke = {
  view: SketchView;
  points: SketchGhostPreviewPoint[];
  closed: boolean;
};

export type SketchGhostPreviewInput = {
  activeStroke?: SketchGhostPreviewStroke | null;
  strokes: SketchGhostPreviewStroke[];
  generating?: boolean;
  autoQueued?: boolean;
  extrudeDepth?: number;
};

export type SketchGhostPreviewStatus = 'open' | 'closed' | 'queued' | 'generating';

export type SketchGhostPreviewState = {
  status: SketchGhostPreviewStatus;
  label: string;
  view: SketchView;
  points: SketchGhostPreviewPoint[];
  closed: boolean;
  path: string;
  extrudeDepth: number;
};

const DEFAULT_EXTRUDE_DEPTH = 12;

export function summarizeSketchGhostPreview(input: SketchGhostPreviewInput): SketchGhostPreviewState | null {
  const stroke = input.activeStroke ?? latestStroke(input.strokes);
  if (!stroke || stroke.points.length === 0) return null;

  const points = stroke.points.map(copyPoint);
  const closed = stroke.closed;
  const status = previewStatus(closed, input.generating === true, input.autoQueued === true);

  return {
    status,
    label: previewLabel(status),
    view: stroke.view,
    points,
    closed,
    path: pointsToPath(points, closed),
    extrudeDepth: input.extrudeDepth ?? DEFAULT_EXTRUDE_DEPTH,
  };
}

function latestStroke(strokes: SketchGhostPreviewStroke[]): SketchGhostPreviewStroke | null {
  return strokes.at(-1) ?? null;
}

function previewStatus(closed: boolean, generating: boolean, autoQueued: boolean): SketchGhostPreviewStatus {
  if (generating) return 'generating';
  if (autoQueued) return 'queued';
  return closed ? 'closed' : 'open';
}

function previewLabel(status: SketchGhostPreviewStatus): string {
  switch (status) {
    case 'generating':
      return 'GENERATING PREVIEW';
    case 'queued':
      return 'AUTO-PREVIEW QUEUED';
    case 'closed':
      return 'CLOSED PROFILE';
    case 'open':
      return 'OPEN PROFILE';
  }
}

function pointsToPath(points: SketchGhostPreviewPoint[], closed: boolean): string {
  const pathPoints = closed ? logicalClosedPathPoints(points) : points;
  const [firstPoint, ...remainingPoints] = pathPoints;
  const commands = [`M ${formatPoint(firstPoint)}`, ...remainingPoints.map((point) => `L ${formatPoint(point)}`)];
  if (closed) commands.push('Z');
  return commands.join(' ');
}

function logicalClosedPathPoints(points: SketchGhostPreviewPoint[]): SketchGhostPreviewPoint[] {
  if (points.length <= 1) return points;

  const firstPoint = points[0];
  const lastPoint = points.at(-1);
  if (lastPoint && samePoint(firstPoint, lastPoint)) {
    return points.slice(0, -1);
  }

  return points;
}

function formatPoint(point: SketchGhostPreviewPoint): string {
  return `${formatCoordinate(point[0])} ${formatCoordinate(point[1])}`;
}

function formatCoordinate(value: number): string {
  const normalizedValue = Object.is(value, -0) ? 0 : value;
  return Number(normalizedValue.toFixed(4)).toString();
}

function samePoint(a: SketchGhostPreviewPoint, b: SketchGhostPreviewPoint): boolean {
  return a[0] === b[0] && a[1] === b[1];
}

function copyPoint(point: SketchGhostPreviewPoint): SketchGhostPreviewPoint {
  return [point[0], point[1]];
}
