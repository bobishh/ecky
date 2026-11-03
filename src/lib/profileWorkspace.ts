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
  ProfileDisplayCurve as ContractProfileDisplayCurve,
  ProfileSketchGeometry as ContractProfileSketchGeometry,
  ProfileSketchLoop as ContractProfileSketchLoop,
  ProfileDrawingVisibility as ContractProfileDrawingVisibility,
  ProjectProfileDrawingRequest,
} from './tauri/contracts';

export type ProfilePlane = ContractProfilePlane;
export type ProfileLoopRole = ContractProfileLoopRole;
export type ProfilePoint = ContractProfileLoop['points'][number];
export type ProfileCircleGeometry = Extract<ContractProfileSketchGeometry, { kind: 'circle' }>;
export type ProfileLoop = ContractProfileLoop & { analyticCircle?: ProfileCircleGeometry };
export type ProfileGeometryOperation = ProfileDerivationOperation;
export type ProfileGeometryAccuracy = ProfileEvidenceAccuracy;
export type ProfileAssetReference = ProfileAsset;
export type ProfileGeometryResponse = ContractProfileGeometryResponse;
export type ProfileDrawingProjectionResponse = ContractProfileDrawingProjectionResponse;
export type ProfileDisplayCurve = ContractProfileDisplayCurve;
export type ProfileSketchLoop = ContractProfileSketchLoop;
export type ProfileDrawingVisibility = ContractProfileDrawingVisibility;
export type ProfileDrawingInput = Pick<ProjectProfileDrawingRequest, 'view' | 'toleranceMm'> & {
  visibility: ProfileDrawingVisibility;
};
export type ProfileDeriveInput = Pick<ProfileGeometryRequest, 'operation' | 'toleranceMm'> & {
  source: 'step' | 'mesh';
};
export type ProfileApplyReference = NonNullable<ProfileFeatureApplyRequest['reference']>;
export type ProfileApplyInput = Pick<
  ProfileFeatureApplyRequest,
  'loops' | 'sketchLoops' | 'plane' | 'offsetMm' | 'clearanceMm' | 'depthMm' | 'partName' | 'reference' | 'drawingProfile'
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

export function profileSketchLoopsForApply(editableLoops: ProfileLoop[]): ProfileSketchLoop[] | undefined {
  if (!editableLoops.some((loop) => loop.analyticCircle)) return undefined;
  return normalizeProfileLoops(editableLoops).map((loop) => ({
    role: loop.role,
    parentIndex: loop.parentIndex,
    geometry: loop.analyticCircle
      ? { kind: 'circle', centerMm: loop.analyticCircle.centerMm, radiusMm: loop.analyticCircle.radiusMm }
      : { kind: 'polygon', pointsMm: loop.points },
  }));
}

export function profileDisplayCurveArcPath(
  curve: Extract<ProfileDisplayCurve, { kind: 'circularArc' | 'ellipticalArc' }>,
): string {
  const ellipse = curve.kind === 'ellipticalArc';
  const rotation = ellipse ? curve.rotationRad : 0;
  const majorRadius = ellipse ? curve.majorRadiusMm : curve.radiusMm;
  const minorRadius = ellipse ? curve.minorRadiusMm : curve.radiusMm;
  const pointAt = (angle: number): ProfilePoint => {
    const x = majorRadius * Math.cos(angle);
    const y = minorRadius * Math.sin(angle);
    return [
      curve.centerMm[0] + x * Math.cos(rotation) - y * Math.sin(rotation),
      curve.centerMm[1] + x * Math.sin(rotation) + y * Math.cos(rotation),
    ];
  };
  const start = pointAt(curve.startAngleRad);
  const end = pointAt(curve.startAngleRad + curve.sweepAngleRad);
  const largeArcFlag = Math.abs(curve.sweepAngleRad) > Math.PI ? 1 : 0;
  const sweepFlag = curve.sweepAngleRad < 0 ? 1 : 0;
  const rotationDegrees = (rotation * -180) / Math.PI;
  return `M ${svgNumber(start[0])} ${svgNumber(-start[1])} A ${svgNumber(majorRadius)} ${svgNumber(minorRadius)} ${svgNumber(rotationDegrees)} ${largeArcFlag} ${sweepFlag} ${svgNumber(end[0])} ${svgNumber(-end[1])}`;
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
  return profileBoundsFromPoints(loops.flatMap((loop) => loop.analyticCircle
    ? circleBoundsPoints(loop.analyticCircle.centerMm, loop.analyticCircle.radiusMm)
    : loop.points));
}

export function profileDrawingBounds(
  displayEdges: ProfileDrawingProjectionResponse['displayEdges'],
  displayCurves: ProfileDisplayCurve[] = [],
): ProfileBounds | null {
  const points = [...displayEdges.flat(), ...displayCurves.flatMap(displayCurveBoundsPoints)];
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
  if (loop.analyticCircle) {
    const { radiusMm } = loop.analyticCircle;
    const area = Math.PI * radiusMm * radiusMm;
    return loop.role === 'hole' ? -area : area;
  }
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
      ...(loop.analyticCircle ? { analyticCircle: {
        kind: 'circle' as const,
        centerMm: [loop.analyticCircle.centerMm[0], loop.analyticCircle.centerMm[1]] as ProfilePoint,
        radiusMm: loop.analyticCircle.radiusMm,
      } } : {}),
    };
  });
}

export function profileLoopError(loops: ProfileLoop[]): string | null {
  if (!loops.length) return 'Draw or extract a closed profile first.';
  const normalized = normalizeProfileLoops(loops);
  for (const [index, loop] of normalized.entries()) {
    if (loop.analyticCircle) {
      const { centerMm, radiusMm } = loop.analyticCircle;
      if (!Number.isFinite(centerMm[0]) || !Number.isFinite(centerMm[1]) || !Number.isFinite(radiusMm) || radiusMm <= 0) {
        return `Circle ${index + 1} needs a finite center and a radius greater than zero.`;
      }
    } else if (loop.points.length < 3) {
      return `Profile ${index + 1} needs at least three vertices.`;
    }
    if (loop.points.some((point) => !isFinitePoint(point))) return `Profile ${index + 1} has a non-finite vertex.`;
    if (Math.abs(profileLoopArea(loop)) < 1e-8) return `Profile ${index + 1} has no measurable area.`;
    if (loop.role === 'hole') {
      const parent = loop.parentIndex === null ? null : normalized[loop.parentIndex];
      if (!parent || parent.role !== 'outer' || loop.parentIndex === index) {
        return `Hole ${index + 1} needs an outer profile.`;
      }
      if (!profileLoopContainedBy(loop, parent)) {
        return `Hole ${index + 1} must sit inside its outer profile.`;
      }
    }
  }
  return null;
}

function profileLoopContainedBy(inner: ProfileLoop, outer: ProfileLoop): boolean {
  if (inner.analyticCircle && outer.analyticCircle) {
    const centerDistance = Math.hypot(
      inner.analyticCircle.centerMm[0] - outer.analyticCircle.centerMm[0],
      inner.analyticCircle.centerMm[1] - outer.analyticCircle.centerMm[1],
    );
    return centerDistance + inner.analyticCircle.radiusMm <= outer.analyticCircle.radiusMm + 1e-8;
  }
  const innerPoints = inner.analyticCircle
    ? profileCirclePoints(inner.analyticCircle.centerMm, inner.analyticCircle.radiusMm)
    : inner.points;
  if (outer.analyticCircle) {
    return innerPoints.every((point) => {
      const distance = Math.hypot(point[0] - outer.analyticCircle!.centerMm[0], point[1] - outer.analyticCircle!.centerMm[1]);
      return distance <= outer.analyticCircle!.radiusMm + 1e-8;
    });
  }
  return innerPoints.every((point) => pointInsideProfile(point, outer));
}

export function pointInsideProfile(point: ProfilePoint, loop: ProfileLoop): boolean {
  if (loop.analyticCircle) {
    const [centerX, centerY] = loop.analyticCircle.centerMm;
    return Math.hypot(point[0] - centerX, point[1] - centerY) < loop.analyticCircle.radiusMm;
  }
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
    ...(loop.analyticCircle ? { analyticCircle: {
      kind: 'circle',
      centerMm: [loop.analyticCircle.centerMm[0], loop.analyticCircle.centerMm[1]],
      radiusMm: loop.analyticCircle.radiusMm,
    } } : {}),
  };
}

export function profileCirclePoints(centerMm: ProfilePoint, radiusMm: number, count = 64): ProfilePoint[] {
  return Array.from({ length: count }, (_, index) => {
    const angle = (Math.PI * 2 * index) / count;
    return [centerMm[0] + radiusMm * Math.cos(angle), centerMm[1] + radiusMm * Math.sin(angle)];
  });
}

function circleBoundsPoints(centerMm: ProfilePoint, radiusMm: number): ProfilePoint[] {
  return [
    [centerMm[0] - radiusMm, centerMm[1]],
    [centerMm[0], centerMm[1] - radiusMm],
    [centerMm[0] + radiusMm, centerMm[1]],
    [centerMm[0], centerMm[1] + radiusMm],
  ];
}

function displayCurveBoundsPoints(curve: ProfileDisplayCurve): ProfilePoint[] {
  if (curve.kind === 'line') return [curve.startMm, curve.endMm];
  const steps = 64;
  if (curve.kind === 'circle') return circleBoundsPoints(curve.centerMm, curve.radiusMm);
  const isEllipse = curve.kind === 'ellipse' || curve.kind === 'ellipticalArc';
  const startAngle = curve.kind === 'circularArc' || curve.kind === 'ellipticalArc' ? curve.startAngleRad : 0;
  const sweep = curve.kind === 'circularArc' || curve.kind === 'ellipticalArc' ? curve.sweepAngleRad : Math.PI * 2;
  const rotation = isEllipse ? curve.rotationRad : 0;
  const majorRadius = isEllipse ? curve.majorRadiusMm : curve.radiusMm;
  const minorRadius = isEllipse ? curve.minorRadiusMm : curve.radiusMm;
  return Array.from({ length: steps + 1 }, (_, index) => {
    const angle = startAngle + (sweep * index) / steps;
    const x = majorRadius * Math.cos(angle);
    const y = minorRadius * Math.sin(angle);
    const cos = Math.cos(rotation);
    const sin = Math.sin(rotation);
    return [curve.centerMm[0] + x * cos - y * sin, curve.centerMm[1] + x * sin + y * cos];
  });
}

function nearlyEqual(left: number, right: number): boolean {
  return Math.abs(left - right) <= 1e-7;
}

function svgNumber(value: number): string {
  if (Math.abs(value) < 1e-12) return '0';
  return Number(value.toFixed(9)).toString();
}
