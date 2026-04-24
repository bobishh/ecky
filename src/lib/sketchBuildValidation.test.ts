import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchBuildValidationSummary } from './sketchBuildValidation';

test('buildSketchBuildValidationSummary passes closed contract, preview artifact, and projections', () => {
  const summary = buildSketchBuildValidationSummary({
    strokes: [
      {
        closed: true,
        view: 'front',
        points: [
          [20, 20],
          [60, 20],
          [20, 20],
        ],
      },
    ],
    draft: {
      source: 'from build123d import *\nwith BuildPart() as part:\n    pass',
    },
    artifactBundle: {
      previewStlPath: '/tmp/ecky/sketch-preview.stl',
      viewerAssets: [{ path: '/tmp/ecky/sketch-preview.glb' }, { path: '/tmp/ecky/sketch-preview.png' }],
    },
    projectionsCount: 3,
    extrudeDepth: 12,
  });

  assert.deepEqual(summary, {
    rows: [
      {
        id: 'closedSketchContract',
        label: 'Sketch contract',
        status: 'pass',
        evidence: 'front view; 3 points; depth 12mm; 3 draft source lines.',
      },
      {
        id: 'previewArtifact',
        label: 'Preview artifact',
        status: 'pass',
        evidence: 'sketch-preview.stl; 2 viewer assets.',
      },
      {
        id: 'projectionCount',
        label: 'Projection count',
        status: 'pass',
        evidence: '3/3 projection views captured.',
      },
    ],
    issues: [],
  });
});

test('buildSketchBuildValidationSummary keeps build evidence pending before preview and projections', () => {
  const summary = buildSketchBuildValidationSummary({
    strokes: [{ closed: true }],
    draft: {
      source: 'profile = closed_profile()',
    },
    artifactBundle: null,
    projectionsCount: 0,
  });

  assert.deepEqual(summary, {
    rows: [
      {
        id: 'closedSketchContract',
        label: 'Sketch contract',
        status: 'pass',
        evidence: 'unknown view; 0 points; depth 12mm; 1 draft source line.',
      },
      {
        id: 'previewArtifact',
        label: 'Preview artifact',
        status: 'pending',
        evidence: 'Waiting for preview artifact path.',
      },
      {
        id: 'projectionCount',
        label: 'Projection count',
        status: 'pending',
        evidence: 'Waiting for preview artifact.',
      },
    ],
    issues: [
      {
        id: 'previewArtifact',
        status: 'pending',
        evidence: 'Waiting for preview artifact path.',
      },
      {
        id: 'projectionCount',
        status: 'pending',
        evidence: 'Waiting for preview artifact.',
      },
    ],
  });
});

test('buildSketchBuildValidationSummary fails with raw provider error evidence', () => {
  const errorText = 'provider body: {"error":"mesh kernel refused profile"}';

  const summary = buildSketchBuildValidationSummary({
    strokes: [{ closed: true }],
    draft: {
      source: 'profile = closed_profile()',
    },
    artifactBundle: null,
    projectionsCount: 0,
    errorText,
  });

  assert.deepEqual(summary, {
    rows: [
      {
        id: 'closedSketchContract',
        label: 'Sketch contract',
        status: 'pass',
        evidence: 'unknown view; 0 points; depth 12mm; 1 draft source line.',
      },
      {
        id: 'previewArtifact',
        label: 'Preview artifact',
        status: 'fail',
        evidence: errorText,
      },
      {
        id: 'projectionCount',
        label: 'Projection count',
        status: 'pending',
        evidence: 'Waiting for preview artifact.',
      },
    ],
    issues: [
      {
        id: 'previewArtifact',
        status: 'fail',
        evidence: errorText,
      },
      {
        id: 'projectionCount',
        status: 'pending',
        evidence: 'Waiting for preview artifact.',
      },
    ],
  });
});
