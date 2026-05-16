import assert from 'node:assert/strict';
import test from 'node:test';

import {
  activeVersionTimelineIndex,
  formatTimelineAgentOrigin,
  isRenderableVersionTimelineMessage,
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

function sampleArtifactBundle(modelId: string = 'model-1'): NonNullable<Message['artifactBundle']> {
  return {
    modelId,
    sourceKind: 'generated',
    contentHash: `hash-${modelId}`,
    fcstdPath: `/tmp/${modelId}.FCStd`,
    manifestPath: `/tmp/${modelId}.json`,
    previewStlPath: `/tmp/${modelId}.stl`,
    viewerAssets: [],
    exportArtifacts: [],
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

test('threadTimelineMessages hides agent tool error noise but keeps user-facing errors', () => {
  const timeline = threadTimelineMessages([
    sampleMessage({
      id: 'agent-tool-error',
      role: 'assistant',
      status: 'error',
      content: 'Expected a symbolic head for runtime list expression.',
      agentOrigin: {
        hostLabel: 'Codex MCP Client',
        clientKind: 'mcp-http',
        agentLabel: 'Ecky',
        llmModelId: null,
        llmModelLabel: null,
        sessionId: 'session-1',
        createdAt: 1,
      },
      timestamp: 2,
    }),
    sampleMessage({
      id: 'generation-error',
      role: 'assistant',
      status: 'error',
      content: 'Generation failed.',
      timestamp: 3,
    }),
  ]);

  assert.deepEqual(
    timeline.map((message) => message.id),
    ['generation-error'],
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

test('timelineVisuals preserves inline attachment image data URLs', () => {
  const visuals = timelineVisuals(
    sampleMessage({
      role: 'user',
      attachmentImages: ['data:image/png;base64,reference'],
    }),
    (path) => `asset://${path}`,
  );

  assert.equal(visuals[0]?.src, 'data:image/png;base64,reference');
});

test('versionTimeline helpers identify and label assistant version messages', () => {
  const versionMessage = sampleMessage({
    role: 'assistant',
    artifactBundle: sampleArtifactBundle(),
  });

  assert.equal(isVersionTimelineMessage(versionMessage), true);
  assert.equal(isRenderableVersionTimelineMessage(versionMessage), true);
  assert.equal(versionTimelineTitle(versionMessage), 'model-1');
});

test('versionTimelineMessages ignores output-only drafts and failed artifacts', () => {
  const outputOnly = sampleMessage({
    id: 'output-only',
    role: 'assistant',
    output: {
      title: 'Draft',
      versionName: 'V-draft',
      response: 'draft',
      interactionMode: 'design',
      macroCode: '...',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
      uiSpec: { fields: [] },
      initialParams: {},
    },
  });
  const failedArtifact = sampleMessage({
    id: 'failed-artifact',
    role: 'assistant',
    status: 'error',
    artifactBundle: sampleArtifactBundle('failed-model'),
  });
  const rendered = sampleMessage({
    id: 'rendered',
    role: 'assistant',
    artifactBundle: sampleArtifactBundle('rendered-model'),
  });

  assert.equal(isVersionTimelineMessage(outputOnly), false);
  assert.equal(isVersionTimelineMessage(failedArtifact), true);
  assert.equal(isRenderableVersionTimelineMessage(failedArtifact), false);
  assert.deepEqual(
    versionTimelineMessages([outputOnly, failedArtifact, rendered]).map((message) => message.id),
    ['rendered'],
  );
});

test('versionTimelineMessages and activeVersionTimelineIndex keep version navigation stable', () => {
  const userMessage = sampleMessage({ id: 'user-1', role: 'user', timestamp: 1 });
  const versionA = sampleMessage({
    id: 'version-a',
    role: 'assistant',
    timestamp: 2,
    artifactBundle: sampleArtifactBundle('model-a'),
    output: {
      title: 'Lamp',
      versionName: 'V-a',
      response: 'a',
      interactionMode: 'design',
      macroCode: '...',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
      uiSpec: { fields: [] },
      initialParams: {},
    },
  });
  const versionB = sampleMessage({
    id: 'version-b',
    role: 'assistant',
    timestamp: 3,
    artifactBundle: sampleArtifactBundle('model-b'),
    output: {
      title: 'Lamp',
      versionName: 'V-b',
      response: 'b',
      interactionMode: 'design',
      macroCode: '...',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
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
    artifactBundle: sampleArtifactBundle('model-live'),
    output: {
      title: 'Lamp',
      versionName: 'V-live',
      response: 'live',
      interactionMode: 'design',
      macroCode: 'live()',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
      uiSpec: { fields: [] },
      initialParams: {},
    },
  });
  const discardedVersion = sampleMessage({
    id: 'version-discarded',
    role: 'assistant',
    status: 'discarded',
    timestamp: 3,
    artifactBundle: sampleArtifactBundle('model-discarded'),
    output: {
      title: 'Lamp',
      versionName: 'V-discarded',
      response: 'discarded',
      interactionMode: 'design',
      macroCode: 'discarded()',
      sourceLanguage: 'legacyPython',
      geometryBackend: 'freecad',
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
