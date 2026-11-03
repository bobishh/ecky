import assert from 'node:assert/strict';
import test from 'node:test';
import {
  normalizeProfileLoops,
  profileLoopArea,
  profileDrawingBounds,
  profileLoopError,
  profileGeometrySettingsMatch,
  profileLoopsForApply,
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
