import assert from 'node:assert/strict';
import test from 'node:test';

import { setSpeechMuted, speakEckyText, stopEckySpeech } from './tts';

test('speakEckyText uses browser speech synthesis when unmuted', () => {
  const spoken: Array<{ text: string; rate: number; pitch: number }> = [];
  const canceled: string[] = [];
  class Utterance {
    text: string;
    rate = 1;
    pitch = 1;
    volume = 1;
    constructor(text: string) {
      this.text = text;
    }
  }
  const fakeWindow = {
    speechSynthesis: {
      speak: (utterance: Utterance) => spoken.push({
        text: utterance.text,
        rate: utterance.rate,
        pitch: utterance.pitch,
      }),
      cancel: () => canceled.push('cancel'),
    },
    SpeechSynthesisUtterance: Utterance,
  };

  Object.defineProperty(globalThis, 'window', {
    value: fakeWindow,
    configurable: true,
  });

  setSpeechMuted(false);
  assert.equal(speakEckyText('  Hello    geometry.  '), true);

  assert.equal(spoken.length, 1);
  assert.equal(spoken[0].text, 'Hello geometry.');
  assert.equal(canceled.length, 1);
  assert.ok(spoken[0].rate < 1);
  assert.ok(spoken[0].pitch >= 1);
});

test('speakEckyText refuses muted or empty text and stop cancels speech', () => {
  let cancelCount = 0;
  class Utterance {
    constructor(public text: string) {}
  }
  Object.defineProperty(globalThis, 'window', {
    value: {
      speechSynthesis: {
        speak: () => assert.fail('muted speech should not speak'),
        cancel: () => {
          cancelCount += 1;
        },
      },
      SpeechSynthesisUtterance: Utterance,
    },
    configurable: true,
  });

  setSpeechMuted(true);
  assert.equal(speakEckyText('quiet'), false);
  setSpeechMuted(false);
  assert.equal(speakEckyText('   '), false);
  stopEckySpeech();
  assert.equal(cancelCount, 2);
});
