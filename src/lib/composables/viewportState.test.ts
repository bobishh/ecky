import assert from 'node:assert/strict';
import test from 'node:test';

import { deriveViewportState } from './viewportState';
import type { AgentOrigin, ArtifactBundle, Message, ViewportCameraState } from '../types/domain';
import type { ConceptPreviewUiState } from '../viewportBlueprint';

function bundle(): ArtifactBundle {
  return {
    schemaVersion: 2,
    modelId: 'model-1',
    sourceKind: 'generated',
    contentHash: 'hash-1',
    artifactVersion: 3,
    fcstdPath: '/tmp/model.FCStd',
    manifestPath: '/tmp/model.json',
    macroPath: '/tmp/model.py',
    previewStlPath: '/tmp/model.stl',
    viewerAssets: [
      {
        partId: 'body',
        nodeId: 'body-node',
        objectName: 'Body001',
        label: 'Body',
        path: '/tmp/body.stl',
        format: 'stl',
      },
    ],
    edgeTargets: [],
    calloutAnchors: [],
    measurementGuides: [],
  };
}

function conceptMessage(id: string): Message {
  return {
    id,
    role: 'assistant',
    content: 'preview',
    status: 'success',
    output: null,
    usage: null,
    artifactBundle: null,
    modelManifest: null,
    agentOrigin: {
      hostLabel: 'Ecky',
      clientKind: 'mcp',
      agentLabel: 'Mesh',
      llmModelId: 'gemini',
      llmModelLabel: 'Gemini',
      sessionId: 'session-1',
      createdAt: 1,
    } as AgentOrigin,
    imageData: 'data:image/png;base64,abc',
    visualKind: 'conceptPreview',
    attachmentImages: [],
    timestamp: 1000,
    deletedAt: null,
  };
}

test('deriveViewportState resolves preview mode, URLs, and active viewport keys', () => {
  const cameraState: ViewportCameraState = {
    position: [1, 2, 3],
    target: [0, 0, 0],
    zoom: 1,
    fov: 35,
  };
  const state = deriveViewportState({
    activeThreadId: 'thread-1',
    activeVersionId: 'msg-1',
    activeArtifactBundle: bundle(),
    activeVersionMessage: conceptMessage('msg-1'),
    activeThreadMessages: [conceptMessage('msg-1')],
    stlUrl: 'file:///tmp/model.stl',
    conceptPreviewUiByThread: {
      'thread-1': {
        pinnedMessageId: 'msg-1',
        lastAutoPinnedMessageId: 'msg-1',
        mode: 'blueprint',
      } satisfies ConceptPreviewUiState,
    },
    cameraStateByTarget: {
      'thread-1:msg-1': cameraState,
    },
    toAssetUrl: (path) => `asset:${path ?? ''}`,
  });

  assert.equal(state.viewerAssets[0]?.path, 'asset:/tmp/body.stl');
  assert.equal(state.hasRenderableModel, true);
  assert.equal(state.activeThreadConceptPreviewState.mode, 'blueprint');
  assert.equal(state.effectiveConceptPreviewMessage?.id, 'msg-1');
  assert.equal(state.viewportPresentationMode, 'blueprint');
  assert.equal(state.showBlueprintViewport, true);
  assert.equal(state.blueprintAttentionVisible, false);
  assert.equal(state.currentViewportTargetKey, 'thread-1:msg-1');
  assert.equal(state.currentViewerModelKey, 'thread-1:msg-1:model-1:3:hash-1');
  assert.deepEqual(state.persistedViewportCameraState, cameraState);
  assert.equal(state.activeVersionAgentLabel, 'Ecky · Gemini');
});
