import assert from 'node:assert/strict';
import test from 'node:test';

import type { MaterializedSemanticControl, MaterializedSemanticView } from './semanticControls';
import { pickOverlayControls } from './semanticControls';

function control(
  primitiveId: string,
  partIds: string[] = [],
): MaterializedSemanticControl {
  return {
    primitiveId,
    label: primitiveId,
    kind: 'number',
    source: 'generated',
    editable: true,
    partIds,
    order: 0,
    rawField: {
      type: 'number',
      key: primitiveId,
      label: primitiveId,
      frozen: false,
    },
    bindings: [],
    value: 0,
  };
}

function view(sections: MaterializedSemanticView['sections']): MaterializedSemanticView {
  return {
    viewId: 'view-main',
    label: 'Main',
    scope: 'global',
    partIds: [],
    isDefault: true,
    source: 'generated',
    status: 'none',
    order: 0,
    sections,
    advisories: [],
  };
}

test('pickOverlayControls prefers part-scoped controls, then appends global controls', () => {
  const result = pickOverlayControls(
    view([
      {
        sectionId: 'core',
        label: 'Core',
        collapsed: false,
        controls: [
          control('body-height', ['body']),
          control('global-thickness'),
          control('rim-diameter', ['rim']),
        ],
      },
    ]),
    'body',
  );

  assert.deepEqual(
    result.map((entry) => entry.primitiveId),
    ['body-height', 'global-thickness'],
  );
});

test('pickOverlayControls falls back to all visible controls when no part-specific controls match', () => {
  const result = pickOverlayControls(
    view([
      {
        sectionId: 'core',
        label: 'Core',
        collapsed: false,
        controls: [
          control('rim-diameter', ['rim']),
          control('global-thickness'),
          control('base-radius', ['base']),
        ],
      },
    ]),
    'body',
  );

  assert.deepEqual(
    result.map((entry) => entry.primitiveId),
    ['rim-diameter', 'global-thickness', 'base-radius'],
  );
});

test('pickOverlayControls ignores collapsed sections and does not impose an arbitrary cap', () => {
  const result = pickOverlayControls(
    view([
      {
        sectionId: 'visible',
        label: 'Visible',
        collapsed: false,
        controls: [
          control('c1', ['body']),
          control('c2', ['body']),
          control('c3', ['body']),
          control('c4', ['body']),
          control('c5', ['body']),
        ],
      },
      {
        sectionId: 'hidden',
        label: 'Hidden',
        collapsed: true,
        controls: [control('hidden-control', ['body'])],
      },
    ]),
    'body',
  );

  assert.deepEqual(
    result.map((entry) => entry.primitiveId),
    ['c1', 'c2', 'c3', 'c4', 'c5'],
  );
});
