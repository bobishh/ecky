import type { SketchOrthographicRepairAction, SketchPoint, SketchStroke } from './sketchWorkspaceState';
import { buildSketchDraftRequest } from './sketchWorkspaceState';
import { closedStrokeBounds, logicalPointCount } from './sketchEditState';

export type SketchOrthographicRepair = SketchOrthographicRepairAction & {
  detail: string;
};

export type SketchOrthographicRepairResult = {
  strokes: SketchStroke[];
  repairs: SketchOrthographicRepair[];
};

type AppliedSketchOrthographicRepair = {
  strokes: SketchStroke[];
  repair: SketchOrthographicRepair;
};

const MAX_REPAIR_PASSES = 8;
const DIMENSION_TOLERANCE = 1e-6;

export function autoRepairOrthographicSketchStrokes(strokes: SketchStroke[]): SketchOrthographicRepairResult {
  let nextStrokes = cloneStrokes(strokes);
  const repairs: SketchOrthographicRepair[] = [];

  for (let pass = 0; pass < MAX_REPAIR_PASSES; pass += 1) {
    const request = buildSketchDraftRequest(nextStrokes);
    if (!('error' in request) || !request.repairAction) break;

    const applied = applyOrthographicRepairAction(nextStrokes, request.repairAction);
    if (!applied) break;
    nextStrokes = applied.strokes;
    repairs.push(applied.repair);
  }

  return { strokes: nextStrokes, repairs };
}

function applyOrthographicRepairAction(
  strokes: SketchStroke[],
  action: SketchOrthographicRepairAction,
): AppliedSketchOrthographicRepair | null {
  const strokeIndex = strokes.findIndex(
    (stroke) => stroke.primitiveId === action.primitiveId && stroke.view === action.view && stroke.closed,
  );
  if (strokeIndex < 0) return null;

  const stroke = strokes[strokeIndex];
  let repairedStroke: SketchStroke;
  let detail: string;

  if (action.kind === 'scaleViewAxis') {
    if (action.target <= 0 || action.current <= 0) return null;
    if (Math.abs(action.target - action.current) <= DIMENSION_TOLERANCE) return null;
    repairedStroke = scaleClosedStrokeAxisAroundCenter(stroke, action.axis, action.target);
    detail = `${action.view.toUpperCase()} ${action.axis.toUpperCase()} ${formatNumber(action.current)}MM -> ${formatNumber(action.target)}MM`;
  } else {
    const currentSize = action.currentMax - action.currentMin;
    const targetSize = action.targetMax - action.targetMin;
    if (!Number.isFinite(currentSize) || !Number.isFinite(targetSize)) return null;
    if (Math.abs(currentSize - targetSize) > DIMENSION_TOLERANCE) return null;
    if (Math.abs(action.currentMin - action.targetMin) <= DIMENSION_TOLERANCE) return null;
    repairedStroke = translateClosedStrokeAxisToRange(stroke, action.axis, action.targetMin);
    detail = `${action.view.toUpperCase()} ${action.axis.toUpperCase()} RANGE ${formatNumber(action.currentMin)}..${formatNumber(action.currentMax)}MM -> ${formatNumber(action.targetMin)}..${formatNumber(action.targetMax)}MM`;
  }

  const nextStrokes = strokes.map((candidate, index) => (index === strokeIndex ? repairedStroke : cloneStroke(candidate)));

  return {
    strokes: nextStrokes,
    repair: {
      ...action,
      detail,
    },
  };
}

function scaleClosedStrokeAxisAroundCenter(stroke: SketchStroke, axis: 'x' | 'y', targetDimension: number): SketchStroke {
  const bounds = closedStrokeBounds(stroke);
  const currentDimension = axis === 'x' ? bounds.width : bounds.height;
  if (currentDimension <= 0) return cloneStroke(stroke);

  const scale = targetDimension / currentDimension;
  const center = axis === 'x' ? (bounds.minX + bounds.maxX) / 2 : (bounds.minY + bounds.maxY) / 2;
  const axisIndex = axis === 'x' ? 0 : 1;
  const logicalCount = logicalPointCount(stroke);
  const points = stroke.points.slice(0, logicalCount).map((point) => {
    const nextPoint = copyPoint(point);
    nextPoint[axisIndex] = roundSketchCoordinate(center + (point[axisIndex] - center) * scale);
    return nextPoint;
  });
  points.push(copyPoint(points[0]));

  return {
    ...stroke,
    points,
  };
}

function translateClosedStrokeAxisToRange(stroke: SketchStroke, axis: 'x' | 'y', targetMin: number): SketchStroke {
  const bounds = closedStrokeBounds(stroke);
  const currentMin = axis === 'x' ? bounds.minX : bounds.minY;
  const delta = targetMin - currentMin;
  const axisIndex = axis === 'x' ? 0 : 1;
  const logicalCount = logicalPointCount(stroke);
  const points = stroke.points.slice(0, logicalCount).map((point) => {
    const nextPoint = copyPoint(point);
    nextPoint[axisIndex] = roundSketchCoordinate(point[axisIndex] + delta);
    return nextPoint;
  });
  points.push(copyPoint(points[0]));

  return {
    ...stroke,
    points,
  };
}

function cloneStrokes(strokes: SketchStroke[]): SketchStroke[] {
  return strokes.map(cloneStroke);
}

function cloneStroke(stroke: SketchStroke): SketchStroke {
  return {
    ...stroke,
    points: stroke.points.map(copyPoint),
    ...(stroke.dimensionLocks ? { dimensionLocks: { ...stroke.dimensionLocks } } : {}),
  };
}

function copyPoint(point: SketchPoint): SketchPoint {
  return [point[0], point[1]];
}

function roundSketchCoordinate(value: number): number {
  return Number(value.toFixed(4));
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(2).replace(/\.?0+$/, '');
}
