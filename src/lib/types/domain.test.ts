import assert from 'node:assert/strict';
import test from 'node:test';

import {
  hasActiveLithophaneAttachments,
  normalizeDesignOutput,
  normalizePostProcessing,
  normalizeThread,
} from './domain';

test('normalizeDesignOutput resolves legacy defaults', () => {
  const output = normalizeDesignOutput({
    engineKind: 'freecad',
  } as any);

  assert.equal(output.sourceLanguage, 'legacyPython');
  assert.equal(output.geometryBackend, 'freecad');
});

test('normalizeThread resolves legacy defaults', () => {
  const thread = normalizeThread({
    engine_kind: 'freecad',
  } as any);

  assert.equal(thread.sourceLanguage, 'legacyPython');
  assert.equal(thread.geometryBackend, 'freecad');
});

test('normalizePostProcessing lifts legacy displacement into a lithophane attachment', () => {
  const normalized = normalizePostProcessing({
    displacement: {
      imageParam: 'image_path',
      projection: 'planar',
      depthMm: 2.5,
      invert: true,
    },
  });

  assert.ok(normalized);
  assert.equal(normalized?.lithophaneAttachments?.length, 1);
  assert.deepEqual(normalized?.lithophaneAttachments?.[0], {
    id: 'legacy-image-path',
    enabled: true,
    source: { kind: 'param', imageParam: 'image_path' },
    targetPartId: '',
    placement: {
      mode: 'partSidePatch',
      side: 'front',
      projection: 'planar',
      widthMm: 0,
      heightMm: 0,
      offsetXMm: 0,
      offsetYMm: 0,
      rotationDeg: 0,
      overflowMode: 'contain',
      bleedMarginMm: 0,
    },
    relief: {
      depthMm: 2.5,
      invert: true,
    },
    color: {
      mode: 'mono',
      channelThicknessMm: 0.4,
    },
  });
});

test('hasActiveLithophaneAttachments ignores disabled attachments', () => {
  assert.equal(
    hasActiveLithophaneAttachments({
      lithophaneAttachments: [
        {
          id: 'off',
          enabled: false,
          source: { kind: 'file', imagePath: '/tmp/x.png' },
          targetPartId: '',
          placement: { mode: 'partSidePatch', side: 'front', projection: 'auto' },
          relief: { depthMm: 1, invert: false },
          color: { mode: 'mono', channelThicknessMm: 0.4 },
        },
      ],
    }),
    false,
  );
});
