import assert from 'node:assert/strict';
import test from 'node:test';

import { summarizeSketchValidationIssue } from './sketchValidationIssueSummary';
import type { SketchValidationIssue } from './tauri/contracts';

test('summarizeSketchValidationIssue prefixes structured locator fields before neutral message', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-alpha',
    kind: 'containmentMismatch',
    view: 'front',
    primitiveId: 'primitive-front-hole',
    edgeId: 'front-v1',
    topology: {
      loopId: 'front-hole',
      edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
      loopRole: 'hole',
      sourceClass: 'derived',
    },
    severity: 'error',
    message: 'projection exits source profile',
  };

  assert.equal(
    summarizeSketchValidationIssue(issue),
    'containment mismatch / FRONT / HOLE / front-v1 / projection exits source profile',
  );
});

test('summarizeSketchValidationIssue omits custom view label for global issues', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    kind: 'topologyMismatch',
    view: 'custom',
    severity: 'error',
    message: 'legacy validation issue',
  };

  assert.equal(summarizeSketchValidationIssue(issue), 'topology mismatch / legacy validation issue');
});
