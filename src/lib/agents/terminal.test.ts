import assert from 'node:assert/strict';
import test from 'node:test';

import {
  agentTerminalSessionKey,
  buildAgentTerminalKeyInput,
  buildAgentTerminalLineInput,
  hasRenderableTerminal,
  mergeAgentTerminalSnapshot,
  pickAgentTerminalAttention,
  pickVisibleAgentTerminal,
  resolveAgentTerminalReplayText,
  resolveTerminalStreamWrite,
  shouldReplayTerminalOnVisibilityRestore,
} from './terminal';

test('pickVisibleAgentTerminal prefers primary attention over newer secondary activity', () => {
  const snapshots = [
    {
      agentId: 'secondary',
      agentLabel: 'Secondary',
      sessionNonce: 1,
      screenText: 'still running',
      vtStream: 'still running',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: true,
      updatedAt: 20,
    },
    {
      agentId: 'primary',
      agentLabel: 'Primary',
      sessionNonce: 2,
      screenText: 'needs trust confirmation',
      vtStream: 'needs trust confirmation',
      vtDelta: null,
      attentionRequired: true,
      summary: 'Claude needs workspace trust confirmation.',
      active: true,
      updatedAt: 10,
    },
  ];

  assert.equal(pickVisibleAgentTerminal(snapshots, 'primary')?.agentId, 'primary');
  assert.equal(pickAgentTerminalAttention(snapshots, 'primary')?.agentId, 'primary');
});

test('pickVisibleAgentTerminal falls back to the primary snapshot when no attention is pending', () => {
  const snapshots = [
    {
      agentId: 'secondary',
      agentLabel: 'Secondary',
      sessionNonce: 1,
      screenText: 'secondary output',
      vtStream: 'secondary output',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: true,
      updatedAt: 20,
    },
    {
      agentId: 'primary',
      agentLabel: 'Primary',
      sessionNonce: 2,
      screenText: 'primary output',
      vtStream: 'primary output',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: true,
      updatedAt: 10,
    },
  ];

  assert.equal(pickVisibleAgentTerminal(snapshots, 'primary')?.agentId, 'primary');
  assert.equal(pickAgentTerminalAttention(snapshots, 'primary'), null);
});

test('pickVisibleAgentTerminal prefers a live secondary terminal over an inactive primary fallback', () => {
  const snapshots = [
    {
      agentId: 'primary',
      agentLabel: 'Primary',
      sessionNonce: 1,
      screenText: 'old primary snapshot',
      vtStream: '',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: false,
      updatedAt: 30,
    },
    {
      agentId: 'secondary',
      agentLabel: 'Secondary',
      sessionNonce: 2,
      screenText: 'secondary output',
      vtStream: 'secondary output',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: true,
      updatedAt: 20,
    },
  ];

  assert.equal(pickVisibleAgentTerminal(snapshots, 'primary')?.agentId, 'secondary');
});

test('hasRenderableTerminal ignores empty inactive snapshots', () => {
  assert.equal(
    hasRenderableTerminal({
      agentId: 'primary',
      agentLabel: 'Primary',
      sessionNonce: 1,
      screenText: '   ',
      vtStream: '',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: false,
      updatedAt: 1,
    }),
    false,
  );
});

test('resolveAgentTerminalReplayText prefers vtStream for live terminals', () => {
  assert.equal(
    resolveAgentTerminalReplayText({
      agentId: 'primary',
      agentLabel: 'Primary',
      sessionNonce: 1,
      screenText: 'sanitized fallback',
      vtStream: '\u001b[?1049hfull tui',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: true,
      updatedAt: 1,
    }),
    '\u001b[?1049hfull tui',
  );
});

test('resolveAgentTerminalReplayText falls back to screenText only for inactive sessions', () => {
  assert.equal(
    resolveAgentTerminalReplayText({
      agentId: 'primary',
      agentLabel: 'Primary',
      sessionNonce: 1,
      screenText: 'last captured snapshot',
      vtStream: '',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: false,
      updatedAt: 1,
    }),
    'last captured snapshot',
  );
});

test('buildAgentTerminalLineInput submits a composed line by default', () => {
  assert.deepEqual(buildAgentTerminalLineInput('claude', '2'), {
    agentId: 'claude',
    text: '2',
    key: null,
    ctrl: false,
    alt: false,
    shift: false,
    meta: false,
    submit: true,
  });
});

test('agentTerminalSessionKey combines agent id and session nonce', () => {
  assert.equal(
    agentTerminalSessionKey({
      agentId: 'gemini',
      agentLabel: 'Gemini',
      sessionNonce: 42,
      screenText: '',
      vtStream: '',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: true,
      updatedAt: 1,
    }),
    'gemini:42',
  );
});

test('agentTerminalSessionKey includes session id when present', () => {
  assert.equal(
    agentTerminalSessionKey({
      agentId: 'gemini',
      agentLabel: 'Gemini',
      sessionId: 'session-7',
      sessionNonce: 42,
      screenText: '',
      vtStream: '',
      vtDelta: null,
      attentionRequired: false,
      summary: null,
      active: true,
      updatedAt: 1,
    }),
    'gemini:session-7:42',
  );
});

test('mergeAgentTerminalSnapshot appends vt deltas without replacing the accumulated replay stream', () => {
  const previous = {
    agentId: 'gemini',
    agentLabel: 'Gemini',
    sessionNonce: 7,
    screenText: 'fallback',
    vtStream: 'abc',
    vtDelta: null,
    attentionRequired: false,
    summary: null,
    active: true,
    updatedAt: 1,
  };

  assert.deepEqual(mergeAgentTerminalSnapshot(previous, {
    ...previous,
    vtStream: '',
    vtDelta: 'def',
    updatedAt: 2,
  }), {
    ...previous,
    vtStream: 'abcdef',
    vtDelta: null,
    updatedAt: 2,
  });
});

test('mergeAgentTerminalSnapshot resets replay state when the PTY session nonce changes', () => {
  const previous = {
    agentId: 'gemini',
    agentLabel: 'Gemini',
    sessionNonce: 7,
    screenText: 'fallback',
    vtStream: 'abc',
    vtDelta: null,
    attentionRequired: false,
    summary: null,
    active: true,
    updatedAt: 1,
  };

  assert.deepEqual(mergeAgentTerminalSnapshot(previous, {
    ...previous,
    sessionNonce: 8,
    vtStream: '',
    vtDelta: 'xyz',
    updatedAt: 2,
  }), {
    ...previous,
    sessionNonce: 8,
    vtStream: 'xyz',
    vtDelta: null,
    updatedAt: 2,
  });
});

test('resolveTerminalStreamWrite appends only the delta for growing live streams', () => {
  assert.deepEqual(resolveTerminalStreamWrite('abc', 'abcdef'), {
    mode: 'append',
    data: 'def',
  });
});

test('resolveTerminalStreamWrite tolerates head trimming without forcing a reset', () => {
  const previous = `${'x'.repeat(80)}tail-and-delta`;
  const next = `${'x'.repeat(48)}tail-and-delta-more`;

  assert.deepEqual(resolveTerminalStreamWrite(previous, next), {
    mode: 'append',
    data: '-more',
  });
});

test('resolveTerminalStreamWrite resets when the stream diverges for real', () => {
  assert.deepEqual(resolveTerminalStreamWrite('abc', 'totally different'), {
    mode: 'reset',
    data: 'totally different',
  });
});

test('shouldReplayTerminalOnVisibilityRestore replays when the terminal becomes visible again', () => {
  assert.equal(
    shouldReplayTerminalOnVisibilityRestore({
      previousSessionKey: 'gemini:1',
      nextSessionKey: 'gemini:1',
      wasVisible: false,
      isVisible: true,
    }),
    true,
  );
});

test('shouldReplayTerminalOnVisibilityRestore replays when the PTY session changes', () => {
  assert.equal(
    shouldReplayTerminalOnVisibilityRestore({
      previousSessionKey: 'gemini:1',
      nextSessionKey: 'gemini:2',
      wasVisible: true,
      isVisible: true,
    }),
    true,
  );
});

test('shouldReplayTerminalOnVisibilityRestore does not replay while still hidden', () => {
  assert.equal(
    shouldReplayTerminalOnVisibilityRestore({
      previousSessionKey: 'gemini:1',
      nextSessionKey: 'gemini:1',
      wasVisible: false,
      isVisible: false,
    }),
    false,
  );
});

test('buildAgentTerminalKeyInput maps terminal navigation keys', () => {
  assert.deepEqual(
    buildAgentTerminalKeyInput('claude', {
      key: 'ArrowDown',
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      metaKey: false,
    }),
    {
      agentId: 'claude',
      text: '',
      key: 'ArrowDown',
      ctrl: false,
      alt: false,
      shift: false,
      meta: false,
      submit: false,
    },
  );
});

test('buildAgentTerminalKeyInput preserves ctrl shortcuts for PTY passthrough', () => {
  assert.deepEqual(
    buildAgentTerminalKeyInput('claude', {
      key: 'c',
      ctrlKey: true,
      altKey: false,
      shiftKey: false,
      metaKey: false,
    }),
    {
      agentId: 'claude',
      text: '',
      key: 'c',
      ctrl: true,
      alt: false,
      shift: false,
      meta: false,
      submit: false,
    },
  );
});
