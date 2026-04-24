export type SketchFitStatus = 'pass' | 'fail' | 'pending';

export interface SketchPoint {
  x: number;
  y: number;
}

export interface SketchView {
  width: number;
  height: number;
}

export interface SketchArtifactEvidence {
  previewArtifactPath?: string;
  previewArtifactUrl?: string;
  source?: string;
}

export interface SketchFitValidationInput {
  profilePoints: SketchPoint[];
  view: SketchView;
  extrudeDepth: number;
  artifactEvidence?: SketchArtifactEvidence;
  backendError?: unknown;
  tolerance?: number;
}

export type SketchFitRowId = 'containment' | 'dimensions' | 'previewArtifact';

export interface SketchFitValidationRow {
  id: SketchFitRowId;
  label: string;
  status: SketchFitStatus;
  message: string;
}

export interface SketchFitValidationSeed {
  status: SketchFitStatus;
  rows: SketchFitValidationRow[];
  evidence: {
    containment: SketchContainmentEvidence;
    dimensions: SketchDimensionEvidence;
    previewArtifact: SketchPreviewArtifactEvidence;
    backendError?: unknown;
  };
}

export interface SketchContainmentEvidence {
  closedProfile: boolean;
  centroid: SketchPoint | null;
  centroidInsideProfile: boolean;
  edgeSafeSamples: SketchPoint[];
  edgeSafeSamplesInsideProfile: boolean;
  pointsWithinView: boolean;
}

export interface SketchDimensionEvidence {
  width: number;
  height: number;
  depth: number;
  tolerance: number;
  positiveAxes: {
    width: boolean;
    height: boolean;
    depth: boolean;
  };
}

export interface SketchPreviewArtifactEvidence {
  previewArtifactPath?: string;
  previewArtifactUrl?: string;
  source?: string;
}

const DEFAULT_TOLERANCE = 0.001;
const EDGE_SAMPLE_INSET_RATIO = 0.01;

export function buildSketchFitValidationSeed(
  input: SketchFitValidationInput,
): SketchFitValidationSeed {
  const tolerance = Math.max(input.tolerance ?? DEFAULT_TOLERANCE, 0);
  const polygon = normalizeClosedProfile(input.profilePoints, tolerance);
  const bounds = getBounds(polygon);
  const containment = buildContainmentEvidence(input, polygon, bounds, tolerance);
  const dimensions = buildDimensionEvidence(bounds, input.extrudeDepth, tolerance);
  const previewArtifact = buildPreviewArtifactEvidence(input.artifactEvidence);

  const rows: SketchFitValidationRow[] = [
    buildContainmentRow(containment),
    buildDimensionRow(dimensions),
    buildPreviewArtifactRow(previewArtifact, input.backendError),
  ];

  return {
    status: aggregateStatus(rows),
    rows,
    evidence: {
      containment,
      dimensions,
      previewArtifact,
      ...(input.backendError === undefined ? {} : { backendError: input.backendError }),
    },
  };
}

function buildContainmentEvidence(
  input: SketchFitValidationInput,
  polygon: SketchPoint[],
  bounds: SketchBounds,
  tolerance: number,
): SketchContainmentEvidence {
  const closedProfile = isClosedProfile(input.profilePoints, tolerance) && polygon.length >= 3;
  const centroid = closedProfile ? polygonCentroid(polygon) : null;
  const edgeSafeSamples = centroid ? buildEdgeSafeSamples(polygon, centroid, bounds, tolerance) : [];
  const centroidInsideProfile = centroid ? pointInPolygon(centroid, polygon, tolerance) : false;
  const edgeSafeSamplesInsideProfile = edgeSafeSamples.length > 0
    && edgeSafeSamples.every((sample) => pointInPolygon(sample, polygon, tolerance));

  return {
    closedProfile,
    centroid,
    centroidInsideProfile,
    edgeSafeSamples,
    edgeSafeSamplesInsideProfile,
    pointsWithinView: input.profilePoints.every((point) => pointWithinView(point, input.view, tolerance)),
  };
}

function buildDimensionEvidence(
  bounds: SketchBounds,
  depth: number,
  tolerance: number,
): SketchDimensionEvidence {
  const width = Number.isFinite(bounds.width) ? bounds.width : 0;
  const height = Number.isFinite(bounds.height) ? bounds.height : 0;

  return {
    width,
    height,
    depth,
    tolerance,
    positiveAxes: {
      width: width > tolerance,
      height: height > tolerance,
      depth: depth > tolerance,
    },
  };
}

function buildPreviewArtifactEvidence(
  artifactEvidence: SketchArtifactEvidence | undefined,
): SketchPreviewArtifactEvidence {
  return {
    ...(artifactEvidence?.previewArtifactPath ? { previewArtifactPath: artifactEvidence.previewArtifactPath } : {}),
    ...(artifactEvidence?.previewArtifactUrl ? { previewArtifactUrl: artifactEvidence.previewArtifactUrl } : {}),
    ...(artifactEvidence?.source ? { source: artifactEvidence.source } : {}),
  };
}

function buildContainmentRow(evidence: SketchContainmentEvidence): SketchFitValidationRow {
  const passed = evidence.closedProfile
    && evidence.centroidInsideProfile
    && evidence.edgeSafeSamplesInsideProfile
    && evidence.pointsWithinView;

  return {
    id: 'containment',
    label: 'Containment',
    status: passed ? 'pass' : 'fail',
    message: passed
      ? 'Closed profile samples stay inside source profile and view.'
      : 'Closed profile containment failed for source profile or view.',
  };
}

function buildDimensionRow(evidence: SketchDimensionEvidence): SketchFitValidationRow {
  const passed = evidence.positiveAxes.width && evidence.positiveAxes.height && evidence.positiveAxes.depth;

  return {
    id: 'dimensions',
    label: 'Dimensions and tolerance',
    status: passed ? 'pass' : 'fail',
    message: passed
      ? 'Width height and depth exceed tolerance.'
      : 'Width height or depth is not positive beyond tolerance.',
  };
}

function buildPreviewArtifactRow(
  evidence: SketchPreviewArtifactEvidence,
  backendError: unknown,
): SketchFitValidationRow {
  if (backendError !== undefined) {
    return {
      id: 'previewArtifact',
      label: 'Preview artifact',
      status: 'fail',
      message: formatBackendError(backendError),
    };
  }

  const hasPreview = Boolean(evidence.previewArtifactPath || evidence.previewArtifactUrl);

  return {
    id: 'previewArtifact',
    label: 'Preview artifact',
    status: hasPreview ? 'pass' : 'pending',
    message: hasPreview ? 'Preview artifact evidence present.' : 'Preview artifact evidence pending.',
  };
}

function aggregateStatus(rows: SketchFitValidationRow[]): SketchFitStatus {
  if (rows.some((row) => row.status === 'fail')) return 'fail';
  if (rows.some((row) => row.status === 'pending')) return 'pending';
  return 'pass';
}

function normalizeClosedProfile(points: SketchPoint[], tolerance: number): SketchPoint[] {
  if (points.length === 0) return [];
  const last = points[points.length - 1];
  if (last && pointsEqual(points[0], last, tolerance)) {
    return points.slice(0, -1);
  }
  return points.slice();
}

function isClosedProfile(points: SketchPoint[], tolerance: number): boolean {
  const first = points[0];
  const last = points[points.length - 1];
  return points.length >= 4 && first !== undefined && last !== undefined && pointsEqual(first, last, tolerance);
}

function pointsEqual(a: SketchPoint | undefined, b: SketchPoint | undefined, tolerance: number): boolean {
  if (!a || !b) return false;
  return Math.abs(a.x - b.x) <= tolerance && Math.abs(a.y - b.y) <= tolerance;
}

interface SketchBounds {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
  width: number;
  height: number;
}

function getBounds(points: SketchPoint[]): SketchBounds {
  if (points.length === 0) {
    return { minX: 0, maxX: 0, minY: 0, maxY: 0, width: 0, height: 0 };
  }

  const xs = points.map((point) => point.x);
  const ys = points.map((point) => point.y);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);

  return {
    minX,
    maxX,
    minY,
    maxY,
    width: maxX - minX,
    height: maxY - minY,
  };
}

function polygonCentroid(points: SketchPoint[]): SketchPoint {
  let twiceArea = 0;
  let x = 0;
  let y = 0;

  for (let index = 0; index < points.length; index += 1) {
    const current = points[index];
    const next = points[(index + 1) % points.length];
    if (!current || !next) continue;
    const cross = current.x * next.y - next.x * current.y;
    twiceArea += cross;
    x += (current.x + next.x) * cross;
    y += (current.y + next.y) * cross;
  }

  if (Math.abs(twiceArea) <= Number.EPSILON) {
    return averagePoint(points);
  }

  return {
    x: x / (3 * twiceArea),
    y: y / (3 * twiceArea),
  };
}

function averagePoint(points: SketchPoint[]): SketchPoint {
  const total = points.reduce(
    (acc, point) => ({ x: acc.x + point.x, y: acc.y + point.y }),
    { x: 0, y: 0 },
  );

  return {
    x: total.x / points.length,
    y: total.y / points.length,
  };
}

function buildEdgeSafeSamples(
  points: SketchPoint[],
  centroid: SketchPoint,
  bounds: SketchBounds,
  tolerance: number,
): SketchPoint[] {
  const inset = Math.max(Math.max(bounds.width, bounds.height) * EDGE_SAMPLE_INSET_RATIO, tolerance);

  return points.map((current, index) => {
    const next = points[(index + 1) % points.length] ?? current;
    const midpoint = {
      x: (current.x + next.x) / 2,
      y: (current.y + next.y) / 2,
    };
    return moveToward(midpoint, centroid, inset);
  });
}

function moveToward(from: SketchPoint, to: SketchPoint, distance: number): SketchPoint {
  const deltaX = to.x - from.x;
  const deltaY = to.y - from.y;
  const length = Math.hypot(deltaX, deltaY);
  if (length === 0) return from;

  const step = Math.min(distance, length);
  return {
    x: from.x + (deltaX / length) * step,
    y: from.y + (deltaY / length) * step,
  };
}

function pointWithinView(point: SketchPoint, view: SketchView, tolerance: number): boolean {
  return point.x >= -tolerance
    && point.y >= -tolerance
    && point.x <= view.width + tolerance
    && point.y <= view.height + tolerance;
}

function pointInPolygon(point: SketchPoint, polygon: SketchPoint[], tolerance: number): boolean {
  let inside = false;

  for (let index = 0, previousIndex = polygon.length - 1; index < polygon.length; previousIndex = index, index += 1) {
    const current = polygon[index];
    const previous = polygon[previousIndex];
    if (!current || !previous) continue;

    if (pointOnSegment(point, previous, current, tolerance)) return true;

    const intersects = (current.y > point.y) !== (previous.y > point.y)
      && point.x < ((previous.x - current.x) * (point.y - current.y)) / (previous.y - current.y) + current.x;

    if (intersects) inside = !inside;
  }

  return inside;
}

function pointOnSegment(point: SketchPoint, a: SketchPoint, b: SketchPoint, tolerance: number): boolean {
  const cross = (point.y - a.y) * (b.x - a.x) - (point.x - a.x) * (b.y - a.y);
  if (Math.abs(cross) > tolerance) return false;

  const dot = (point.x - a.x) * (b.x - a.x) + (point.y - a.y) * (b.y - a.y);
  if (dot < -tolerance) return false;

  const squaredLength = (b.x - a.x) ** 2 + (b.y - a.y) ** 2;
  return dot <= squaredLength + tolerance;
}

function formatBackendError(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;

  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}
