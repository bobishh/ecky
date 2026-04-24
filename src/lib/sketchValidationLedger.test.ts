import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchValidationRows } from './sketchValidationLedger';
import type { SketchStroke } from './sketchWorkspaceState';
import type { SketchDraftSource } from './tauri/contracts';

const closedProfile: SketchStroke = {
  primitiveId: 'primitive-front-1',
  view: 'front',
  points: [
    [20, 20],
    [60, 20],
    [60, 60],
    [20, 20],
  ],
  closed: true,
};

const lockedClosedProfile: SketchStroke = {
  ...closedProfile,
  points: [
    [25, 20],
    [75, 20],
    [75, 50],
    [25, 50],
    [25, 20],
  ],
  dimensionLocks: { width: true, height: true },
};

const openProfile: SketchStroke = {
  primitiveId: 'primitive-front-1',
  view: 'front',
  points: [
    [20, 20],
    [60, 20],
    [60, 60],
  ],
  closed: false,
};

const draft: SketchDraftSource = {
  sourceLanguage: 'build123d',
  geometryBackend: 'build123d',
  macroDialect: 'build123d',
  source: 'from build123d import *\nwith BuildPart() as part:\n    pass\nshow_object(part)',
};

test('buildSketchValidationRows passes closed profile, source draft, mesh preview, and three projections', () => {
  const rows = buildSketchValidationRows({
    strokes: [closedProfile],
    draft,
    artifactBundle: {
      previewStlPath: '/tmp/ecky/sketch-preview.stl',
      viewerAssets: [{ path: '/tmp/ecky/sketch-preview.glb' }, { path: '/tmp/ecky/sketch-preview.png' }],
    },
    projectionsCount: 3,
    extrudeDepth: 12,
    errorText: '',
  });

  assert.deepEqual(rows, [
    {
      id: 'closedProfile',
      label: 'Closed profile',
      status: 'pass',
      detail: '1 closed stroke.',
    },
    {
      id: 'sketchContract',
      label: 'Sketch contract',
      status: 'pass',
      detail: 'front view; 4 points; depth 12mm; 4 draft source lines.',
    },
    {
      id: 'source',
      label: 'Source generated',
      status: 'pass',
      detail: '4 source lines.',
    },
    {
      id: 'previewArtifact',
      label: 'Preview artifact',
      status: 'pass',
      detail: 'sketch-preview.stl; 2 viewer assets.',
    },
    {
      id: 'sourceFitCheck',
      label: 'Source fit check',
      status: 'pass',
      detail: 'Containment pass; tolerance pass; preview artifact pass.',
    },
    {
      id: 'mesh',
      label: 'Mesh preview',
      status: 'pass',
      detail: 'sketch-preview.stl with 2 viewer assets.',
    },
    {
      id: 'projections',
      label: 'Projections check',
      status: 'pass',
      detail: '3 projection views.',
    },
  ]);
});

test('buildSketchValidationRows shows constraint solver evidence for locked dimensions', () => {
  const rows = buildSketchValidationRows({
    strokes: [lockedClosedProfile],
    draft,
    artifactBundle: {
      previewStlPath: '/tmp/ecky/sketch-preview.stl',
      viewerAssets: [{ path: '/tmp/ecky/sketch-preview.glb' }],
    },
    projectionsCount: 3,
    extrudeDepth: 12,
    errorText: '',
  });

  assert.deepEqual(rows.find((row) => row.id === 'constraintSolver'), {
    id: 'constraintSolver',
    label: 'Constraint solver',
    status: 'pass',
    detail: 'locked-axis translation; width 50mm; height 30mm.',
  });
});

test('buildSketchValidationRows shows source constraint value evidence for locked dimensions', () => {
  const rows = buildSketchValidationRows({
    strokes: [lockedClosedProfile],
    draft,
    artifactBundle: {
      previewStlPath: '/tmp/ecky/sketch-preview.stl',
      viewerAssets: [{ path: '/tmp/ecky/sketch-preview.glb' }],
    },
    projectionsCount: 3,
    extrudeDepth: 12,
    errorText: '',
  });

  assert.deepEqual(rows.find((row) => row.id === 'constraintValues'), {
    id: 'constraintValues',
    label: 'Constraint values',
    status: 'pass',
    detail: 'width 50mm; height 30mm.',
  });
});

test('buildSketchValidationRows fails open profile and leaves downstream rows pending', () => {
  const rows = buildSketchValidationRows({
    strokes: [openProfile],
    draft: null,
    artifactBundle: null,
    projectionsCount: 0,
    errorText: '',
  });

  assert.deepEqual(rows, [
    {
      id: 'closedProfile',
      label: 'Closed profile',
      status: 'fail',
      detail: 'Close profile before preview.',
    },
    {
      id: 'sketchContract',
      label: 'Sketch contract',
      status: 'fail',
      detail: '0 closed strokes; closed profile required before build.',
    },
    {
      id: 'source',
      label: 'Source generated',
      status: 'pending',
      detail: 'Waiting for closed profile.',
    },
    {
      id: 'previewArtifact',
      label: 'Preview artifact',
      status: 'pending',
      detail: 'Waiting for closed sketch contract.',
    },
    {
      id: 'sourceFitCheck',
      label: 'Source fit check',
      status: 'pending',
      detail: 'Waiting for closed source profile.',
    },
    {
      id: 'mesh',
      label: 'Mesh preview',
      status: 'pending',
      detail: 'Waiting for source draft.',
    },
    {
      id: 'projections',
      label: 'Projections check',
      status: 'pending',
      detail: 'Waiting for mesh preview.',
    },
  ]);
});

test('buildSketchValidationRows fails source and mesh with raw backend error text', () => {
  const errorText = 'provider body: {"error":"mesh kernel refused profile"}';

  const rows = buildSketchValidationRows({
    strokes: [closedProfile],
    draft: null,
    artifactBundle: null,
    projectionsCount: 0,
    errorText,
  });

  assert.deepEqual(rows, [
    {
      id: 'closedProfile',
      label: 'Closed profile',
      status: 'pass',
      detail: '1 closed stroke.',
    },
    {
      id: 'sketchContract',
      label: 'Sketch contract',
      status: 'pending',
      detail: '1 closed stroke; waiting for draft source.',
    },
    {
      id: 'source',
      label: 'Source generated',
      status: 'fail',
      detail: errorText,
    },
    {
      id: 'previewArtifact',
      label: 'Preview artifact',
      status: 'fail',
      detail: errorText,
    },
    {
      id: 'sourceFitCheck',
      label: 'Source fit check',
      status: 'fail',
      detail: errorText,
    },
    {
      id: 'mesh',
      label: 'Mesh preview',
      status: 'fail',
      detail: errorText,
    },
    {
      id: 'projections',
      label: 'Projections check',
      status: 'pending',
      detail: 'Waiting for mesh preview.',
    },
  ]);
});
