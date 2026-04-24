import assert from 'node:assert/strict';
import test from 'node:test';

import { summarizeSketchPreviewStep } from './sketchAutoPreview';

test('summarizeSketchPreviewStep blocks idle preview while profile remains open', () => {
  const step = summarizeSketchPreviewStep({
    hasClosedProfile: false,
    hasDraft: false,
    generating: false,
    errorText: '',
    autoQueued: false,
  });

  assert.deepEqual(step, {
    state: 'blocked',
    label: 'PROFILE OPEN',
    detail: 'Close profile before preview.',
  });
});

test('summarizeSketchPreviewStep reports queued auto-preview for closed profile', () => {
  const step = summarizeSketchPreviewStep({
    hasClosedProfile: true,
    hasDraft: false,
    generating: false,
    errorText: '',
    autoQueued: true,
  });

  assert.deepEqual(step, {
    state: 'queued',
    label: 'AUTO-PREVIEW QUEUED',
    detail: 'Closed profile queued for preview.',
  });
});

test('summarizeSketchPreviewStep reports active generation', () => {
  const step = summarizeSketchPreviewStep({
    hasClosedProfile: true,
    hasDraft: false,
    generating: true,
    errorText: '',
    autoQueued: true,
  });

  assert.deepEqual(step, {
    state: 'generating',
    label: 'GENERATING PREVIEW',
    detail: 'Preview request in flight.',
  });
});

test('summarizeSketchPreviewStep accepts ready draft as preview ready', () => {
  const step = summarizeSketchPreviewStep({
    hasClosedProfile: true,
    hasDraft: true,
    generating: false,
    errorText: '',
    autoQueued: false,
  });

  assert.deepEqual(step, {
    state: 'accepted',
    label: 'PREVIEW READY',
    detail: 'Draft accepted; preview ready.',
  });
});

test('summarizeSketchPreviewStep reports failure with raw error detail', () => {
  const errorText = 'provider body: {"error":"mesh kernel refused profile"}';
  const step = summarizeSketchPreviewStep({
    hasClosedProfile: true,
    hasDraft: false,
    generating: false,
    errorText,
    autoQueued: false,
  });

  assert.deepEqual(step, {
    state: 'failed',
    label: 'PREVIEW FAILED',
    detail: errorText,
  });
});
