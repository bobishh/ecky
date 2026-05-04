import assert from 'node:assert/strict';
import test from 'node:test';

import { resolveAssistantSpeechText, resolveGenieSpeechCue } from './speechPolicy';
import type { Message } from '../types/domain';

function assistantMessage(overrides: Partial<Message>): Message {
  return {
    id: 'msg-1',
    role: 'assistant',
    content: 'Assistant content.',
    status: 'success',
    output: null,
    usage: null,
    artifactBundle: null,
    modelManifest: null,
    agentOrigin: null,
    imageData: null,
    visualKind: null,
    attachmentImages: [],
    timestamp: 100,
    ...overrides,
  };
}

test('resolveAssistantSpeechText speaks final generated LLM responses', () => {
  const message = assistantMessage({
    output: {
      title: 'Bracket',
      versionName: 'V1',
      response: 'Bracket generated with two mounting holes.',
      interactionMode: 'design',
      macroCode: 'from build123d import *',
      macroDialect: 'build123d',
      engineKind: 'build123d',
      sourceLanguage: 'build123d',
      geometryBackend: 'build123d',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    },
    artifactBundle: {
      modelId: 'model-1',
      sourceKind: 'generated',
      contentHash: 'hash',
      fcstdPath: '/tmp/model.FCStd',
      manifestPath: '/tmp/manifest.json',
      previewStlPath: '/tmp/preview.stl',
      viewerAssets: [],
    },
  });

  assert.equal(resolveAssistantSpeechText(message), 'Bracket generated with two mounting holes.');
});

test('resolveAssistantSpeechText suppresses MCP working version draft updates', () => {
  const message = assistantMessage({
    content: 'Primary updated V-mcp-20260501.',
    output: {
      title: 'Ramp',
      versionName: 'V-mcp-20260501',
      response: 'Draft update via macro replacement.',
      interactionMode: 'design',
      macroCode: '(model)',
      macroDialect: 'ecky',
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'freecad',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    },
    artifactBundle: {
      modelId: 'draft-model',
      sourceKind: 'generated',
      contentHash: 'hash',
      fcstdPath: '/tmp/draft.FCStd',
      manifestPath: '/tmp/draft-manifest.json',
      previewStlPath: '/tmp/draft.stl',
      viewerAssets: [],
    },
    agentOrigin: {
      hostLabel: 'Codex',
      clientKind: 'mcp',
      agentLabel: 'Primary',
      llmModelId: 'gpt-5',
      llmModelLabel: 'GPT-5',
      sessionId: 'session-1',
      createdAt: 100,
    },
  });

  assert.equal(resolveAssistantSpeechText(message), '');
});

test('resolveAssistantSpeechText speaks raw error messages', () => {
  const message = assistantMessage({
    status: 'error',
    content: 'Render Error: FreeCAD failed with exit code 1.',
  });

  assert.equal(resolveAssistantSpeechText(message), 'Render Error: FreeCAD failed with exit code 1.');
});

test('resolveAssistantSpeechText ignores working status copy', () => {
  const message = assistantMessage({
    status: 'working',
    content: 'Rendering model...',
  });

  assert.equal(resolveAssistantSpeechText(message), '');
});

test('resolveGenieSpeechCue prefers active request error bubble over stale assistant replies', () => {
  const message = assistantMessage({
    id: 'old-assistant',
    output: {
      title: 'Old',
      versionName: 'V1',
      response: 'Old successful response.',
      interactionMode: 'design',
      macroCode: '',
      macroDialect: 'build123d',
      engineKind: 'build123d',
      sourceLanguage: 'build123d',
      geometryBackend: 'build123d',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    },
  });

  assert.deepEqual(
    resolveGenieSpeechCue({
      latestAssistantMessage: message,
      assistantFresh: true,
      visibleBubble: 'Generation Failed: provider timed out',
      activeErrorId: 'req-1',
      activeErrorText: 'Generation Failed: provider timed out',
    }),
    {
      key: 'error:req-1:Generation Failed: provider timed out',
      text: 'Generation Failed: provider timed out',
    },
  );
});

test('resolveGenieSpeechCue does not speak hidden stale assistant replies', () => {
  const message = assistantMessage({
    id: 'fresh-assistant',
    output: {
      title: 'Fresh',
      versionName: 'V1',
      response: 'Fresh successful response.',
      interactionMode: 'design',
      macroCode: '',
      macroDialect: 'build123d',
      engineKind: 'build123d',
      sourceLanguage: 'build123d',
      geometryBackend: 'build123d',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    },
  });

  assert.equal(
    resolveGenieSpeechCue({
      latestAssistantMessage: message,
      assistantFresh: true,
      visibleBubble: 'Rendering model...',
      activeErrorId: null,
      activeErrorText: null,
    }),
    null,
  );
});
