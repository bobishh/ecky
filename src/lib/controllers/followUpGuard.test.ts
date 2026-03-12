import assert from 'node:assert/strict';
import test from 'node:test';

import type { Attachment, Message, Thread } from '../types/domain';
import { detectFollowUpAnswer } from './followUpGuard';

function makeMessage(overrides: Partial<Message> = {}): Message {
  return {
    id: overrides.id ?? 'message-1',
    role: overrides.role ?? 'assistant',
    content: overrides.content ?? 'Which side?',
    status: overrides.status ?? 'success',
    output: overrides.output ?? null,
    usage: overrides.usage ?? null,
    artifactBundle: overrides.artifactBundle ?? null,
    modelManifest: overrides.modelManifest ?? null,
    imageData: overrides.imageData ?? null,
    attachmentImages: overrides.attachmentImages ?? [],
    timestamp: overrides.timestamp ?? 1,
  };
}

function makeThread(messages: Message[]): Thread {
  return {
    id: 'thread-1',
    title: 'Test Thread',
    summary: '',
    messages,
    updatedAt: 1,
    versionCount: 1,
    pendingCount: 0,
    errorCount: 0,
    genieTraits: null,
  };
}

test('matches a short answer to the last assistant clarification question', () => {
  const result = detectFollowUpAnswer({
    promptText: 'left',
    attachments: [],
    activeThread: makeThread([makeMessage({ content: 'Which side?' })]),
    explicitQuestionOnly: false,
  });

  assert.equal(result.matched, true);
  assert.equal(result.question, 'Which side?');
  assert.equal(result.reason, 'matched');
});

test('matches clarification turns without a literal question mark', () => {
  const result = detectFollowUpAnswer({
    promptText: 'left',
    attachments: [],
    activeThread: makeThread([
      makeMessage({
        content:
          'The frame can support either orientation. Need one more detail before I continue: choose left or right side',
      }),
    ]),
    explicitQuestionOnly: false,
  });

  assert.equal(result.matched, true);
  assert.equal(result.reason, 'matched');
});

test('does not match when the last assistant turn already has geometry output', () => {
  const result = detectFollowUpAnswer({
    promptText: 'left',
    attachments: [],
    activeThread: makeThread([
      makeMessage({
        content: 'Done',
        output: {
          title: 'Pot',
          versionName: 'v1',
          response: 'ok',
          interactionMode: 'design',
          macroCode: 'print(1)',
          uiSpec: { fields: [] },
          initialParams: {},
        },
      }),
    ]),
    explicitQuestionOnly: false,
  });

  assert.equal(result.matched, false);
  assert.equal(
    result.reason,
    'last persisted message is not an assistant clarification question',
  );
});

test('does not match when attachments are present', () => {
  const result = detectFollowUpAnswer({
    promptText: 'left',
    attachments: [{ path: '/tmp/x.png', name: 'x', explanation: '', type: 'image' } satisfies Attachment],
    activeThread: makeThread([makeMessage({ content: 'Which side?' })]),
    explicitQuestionOnly: false,
  });

  assert.equal(result.matched, false);
  assert.equal(result.reason, 'attachments present');
});

test('does not match explicit question-only replies', () => {
  const result = detectFollowUpAnswer({
    promptText: 'answer only: left',
    attachments: [],
    activeThread: makeThread([makeMessage({ content: 'Which side?' })]),
    explicitQuestionOnly: true,
  });

  assert.equal(result.matched, false);
  assert.equal(result.reason, 'explicit question-only request');
});

test('does not match long standalone prompts', () => {
  const result = detectFollowUpAnswer({
    promptText: 'x'.repeat(221),
    attachments: [],
    activeThread: makeThread([makeMessage({ content: 'Which side?' })]),
    explicitQuestionOnly: false,
  });

  assert.equal(result.matched, false);
  assert.equal(result.reason, 'prompt exceeds narrow-answer limit');
});

test('does not match without a prior assistant clarification turn', () => {
  const result = detectFollowUpAnswer({
    promptText: 'left',
    attachments: [],
    activeThread: makeThread([makeMessage({ role: 'user', content: 'make it bigger' })]),
    explicitQuestionOnly: false,
  });

  assert.equal(result.matched, false);
  assert.equal(
    result.reason,
    'last persisted message is not an assistant clarification question',
  );
});
