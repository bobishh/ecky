import assert from 'node:assert/strict';
import test from 'node:test';

import {
  collectNodePartIds,
  collectScopedParameterKeys,
  deriveAssemblyTreeFromParts,
  findAssemblyNodeByPartId,
  resolveAssemblyTree,
} from './assemblyTree';
import type { ModelManifest, PartBinding } from './types/domain';

function samplePart(partId: string, label: string, parameterKeys: string[], xMax: number): PartBinding {
  return {
    partId,
    freecadObjectName: partId,
    label,
    kind: 'solid',
    semanticRole: null,
    viewerAssetPath: null,
    viewerNodeIds: [],
    parameterKeys,
    editable: true,
    bounds: { xMin: 0, yMin: 0, zMin: 0, xMax, yMax: 10, zMax: 10 },
    volume: null,
    area: null,
  };
}

test('deriveAssemblyTreeFromParts groups prefixed subparts and keeps standalone parts flat', () => {
  const tree = deriveAssemblyTreeFromParts([
    samplePart('case/base', 'Base Shell', ['width', 'wall'], 40),
    samplePart('case/lid', 'Top Lid', ['width', 'clearance'], 42),
    samplePart('mount', 'Bracket', ['hole_d'], 15),
  ]);

  assert.equal(tree.length, 2);
  assert.equal(tree[0].id, 'group:case');
  assert.equal(tree[0].partKey, null);
  assert.deepEqual(collectNodePartIds(tree[0]), ['case/base', 'case/lid']);
  assert.deepEqual(tree[0].parameterKeys, ['width', 'wall', 'clearance']);
  assert.equal(tree[0].bounds?.xMax, 42);
  assert.equal(tree[1].id, 'part:mount');
  assert.equal(tree[1].partKey, 'mount');
});

test('resolveAssemblyTree prefers manifest.assemblyTree and falls back to parts', () => {
  const manifest = {
    modelId: 'm1',
    sourceKind: 'generated',
    document: { documentName: 'd', documentLabel: 'd', objectCount: 2, warnings: [] },
    parts: [samplePart('frame/left', 'Left', ['span'], 10), samplePart('frame/right', 'Right', ['span'], 20)],
    parameterGroups: [
      {
        groupId: 'g1',
        label: 'Frame',
        parameterKeys: ['thickness'],
        partIds: ['frame/left'],
        editable: true,
      },
    ],
    selectionTargets: [],
    warnings: [],
    controlPrimitives: [],
    controlViews: [],
    controlRelations: [],
    enrichmentState: { status: 'none', proposals: [] },
  } as unknown as ModelManifest;

  const tree = resolveAssemblyTree(manifest);
  assert.equal(tree.length, 1);
  assert.equal(findAssemblyNodeByPartId(tree, 'frame/left')?.name, 'Left');

  const scoped = collectScopedParameterKeys(manifest, ['frame/left']);
  assert.deepEqual([...scoped].sort(), ['span', 'thickness']);
});
