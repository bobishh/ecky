import assert from 'node:assert/strict';
import test from 'node:test';
import {
  calibratedDistanceMm,
  drawingCandidatesForApply,
  drawingCandidateIsSelectable,
  imagePointToViewMm,
  validateDrawingCalibration,
} from './drawingPhoto';
import type { DrawingPrimitive, DrawingProfileCandidate, DrawingRecognitionResponse } from './tauri/contracts';

const primitives: DrawingPrimitive[] = [
  { kind: 'line', id: 'a', startMm: [0, 0], endMm: [80, 0], residualMm: 0.1 },
  { kind: 'circle', id: 'hole', centerMm: [40, 25], radiusMm: 5, residualMm: 0.1 },
];
const profiles: DrawingProfileCandidate[] = [
  { id: 'outer', primitiveIds: ['a'], role: 'outer', parentId: null, closed: true, residualMm: 0.1, issues: [] },
  { id: 'hole-profile', primitiveIds: ['hole'], role: 'hole', parentId: 'outer', closed: true, residualMm: 0.1, issues: [] },
];

test('drawing photo helper maps transformed preview pixels into bottom-left millimeter coordinates', () => {
  assert.deepEqual(imagePointToViewMm([100, 80], [1, 0, 0, 0, 1, 0, 0, 0, 1], 0.1, 100), [10, 2]);
});

test('drawing photo helper reports measured calibration distance using backend uniform scale', () => {
  const recognition = {
    sourceToPreviewTransform: [1, 0, 0, 0, 1, 0, 0, 0, 1],
    millimetersPerPixel: 0.2,
    previewHeightPx: 100,
  } as Pick<DrawingRecognitionResponse, 'sourceToPreviewTransform' | 'millimetersPerPixel' | 'previewHeightPx'>;
  assert.equal(calibratedDistanceMm({ startPx: [10, 20], endPx: [410, 20], distanceMm: 80 }, recognition), 80);
});

test('drawing photo helper rejects invalid calibration instead of guessing a scale', () => {
  assert.match(validateDrawingCalibration(null) ?? '', /two scale points/);
  assert.match(validateDrawingCalibration({ startPx: [2, 2], endPx: [2, 2], distanceMm: 4 }) ?? '', /distinct/);
  assert.match(validateDrawingCalibration({ startPx: [1, 1], endPx: [5, 1], distanceMm: 0 }) ?? '', /greater than zero/);
  assert.equal(validateDrawingCalibration({ startPx: [1, 1], endPx: [5, 1], distanceMm: 4 }), null);
});

test('drawing photo helper accepts closed profiles and retains analytic circles', () => {
  assert.equal(drawingCandidateIsSelectable(profiles[0]!, primitives), true);
  assert.equal(drawingCandidateIsSelectable({ ...profiles[0]!, issues: ['Open junction'] }, primitives), false);
  assert.deepEqual(drawingCandidatesForApply(profiles, ['outer', 'hole-profile'], primitives), primitives);
  assert.equal(drawingCandidatesForApply(profiles, ['hole-profile'], primitives), null);
  assert.deepEqual(drawingCandidatesForApply(profiles, ['outer'], primitives), [primitives[0]]);
});
