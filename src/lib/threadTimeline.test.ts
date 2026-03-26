import assert from 'node:assert/strict';
import test from 'node:test';

import {
  activeVersionTimelineIndex,
  formatTimelineAgentOrigin,
  isVersionTimelineMessage,
  threadTimelineMessages,
  timelineVisuals,
  versionTimelineMessages,
  versionTimelineTitle,
} from './threadTimeline';
import type { Message } from './types/domain';

function sampleMessage(overrides: Partial<Message>): Message {
  return {
    id: overrides.id ?? 'msg-1',
    role: overrides.role ?? 'user',
    content: overrides.content ?? '',
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

test('threadTimelineMessages keeps a single flat sorted visible history', () => {
  const timeline = threadTimelineMessages([
    sampleMessage({ id: 'discarded', timestamp: 3, status: 'discarded' }),
    sampleMessage({ id: 'later', timestamp: 2 }),
    sampleMessage({ id: 'earlier', timestamp: 1 }),
  ]);

  assert.deepEqual(
    timeline.map((message) => message.id),
    ['earlier', 'later'],
  );
});

test('threadTimelineMessages preserves source order when timestamps collide', () => {
  const timeline = threadTimelineMessages([
    sampleMessage({ id: 'user-last', timestamp: 5 }),
    sampleMessage({ id: 'assistant-after', role: 'assistant', timestamp: 5 }),
  ]);

  assert.deepEqual(
    timeline.map((message) => message.id),
    ['user-last', 'assistant-after'],
  );
});

test('timelineVisuals converts attachment image paths through the provided asset helper', () => {
  const visuals = timelineVisuals(
    sampleMessage({
      role: 'assistant',
      imageData: 'data:image/png;base64,concept',
      attachmentImages: ['/tmp/ref.png'],
    }),
    (path) => `asset://${path}`,
  );

  assert.equal(visuals[0]?.src, 'data:image/png;base64,concept');
  assert.equal(visuals[1]?.src, 'asset:///tmp/ref.png');
});

test('versionTimeline helpers identify and label assistant version messages', () => {
  const versionMessage = sampleMessage({
    role: 'assistant',
    artifactBundle: {
      modelId: 'model-1',
      sourceKind: 'generated',
      contentHash: 'hash',
      fcstdPath: '/tmp/model.FCStd',
      manifestPath: '/tmp/model.json',
      previewStlPath: '/tmp/model.stl',
      viewerAssets: [],
      exportArtifacts: [],
    },
  });

  assert.equal(isVersionTimelineMessage(versionMessage), true);
  assert.equal(versionTimelineTitle(versionMessage), 'model-1');
});

test('versionTimelineMessages and activeVersionTimelineIndex keep version navigation stable', () => {
  const userMessage = sampleMessage({ id: 'user-1', role: 'user', timestamp: 1 });
  const versionA = sampleMessage({
    id: 'version-a',
    role: 'assistant',
    timestamp: 2,
    output: {
      title: 'Lamp',
      versionName: 'V-a',
      response: 'a',
      interactionMode: 'design',
      macroCode: 'a()',
      uiSpec: { fields: [] },
      initialParams: {},
    },
  });
  const versionB = sampleMessage({
    id: 'version-b',
    role: 'assistant',
    timestamp: 3,
    output: {
      title: 'Lamp',
      versionName: 'V-b',
      response: 'b',
      interactionMode: 'design',
      macroCode: 'b()',
      uiSpec: { fields: [] },
      initialParams: {},
    },
  });

  const versions = versionTimelineMessages([versionB, userMessage, versionA]);
  assert.deepEqual(
    versions.map((message) => message.id),
    ['version-a', 'version-b'],
  );
  assert.equal(activeVersionTimelineIndex(versions, 'version-a'), 0);
  assert.equal(activeVersionTimelineIndex(versions, 'missing'), 1);
});

test('discarded version messages stay in the timeline but drop out of the carousel', () => {
  const liveVersion = sampleMessage({
    id: 'version-live',
    role: 'assistant',
    timestamp: 2,
    output: {
      title: 'Lamp',
      versionName: 'V-live',
      response: 'live',
      interactionMode: 'design',
      macroCode: 'live()',
      uiSpec: { fields: [] },
      initialParams: {},
    },
  });
  const discardedVersion = sampleMessage({
    id: 'version-discarded',
    role: 'assistant',
    status: 'discarded',
    timestamp: 3,
    output: {
      title: 'Lamp',
      versionName: 'V-discarded',
      response: 'discarded',
      interactionMode: 'design',
      macroCode: 'discarded()',
      uiSpec: { fields: [] },
      initialParams: {},
    },
  });

  const timeline = threadTimelineMessages([liveVersion, discardedVersion]);
  const versions = versionTimelineMessages([liveVersion, discardedVersion]);

  assert.deepEqual(
    timeline.map((message) => message.id),
    ['version-live', 'version-discarded'],
  );
  assert.deepEqual(
    versions.map((message) => message.id),
    ['version-live'],
  );
});

test('formatTimelineAgentOrigin keeps host-only labels when model matches host', () => {
  assert.equal(
    formatTimelineAgentOrigin({
      hostLabel: 'Claude',
      clientKind: 'mcp-http',
      agentLabel: 'Claude',
      llmModelId: 'Claude',
      llmModelLabel: 'Claude',
      sessionId: 'session-1',
      createdAt: 1,
    }),
    'Claude',
  );
});
