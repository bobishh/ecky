import assert from 'node:assert/strict';
import test from 'node:test';

import { runVerificationRound, runTwoStageVerification } from './verificationLoop';
import type { VerificationLoopOptions, TwoStageOptions } from './verificationLoop';
import type { StructuralVerificationResult, VisualVerificationResult } from '../types/domain';

const SCREENSHOTS = ['data:image/jpeg;base64,angle1', 'data:image/jpeg;base64,angle2'];

function visualPass(): VisualVerificationResult {
  return { passed: true, summary: 'Looks correct.', issues: [] };
}

function visualFail(description: string): VisualVerificationResult {
  return {
    passed: false,
    summary: description,
    issues: [{ category: 'other', description, partLabel: null }],
  };
}

function baseOpts(overrides: Partial<VerificationLoopOptions> = {}): VerificationLoopOptions {
  return {
    originalPrompt: 'make a dome with radius 30',
    maxVerifyAttempts: 2,
    currentGenerationAttempt: 1,
    maxGenerationAttempts: 3,
    capture: () => SCREENSHOTS,
    verify: async () => visualPass(),
    ...overrides,
  };
}

// ── disabled / skipped ───────────────────────────────────────────────────────

test('skips verification when maxVerifyAttempts is 0', async () => {
  const result = await runVerificationRound(1, baseOpts({ maxVerifyAttempts: 0 }));
  assert.equal(result.kind, 'skipped');
  assert.match((result as any).reason, /disabled/);
});

test('skips when capture returns no screenshots (viewer not ready)', async () => {
  const result = await runVerificationRound(1, baseOpts({ capture: () => [] }));
  assert.equal(result.kind, 'skipped');
  assert.match((result as any).reason, /viewer not ready/);
});

test('skips when the verify call throws', async () => {
  const result = await runVerificationRound(1, baseOpts({
    verify: async () => { throw new Error('network error'); },
  }));
  assert.equal(result.kind, 'skipped');
  assert.match((result as any).reason, /failed/);
});

// ── passed ───────────────────────────────────────────────────────────────────

test('returns passed when LLM says the model is correct', async () => {
  const result = await runVerificationRound(1, baseOpts({
    verify: async () => visualPass(),
  }));
  assert.equal(result.kind, 'passed');
});

// ── repair needed ────────────────────────────────────────────────────────────

test('returns repair_needed when model fails and retries remain', async () => {
  const result = await runVerificationRound(1, baseOpts({
    verify: async () => visualFail('dome renders as wireframe'),
    maxVerifyAttempts: 2,
    currentGenerationAttempt: 1,
    maxGenerationAttempts: 3,
  }));
  assert.equal(result.kind, 'repair_needed');
  assert.match((result as any).repairPrompt, /wireframe/);
  assert.match((result as any).repairPrompt, /make a dome/);
});

test('repair prompt contains original user request', async () => {
  const result = await runVerificationRound(1, baseOpts({
    originalPrompt: 'hollow sphere radius 50',
    verify: async () => visualFail('sphere is solid not hollow'),
  }));
  assert.equal(result.kind, 'repair_needed');
  assert.match((result as any).repairPrompt, /hollow sphere radius 50/);
});

// ── terminal failure ─────────────────────────────────────────────────────────

test('returns failed_terminal when last verify attempt is exhausted', async () => {
  const result = await runVerificationRound(2, baseOpts({
    verify: async () => visualFail('shape is wrong'),
    maxVerifyAttempts: 2,
    currentGenerationAttempt: 1,
    maxGenerationAttempts: 3,
  }));
  assert.equal(result.kind, 'failed_terminal');
  assert.match((result as any).issues, /wrong/);
});

test('returns failed_terminal when generation attempts also exhausted', async () => {
  const result = await runVerificationRound(1, baseOpts({
    verify: async () => visualFail('topology error'),
    maxVerifyAttempts: 3,
    currentGenerationAttempt: 3,
    maxGenerationAttempts: 3,
  }));
  assert.equal(result.kind, 'failed_terminal');
});

// ── screenshots passed to verify ─────────────────────────────────────────────

test('verify receives exactly the screenshots from capture', async () => {
  let capturedScreenshots: string[] = [];
  await runVerificationRound(1, baseOpts({
    verify: async (_prompt, shots) => {
      capturedScreenshots = shots;
      return visualPass();
    },
  }));
  assert.deepEqual(capturedScreenshots, SCREENSHOTS);
});

test('verify receives the original prompt', async () => {
  let capturedPrompt = '';
  await runVerificationRound(1, baseOpts({
    originalPrompt: 'a twisted vase',
    verify: async (prompt) => {
      capturedPrompt = prompt;
      return visualPass();
    },
  }));
  assert.equal(capturedPrompt, 'a twisted vase');
});

// ── Two-stage verification ──────────────────────────────────────────────────

const STRUCTURAL_PASS: StructuralVerificationResult = {
  passed: true,
  summary: 'All structural checks passed.',
  issues: [],
  metrics: { partCount: 1, previewStlSizeBytes: 1024, totalVolume: 1000, totalArea: 600, bbox: null },
  verifierStatus: 'ok',
};

const STRUCTURAL_FAIL: StructuralVerificationResult = {
  passed: false,
  summary: 'Structural verification failed: PREVIEW_STL_MISSING',
  issues: [{ code: 'PREVIEW_STL_MISSING', message: 'Preview STL not found.', partId: null, numericPayload: null }],
  metrics: { partCount: 0, previewStlSizeBytes: null, totalVolume: null, totalArea: null, bbox: null },
  verifierStatus: 'ok',
};

const STRUCTURAL_SKIPPED: StructuralVerificationResult = {
  passed: false,
  summary: 'Verifier unavailable.',
  issues: [],
  metrics: { partCount: 0, previewStlSizeBytes: null, totalVolume: null, totalArea: null, bbox: null },
  verifierStatus: 'skipped_unavailable',
};

function twoStageOpts(overrides: Partial<TwoStageOptions> = {}): TwoStageOptions {
  return {
    modelId: 'generated-test-001',
    originalPrompt: 'make a dome with radius 30',
    maxVerifyAttempts: 2,
    currentGenerationAttempt: 1,
    maxGenerationAttempts: 3,
    capture: () => SCREENSHOTS,
    verifyScreenshot: async () => visualPass(),
    verifyStructural: async () => STRUCTURAL_PASS,
    ...overrides,
  };
}

test('two-stage: structural fail triggers repair without running screenshots', async () => {
  let screenshotCalled = false;
  const result = await runTwoStageVerification(1, twoStageOpts({
    verifyStructural: async () => STRUCTURAL_FAIL,
    verifyScreenshot: async () => { screenshotCalled = true; return visualPass(); },
  }));
  assert.equal(result.kind, 'repair_needed');
  assert.ok('repairPrompt' in result);
  assert.match(result.repairPrompt, /PREVIEW_STL_MISSING/);
  assert.equal(screenshotCalled, false);
});

test('two-stage: structural pass + screenshot pass → passed', async () => {
  const result = await runTwoStageVerification(1, twoStageOpts({
    verifyStructural: async () => STRUCTURAL_PASS,
    verifyScreenshot: async () => visualPass(),
  }));
  assert.equal(result.kind, 'passed');
});

test('two-stage: structural pass + screenshot fail → repair_needed with screenshot issues', async () => {
  const result = await runTwoStageVerification(1, twoStageOpts({
    verifyStructural: async () => STRUCTURAL_PASS,
    verifyScreenshot: async () => visualFail('dome missing cap'),
  }));
  assert.equal(result.kind, 'repair_needed');
  assert.ok('repairPrompt' in result);
  assert.match(result.repairPrompt, /dome missing cap/);
});

test('two-stage: structural skipped → falls through to screenshot verification', async () => {
  let screenshotCalled = false;
  const result = await runTwoStageVerification(1, twoStageOpts({
    verifyStructural: async () => STRUCTURAL_SKIPPED,
    verifyScreenshot: async () => { screenshotCalled = true; return visualPass(); },
  }));
  assert.equal(result.kind, 'passed');
  assert.equal(screenshotCalled, true);
});

test('two-stage: structural terminal failure (last attempt) returns structural findings', async () => {
  const result = await runTwoStageVerification(1, twoStageOpts({
    verifyStructural: async () => STRUCTURAL_FAIL,
    currentGenerationAttempt: 3,
    maxGenerationAttempts: 3,
  }));
  assert.equal(result.kind, 'failed_terminal');
  assert.ok('issues' in result);
  assert.match(result.issues, /PREVIEW_STL_MISSING/);
});

test('two-stage: structural verify throws → falls through to screenshot', async () => {
  let screenshotCalled = false;
  const result = await runTwoStageVerification(1, twoStageOpts({
    verifyStructural: async () => { throw new Error('crash'); },
    verifyScreenshot: async () => { screenshotCalled = true; return visualPass(); },
  }));
  assert.equal(result.kind, 'passed');
  assert.equal(screenshotCalled, true);
});
