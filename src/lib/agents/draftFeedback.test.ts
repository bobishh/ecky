import assert from 'node:assert/strict';
import test from 'node:test';

import {
  composeAgentDraftFeedbackBubbleText,
  isVisibleAgentDraftFeedback,
  summarizeAgentDraftFeedback,
  type AgentDraftFeedback,
} from './draftFeedback';

const feedback: AgentDraftFeedback = {
  status: 'failed',
  summary: 'Wall continuity check failed because the overlap left two disconnected slots in the preview mesh.',
  items: [{ code: 'continuity', message: 'Preview mesh split into two shells.' }],
  source: 'structuralVerification',
  threadId: 'thread-1',
  previewId: 'preview-1',
  sessionId: 'session-1',
};

test('summarizeAgentDraftFeedback compacts long feedback for the bubble', () => {
  const compact = summarizeAgentDraftFeedback(feedback, 72);
  assert.equal(compact.endsWith('…'), true);
  assert.equal(compact.includes('preview mesh'), false);
});

test('isVisibleAgentDraftFeedback only shows feedback for the active draft preview', () => {
  assert.equal(isVisibleAgentDraftFeedback(feedback, 'thread-1', 'preview-1'), true);
  assert.equal(isVisibleAgentDraftFeedback(feedback, 'thread-1', 'version-2'), false);
  assert.equal(isVisibleAgentDraftFeedback(feedback, 'thread-2', 'preview-1'), false);
});

test('Given draft feedback with authoring lints When composing bubble text Then includes lint summary', () => {
  const text = composeAgentDraftFeedbackBubbleText({
    feedback: {
      ...feedback,
      summary: 'Draft preview passed structural verification.',
      authoringLints: [
        {
          message:
            'Repeated anonymous delta on `slotWidth` in part `holder`. Extract `slot_margin_x` parameter and reuse.',
        },
      ],
    },
  });

  assert.match(text, /Draft preview passed structural verification/);
  assert.match(text, /Authoring lint:/);
  assert.match(text, /Repeated anonymous delta on `slotWidth`/);
});

test('Given no feedback and no lints When composing bubble text Then returns empty string', () => {
  const text = composeAgentDraftFeedbackBubbleText({
    feedback: null,
    fallbackAuthoringLints: [],
  });

  assert.equal(text, '');
});
