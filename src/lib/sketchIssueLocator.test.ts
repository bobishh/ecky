import assert from 'node:assert/strict';
import test from 'node:test';

import { findSketchIssueMatch } from './sketchIssueLocator';
import type { SketchDocument, SketchValidationIssue } from './tauri/contracts';

const document: SketchDocument = {
  documentId: 'doc-issue-locator',
  units: 'mm',
  sketches: [
    {
      sketchId: 'sketch-front',
      view: 'front',
      primitives: [
        {
          primitiveId: 'front-outer',
          kind: 'polyline',
          closed: true,
          points: [
            [0, 0],
            [20, 0],
            [20, 10],
            [0, 10],
            [0, 0],
          ],
          topology: {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
        {
          primitiveId: 'front-hole',
          kind: 'polyline',
          closed: true,
          points: [
            [3, 2],
            [7, 2],
            [7, 3],
            [3, 3],
            [3, 2],
          ],
          topology: {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            loopRole: 'hole',
            sourceClass: 'derived',
          },
        },
      ],
    },
    {
      sketchId: 'sketch-top',
      view: 'top',
      primitives: [
        {
          primitiveId: 'top-outer',
          kind: 'polyline',
          closed: true,
          points: [
            [0, 0],
            [20, 0],
            [20, 10],
            [0, 10],
            [0, 0],
          ],
          topology: {
            loopId: 'top-outer',
            edgeIds: ['top-a', 'top-b', 'top-c', 'top-d'],
            loopRole: 'outer',
            sourceClass: 'derived',
          },
        },
      ],
    },
  ],
};

test('findSketchIssueMatch resolves topology-only issue with generic sketch metadata', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'model',
    kind: 'containmentMismatch',
    view: 'custom',
    primitiveId: 'stale-outer',
    edgeId: 'front-v1',
    severity: 'error',
    message: 'projection exits source profile',
    topology: {
      loopId: 'hole-alpha',
      edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
      loopRole: 'hole',
      sourceClass: 'derived',
    },
  };

  const match = findSketchIssueMatch(document, issue);

  assert.equal(match?.sketch.sketchId, 'sketch-front');
  assert.equal(match?.primitive?.primitiveId, 'front-hole');
});

test('findSketchIssueMatch prefers topology over stale sketchId and stale view', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-top',
    kind: 'containmentMismatch',
    view: 'top',
    primitiveId: 'top-outer',
    edgeId: 'front-v1',
    severity: 'error',
    message: 'projection exits source profile',
    topology: {
      loopId: 'hole-alpha',
      edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
      loopRole: 'hole',
      sourceClass: 'derived',
    },
  };

  const match = findSketchIssueMatch(document, issue);

  assert.equal(match?.sketch.sketchId, 'sketch-front');
  assert.equal(match?.primitive?.primitiveId, 'front-hole');
});

test('findSketchIssueMatch ignores unscoped primitive fallback without stronger locator', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'model',
    kind: 'containmentMismatch',
    view: 'custom',
    primitiveId: 'front-hole',
    severity: 'error',
    message: 'projection exits source profile',
  };

  const match = findSketchIssueMatch(document, issue);

  assert.equal(match, null);
});

test('findSketchIssueMatch resolves exact sketch plus primitive identity without topology', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    kind: 'boundsMismatch',
    view: 'front',
    primitiveId: 'front-hole',
    severity: 'error',
    message: 'projection exits source profile',
  };

  const match = findSketchIssueMatch(document, issue);

  assert.equal(match?.sketch.sketchId, 'sketch-front');
  assert.equal(match?.primitive?.primitiveId, 'front-hole');
});

test('findSketchIssueMatch prefers exact sketch plus primitive identity over duplicate topology in another sketch', () => {
  const duplicateDocument: SketchDocument = {
    ...document,
    sketches: [
      {
        sketchId: 'sketch-top-duplicate',
        view: 'top',
        primitives: [
          {
            primitiveId: 'top-hole',
            kind: 'polyline',
            closed: true,
            points: [
              [3, 2],
              [7, 2],
              [7, 3],
              [3, 3],
              [3, 2],
            ],
            topology: {
              loopId: 'shared-hole',
              edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
        ],
      },
      {
        sketchId: 'sketch-front-exact',
        view: 'front',
        primitives: [
          {
            primitiveId: 'front-hole',
            kind: 'polyline',
            closed: true,
            points: [
              [3, 2],
              [7, 2],
              [7, 3],
              [3, 3],
              [3, 2],
            ],
            topology: {
              loopId: 'shared-hole',
              edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
        ],
      },
    ],
  };
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front-exact',
    kind: 'containmentMismatch',
    view: 'front',
    primitiveId: 'front-hole',
    edgeId: 'front-v1',
    severity: 'error',
    message: 'projection exits source profile',
    topology: {
      loopId: 'shared-hole',
      edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
      loopRole: 'hole',
      sourceClass: 'derived',
    },
  };

  const match = findSketchIssueMatch(duplicateDocument, issue);

  assert.equal(match?.sketch.sketchId, 'sketch-front-exact');
  assert.equal(match?.primitive?.primitiveId, 'front-hole');
});

test('findSketchIssueMatch returns null when sketch-only locator has no primitive evidence', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    kind: 'containmentMismatch',
    view: 'front',
    severity: 'error',
    message: 'projection exits source profile',
  };

  const match = findSketchIssueMatch(document, issue);

  assert.equal(match, null);
});

test('findSketchIssueMatch ignores stale sketch and view without topology or edge evidence', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-top',
    kind: 'containmentMismatch',
    view: 'top',
    primitiveId: 'ghost-primitive',
    severity: 'error',
    message: 'projection exits source profile',
  };

  const match = findSketchIssueMatch(document, issue);

  assert.equal(match, null);
});

test('findSketchIssueMatch ignores message text when no structured locator exists', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'model',
    kind: 'topologyMismatch',
    view: 'custom',
    primitiveId: null,
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: front face loop cannot be matched',
  };

  const match = findSketchIssueMatch(document, issue);

  assert.equal(match, null);
});
