import type { SketchView } from './tauri/contracts';

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

export type SketchPointHit = {
  strokeIndex: number;
  pointIndex: number;
};

const INVALID_POINT_ERROR = 'Invalid sketch point.';
const INVALID_POINT_INDEX_ERROR = 'Invalid sketch point index.';
const CLOSED_STROKE_REQUIRED_ERROR = 'Closed stroke required.';
const INVALID_GRID_SIZE_ERROR = 'Invalid sketch grid size.';
const INVALID_COORDINATE_ERROR = 'Invalid sketch coordinate.';
const INVALID_AXIS_ERROR = 'Invalid sketch axis.';
const INVALID_DIMENSION_ERROR = 'Invalid sketch dimension.';
const NON_POSITIVE_DIMENSION_ERROR = 'Sketch dimension must be positive.';
const INVALID_PROFILE_BOUNDS_ERROR = 'Sketch profile bounds invalid.';
const CLOSED_PROFILE_MIN_POINTS_ERROR = 'Closed profile needs at least 3 points.';
const LOCKED_DIMENSION_CHANGE_ERROR = 'Locked sketch dimension would change.';
const DIMENSION_LOCK_TOLERANCE = 1e-6;

export type SketchStrokeBounds = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  width: number;
  height: number;
};

export function editablePointIndices(stroke: SketchStroke): number[] {
  if (stroke.points.length === 0) return [];

  if (!stroke.closed) {
    return stroke.points.map((_, index) => index);
  }

  if (stroke.points.length === 1) {
    return [0];
  }

  return stroke.points.slice(0, -1).map((_, index) => index);
}

export function canEditStrokePoint(stroke: SketchStroke, pointIndex: number): boolean {
  return normalizeEditablePointIndex(stroke, pointIndex) !== null;
}

export function logicalPointCount(stroke: SketchStroke): number {
  if (!stroke.closed || stroke.points.length <= 1) {
    return stroke.points.length;
  }

  return stroke.points.length - 1;
}

export function snapPointToGrid(point: SketchPoint, gridSize: number): SketchPoint {
  const normalizedGridSize = normalizeSketchGridSize(gridSize);

  const nextPoint = copyPoint(point);
  return [snapCoordinateToGrid(nextPoint[0], normalizedGridSize), snapCoordinateToGrid(nextPoint[1], normalizedGridSize)];
}

export function normalizeSketchGridSize(value: unknown): number {
  const parsedValue = typeof value === 'string' ? Number(value.trim()) : value;
  if (typeof parsedValue !== 'number' || !Number.isFinite(parsedValue) || parsedValue <= 0) {
    throw new Error(INVALID_GRID_SIZE_ERROR);
  }

  return parsedValue;
}

export function normalizeSketchCoordinate(value: unknown): number {
  const trimmedValue = typeof value === 'string' ? value.trim() : value;
  const parsedValue = typeof trimmedValue === 'string' && trimmedValue !== '' ? Number(trimmedValue) : trimmedValue;
  if (typeof parsedValue !== 'number' || !Number.isFinite(parsedValue)) {
    throw new Error(INVALID_COORDINATE_ERROR);
  }

  return parsedValue;
}

export function normalizeSketchDimension(value: unknown): number {
  const trimmedValue = typeof value === 'string' ? value.trim() : value;
  const parsedValue = typeof trimmedValue === 'string' && trimmedValue !== '' ? Number(trimmedValue) : trimmedValue;
  if (typeof parsedValue !== 'number' || !Number.isFinite(parsedValue)) {
    throw new Error(INVALID_DIMENSION_ERROR);
  }
  if (parsedValue <= 0) {
    throw new Error(NON_POSITIVE_DIMENSION_ERROR);
  }

  return parsedValue;
}

export function moveClosedStrokePoint(stroke: SketchStroke, pointIndex: number, point: SketchPoint): SketchStroke {
  if (!stroke.closed) {
    throw new Error(CLOSED_STROKE_REQUIRED_ERROR);
  }

  const normalizedIndex = normalizeEditablePointIndex(stroke, pointIndex);
  if (normalizedIndex === null) {
    throw new Error(INVALID_POINT_INDEX_ERROR);
  }

  const nextPoint = copyPoint(point);
  const points = stroke.points.map(copyPoint);
  points[normalizedIndex] = nextPoint;

  if (normalizedIndex === 0 || normalizedIndex === points.length - 1) {
    points[0] = nextPoint;
    points[points.length - 1] = copyPoint(nextPoint);
  }

  return {
    ...stroke,
    points,
  };
}

export function moveClosedStrokePointWithDimensionLocks(
  stroke: SketchStroke,
  pointIndex: number,
  point: SketchPoint,
): SketchStroke {
  if (!stroke.closed) {
    throw new Error(CLOSED_STROKE_REQUIRED_ERROR);
  }

  const normalizedIndex = normalizeEditablePointIndex(stroke, pointIndex);
  if (normalizedIndex === null) {
    throw new Error(INVALID_POINT_INDEX_ERROR);
  }

  const nextPoint = copyPoint(point);
  const locks = stroke.dimensionLocks;
  if (!locks?.width && !locks?.height) {
    return moveClosedStrokePoint(stroke, normalizedIndex, nextPoint);
  }

  const originalPoint = copyPoint(stroke.points[normalizedIndex]);
  const dx = nextPoint[0] - originalPoint[0];
  const dy = nextPoint[1] - originalPoint[1];
  const logicalCount = logicalPointCount(stroke);
  const points = stroke.points.slice(0, logicalCount).map((candidatePoint, index) => {
    const copiedPoint = copyPoint(candidatePoint);
    const movedPoint = [
      locks.width ? copiedPoint[0] + dx : copiedPoint[0],
      locks.height ? copiedPoint[1] + dy : copiedPoint[1],
    ] satisfies SketchPoint;

    if (index === normalizedIndex) {
      if (!locks.width) movedPoint[0] = nextPoint[0];
      if (!locks.height) movedPoint[1] = nextPoint[1];
    }

    return movedPoint;
  });
  points.push(copyPoint(points[0]));

  return {
    ...stroke,
    points,
  };
}

export function moveClosedStrokePointCoordinate(
  stroke: SketchStroke,
  pointIndex: number,
  axis: string,
  value: unknown,
): SketchStroke {
  if (!stroke.closed) {
    throw new Error(CLOSED_STROKE_REQUIRED_ERROR);
  }

  const normalizedIndex = normalizeEditablePointIndex(stroke, pointIndex);
  if (normalizedIndex === null) {
    throw new Error(INVALID_POINT_INDEX_ERROR);
  }

  if (axis !== 'x' && axis !== 'y') {
    throw new Error(INVALID_AXIS_ERROR);
  }

  const point = copyPoint(stroke.points[normalizedIndex]);
  point[axis === 'x' ? 0 : 1] = normalizeSketchCoordinate(value);
  return moveClosedStrokePoint(stroke, normalizedIndex, point);
}

export function deleteClosedStrokePoint(stroke: SketchStroke, pointIndex: number): SketchStroke {
  if (!stroke.closed) {
    throw new Error(CLOSED_STROKE_REQUIRED_ERROR);
  }

  const normalizedIndex = normalizeEditablePointIndex(stroke, pointIndex);
  if (normalizedIndex === null) {
    throw new Error(INVALID_POINT_INDEX_ERROR);
  }

  const logicalCount = logicalPointCount(stroke);
  if (logicalCount - 1 < 3) {
    throw new Error(CLOSED_PROFILE_MIN_POINTS_ERROR);
  }

  const logicalPoints = stroke.points.slice(0, logicalCount).map(copyPoint);
  const points = logicalPoints.filter((_, index) => index !== normalizedIndex);
  points.push(copyPoint(points[0]));

  return {
    ...stroke,
    points,
  };
}

export function closedStrokeBounds(stroke: SketchStroke): SketchStrokeBounds {
  if (!stroke.closed) {
    throw new Error(CLOSED_STROKE_REQUIRED_ERROR);
  }

  if (stroke.points.length === 0) {
    throw new Error(INVALID_POINT_ERROR);
  }

  const points = stroke.points.map(copyPoint);
  const logicalPoints = points.slice(0, logicalPointCount(stroke));
  const xs = logicalPoints.map((point) => point[0]);
  const ys = logicalPoints.map((point) => point[1]);
  const minX = Math.min(...xs);
  const minY = Math.min(...ys);
  const maxX = Math.max(...xs);
  const maxY = Math.max(...ys);
  const width = maxX - minX;
  const height = maxY - minY;

  return { minX, minY, maxX, maxY, width, height };
}

export function resizeClosedStrokeBounds(stroke: SketchStroke, width: unknown, height: unknown): SketchStroke {
  const bounds = closedStrokeBounds(stroke);
  if (bounds.width <= 0 || bounds.height <= 0) {
    throw new Error(INVALID_PROFILE_BOUNDS_ERROR);
  }

  const targetWidth = normalizeSketchDimension(width);
  const targetHeight = normalizeSketchDimension(height);
  const scaleX = targetWidth / bounds.width;
  const scaleY = targetHeight / bounds.height;
  const logicalCount = logicalPointCount(stroke);
  const points = stroke.points.slice(0, logicalCount).map((point) => {
    const copiedPoint = copyPoint(point);
    return [
      bounds.minX + (copiedPoint[0] - bounds.minX) * scaleX,
      bounds.minY + (copiedPoint[1] - bounds.minY) * scaleY,
    ] satisfies SketchPoint;
  });
  points.push(copyPoint(points[0]));

  return {
    ...stroke,
    points,
  };
}

export function resizeClosedStrokeBoundsSnapped(
  stroke: SketchStroke,
  width: unknown,
  height: unknown,
  gridSize: unknown,
): SketchStroke {
  const normalizedGridSize = normalizeSketchGridSize(gridSize);
  const snappedWidth = snapCoordinateToGrid(normalizeSketchDimension(width), normalizedGridSize);
  const snappedHeight = snapCoordinateToGrid(normalizeSketchDimension(height), normalizedGridSize);
  return resizeClosedStrokeBounds(stroke, snappedWidth, snappedHeight);
}

export function setClosedStrokeBoundsOrigin(stroke: SketchStroke, minX: unknown, minY: unknown): SketchStroke {
  const bounds = closedStrokeBounds(stroke);
  const targetMinX = normalizeSketchCoordinate(minX);
  const targetMinY = normalizeSketchCoordinate(minY);
  const dx = targetMinX - bounds.minX;
  const dy = targetMinY - bounds.minY;
  const logicalCount = logicalPointCount(stroke);
  const points = stroke.points.slice(0, logicalCount).map((point) => {
    const copiedPoint = copyPoint(point);
    return [copiedPoint[0] + dx, copiedPoint[1] + dy] satisfies SketchPoint;
  });
  points.push(copyPoint(points[0]));

  return {
    ...stroke,
    points,
  };
}

export function setClosedStrokeBoundsOriginSnapped(
  stroke: SketchStroke,
  minX: unknown,
  minY: unknown,
  gridSize: unknown,
): SketchStroke {
  const origin = snapPointToGrid([normalizeSketchCoordinate(minX), normalizeSketchCoordinate(minY)], normalizeSketchGridSize(gridSize));
  return setClosedStrokeBoundsOrigin(stroke, origin[0], origin[1]);
}

export function assertLockedDimensionsPreserved(previous: SketchStroke, next: SketchStroke): void {
  const locks = previous.dimensionLocks;
  if (!locks?.width && !locks?.height) return;

  const previousBounds = closedStrokeBounds(previous);
  const nextBounds = closedStrokeBounds(next);
  if (locks.width && Math.abs(previousBounds.width - nextBounds.width) > DIMENSION_LOCK_TOLERANCE) {
    throw new Error(LOCKED_DIMENSION_CHANGE_ERROR);
  }
  if (locks.height && Math.abs(previousBounds.height - nextBounds.height) > DIMENSION_LOCK_TOLERANCE) {
    throw new Error(LOCKED_DIMENSION_CHANGE_ERROR);
  }
}

export function hitTestSketchPoint(
  strokes: SketchStroke[],
  view: SketchView,
  point: SketchPoint,
  radius: number,
): SketchPointHit | null {
  if (!Number.isFinite(radius) || radius < 0) return null;

  const targetPoint = copyPoint(point);
  let bestHit: SketchPointHit | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;

  for (let strokeIndex = strokes.length - 1; strokeIndex >= 0; strokeIndex -= 1) {
    const stroke = strokes[strokeIndex];
    if (stroke.view !== view) continue;

    const indices = editablePointIndices(stroke);
    for (const pointIndex of indices) {
      const candidatePoint = stroke.points[pointIndex];
      const distance = distanceBetween(candidatePoint, targetPoint);
      if (distance > radius || distance >= bestDistance) continue;

      bestDistance = distance;
      bestHit = { strokeIndex, pointIndex };
    }
  }

  return bestHit;
}

function normalizeEditablePointIndex(stroke: SketchStroke, pointIndex: number): number | null {
  if (!Number.isInteger(pointIndex)) return null;
  if (pointIndex < 0) return null;
  if (stroke.points.length === 0) return null;

  const lastIndex = stroke.points.length - 1;
  if (stroke.closed) {
    if (pointIndex === lastIndex) return 0;
    if (pointIndex > lastIndex) return null;
    return pointIndex;
  }

  if (pointIndex > lastIndex) return null;
  return pointIndex;
}

function copyPoint(point: SketchPoint): SketchPoint {
  if (!isSketchPoint(point)) {
    throw new Error(INVALID_POINT_ERROR);
  }

  return [point[0], point[1]];
}

function isSketchPoint(point: unknown): point is SketchPoint {
  if (!Array.isArray(point) || point.length !== 2) return false;
  return Number.isFinite(point[0]) && Number.isFinite(point[1]);
}

function snapCoordinateToGrid(value: number, gridSize: number): number {
  const snapped = Math.round(value / gridSize) * gridSize;
  return Object.is(snapped, -0) ? 0 : snapped;
}

function distanceBetween(a: SketchPoint, b: SketchPoint): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1]);
}
