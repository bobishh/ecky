import type {
  ProfileAsset,
  ProfileDerivationOperation,
  ProfileEvidenceAccuracy,
  ProfileFeatureApplyRequest,
  ProfileGeometryRequest,
  ProfileGeometryResponse as ContractProfileGeometryResponse,
  ProfileLoop as ContractProfileLoop,
  ProfileLoopRole as ContractProfileLoopRole,
  ProfilePlane as ContractProfilePlane,
  ProfileDrawingProjectionResponse as ContractProfileDrawingProjectionResponse,
  ProjectProfileDrawingRequest,
} from './tauri/contracts';

export type ProfilePlane = ContractProfilePlane;
export type ProfileLoopRole = ContractProfileLoopRole;
export type ProfilePoint = ContractProfileLoop['points'][number];
export type ProfileLoop = ContractProfileLoop;
export type ProfileGeometryOperation = ProfileDerivationOperation;
export type ProfileGeometryAccuracy = ProfileEvidenceAccuracy;
export type ProfileAssetReference = ProfileAsset;
export type ProfileGeometryResponse = ContractProfileGeometryResponse;
export type ProfileDrawingProjectionResponse = ContractProfileDrawingProjectionResponse;
export type ProfileDrawingInput = Pick<ProjectProfileDrawingRequest, 'view' | 'toleranceMm'>;
export type ProfileDeriveInput = Pick<ProfileGeometryRequest, 'operation' | 'toleranceMm'> & {
  source: 'step' | 'mesh';
};
export type ProfileApplyReference = NonNullable<ProfileFeatureApplyRequest['reference']>;
export type ProfileApplyInput = Pick<
  ProfileFeatureApplyRequest,
  'loops' | 'plane' | 'offsetMm' | 'clearanceMm' | 'depthMm' | 'partName' | 'reference' | 'drawingProfile'
>;
export type ProfileApplyOutcome = { status: 'saved' } | { status: 'error'; message: string };

export type ProfileBounds = { minX: number; minY: number; width: number; height: number };
export type ProfileViewPendingRequest = { scopeKey: string; requestId: number } | null;

export function profilePlacementForOperation(
  operation: ProfileGeometryOperation,
): { plane: ProfilePlane; offsetMm: number } {
  if (operation.kind === 'section') {
    return { plane: operation.plane, offsetMm: operation.offsetMm };
  }
  const plane: Record<'front' | 'top' | 'side', ProfilePlane> = {
    front: 'xz',
    top: 'xy',
    side: 'yz',
  };
  return { plane: plane[operation.view], offsetMm: 0 };
}

export function profileRequestIsCurrent(
  requestScope: string,
  requestId: number,
  currentScope: string,
  activeRequestId: number | null,
): boolean {
  return requestScope === currentScope && requestId === activeRequestId;
}

export function finishProfileViewRequest(
  pending: ProfileViewPendingRequest,
  completedRequestId: number,
): ProfileViewPendingRequest {
  return pending?.requestId === completedRequestId ? null : pending;
}

export function profileGeometrySettingsMatch(
  response: Pick<ProfileGeometryResponse, 'operation' | 'toleranceMm'>,
  operation: ProfileGeometryOperation,
  toleranceMm: number,
): boolean {
  if (response.toleranceMm !== toleranceMm || response.operation.kind !== operation.kind) return false;
  if (response.operation.kind === 'section' && operation.kind === 'section') {
    return response.operation.plane === operation.plane
      && response.operation.offsetMm === operation.offsetMm;
  }
  return response.operation.kind === 'projection'
    && operation.kind === 'projection'
    && response.operation.view === operation.view;
}

export function profileLoopsForApply(
  evidence: ProfileGeometryResponse | null,
  convertedToApproximate: boolean,
  editableLoops: ProfileLoop[],
): ProfileLoop[] | undefined {
  if (evidence?.accuracy === 'exactBrep' && !convertedToApproximate) return undefined;
  return normalizeProfileLoops(editableLoops);
}

export function profileReferenceForApply(
  evidence: ProfileGeometryResponse | null,
  convertedToApproximate: boolean,
): ProfileApplyReference | null {
  if (!evidence) return null;
  return {
    sourceArtifactPath: evidence.sourceArtifactPath,
    sourceArtifactDigest: evidence.sourceArtifactDigest,
    operation: evidence.operation,
    toleranceMm: evidence.toleranceMm,
    accuracy: convertedToApproximate ? 'approximatePolygon' : evidence.accuracy,
    profileAsset: convertedToApproximate ? null : evidence.profileAsset ?? null,
  };
}

export function removeProfileLoop(loops: ProfileLoop[], removeIndex: number): ProfileLoop[] {
  const removed = loops[removeIndex];
  if (!removed) return loops.map(cloneProfileLoop);

  const removedIndices = new Set([removeIndex]);
  if (removed.role === 'outer') {
    loops.forEach((loop, index) => {
      if (loop.role === 'hole' && loop.parentIndex === removeIndex) removedIndices.add(index);
    });
  }

  const nextIndex = new Map<number, number>();
  let indexAfterRemoval = 0;
  loops.forEach((_, index) => {
    if (removedIndices.has(index)) return;
    nextIndex.set(index, indexAfterRemoval);
    indexAfterRemoval += 1;
  });

  return loops.flatMap((loop, index) => {
    if (removedIndices.has(index)) return [];
    if (loop.role === 'outer') return [cloneProfileLoop(loop)];
    const parentIndex = loop.parentIndex === null ? null : nextIndex.get(loop.parentIndex) ?? null;
    return parentIndex === null ? [] : [{ ...cloneProfileLoop(loop), parentIndex }];
  });
}

export function profileBounds(loops: ProfileLoop[]): ProfileBounds {
  return profileBoundsFromPoints(loops.flatMap((loop) => loop.points));
}

export function profileDrawingBounds(displayEdges: ProfileDrawingProjectionResponse['displayEdges']): ProfileBounds | null {
  const points = displayEdges.flat();
  return points.length ? profileBoundsFromPoints(points) : null;
}

function profileBoundsFromPoints(rawPoints: ProfilePoint[]): ProfileBounds {
  const points = rawPoints.filter(isFinitePoint);
  if (!points.length) return { minX: -50, minY: -50, width: 100, height: 100 };

  const extents = points.reduce(
    (current, [x, y]) => ({
      minX: Math.min(current.minX, x),
      minY: Math.min(current.minY, y),
      maxX: Math.max(current.maxX, x),
      maxY: Math.max(current.maxY, y),
    }),
    { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity },
  );
  const spanX = extents.maxX - extents.minX;
  const spanY = extents.maxY - extents.minY;
  const padding = Math.max(spanX, spanY, 10) * 0.12;
  return {
    minX: extents.minX - padding,
    minY: extents.minY - padding,
    width: Math.max(spanX + padding * 2, 10),
    height: Math.max(spanY + padding * 2, 10),
  };
}

export function profilePointFromPointer(
  clientX: number,
  clientY: number,
  rect: Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>,
  bounds: ProfileBounds,
): ProfilePoint {
  const scale = Math.min(
    rect.width > 0 ? rect.width / bounds.width : 0,
    rect.height > 0 ? rect.height / bounds.height : 0,
  );
  const renderedWidth = bounds.width * scale;
  const renderedHeight = bounds.height * scale;
  const insetX = (rect.width - renderedWidth) / 2;
  const insetY = (rect.height - renderedHeight) / 2;
  const ratioX = renderedWidth > 0 ? (clientX - rect.left - insetX) / renderedWidth : 0;
  const ratioY = renderedHeight > 0 ? (clientY - rect.top - insetY) / renderedHeight : 0;
  return [bounds.minX + ratioX * bounds.width, bounds.minY + (1 - ratioY) * bounds.height];
}

export function profileLoopArea(loop: ProfileLoop): number {
  let area = 0;
  for (let index = 0; index < loop.points.length; index += 1) {
    const current = loop.points[index];
    const next = loop.points[(index + 1) % loop.points.length];
    if (!current || !next) continue;
    area += current[0] * next[1] - next[0] * current[1];
  }
  return area / 2;
}

export function normalizeProfileLoops(loops: ProfileLoop[]): ProfileLoop[] {
  return loops.map((loop) => {
    const points = loop.points
      .filter(isFinitePoint)
      .map(([x, y]) => [x, y] as ProfilePoint);
    const first = points[0];
    const last = points.at(-1);
    if (points.length > 2 && first && last && nearlyEqual(first[0], last[0]) && nearlyEqual(first[1], last[1])) {
      points.pop();
    }
    return {
      points,
      role: loop.role,
      parentIndex: loop.role === 'hole' ? loop.parentIndex : null,
    };
  });
}

export function profileLoopError(loops: ProfileLoop[]): string | null {
  if (!loops.length) return 'Draw or extract a closed profile first.';
  const normalized = normalizeProfileLoops(loops);
  for (const [index, loop] of normalized.entries()) {
    if (loop.points.length < 3) return `Profile ${index + 1} needs at least three vertices.`;
    if (loop.points.some((point) => !isFinitePoint(point))) return `Profile ${index + 1} has a non-finite vertex.`;
    if (Math.abs(profileLoopArea(loop)) < 1e-8) return `Profile ${index + 1} has no measurable area.`;
    if (loop.role === 'hole') {
      const parent = loop.parentIndex === null ? null : normalized[loop.parentIndex];
      if (!parent || parent.role !== 'outer' || loop.parentIndex === index) {
        return `Hole ${index + 1} needs an outer profile.`;
      }
      const firstPoint = loop.points[0];
      if (firstPoint && !pointInsideProfile(firstPoint, parent)) {
        return `Hole ${index + 1} must sit inside its outer profile.`;
      }
    }
  }
  return null;
}

export function pointInsideProfile(point: ProfilePoint, loop: ProfileLoop): boolean {
  let inside = false;
  const [x, y] = point;
  for (let index = 0, previous = loop.points.length - 1; index < loop.points.length; previous = index, index += 1) {
    const currentPoint = loop.points[index];
    const previousPoint = loop.points[previous];
    if (!currentPoint || !previousPoint) continue;
    const [currentX, currentY] = currentPoint;
    const [previousX, previousY] = previousPoint;
    const crosses = currentY > y !== previousY > y
      && x < ((previousX - currentX) * (y - currentY)) / (previousY - currentY || Number.EPSILON) + currentX;
    if (crosses) inside = !inside;
  }
  return inside;
}

function isFinitePoint(point: ProfilePoint): boolean {
  return Number.isFinite(point[0]) && Number.isFinite(point[1]);
}

function cloneProfileLoop(loop: ProfileLoop): ProfileLoop {
  return {
    points: loop.points.map(([x, y]) => [x, y]),
    role: loop.role,
    parentIndex: loop.parentIndex,
  };
}

function nearlyEqual(left: number, right: number): boolean {
  return Math.abs(left - right) <= 1e-7;
}
