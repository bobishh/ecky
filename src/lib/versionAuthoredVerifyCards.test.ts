import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Message, Request, StructuralVerificationResult } from './types/domain';
import { buildVersionAuthoredVerifyChipMap } from './versionAuthoredVerifyCards';

function versionMessage(id: string): Message {
  return {
    id,
    role: 'assistant',
    content: 'Version ready.',
    status: 'success',
    output: {
      title: 'Bracket',
      versionName: 'V1',
      response: 'Ready.',
      interactionMode: 'design',
      macroCode: '(model)',
      sourceLanguage: 'ecky',
      geometryBackend: 'build123d',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    },
    artifactBundle: {
      modelId: 'model-1',
      sourceKind: 'generated',
      sourceLanguage: 'ecky',
      geometryBackend: 'build123d',
      contentHash: 'hash-1',
      fcstdPath: '',
      manifestPath: '/tmp/manifest.json',
      previewStlPath: '/tmp/model.stl',
      viewerAssets: [],
    },
    modelManifest: {
      modelId: 'model-1',
      sourceKind: 'generated',
      document: {
        documentName: 'Bracket',
        documentLabel: 'Bracket',
        objectCount: 1,
        warnings: [],
      },
      parts: [],
      parameterGroups: [],
      selectionTargets: [],
      warnings: [],
      enrichmentState: { status: 'none', proposals: [] },
    },
    timestamp: 1,
  };
}

function structuralResult(): StructuralVerificationResult {
  return {
    passed: true,
    summary: 'Checks passed.',
    issues: [],
    authoredVerifyChecks: [
      {
        tag: 'step_export',
        status: 'passed',
        message: 'STEP export present.',
        stableNodeId: 'verify:0',
      },
      {
        tag: 'bad_clearance',
        status: 'failed',
        message: 'Clearance 0.3mm below minimum.',
        stableNodeId: null,
      },
    ],
    metrics: {
      partCount: 1,
      totalVolume: 12,
      totalArea: 8,
      bbox: null,
      previewStlSizeBytes: 1024,
      previewStlTriangleCount: 8,
      previewStlComponentCount: 1,
      previewStlNonManifoldEdgeCount: 0,
      previewStlOverhangTriangleCount: 0,
      previewStlOverhangRatio: 0,
    },
    verifierStatus: 'ok',
    verifierSource: 'rust_structural',
  };
}

function request(messageId: string, result: StructuralVerificationResult): Request {
  return {
    id: 'req-1',
    prompt: 'make bracket',
    attachments: [],
    createdAt: 1,
    phase: 'success',
    attempt: 1,
    maxAttempts: 1,
    maxVerifyAttempts: 2,
    isQuestion: false,
    lightResponse: '',
    screenshot: null,
    threadId: 'thread-1',
    result: {
      design: versionMessage(messageId).output ?? null,
      threadId: 'thread-1',
      messageId,
      stlUrl: '/tmp/model.stl',
      artifactBundle: versionMessage(messageId).artifactBundle ?? null,
      modelManifest: versionMessage(messageId).modelManifest ?? null,
      structuralVerification: result,
    },
    error: null,
    cookingStartTime: null,
    cookingElapsed: 0,
  };
}

test('Given same-session version verification When building card chip map Then message id resolves authored verify chips', () => {
  const message = versionMessage('msg-1');
  const chipMap = buildVersionAuthoredVerifyChipMap([message], [request(message.id, structuralResult())]);

  assert.deepEqual(Object.keys(chipMap), ['msg-1']);
  assert.equal(chipMap['msg-1']?.length, 2);
  assert.deepEqual(
    chipMap['msg-1']?.map((chip) => ({
      label: chip.label,
      tone: chip.tone,
      stableNodeId: chip.stableNodeId,
    })),
    [
      { label: 'step_export', tone: 'green', stableNodeId: 'verify:0' },
      { label: 'bad_clearance', tone: 'red', stableNodeId: null },
    ],
  );
});

test('Given missing or non-version requests When building card chip map Then chips stay empty', () => {
  const chipMap = buildVersionAuthoredVerifyChipMap(
    [versionMessage('msg-1')],
    [
      {
        ...request('msg-2', structuralResult()),
        phase: 'error',
      },
    ],
  );

  assert.deepEqual(chipMap, {});
});

test('Given persisted message verification When building card chip map Then message result wins over request fallback', () => {
  const message = {
    ...versionMessage('msg-1'),
    structuralVerification: {
      ...structuralResult(),
      authoredVerifyChecks: [
        {
          tag: 'persisted_only',
          status: 'passed',
          message: 'Persisted result survives history reload.',
          stableNodeId: 'verify:persisted',
        },
      ],
    },
  } satisfies Message;

  const chipMap = buildVersionAuthoredVerifyChipMap([message], [request(message.id, structuralResult())]);

  assert.deepEqual(
    chipMap['msg-1']?.map((chip) => chip.label),
    ['persisted_only'],
  );
});
