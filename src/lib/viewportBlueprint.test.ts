import assert from 'node:assert/strict';
import test from 'node:test';

import type { Message } from './types/domain';
import {
  isConceptPreviewMessage,
  listConceptPreviewMessages,
  resolveLatestConceptPreviewMessage,
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

test('latest preview resolver returns the newest concept image', () => {
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

  assert.equal(resolveLatestConceptPreviewMessage([first, second])?.id, 'second');
});
