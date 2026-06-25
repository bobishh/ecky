import assert from 'node:assert/strict';
import test from 'node:test';

import {
  appendSessionEvent,
  composeBubbleEvent,
  composeCodeDiffView,
  composeSessionActivity,
  relatedSessionEvents,
  type SessionEvent,
} from './sessionActivity';

function makeEvent(overrides: Partial<SessionEvent>): SessionEvent {
  return {
    id: overrides.id ?? 'event-1',
    sessionId: overrides.sessionId ?? 'session-1',
    threadId: overrides.threadId !== undefined ? overrides.threadId : 'thread-1',
    versionId: overrides.versionId !== undefined ? overrides.versionId : 'version-1',
    actor:
      overrides.actor ?? {
        kind: 'agent',
        id: 'agent-1',
        label: 'Ecky',
      },
    kind: overrides.kind ?? 'agent_action_finished',
    title: overrides.title ?? 'Agent action',
    summary: overrides.summary ?? 'Agent action finished.',
    timestamp: overrides.timestamp ?? 1,
    severity: overrides.severity ?? 'info',
    artifacts: overrides.artifacts,
    diffs: overrides.diffs,
    raw: overrides.raw,
  };
}

test('appendSessionEvent sorts by timestamp and keeps source order on ties', () => {
  const original = [
    makeEvent({ id: 'late', timestamp: 20 }),
    makeEvent({ id: 'tie-a', timestamp: 10 }),
  ];

  const appended = appendSessionEvent(original, makeEvent({ id: 'tie-b', timestamp: 10 }));

  assert.deepEqual(
    appended.map((event) => event.id),
    ['tie-a', 'tie-b', 'late'],
  );
  assert.deepEqual(
    original.map((event) => event.id),
    ['late', 'tie-a'],
  );
});

test('composeSessionActivity scopes visible events to active thread and version', () => {
  const activity = composeSessionActivity(
    [
      makeEvent({ id: 'thread-older', timestamp: 1, threadId: 'thread-a', versionId: 'version-a' }),
      makeEvent({ id: 'thread-other', timestamp: 2, threadId: 'thread-b', versionId: 'version-b' }),
      makeEvent({ id: 'thread-version', timestamp: 3, threadId: 'thread-a', versionId: 'version-b' }),
      makeEvent({ id: 'thread-newer', timestamp: 4, threadId: 'thread-a', versionId: 'version-a' }),
    ],
    'thread-a',
    'version-a',
  );

  assert.deepEqual(
    activity.events.map((event) => event.id),
    ['thread-older', 'thread-other', 'thread-version', 'thread-newer'],
  );
  assert.deepEqual(
    activity.threadEvents.map((event) => event.id),
    ['thread-older', 'thread-version', 'thread-newer'],
  );
  assert.deepEqual(
    activity.versionEvents.map((event) => event.id),
    ['thread-older', 'thread-newer'],
  );
  assert.deepEqual(
    activity.visibleEvents.map((event) => event.id),
    ['thread-older', 'thread-newer'],
  );
  assert.equal(activity.latestEvent?.id, 'thread-newer');
});

test('composeBubbleEvent prefers severity over plain agent chatter', () => {
  const activity = composeSessionActivity(
    [
      makeEvent({
        id: 'info-action',
        timestamp: 1,
        kind: 'agent_action_finished',
        severity: 'info',
        summary: 'Agent finished a background task.',
      }),
      makeEvent({
        id: 'warning-event',
        timestamp: 2,
        kind: 'render_failed',
        severity: 'warning',
        summary: 'Render failed with a bounding box mismatch.',
      }),
      makeEvent({
        id: 'error-event',
        timestamp: 3,
        kind: 'validation_reported',
        severity: 'error',
        summary:
          'Validation failed with raw backend output and more detail than bubble space should hold, including a second clause that pushes the text past the compact threshold.',
      }),
    ],
    'thread-1',
    'version-1',
  );

  const bubble = composeBubbleEvent(activity);

  assert.equal(bubble.event?.id, 'error-event');
  assert.equal(bubble.openTarget, 'activity');
  assert.equal(bubble.compact, true);
  assert.equal(bubble.summary.endsWith('…'), true);
});

test('composeBubbleEvent falls back to latest agent action when no higher severity exists', () => {
  const activity = composeSessionActivity(
    [
      makeEvent({
        id: 'agent-start',
        timestamp: 1,
        kind: 'agent_action_started',
        severity: 'info',
        summary: 'Agent started collecting preview evidence.',
      }),
      makeEvent({
        id: 'agent-finish',
        timestamp: 2,
        kind: 'macro_patch_applied',
        severity: 'success',
        summary: 'Applied the macro patch to the working copy.',
      }),
    ],
    'thread-1',
    'version-1',
  );

  const bubble = composeBubbleEvent(activity);

  assert.equal(bubble.event?.id, 'agent-finish');
  assert.equal(bubble.compact, false);
  assert.equal(bubble.summary, 'Applied the macro patch to the working copy.');
});

test('composeCodeDiffView picks the latest macro diff and keeps current code separate', () => {
  const activity = composeSessionActivity(
    [
      makeEvent({
        id: 'macro-old',
        timestamp: 1,
        kind: 'macro_patch_proposed',
        severity: 'question',
        title: 'Macro patch proposed',
        summary: 'Proposed a macro patch.',
        diffs: [
          {
            kind: 'text',
            path: 'src/main.py',
            before: 'print("old")\n',
            after: 'print("older")\n',
          },
        ],
      }),
      makeEvent({
        id: 'macro-new',
        timestamp: 3,
        kind: 'macro_patch_applied',
        severity: 'success',
        title: 'Macro patch applied',
        summary: 'Applied the latest macro patch.',
        diffs: [
          {
            kind: 'text',
            path: 'src/main.py',
            before: 'print("older")\n',
            after: 'print("new")\n',
          },
        ],
      }),
      makeEvent({
        id: 'render',
        timestamp: 4,
        kind: 'render_succeeded',
        severity: 'success',
        summary: 'Render succeeded.',
      }),
    ],
    'thread-1',
    'version-1',
  );

  const diffView = composeCodeDiffView(activity, 'print("current")\n');

  assert.equal(diffView.event?.id, 'macro-new');
  assert.equal(diffView.hasDiff, true);
  assert.equal(diffView.previousCode, 'print("older")\n');
  assert.equal(diffView.nextCode, 'print("new")\n');
  assert.equal(diffView.currentCode, 'print("current")\n');
  assert.equal(diffView.diff?.path, 'src/main.py');
});

test('composeCodeDiffView returns an empty state when no macro event exists', () => {
  const activity = composeSessionActivity(
    [
      makeEvent({
        id: 'render',
        timestamp: 1,
        kind: 'render_succeeded',
        severity: 'success',
        summary: 'Render succeeded.',
      }),
    ],
    'thread-1',
    'version-1',
  );

  const diffView = composeCodeDiffView(activity, 'print("current")\n');

  assert.equal(diffView.event, null);
  assert.equal(diffView.hasDiff, false);
  assert.equal(diffView.currentCode, 'print("current")\n');
  assert.equal(diffView.nextCode, 'print("current")\n');
});

test('relatedSessionEvents links render, validation, and preview for one version', () => {
  const events = [
    makeEvent({ id: 'render-start', timestamp: 1, kind: 'render_started' }),
    makeEvent({ id: 'render-ok', timestamp: 2, kind: 'render_succeeded', severity: 'success' }),
    makeEvent({ id: 'validation', timestamp: 3, kind: 'validation_reported', severity: 'warning' }),
    makeEvent({ id: 'preview', timestamp: 4, kind: 'preview_updated' }),
    makeEvent({ id: 'other-version', timestamp: 5, kind: 'render_succeeded', versionId: 'version-2' }),
    makeEvent({ id: 'macro', timestamp: 6, kind: 'macro_patch_applied' }),
  ];

  const related = relatedSessionEvents(events, 'preview');

  assert.deepEqual(
    related.map((event) => event.id),
    ['render-start', 'render-ok', 'validation'],
  );
});

test('relatedSessionEvents excludes self and unrelated kinds', () => {
  const events = [
    makeEvent({ id: 'params', timestamp: 1, kind: 'params_changed' }),
    makeEvent({ id: 'render-ok', timestamp: 2, kind: 'render_succeeded' }),
    makeEvent({ id: 'preview', timestamp: 3, kind: 'preview_updated' }),
  ];

  const related = relatedSessionEvents(events, 'render-ok');

  assert.deepEqual(
    related.map((event) => event.id),
    ['preview'],
  );
});

test('relatedSessionEvents returns empty without a version anchor', () => {
  const events = [
    makeEvent({ id: 'render-ok', timestamp: 1, kind: 'render_succeeded', versionId: null }),
    makeEvent({ id: 'preview', timestamp: 2, kind: 'preview_updated', versionId: null }),
  ];

  assert.deepEqual(relatedSessionEvents(events, 'preview'), []);
});

test('relatedSessionEvents returns empty for unknown or non-render event', () => {
  const events = [
    makeEvent({ id: 'macro', timestamp: 1, kind: 'macro_patch_applied' }),
    makeEvent({ id: 'render-ok', timestamp: 2, kind: 'render_succeeded' }),
  ];

  assert.deepEqual(relatedSessionEvents(events, 'macro'), []);
  assert.deepEqual(relatedSessionEvents(events, 'missing'), []);
});
