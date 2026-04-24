import type { SketchPoint, SketchStroke } from './sketchWorkspaceState';

export type SketchCleanupResult = { strokes: SketchStroke[]; evidence: string[] } | { error: string };

const NO_CLOSED_PROFILE_ERROR = 'Close profile before cleanup.';
const INVALID_BOUNDS_ERROR = 'Sketch cleanup needs a non-zero closed profile.';

export function cleanupSketchStrokes(strokes: SketchStroke[]): SketchCleanupResult {
  const strokeIndex = latestClosedStrokeIndex(strokes);
  if (strokeIndex === null) return { error: NO_CLOSED_PROFILE_ERROR };

  const stroke = strokes[strokeIndex];
  const bounds = strokeBounds(stroke);
  if (!bounds || bounds.width <= 0 || bounds.height <= 0) return { error: INVALID_BOUNDS_ERROR };

  const cleanedStroke: SketchStroke = {
    ...stroke,
    points: rectanglePoints(bounds),
    closed: true,
  };

  const nextStrokes = strokes.map((candidate, index) => (index === strokeIndex ? cleanedStroke : candidate));

  return {
    strokes: nextStrokes,
    evidence: [
      `${stroke.primitiveId} cleaned to rectangle width ${formatNumber(bounds.width)}mm height ${formatNumber(bounds.height)}mm.`,
    ],
  };
}

function latestClosedStrokeIndex(strokes: SketchStroke[]): number | null {
  for (let index = strokes.length - 1; index >= 0; index -= 1) {
    if (strokes[index]?.closed) return index;
  }
  return null;
}

function strokeBounds(stroke: SketchStroke): { minX: number; minY: number; maxX: number; maxY: number; width: number; height: number } | null {
  if (stroke.points.length === 0) return null;

  const xs = stroke.points.map(([x]) => x);
  const ys = stroke.points.map(([, y]) => y);
  const minX = Math.min(...xs);
  const minY = Math.min(...ys);
  const maxX = Math.max(...xs);
  const maxY = Math.max(...ys);
  const width = maxX - minX;
  const height = maxY - minY;

  if (![minX, minY, maxX, maxY, width, height].every(Number.isFinite)) return null;
  return { minX, minY, maxX, maxY, width, height };
}

function rectanglePoints(bounds: { minX: number; minY: number; maxX: number; maxY: number }): SketchPoint[] {
  return [
    [bounds.minX, bounds.minY],
    [bounds.maxX, bounds.minY],
    [bounds.maxX, bounds.maxY],
    [bounds.minX, bounds.maxY],
    [bounds.minX, bounds.minY],
  ];
}

function formatNumber(value: number): string {
  if (Number.isInteger(value)) return String(value);
  return value.toFixed(2).replace(/\.?0+$/, '');
}
