import assert from 'node:assert/strict';
import test from 'node:test';

import {
  brepHiddenLineViewHasWarning,
  brepHiddenLineWarningMessages,
} from './sketchHiddenLineWarnings';
import type { BrepHiddenLineProjectionResponse } from './tauri/contracts';

const response: BrepHiddenLineProjectionResponse = {
  modelId: 'model-1',
  sourceArtifactPath: '/tmp/model.FCStd',
  views: [],
  warningEntries: [
    {
      kind: 'projectionNoEdges',
      view: 'top',
      message: 'projection produced no edges.',
    },
  ],
  validation: null,
};

test('brepHiddenLineWarningMessages prefers structured warning entries', () => {
  assert.deepEqual(brepHiddenLineWarningMessages(response), ['TOP projection produced no edges.']);
});

test('brepHiddenLineViewHasWarning matches exact view from structured warning entry', () => {
  assert.equal(brepHiddenLineViewHasWarning(response, 'top'), true);
  assert.equal(brepHiddenLineViewHasWarning(response, 'front'), false);
});

test('brepHiddenLineWarningMessages returns empty when structured warning entries are absent', () => {
  assert.deepEqual(
    brepHiddenLineWarningMessages({
      ...response,
      warningEntries: [],
    }),
    [],
  );
});
