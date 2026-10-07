import assert from 'node:assert/strict';
import test from 'node:test';
import { CodexVoiceMedia } from './codexVoice';

test('closing voice during pending microphone permission releases late tracks without starting provider', async () => {
  let resolveMic!: (stream: MediaStream) => void;
  const calls: string[] = [];
  const media = new CodexVoiceMedia('thread', {
    microphone: () => new Promise((resolve) => { resolveMic = resolve; }),
    start: async () => { calls.push('start'); throw new Error('must not start'); },
    stop: async () => { calls.push('stop'); },
    listen: async () => () => {},
    peer: () => { throw new Error('must not create peer'); },
    audio: () => { throw new Error('must not create audio'); },
    sessionId: () => 'session',
  }, () => {});
  const connecting = media.connect();
  await media.close();
  resolveMic({ getTracks: () => [{ stop: () => calls.push('track-stop') }] } as unknown as MediaStream);
  await connecting;
  assert.deepEqual(calls, ['track-stop']);
});

test('ending pending provider negotiation sends stop intent before its acknowledgement', async () => {
  const calls: string[] = [];
  let resolveStart!: (value: { threadId: string; sessionId: string; sdp: string }) => void;
  const peer = {
    localDescription: { sdp: 'offer' }, addTrack() {}, createDataChannel() {},
    createOffer: async () => ({ type: 'offer', sdp: 'offer' }),
    setLocalDescription: async () => {}, setRemoteDescription: async () => {}, close() {},
  } as unknown as RTCPeerConnection;
  const media = new CodexVoiceMedia('thread', {
    microphone: async () => ({ getTracks: () => [{ stop: () => calls.push('track-stop') }] }) as unknown as MediaStream,
    peer: () => peer,
    audio: () => ({ pause() {}, srcObject: null }) as unknown as HTMLAudioElement,
    sessionId: () => 'session', listen: async () => () => {},
    start: () => { calls.push('start'); return new Promise((resolve) => { resolveStart = resolve; }); },
    stop: async () => { calls.push('stop'); },
  }, () => {});
  const connecting = media.connect();
  while (!resolveStart) await new Promise((resolve) => setTimeout(resolve, 0));
  const closing = media.close();
  await Promise.resolve();
  try { assert.deepEqual(calls, ['start', 'track-stop', 'stop']); }
  finally { resolveStart({ threadId: 'native', sessionId: 'session', sdp: 'answer' }); await connecting; await closing; }
});

test('pending media consumes native errors for its own session only', async () => {
  const states: string[] = [];
  let emit!: (event: any) => void;
  let resolveStart!: (value: { threadId: string; sessionId: string; sdp: string }) => void;
  const peer = {
    localDescription: { sdp: 'offer' }, addTrack() {}, createDataChannel() {},
    createOffer: async () => ({ type: 'offer', sdp: 'offer' }),
    setLocalDescription: async () => {}, setRemoteDescription: async () => {}, close() {},
  } as unknown as RTCPeerConnection;
  const media = new CodexVoiceMedia('thread', {
    microphone: async () => ({ getTracks: () => [{ stop() {} }] }) as unknown as MediaStream,
    peer: () => peer, audio: () => ({ pause() {}, srcObject: null }) as unknown as HTMLAudioElement,
    sessionId: () => 'own-session',
    listen: async (callback) => { emit = callback; return () => {}; },
    start: () => new Promise((resolve) => { resolveStart = resolve; }), stop: async () => {},
  }, (state, detail) => states.push(detail ?? state));
  const connecting = media.connect();
  while (!resolveStart) await new Promise((resolve) => setTimeout(resolve, 0));
  emit({ threadId: 'native', sessionId: 'other-session', method: 'thread/realtime/error', params: { message: 'unrelated' } });
  emit({ threadId: 'native', sessionId: 'own-session', method: 'thread/realtime/error', params: { message: 'native failure' } });
  try { assert.deepEqual(states, ['connecting', 'native failure']); }
  finally { resolveStart({ threadId: 'native', sessionId: 'own-session', sdp: 'answer' }); await connecting; await media.close(); }
});
