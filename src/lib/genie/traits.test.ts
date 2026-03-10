import assert from 'node:assert/strict';
import test from 'node:test';

import {
  DEFAULT_GENIE_TRAITS,
  resolveModeTraits,
  seededUnit,
} from './traits';

test('resolveModeTraits is deterministic for the same DNA and mode', () => {
  const base = {
    ...DEFAULT_GENIE_TRAITS,
    seed: 42,
    colorHue: 120,
    vertexCount: 18,
    thinkingBias: 0.82,
    repairBias: 0.34,
    renderBias: 0.67,
    expressiveness: 0.92,
  };

  const first = resolveModeTraits(base, 'thinking');
  const second = resolveModeTraits(base, 'thinking');

  assert.deepEqual(first, second);
});

test('mode biases materially alter resolved profiles', () => {
  const lowThinking = resolveModeTraits(
    {
      ...DEFAULT_GENIE_TRAITS,
      seed: 7,
      thinkingBias: 0.2,
    },
    'thinking',
  );
  const highThinking = resolveModeTraits(
    {
      ...DEFAULT_GENIE_TRAITS,
      seed: 7,
      thinkingBias: 1,
    },
    'thinking',
  );
  assert.ok(highThinking.vertexCount > lowThinking.vertexCount);
  assert.ok(highThinking.jitterScale > lowThinking.jitterScale);

  const lowRepair = resolveModeTraits(
    {
      ...DEFAULT_GENIE_TRAITS,
      seed: 7,
      repairBias: 0.2,
    },
    'repairing',
  );
  const highRepair = resolveModeTraits(
    {
      ...DEFAULT_GENIE_TRAITS,
      seed: 7,
      repairBias: 1,
    },
    'repairing',
  );
  assert.ok(highRepair.warpScale > lowRepair.warpScale);
  assert.ok(highRepair.asymmetry > lowRepair.asymmetry);

  const mutedSpeaking = resolveModeTraits(
    {
      ...DEFAULT_GENIE_TRAITS,
      seed: 7,
      expressiveness: 0.35,
    },
    'speaking',
  );
  const expressiveSpeaking = resolveModeTraits(
    {
      ...DEFAULT_GENIE_TRAITS,
      seed: 7,
      expressiveness: 1,
    },
    'speaking',
  );
  assert.ok(expressiveSpeaking.mouthOpenAmplitude > mutedSpeaking.mouthOpenAmplitude);
  assert.ok(expressiveSpeaking.eyeSize > mutedSpeaking.eyeSize);
});

test('seeded offsets stay stable per seed and vary across seeds', () => {
  const idleA = resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 101 }, 'idle');
  const idleB = resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 101 }, 'idle');
  const idleC = resolveModeTraits({ ...DEFAULT_GENIE_TRAITS, seed: 202 }, 'idle');

  assert.deepEqual(idleA.seedOffsets, idleB.seedOffsets);
  assert.notDeepEqual(idleA.seedOffsets, idleC.seedOffsets);
  assert.equal(seededUnit(101, 4), seededUnit(101, 4));
  assert.notEqual(seededUnit(101, 4), seededUnit(202, 4));
});
