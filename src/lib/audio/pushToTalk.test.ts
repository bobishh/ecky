import assert from 'node:assert/strict';
import test from 'node:test';

import { appendTranscriptToPrompt, encodePcm16Wav, supportsPromptVoiceInput } from './pushToTalk';

test('voice capture cannot route managed-provider or MCP dialogue to separate STT', () => {
  assert.equal(supportsPromptVoiceInput({ mode: 'generate' }), true);
  assert.equal(supportsPromptVoiceInput({ mode: 'mcp-idle' }), false);
  assert.equal(supportsPromptVoiceInput({ mode: 'agent-reply', requestId: 'request', agentLabel: 'Codex' }), false);
  for (const providerId of ['codex', 'agy']) {
    assert.equal(supportsPromptVoiceInput({
      mode: 'provider', providerId, externalConversationId: null,
      label: providerId, supportsSteer: false, supportsStop: true,
    }), false);
  }
});

test('encodePcm16Wav writes mono PCM WAV header and clamps samples', () => {
  const wav = encodePcm16Wav(new Float32Array([-2, 0, 2]), 16_000);
  const view = new DataView(wav.buffer, wav.byteOffset, wav.byteLength);
  const text = new TextDecoder('ascii').decode(wav);

  assert.equal(text.slice(0, 4), 'RIFF');
  assert.equal(text.slice(8, 12), 'WAVE');
  assert.equal(text.slice(12, 16), 'fmt ');
  assert.equal(view.getUint16(20, true), 1);
  assert.equal(view.getUint16(22, true), 1);
  assert.equal(view.getUint32(24, true), 16_000);
  assert.equal(text.slice(36, 40), 'data');
  assert.equal(view.getUint32(40, true), 6);
  assert.equal(view.getInt16(44, true), -32768);
  assert.equal(view.getInt16(46, true), 0);
  assert.equal(view.getInt16(48, true), 32767);
});

test('appendTranscriptToPrompt preserves existing prompt spacing', () => {
  assert.equal(appendTranscriptToPrompt('', ' make a bracket '), 'make a bracket');
  assert.equal(appendTranscriptToPrompt('make a', 'bracket'), 'make a bracket');
  assert.equal(appendTranscriptToPrompt('make a\n', 'bracket'), 'make a\nbracket');
});
