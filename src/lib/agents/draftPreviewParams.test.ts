import assert from 'node:assert/strict';
import test from 'node:test';

import { mergeDraftPreviewParams, resolveDraftPreviewDesign } from './draftPreviewParams';
import type { DesignOutput } from '../types/domain';

function design(initialParams: Record<string, unknown>): DesignOutput {
  return {
    title: 'Woodlouse hotel',
    versionName: 'draft',
    response: '',
    interactionMode: 'design',
    macroCode: '(model)',
    macroDialect: 'ecky',
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'freecad',
    uiSpec: { fields: [] },
    initialParams,
    postProcessing: null,
  } as DesignOutput;
}

test('mergeDraftPreviewParams keeps current values for matching draft keys', () => {
  assert.deepEqual(
    mergeDraftPreviewParams(
      {
        length: 150,
        width: 92,
        svg_icon_width: 8,
        svg_fit_mode: 'contain',
      },
      {
        length: 100,
        width: 80,
        stale_local_only: 'drop',
      },
    ),
    {
      length: 100,
      width: 80,
      svg_icon_width: 8,
      svg_fit_mode: 'contain',
    },
  );
});

test('resolveDraftPreviewDesign preserves params only for active thread', () => {
  const preview = design({ length: 150, width: 92, svg_icon_width: 8 });
  const sameThread = resolveDraftPreviewDesign({
    design: preview,
    previewThreadId: 'thread-1',
    activeThreadId: 'thread-1',
    currentParams: { length: 100, width: 80 },
  });
  const otherThread = resolveDraftPreviewDesign({
    design: preview,
    previewThreadId: 'thread-2',
    activeThreadId: 'thread-1',
    currentParams: { length: 100, width: 80 },
  });

  assert.deepEqual(sameThread.initialParams, { length: 100, width: 80, svg_icon_width: 8 });
  assert.deepEqual(otherThread.initialParams, { length: 150, width: 92, svg_icon_width: 8 });
});
