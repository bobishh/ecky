import assert from 'node:assert/strict';
import test from 'node:test';

import type { Message } from './types/domain';
import {
  buildGenerateFromConceptPrompt,
  cycleConceptPreviewMessageId,
  isConceptPreviewMessage,
  listConceptPreviewMessages,
  reconcileConceptPreviewUiState,
  resolveEffectiveConceptPreviewMessage,
} from './viewportBlueprint';

function sampleMessage(overrides: Partial<Message> = {}): Message {
  return {
    id: overrides.id ?? crypto.randomUUID(),
    role: overrides.role ?? 'assistant',
    content: overrides.content ?? 'Concept direction',
    status: overrides.status ?? 'success',
    output: overrides.output ?? null,
    usage: overrides.usage ?? null,
    artifactBundle: overrides.artifactBundle ?? null,
    modelManifest: overrides.modelManifest ?? null,
    agentOrigin: overrides.agentOrigin ?? null,
    imageData: overrides.imageData ?? null,
    visualKind: overrides.visualKind ?? null,
    attachmentImages: overrides.attachmentImages ?? [],
    timestamp: overrides.timestamp ?? 1,
  };
}

test('concept preview selector keeps only assistant concept-preview images', () => {
  const concept = sampleMessage({
    id: 'concept',
    role: 'assistant',
    imageData: 'data:image/png;base64,concept',
    visualKind: 'conceptPreview',
  });
  const screenshot = sampleMessage({
    id: 'viewport-shot',
    role: 'user',
    imageData: 'data:image/png;base64,viewport',
  });
  const plainAssistantImage = sampleMessage({
    id: 'plain-image',
    role: 'assistant',
    imageData: 'data:image/png;base64,plain',
  });

  assert.equal(isConceptPreviewMessage(concept), true);
  assert.equal(isConceptPreviewMessage(screenshot), false);
  assert.equal(isConceptPreviewMessage(plainAssistantImage), false);
  assert.deepEqual(listConceptPreviewMessages([concept, screenshot, plainAssistantImage]).map((message) => message.id), [
    'concept',
  ]);
});

test('reconcileConceptPreviewUiState auto-pins the latest preview and keeps manual pin until a newer preview arrives', () => {
  const first = sampleMessage({
    id: 'first',
    imageData: 'data:image/png;base64,first',
    visualKind: 'conceptPreview',
    timestamp: 1,
  });
  const second = sampleMessage({
    id: 'second',
    imageData: 'data:image/png;base64,second',
    visualKind: 'conceptPreview',
    timestamp: 2,
  });

  const initial = reconcileConceptPreviewUiState({
    messages: [first, second],
    previous: { pinnedMessageId: null, lastAutoPinnedMessageId: null, mode: 'model' },
    hasModel: true,
  });
  assert.equal(initial.nextState.pinnedMessageId, 'second');

  const manual = reconcileConceptPreviewUiState({
    messages: [first, second],
    previous: {
      pinnedMessageId: 'first',
      lastAutoPinnedMessageId: 'second',
      mode: 'blueprint',
    },
    hasModel: true,
  });
  assert.equal(manual.nextState.pinnedMessageId, 'first');
  assert.equal(manual.effectiveMessage?.id, 'first');

  const third = sampleMessage({
    id: 'third',
    imageData: 'data:image/png;base64,third',
    visualKind: 'conceptPreview',
    timestamp: 3,
  });
  const updated = reconcileConceptPreviewUiState({
    messages: [first, second, third],
    previous: manual.nextState,
    hasModel: true,
  });
  assert.equal(updated.nextState.pinnedMessageId, 'third');
  assert.equal(updated.nextState.lastAutoPinnedMessageId, 'third');
});

test('reconcileConceptPreviewUiState forces blueprint when no model is available and preserves the current mode when a model exists', () => {
  const concept = sampleMessage({
    id: 'concept',
    imageData: 'data:image/png;base64,concept',
    visualKind: 'conceptPreview',
  });

  const noModel = reconcileConceptPreviewUiState({
    messages: [concept],
    previous: { pinnedMessageId: null, lastAutoPinnedMessageId: null, mode: 'model' },
    hasModel: false,
  });
  assert.equal(noModel.nextState.mode, 'blueprint');

  const withModel = reconcileConceptPreviewUiState({
    messages: [concept, sampleMessage({
      id: 'concept-2',
      imageData: 'data:image/png;base64,concept-2',
      visualKind: 'conceptPreview',
      timestamp: 2,
    })],
    previous: {
      pinnedMessageId: 'concept',
      lastAutoPinnedMessageId: 'concept',
      mode: 'model',
    },
    hasModel: true,
  });
  assert.equal(withModel.nextState.mode, 'model');
});

test('effective preview falls back to the latest preview and cycling wraps around', () => {
  const first = sampleMessage({
    id: 'first',
    imageData: 'data:image/png;base64,first',
    visualKind: 'conceptPreview',
    timestamp: 1,
  });
  const second = sampleMessage({
    id: 'second',
    imageData: 'data:image/png;base64,second',
    visualKind: 'conceptPreview',
    timestamp: 2,
  });

  assert.equal(resolveEffectiveConceptPreviewMessage([first, second], 'missing')?.id, 'second');
  assert.equal(cycleConceptPreviewMessageId([first, second], 'first'), 'second');
  assert.equal(cycleConceptPreviewMessageId([first, second], 'second'), 'first');
});

test('generate-from-concept prompt includes the concept note when available', () => {
  assert.match(
    buildGenerateFromConceptPrompt(sampleMessage({ content: 'Brutalist crab shrine vase' })),
    /Brutalist crab shrine vase/,
  );
  assert.match(buildGenerateFromConceptPrompt(null), /concept preview/i);
});
