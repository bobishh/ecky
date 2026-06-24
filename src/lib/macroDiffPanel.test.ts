import assert from 'node:assert/strict';
import test from 'node:test';

import { composeMacroDiffPanelModel } from './macroDiffPanel';
import type { SessionCodeDiffView, SessionEvent } from './sessionActivity';

function macroEvent(overrides: Partial<SessionEvent> = {}): SessionEvent {
  return {
    id: 'evt-1',
    sessionId: 'session-1',
    threadId: 'thread-1',
    versionId: 'version-1',
    actor: { kind: 'agent', id: 'ecky', label: 'ECKY' },
    kind: 'macro_patch_applied',
    title: 'MACRO PATCH APPLIED',
    summary: 'Adjusted side wall thickness',
    timestamp: Date.UTC(2026, 6, 4, 12, 30, 45),
    severity: 'success',
    ...overrides,
  };
}

function diffView(overrides: Partial<SessionCodeDiffView> = {}): SessionCodeDiffView {
  return {
    event: macroEvent(),
    title: 'MACRO PATCH APPLIED',
    summary: 'Adjusted side wall thickness',
    currentCode: 'alpha\ngamma\nbeta',
    previousCode: 'alpha\nbeta',
    nextCode: 'alpha\ngamma\nbeta',
    diff: {
      kind: 'text',
      label: 'macro',
      path: null,
      key: null,
      before: 'alpha\nbeta',
      after: 'alpha\ngamma\nbeta',
    },
    diffs: [],
    hasDiff: true,
    ...overrides,
  };
}

test('composeMacroDiffPanelModel returns null without a macro event', () => {
  const model = composeMacroDiffPanelModel(
    diffView({ event: null, diff: null, hasDiff: false }),
  );
  assert.equal(model, null);
});

test('composeMacroDiffPanelModel exposes actor, timestamp, and summaries', () => {
  const model = composeMacroDiffPanelModel(diffView());

  assert.ok(model, 'model exists for macro event');
  assert.equal(model.title, 'MACRO PATCH APPLIED');
  assert.equal(model.actorLabel, 'ECKY');
  assert.equal(model.summary, 'Adjusted side wall thickness');
  assert.equal(model.timestamp, Date.UTC(2026, 6, 4, 12, 30, 45));
  assert.ok(model.timeLabel.length > 0, 'time label is rendered');
  assert.equal(model.oldSummary, '2 lines');
  assert.equal(model.newSummary, '3 lines (+1 / −0)');
});

test('composeMacroDiffPanelModel lists changed lines with line numbers', () => {
  const model = composeMacroDiffPanelModel(diffView());

  assert.ok(model, 'model exists');
  assert.equal(model.changedLineCount, 1);
  const changedRows = model.rows.filter((row) => row.kind !== 'context');
  assert.equal(changedRows.length, 1);
  assert.equal(changedRows[0].kind, 'insert');
  assert.equal(changedRows[0].newLineNumber, 2);
  assert.equal(changedRows[0].newText, 'gamma');
});

test('composeMacroDiffPanelModel labels user and system actors by kind', () => {
  const userModel = composeMacroDiffPanelModel(
    diffView({ event: macroEvent({ actor: { kind: 'user', id: 'bogdan' } }) }),
  );
  assert.ok(userModel);
  assert.equal(userModel.actorLabel, 'USER');

  const systemModel = composeMacroDiffPanelModel(
    diffView({ event: macroEvent({ actor: { kind: 'system', id: 'ecky' } }) }),
  );
  assert.ok(systemModel);
  assert.equal(systemModel.actorLabel, 'SYSTEM');
});

test('composeMacroDiffPanelModel reports an empty diff honestly', () => {
  const model = composeMacroDiffPanelModel(
    diffView({
      diff: {
        kind: 'text',
        label: 'macro',
        path: null,
        key: null,
        before: 'alpha\nbeta',
        after: 'alpha\nbeta',
      },
      previousCode: 'alpha\nbeta',
      nextCode: 'alpha\nbeta',
    }),
  );

  assert.ok(model, 'model still exists for macro event without changes');
  assert.equal(model.hasDiff, false);
  assert.equal(model.changedLineCount, 0);
  assert.deepEqual(model.rows, []);
});
