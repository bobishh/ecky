import assert from 'node:assert/strict';
import test from 'node:test';

import {
  compactThreadActivitySummary,
  formatAgentActivityElapsed,
  isThreadAgentBusy,
  resolveActiveMcpBubble,
  resolveGenieBubblePresentation,
  resolveTerminalActivityMeta,
} from './activity';

test('isThreadAgentBusy only treats active non-waiting sessions as whole-turn busy', () => {
  assert.equal(
    isThreadAgentBusy({
      connectionState: 'active',
      agentLabel: 'Gemini',
      llmModelLabel: null,
      providerKind: 'gemini',
      sessionId: 'session-1',
      phase: 'patching_macro',
      statusText: 'Working',
      busy: true,
      activityLabel: null,
      activityStartedAt: null,
      attentionKind: null,
      waitingOnPrompt: false,
      updatedAt: 1,
    }),
    true,
  );
  assert.equal(
    isThreadAgentBusy({
      connectionState: 'active',
      agentLabel: 'Gemini',
      llmModelLabel: null,
      providerKind: 'gemini',
      sessionId: 'session-1',
      phase: 'waiting_for_user',
      statusText: 'Waiting',
      busy: true,
      activityLabel: null,
      activityStartedAt: null,
      attentionKind: null,
      waitingOnPrompt: true,
      updatedAt: 1,
    }),
    false,
  );
});

test('formatAgentActivityElapsed renders compact minutes and seconds', () => {
  assert.equal(formatAgentActivityElapsed(100, 265), '2m 45s');
  assert.equal(formatAgentActivityElapsed(null, 265), null);
});

test('resolveActiveMcpBubble prefers provider activity labels over cooking phrases and sanitized fallback', () => {
  assert.equal(
    resolveActiveMcpBubble({
      threadAgentState: {
        connectionState: 'active',
        agentLabel: 'Gemini',
        llmModelLabel: null,
        providerKind: 'gemini',
        sessionId: 'session-1',
        phase: 'patching_macro',
        statusText: 'Working',
        busy: true,
        activityLabel: 'Developing the next iteration',
        activityStartedAt: 100,
        attentionKind: null,
        waitingOnPrompt: false,
        updatedAt: 1,
      },
      visibleAgentTerminal: {
        agentId: 'gemini',
        agentLabel: 'Gemini',
        providerKind: 'gemini',
        sessionNonce: 1,
        screenText: '',
        vtStream: '',
        vtDelta: null,
        attentionRequired: false,
        busy: true,
        activityLabel: 'ignored terminal label',
        activityStartedAt: 100,
        attentionKind: null,
        summary: 'sanitized fallback',
        active: true,
        updatedAt: 1,
      },
      cookingPhrase: 'Packing constraints and dimensions into a fresh build plan.',
      nowSecs: 265,
    }),
    'Developing the next iteration · 2m 45s',
  );
});

test('resolveActiveMcpBubble falls back to cooking phrase and status when no activity label exists', () => {
  assert.equal(
    resolveActiveMcpBubble({
      threadAgentState: {
        connectionState: 'active',
        agentLabel: 'Claude',
        llmModelLabel: null,
        providerKind: 'claude',
        sessionId: 'session-1',
        phase: 'patching_macro',
        statusText: 'Working',
        busy: true,
        activityLabel: null,
        activityStartedAt: 100,
        attentionKind: null,
        waitingOnPrompt: false,
        updatedAt: 1,
      },
      visibleAgentTerminal: {
        agentId: 'claude',
        agentLabel: 'Claude',
        providerKind: 'claude',
        sessionNonce: 1,
        screenText: '',
        vtStream: '',
        vtDelta: null,
        attentionRequired: false,
        busy: true,
        activityLabel: null,
        activityStartedAt: 100,
        attentionKind: null,
        summary: 'sanitized terminal fallback',
        active: true,
        updatedAt: 1,
      },
      cookingPhrase: 'Checking manifold integrity and shell continuity.',
      nowSecs: 160,
    }),
    'Checking manifold integrity and shell continuity. · 1m 00s',
  );

  assert.equal(
    resolveActiveMcpBubble({
      threadAgentState: {
        connectionState: 'active',
        agentLabel: 'Claude',
        llmModelLabel: null,
        providerKind: 'claude',
        sessionId: 'session-1',
        phase: 'patching_macro',
        statusText: 'status fallback',
        busy: false,
        activityLabel: null,
        activityStartedAt: null,
        attentionKind: null,
        waitingOnPrompt: false,
        updatedAt: 1,
      },
      visibleAgentTerminal: {
        agentId: 'claude',
        agentLabel: 'Claude',
        providerKind: 'claude',
        sessionNonce: 1,
        screenText: '',
        vtStream: '',
        vtDelta: null,
        attentionRequired: false,
        busy: false,
        activityLabel: null,
        activityStartedAt: null,
        attentionKind: null,
        summary: 'sanitized terminal fallback',
        active: true,
        updatedAt: 1,
      },
      cookingPhrase: '',
      nowSecs: 160,
    }),
    'status fallback',
  );
});

test('resolveTerminalActivityMeta falls back to terminal summary before status text', () => {
  assert.equal(
    resolveTerminalActivityMeta({
      threadAgentState: {
        connectionState: 'active',
        agentLabel: 'Claude',
        llmModelLabel: null,
        providerKind: 'claude',
        sessionId: 'session-1',
        phase: 'patching_macro',
        statusText: 'status fallback',
        busy: false,
        activityLabel: null,
        activityStartedAt: null,
        attentionKind: null,
        waitingOnPrompt: false,
        updatedAt: 1,
      },
      visibleAgentTerminal: {
        agentId: 'claude',
        agentLabel: 'Claude',
        providerKind: 'claude',
        sessionNonce: 1,
        screenText: '',
        vtStream: '',
        vtDelta: null,
        attentionRequired: false,
        busy: false,
        activityLabel: null,
        activityStartedAt: null,
        attentionKind: null,
        summary: 'terminal summary',
        active: true,
        updatedAt: 1,
      },
      nowSecs: 160,
    }),
    'terminal summary',
  );
});

test('compactThreadActivitySummary keeps only the first paragraph and truncates long text', () => {
  assert.equal(
    compactThreadActivitySummary(
      'First paragraph stays visible.\n\nSecond paragraph should not appear in the compact card.',
    ),
    'First paragraph stays visible.',
  );

  const compact = compactThreadActivitySummary(
    'This is a deliberately verbose activity summary that should be shortened before it eats the whole dialogue pane with duplicated agent copy.',
    72,
  );
  assert.equal(compact.endsWith('…'), true);
  assert.equal(compact.includes('dialogue pane'), false);
});

test('resolveGenieBubblePresentation compacts preview validation feedback and keeps preview context visible', () => {
  const bubble = resolveGenieBubblePresentation({
    threadAgentState: {
      connectionState: 'active',
      agentLabel: 'Codex',
      llmModelLabel: 'gpt-5',
      providerKind: 'openai',
      sessionId: 'session-1',
      phase: 'patching_macro',
      statusText: 'Preview validation found a containment mismatch on front profile. Repairing source bounds and rerunning exact hidden-line validation.',
      busy: true,
      activityLabel: 'Preview validation found a containment mismatch on front profile. Repairing source bounds and rerunning exact hidden-line validation.',
      activityStartedAt: 100,
      attentionKind: null,
      waitingOnPrompt: false,
      updatedAt: 1,
    },
    activeMcpBubbleSummary: 'Preview validation found a containment mismatch on front profile. Repairing source bounds and rerunning exact hidden-line validation.',
    hasPreviewArtifact: true,
    previewArtifactName: 'preview-feedback.stl',
  });

  assert.equal(bubble.compact, true);
  assert.equal(bubble.badge, 'PREVIEW CHECK');
  assert.equal(bubble.contextLabel, 'preview-feedback.stl');
  assert.match(bubble.text, /Preview validation found a containment mismatch/);
});

test('resolveGenieBubblePresentation keeps prompt priority over preview repair status', () => {
  const bubble = resolveGenieBubblePresentation({
    pendingAgentPrompt: {
      message: 'Need tolerance target before I rerun the preview.',
      agentLabel: 'Codex',
    },
    threadAgentState: {
      connectionState: 'active',
      agentLabel: 'Codex',
      llmModelLabel: 'gpt-5',
      providerKind: 'openai',
      sessionId: 'session-1',
      phase: 'patching_macro',
      statusText: 'Repairing source bounds.',
      busy: true,
      activityLabel: 'Repairing source bounds.',
      activityStartedAt: 100,
      attentionKind: null,
      waitingOnPrompt: false,
      updatedAt: 1,
    },
    activeMcpBubbleSummary: 'Repairing source bounds.',
    repairMessage: 'Repairing source bounds.',
    hasPreviewArtifact: true,
    previewArtifactName: 'preview-feedback.stl',
  });

  assert.equal(bubble.source, 'pendingPrompt');
  assert.equal(bubble.compact, false);
  assert.equal(bubble.badge, null);
  assert.equal(bubble.text, 'Need tolerance target before I rerun the preview.');
});
