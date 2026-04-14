import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveViewerTone } from './viewerLook';
import type { PartBinding } from './types/domain';

function part(partId: string, label = partId, overrides: Partial<PartBinding> = {}): PartBinding {
  return {
    partId,
    freecadObjectName: partId,
    label,
    kind: 'solid',
    semanticRole: null,
    viewerAssetPath: null,
    viewerNodeIds: [],
    parameterKeys: [],
    editable: true,
    bounds: null,
    volume: null,
    area: null,
    ...overrides,
  };
}

test('resolveViewerTone uses manifest semantics before fallback order', () => {
  const manifestParts = [part('basket', 'Drain basket'), part('clamp-top', 'Pole clamp')];
  assert.notDeepEqual(resolveViewerTone('basket', manifestParts), resolveViewerTone('clamp-top', manifestParts));
  assert.deepEqual(resolveViewerTone('clamp-top', manifestParts), resolveViewerTone('clamp-top', manifestParts));
});

test('resolveViewerTone uses density hints from manifest geometry data', () => {
  const shell = part('shade', 'Lamp shade', {
    bounds: { xMin: 0, yMin: 0, zMin: 0, xMax: 100, yMax: 100, zMax: 100 },
    volume: 50000,
  });
  const solid = part('mount', 'Mount block', {
    bounds: { xMin: 0, yMin: 0, zMin: 0, xMax: 50, yMax: 50, zMax: 50 },
    volume: 100000,
  });
  assert.notDeepEqual(resolveViewerTone('shade', [shell, solid]), resolveViewerTone('mount', [shell, solid]));
});

test('resolveViewerTone falls back to the primary tone for unknown parts', () => {
  const manifestParts = [part('basket')];
  assert.deepEqual(resolveViewerTone('unknown', manifestParts), resolveViewerTone(null, manifestParts));
});
