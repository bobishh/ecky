import assert from 'node:assert/strict';
import test from 'node:test';

import {
  inferModelCapabilities,
  isVisionCapableModel,
  visionUnavailableReason,
} from './modelCapabilities';

test('treats NVIDIA hosted llama instruct models as text-only by default', () => {
  assert.equal(
    isVisionCapableModel('openai', 'https://integrate.api.nvidia.com/v1', 'meta/llama-3.1-70b-instruct'),
    false,
  );
});

test('treats NVIDIA hosted multimodal models as vision-capable', () => {
  assert.equal(
    isVisionCapableModel('openai', 'https://integrate.api.nvidia.com/v1', 'microsoft/phi-4-multimodal-instruct'),
    true,
  );
  assert.equal(
    isVisionCapableModel('openai', 'https://integrate.api.nvidia.com/v1', 'nvidia / nemotron-nano-12b-v2-vl'),
    true,
  );
});

test('does not restrict non-NIM OpenAI-compatible endpoints', () => {
  assert.equal(
    isVisionCapableModel('openai', 'https://api.openai.com/v1', 'gpt-4.1'),
    true,
  );
});

test('does not warn before a NIM model is selected', () => {
  assert.deepEqual(
    inferModelCapabilities('openai', 'https://integrate.api.nvidia.com/v1', ''),
    {
      supportsVision: true,
      reason: null,
    },
  );
});

test('returns explanatory reason for text-only NIM models', () => {
  assert.match(
    visionUnavailableReason('openai', 'https://integrate.api.nvidia.com/v1', 'meta/llama-3.1-70b-instruct') ?? '',
    /nvidia nim/i,
  );
});

test('exposes normalized capability summary', () => {
  assert.deepEqual(
    inferModelCapabilities('openai', 'https://integrate.api.nvidia.com/v1', 'meta/llama-3.1-70b-instruct'),
    {
      supportsVision: false,
      reason:
        'Selected NVIDIA NIM model looks text-only. Image attachments, concept-preview reuse, and screenshot verification are unavailable.',
    },
  );
});
