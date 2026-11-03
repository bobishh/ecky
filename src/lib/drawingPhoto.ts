import type {
  DrawingPrimitive,
  DrawingProfileCandidate,
  DrawingRecognitionResponse,
  DrawingScaleCalibration,
} from './tauri/contracts';

export type DrawingPoint = [number, number];

export function imagePointToViewMm(
  point: DrawingPoint,
  sourceToPreviewTransform: number[],
  millimetersPerPixel: number,
  previewHeightPx: number,
): DrawingPoint | null {
  if (sourceToPreviewTransform.length !== 9 || !Number.isFinite(millimetersPerPixel) || millimetersPerPixel <= 0) return null;
  const [x, y] = point;
  const u = sourceToPreviewTransform[0]! * x + sourceToPreviewTransform[1]! * y + sourceToPreviewTransform[2]!;
  const v = sourceToPreviewTransform[3]! * x + sourceToPreviewTransform[4]! * y + sourceToPreviewTransform[5]!;
  const w = sourceToPreviewTransform[6]! * x + sourceToPreviewTransform[7]! * y + sourceToPreviewTransform[8]!;
  if (!Number.isFinite(w) || Math.abs(w) < 1e-12) return null;
  return [u / w * millimetersPerPixel, (previewHeightPx - v / w) * millimetersPerPixel];
}

export function calibratedDistanceMm(
  calibration: DrawingScaleCalibration,
  result: Pick<DrawingRecognitionResponse, 'sourceToPreviewTransform' | 'millimetersPerPixel' | 'previewHeightPx'>,
): number | null {
  const start = imagePointToViewMm(calibration.startPx, result.sourceToPreviewTransform, result.millimetersPerPixel, result.previewHeightPx);
  const end = imagePointToViewMm(calibration.endPx, result.sourceToPreviewTransform, result.millimetersPerPixel, result.previewHeightPx);
  if (!start || !end) return null;
  return Math.hypot(end[0] - start[0], end[1] - start[1]);
}

export function drawingCandidateIsSelectable(
  candidate: DrawingProfileCandidate,
  primitives: DrawingPrimitive[],
): boolean {
  if (!candidate.closed || candidate.issues.length > 0 || candidate.primitiveIds.length === 0) return false;
  const primitiveIds = new Set(primitives.map((primitive) => primitive.id));
  return candidate.primitiveIds.every((id) => primitiveIds.has(id));
}

export function drawingCandidatesForApply(
  profiles: DrawingProfileCandidate[],
  selectedProfileIds: string[],
  primitives: DrawingPrimitive[],
): DrawingPrimitive[] | null {
  const selected = new Set(selectedProfileIds);
  const candidates = profiles.filter((profile) => selected.has(profile.id));
  if (!candidates.length || candidates.some((profile) => !drawingCandidateIsSelectable(profile, primitives))) return null;
  const outerIds = new Set(candidates.filter((profile) => profile.role === 'outer').map((profile) => profile.id));
  if (!outerIds.size) return null;
  if (candidates.some((profile) => profile.role === 'hole' && (!profile.parentId || !outerIds.has(profile.parentId)))) return null;
  const primitiveIds = new Set(candidates.flatMap((profile) => profile.primitiveIds));
  const geometry = primitives.filter((primitive) => primitiveIds.has(primitive.id));
  return geometry.length ? geometry.map(cloneDrawingPrimitive) : null;
}

export function cloneDrawingPrimitive(primitive: DrawingPrimitive): DrawingPrimitive {
  switch (primitive.kind) {
    case 'line':
      return { ...primitive, startMm: [...primitive.startMm], endMm: [...primitive.endMm] };
    case 'circle':
      return { ...primitive, centerMm: [...primitive.centerMm] };
    case 'polyline':
      return { ...primitive, pointsMm: primitive.pointsMm.map(([x, y]) => [x, y]) };
  }
}

export function validateDrawingCalibration(calibration: DrawingScaleCalibration | null): string | null {
  if (!calibration) return 'Select two scale points and enter their known distance.';
  if (![...calibration.startPx, ...calibration.endPx, calibration.distanceMm].every(Number.isFinite)) {
    return 'Scale calibration must contain finite values.';
  }
  if (Math.hypot(calibration.endPx[0] - calibration.startPx[0], calibration.endPx[1] - calibration.startPx[1]) < 1) {
    return 'Choose two distinct scale points at least one pixel apart.';
  }
  if (calibration.distanceMm <= 0) return 'Known distance must be greater than zero.';
  return null;
}
