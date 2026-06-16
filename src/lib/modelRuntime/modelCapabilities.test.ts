import assert from 'node:assert/strict';
import test from 'node:test';

import {
  inferModelCapabilities,
  inferVisionByName,
  isVisionCapableModel,
  overrideForEngine,
  resolveEngineCapabilitySummary,
  resolveEngineVision,
  visionUnavailableReason,
} from './modelCapabilities';

test('treats instruct models as text-only by name pattern', () => {
  assert.equal(inferVisionByName('meta/llama-3.1-70b-instruct'), false);
  assert.equal(inferVisionByName('nvidia / nemotron-nano-12b-v2-instruct'), false);
});

test('treats multimodal / vl models as vision-capable by name pattern', () => {
  assert.equal(inferVisionByName('microsoft/phi-4-multimodal-instruct'), true);
  assert.equal(inferVisionByName('nvidia / nemotron-nano-12b-v2-vl'), true);
});

test('treats GLM text models as text-only', () => {
  assert.equal(inferVisionByName('glm-5.2'), false);
  assert.equal(inferVisionByName('glm-4.5-air'), false);
  assert.equal(inferVisionByName('glm-4.6'), false);
});

test('treats GLM vision models as vision-capable', () => {
  assert.equal(inferVisionByName('glm-5v-turbo'), true);
  assert.equal(inferVisionByName('glm-4v'), true);
});

test('treats known vision-capable families as vision', () => {
  assert.equal(inferVisionByName('gpt-4o'), true);
  assert.equal(inferVisionByName('gpt-4.1'), true);
  assert.equal(inferVisionByName('claude-opus-4'), true);
  assert.equal(inferVisionByName('gemini-2.5-flash'), true);
});

test('returns null (optimistic) for unknown model names', () => {
  assert.equal(inferVisionByName('some-custom-finetune'), null);
  assert.equal(inferVisionByName(''), null);
});

test('legacy field-based helpers default to vision-capable for unknown names', () => {
  assert.equal(isVisionCapableModel('openai', 'https://api.openai.com/v1', 'custom-model'), true);
});

test('does not warn before a model is selected', () => {
  assert.deepEqual(
    inferModelCapabilities('openai', 'https://integrate.api.nvidia.com/v1', ''),
    { supportsVision: true, reason: null },
  );
});

test('returns explanatory reason for text-only models', () => {
  assert.match(
    visionUnavailableReason('openai', 'https://integrate.api.nvidia.com/v1', 'meta/llama-3.1-70b-instruct') ?? '',
    /text-only/i,
  );
});

test('engine override is authoritative for vision', () => {
  const engine = {
    provider: 'openai',
    baseUrl: 'https://api.z.ai/api/coding/paas/v4',
    model: 'glm-5.2',
    visionOverrides: { 'glm-5.2': 'vision' as const },
  };
  assert.equal(overrideForEngine(engine, 'glm-5.2'), 'vision');
  assert.equal(resolveEngineVision(engine), true);
});

test('engine override is authoritative for text-only even when name looks vision-capable', () => {
  const engine = {
    provider: 'openai',
    baseUrl: '',
    model: 'gpt-4o',
    visionOverrides: { 'gpt-4o': 'textOnly' as const },
  };
  assert.equal(resolveEngineVision(engine), false);
});

test('engine resolution falls back to name inference when no override', () => {
  const engine = {
    provider: 'zai',
    baseUrl: 'https://api.z.ai/api/coding/paas/v4',
    model: 'glm-5.2',
    visionOverrides: {},
  };
  assert.equal(resolveEngineVision(engine), false);
  assert.equal(resolveEngineCapabilitySummary(engine).supportsVision, false);
  assert.match(resolveEngineCapabilitySummary(engine).reason ?? '', /text-only/i);
});

test('engine resolution is optimistic for unknown models', () => {
  const engine = {
    provider: 'openai',
    baseUrl: 'https://api.openai.com/v1',
    model: 'custom-finetune',
    visionOverrides: {},
  };
  assert.equal(resolveEngineVision(engine), true);
});
