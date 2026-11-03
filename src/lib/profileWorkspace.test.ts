import assert from 'node:assert/strict';
import test from 'node:test';
import {
  normalizeProfileLoops,
  profileLoopArea,
  profileDrawingBounds,
  profileDisplayCurveArcPath,
  profileBounds,
  profileLoopError,
  profileGeometrySettingsMatch,
  profileLoopsForApply,
  profileSketchLoopsForApply,
  profilePlacementForOperation,
  profileRequestIsCurrent,
  finishProfileViewRequest,
  profileReferenceForApply,
  profilePointFromPointer,
  removeProfileLoop,
  type ProfileLoop,
  type ProfileGeometryResponse,
} from './profileWorkspace';

const square: ProfileLoop = {
  points: [[0, 0], [20, 0], [20, 10], [0, 10]],
  role: 'outer',
  parentIndex: null,
};

test('profile canvas maps pointer coordinates to millimeters with an upward-positive Y axis', () => {
  assert.deepEqual(
    profilePointFromPointer(25, 10, { left: 0, top: 0, width: 100, height: 100 }, {
      minX: -50,
      minY: -25,
      width: 100,
      height: 50,
    }),
    [-25, 40],
  );
});

test('projected open edges define fit bounds without becoming a closed profile', () => {
  const bounds = profileDrawingBounds([[[-10, 0], [0, 0]], [[100, 20], [100, 40]]]);
  assert.ok(bounds);
  assert.equal(bounds.minX, -23.2);
  assert.equal(bounds.minY, -13.2);
  assert.equal(bounds.width, 136.4);
  assert.equal(bounds.height, 66.4);
});

test('profile loops normalize a repeated closing point and retain implicit closure', () => {
  const loops = normalizeProfileLoops([{ ...square, points: [...square.points, [0, 0]] }]);
  assert.equal(loops[0]?.points.length, 4);
  assert.equal(profileLoopArea(loops[0]!), 200);
  assert.equal(profileLoopError(loops), null);
});

test('normalizing a one-vertex draft does not mistake its only point for a closing duplicate', () => {
  assert.deepEqual(normalizeProfileLoops([{ ...square, points: [[-4, 9]] }]), [
    { points: [[-4, 9]], role: 'outer', parentIndex: null },
  ]);
});

test('profile validation rejects open rings with too few distinct vertices', () => {
  assert.equal(profileLoopError([{ ...square, points: [[0, 0], [4, 0]] }]), 'Profile 1 needs at least three vertices.');
});

test('analytic circles keep exact sketch geometry through normalization, bounds, and apply conversion', () => {
  const circle: ProfileLoop = {
    points: [[-5, 0], [0, -5], [5, 0], [0, 5]],
    analyticCircle: { kind: 'circle', centerMm: [0, 0], radiusMm: 5 },
    role: 'outer',
    parentIndex: null,
  };
  const normalized = normalizeProfileLoops([circle]);
  assert.deepEqual(normalized[0]?.analyticCircle, { kind: 'circle', centerMm: [0, 0], radiusMm: 5 });
  assert.equal(profileLoopArea(normalized[0]!), Math.PI * 25);
  const bounds = profileBounds(normalized);
  assert.equal(bounds.width, 12.4);
  assert.equal(bounds.height, 12.4);
  assert.ok(bounds.minX < -5);
  assert.ok(bounds.minY < -5);
  assert.deepEqual(profileSketchLoopsForApply(normalized), [{
    role: 'outer',
    parentIndex: null,
    geometry: { kind: 'circle', centerMm: [0, 0], radiusMm: 5 },
  }]);
  assert.equal(profileLoopError(normalized), null);
});

test('analytic projection curve bounds include circle and ellipse extents without sampled edges', () => {
  const bounds = profileDrawingBounds([], [
    { kind: 'circle', centerMm: [0, 0], radiusMm: 5 },
    { kind: 'ellipse', centerMm: [20, 10], majorRadiusMm: 8, minorRadiusMm: 4, rotationRad: Math.PI / 2 },
  ]);
  assert.ok(bounds);
  assert.ok(bounds.minX <= -5);
  assert.ok(bounds.minY <= -5);
  assert.ok(bounds.width >= 33);
  assert.ok(bounds.height >= 16);
});

test('projection arc paths preserve major sweeps and ellipse rotation', () => {
  const circleArc = profileDisplayCurveArcPath({
    kind: 'circularArc', centerMm: [0, 0], radiusMm: 3, startAngleRad: 0, sweepAngleRad: Math.PI * 1.5,
  });
  assert.match(circleArc, /^M 3 0 A 3 3 0 1 0 /);
  const ellipseArc = profileDisplayCurveArcPath({
    kind: 'ellipticalArc', centerMm: [1, 2], majorRadiusMm: 8, minorRadiusMm: 4,
    rotationRad: Math.PI / 4, startAngleRad: 0.2, sweepAngleRad: -1.1,
  });
  assert.match(ellipseArc, / A 8 4 -45 0 1 /);
});

test('profile evidence must be re-derived when operation or tolerance changes', () => {
  const section = { operation: { kind: 'section' as const, plane: 'xy' as const, offsetMm: 5 }, toleranceMm: 0.1 };
  assert.equal(profileGeometrySettingsMatch(section, { kind: 'section', plane: 'xy', offsetMm: 5 }, 0.1), true);
  assert.equal(profileGeometrySettingsMatch(section, { kind: 'section', plane: 'xz', offsetMm: 5 }, 0.1), false);
  assert.equal(profileGeometrySettingsMatch(section, { kind: 'section', plane: 'xy', offsetMm: 6 }, 0.1), false);
  assert.equal(profileGeometrySettingsMatch(section, { kind: 'section', plane: 'xy', offsetMm: 5 }, 0.2), false);
});

test('section and canonical projection initialize a matching feature placement plane', () => {
  assert.deepEqual(profilePlacementForOperation({ kind: 'section', plane: 'yz', offsetMm: 12 }), {
    plane: 'yz',
    offsetMm: 12,
  });
  assert.deepEqual(profilePlacementForOperation({ kind: 'projection', view: 'front' }), { plane: 'xz', offsetMm: 0 });
  assert.deepEqual(profilePlacementForOperation({ kind: 'projection', view: 'top' }), { plane: 'xy', offsetMm: 0 });
  assert.deepEqual(profilePlacementForOperation({ kind: 'projection', view: 'side' }), { plane: 'yz', offsetMm: 0 });
});

test('a late request cannot replace or clear a newer request for the same scope', () => {
  assert.equal(profileRequestIsCurrent('thread-v1', 4, 'thread-v1', 5), false);
  assert.equal(profileRequestIsCurrent('thread-v1', 5, 'thread-v1', 5), true);
  assert.equal(profileRequestIsCurrent('thread-v1', 4, 'thread-v2', 4), false);
});

test('a completed view request clears its own pending token after a scope change', () => {
  const stalePending = { scopeKey: 'thread-v1', requestId: 8 };
  assert.equal(finishProfileViewRequest(stalePending, 8), null);
  const newerPending = { scopeKey: 'thread-v2', requestId: 9 };
  assert.equal(finishProfileViewRequest(newerPending, 8), newerPending);
});

test('removing an outer loop removes its holes and remaps the remaining parents', () => {
  const loops: ProfileLoop[] = [
    square,
    { points: [[2, 2], [4, 2], [4, 4], [2, 4]], role: 'hole', parentIndex: 0 },
    { points: [[30, 0], [40, 0], [40, 10], [30, 10]], role: 'outer', parentIndex: null },
    { points: [[32, 2], [34, 2], [34, 4], [32, 4]], role: 'hole', parentIndex: 2 },
  ];
  assert.deepEqual(removeProfileLoop(loops, 0).map((loop) => [loop.role, loop.parentIndex]), [
    ['outer', null],
    ['hole', 0],
  ]);
});

test('exact display samples never become submitted loop geometry', () => {
  const evidence: ProfileGeometryResponse = {
    sourceArtifactPath: '/models/source.step',
    sourceArtifactDigest: 'source-digest',
    operation: { kind: 'section', plane: 'xy', offsetMm: 0 },
    toleranceMm: 0.1,
    units: 'mm',
    accuracy: 'exactBrep',
    profileAsset: { artifactPath: '/app/profiles/profile.step', artifactDigest: 'profile-digest' },
    loops: [square],
    displayAccuracy: 'approximateDisplayOnly',
    warnings: [],
  };
  assert.equal(profileLoopsForApply(evidence, false, [square]), undefined);
  assert.deepEqual(profileReferenceForApply(evidence, false), {
    sourceArtifactPath: '/models/source.step',
    sourceArtifactDigest: 'source-digest',
    operation: { kind: 'section', plane: 'xy', offsetMm: 0 },
    toleranceMm: 0.1,
    accuracy: 'exactBrep',
    profileAsset: { artifactPath: '/app/profiles/profile.step', artifactDigest: 'profile-digest' },
  });
});

test('explicit exact-to-polygon conversion submits editable loops as approximate', () => {
  const evidence: ProfileGeometryResponse = {
    sourceArtifactPath: '/models/source.step',
    sourceArtifactDigest: 'source-digest',
    operation: { kind: 'projection', view: 'front' },
    toleranceMm: 0.2,
    units: 'mm',
    accuracy: 'exactBrep',
    profileAsset: { artifactPath: '/app/profiles/profile.step', artifactDigest: 'profile-digest' },
    loops: [square],
    displayAccuracy: 'approximateDisplayOnly',
    warnings: [],
  };
  assert.deepEqual(profileLoopsForApply(evidence, true, [square]), [square]);
  assert.deepEqual(profileReferenceForApply(evidence, true), {
    sourceArtifactPath: '/models/source.step',
    sourceArtifactDigest: 'source-digest',
    operation: { kind: 'projection', view: 'front' },
    toleranceMm: 0.2,
    accuracy: 'approximatePolygon',
    profileAsset: null,
  });
});
