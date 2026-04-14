import assert from 'node:assert/strict';
import test from 'node:test';

import { runStructuralCheck } from './structuralVerification';
import type { StructuralCheckOptions } from './structuralVerification';
import type { StructuralVerificationResult } from '../types/domain';

const PASS_RESULT: StructuralVerificationResult = {
  passed: true,
  summary: 'All structural checks passed.',
  issues: [],
  metrics: {
    partCount: 1,
    previewStlSizeBytes: 1024,
    totalVolume: 1000,
    totalArea: 600,
    bbox: { xMin: -10, yMin: -10, zMin: 0, xMax: 10, yMax: 10, zMax: 20 },
  },
  verifierStatus: 'ok',
};

const FAIL_RESULT: StructuralVerificationResult = {
  passed: false,
  summary: 'Structural verification failed: PREVIEW_STL_MISSING',
  issues: [
    {
      code: 'PREVIEW_STL_MISSING',
      message: 'Preview STL file not found.',
      partId: null,
      numericPayload: null,
    },
  ],
  metrics: {
    partCount: 0,
    previewStlSizeBytes: null,
    totalVolume: null,
    totalArea: null,
    bbox: null,
  },
  verifierStatus: 'ok',
};

const SKIPPED_RESULT: StructuralVerificationResult = {
  passed: false,
  summary: 'Verifier unavailable.',
  issues: [],
  metrics: {
    partCount: 0,
    previewStlSizeBytes: null,
    totalVolume: null,
    totalArea: null,
    bbox: null,
  },
  verifierStatus: 'skipped_unavailable',
};

function baseOpts(overrides: Partial<StructuralCheckOptions> = {}): StructuralCheckOptions {
  return {
    modelId: 'generated-test-001',
    originalPrompt: 'make a dome with radius 30',
    currentGenerationAttempt: 1,
    maxGenerationAttempts: 3,
    verify: async () => PASS_RESULT,
    ...overrides,
  };
}

// ── structural pass ─────────────────────────────────────────────────────────

test('returns structural_passed when all checks pass', async () => {
  const result = await runStructuralCheck(baseOpts());
  assert.equal(result.kind, 'structural_passed');
  assert.ok('metrics' in result && result.metrics.partCount === 1);
});

// ── repair needed ───────────────────────────────────────────────────────────

test('returns repair_needed when structural check fails and retries remain', async () => {
  const result = await runStructuralCheck(baseOpts({
    verify: async () => FAIL_RESULT,
    currentGenerationAttempt: 1,
    maxGenerationAttempts: 3,
  }));
  assert.equal(result.kind, 'repair_needed');
  assert.ok('repairPrompt' in result);
  assert.match(result.repairPrompt, /PREVIEW_STL_MISSING/);
  assert.match(result.repairPrompt, /make a dome/);
});

test('repair prompt includes issue codes and summary', async () => {
  const multiIssueFail: StructuralVerificationResult = {
    ...FAIL_RESULT,
    summary: 'Structural verification failed: PREVIEW_STL_MISSING, MANIFEST_PARTS_EMPTY',
    issues: [
      { code: 'PREVIEW_STL_MISSING', message: 'Preview STL file not found.', partId: null, numericPayload: null },
      { code: 'MANIFEST_PARTS_EMPTY', message: 'Manifest contains no parts.', partId: null, numericPayload: null },
    ],
  };
  const result = await runStructuralCheck(baseOpts({
    verify: async () => multiIssueFail,
  }));
  assert.equal(result.kind, 'repair_needed');
  assert.ok('repairPrompt' in result);
  assert.match(result.repairPrompt, /PREVIEW_STL_MISSING/);
  assert.match(result.repairPrompt, /MANIFEST_PARTS_EMPTY/);
});

// ── terminal failure ────────────────────────────────────────────────────────

test('returns failed_terminal when structural check fails on last attempt', async () => {
  const result = await runStructuralCheck(baseOpts({
    verify: async () => FAIL_RESULT,
    currentGenerationAttempt: 3,
    maxGenerationAttempts: 3,
  }));
  assert.equal(result.kind, 'failed_terminal');
  assert.ok('issues' in result);
  assert.match(result.issues, /PREVIEW_STL_MISSING/);
});

// ── skipped ─────────────────────────────────────────────────────────────────

test('returns structural_skipped when verifier is unavailable', async () => {
  const result = await runStructuralCheck(baseOpts({
    verify: async () => SKIPPED_RESULT,
  }));
  assert.equal(result.kind, 'structural_skipped');
  assert.ok('reason' in result);
  assert.match(result.reason, /unavailable/i);
});

test('returns structural_skipped when verify throws', async () => {
  const result = await runStructuralCheck(baseOpts({
    verify: async () => { throw new Error('backend unavailable'); },
  }));
  assert.equal(result.kind, 'structural_skipped');
  assert.ok('reason' in result);
  assert.match(result.reason, /failed/i);
});

// ── verify receives correct arguments ───────────────────────────────────────

test('verify receives modelId and originalPrompt', async () => {
  let capturedModelId = '';
  let capturedPrompt = '';
  await runStructuralCheck(baseOpts({
    modelId: 'generated-my-model',
    originalPrompt: 'a twisted vase',
    verify: async (modelId, prompt) => {
      capturedModelId = modelId;
      capturedPrompt = prompt;
      return PASS_RESULT;
    },
  }));
  assert.equal(capturedModelId, 'generated-my-model');
  assert.equal(capturedPrompt, 'a twisted vase');
});
