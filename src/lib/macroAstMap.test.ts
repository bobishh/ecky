import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildMacroAstMapProjection } from './macroAstMap';

test('buildMacroAstMapProjection projects a stable source-backed tree', () => {
  const input = {
    macroCode: '(model (part region (input port) (param anchor)))',
    modelManifest: {
      modelId: 'seeded-macro',
      sourceKind: 'generated',
      document: {
        documentName: 'Seeded Macro',
        documentLabel: 'Seeded Macro',
        objectCount: 4,
        warnings: [],
      },
      parts: [
        {
          partId: 'part-model',
          freecadObjectName: 'model_body',
          label: 'Model',
          kind: 'solid',
          editable: true,
          parameterKeys: ['model_size_mm'],
        },
        {
          partId: 'part-region',
          freecadObjectName: 'part_region_shell',
          label: 'Part/Region',
          kind: 'solid',
          editable: true,
          parameterKeys: ['part_region_mm'],
        },
      ],
    },
    uiSpec: {
      fields: [
        { type: 'number', key: 'model_size_mm', label: 'Model Size' },
        { type: 'number', key: 'part_region_mm', label: 'Part Region' },
        { type: 'number', key: 'inline_anchor_width_mm', label: 'Inline Anchor Width' },
      ],
    },
    parameters: {
      model_size_mm: 10,
      part_region_mm: 12,
      inline_anchor_width_mm: 4,
    },
  };

  const projectionA = buildMacroAstMapProjection(input as any);
  const projectionB = buildMacroAstMapProjection({
    ...input,
    macroCode: '(model\n  (part region\n    (input port)\n    (param anchor)))',
  } as any);

  assert.equal(projectionA.root.id, 'macro-root');
  assert.equal(projectionA.root.syntaxLabel, 'MODEL');

  // Shared group first: the unclaimed field renders once at model level.
  const shared = projectionA.root.children[0]!;
  assert.equal(shared.id, 'shared-params');
  assert.equal(shared.syntaxLabel, 'SHARED');
  assert.equal(shared.children.length, 1);
  assert.equal(shared.children[0]?.fieldKey, 'inline_anchor_width_mm');
  assert.equal(shared.children[0]?.kind, 'param');
  assert.equal(shared.children[0]?.value, 4);

  // Parts own exactly their claimed fields; params attach directly (no port tier).
  const model = projectionA.root.children[1]!;
  assert.equal(model.label, 'Model');
  assert.equal(model.syntaxLabel, 'SOLID');
  assert.equal(model.children.length, 1);
  assert.equal(model.children[0]?.kind, 'param');
  assert.equal(model.children[0]?.label, 'Model Size');
  assert.equal(model.children[0]?.syntaxLabel, 'NUMBER');
  assert.equal(model.children[0]?.children.length, 0);

  const region = projectionA.root.children[2]!;
  assert.equal(region.children[0]?.fieldKey, 'part_region_mm');

  // Identity is stable across formatting-only changes.
  assert.deepEqual(
    projectionB.root.children.map((child) => child.id),
    projectionA.root.children.map((child) => child.id),
  );
  assert.equal(projectionB.root.children[1]?.children[0]?.id, projectionA.root.children[1]?.children[0]?.id);
});

test('fields claimed by several parts collapse into the shared group', () => {
  const projection = buildMacroAstMapProjection({
    macroCode: '(model (part a (box 1 1 1)) (part b (box 2 2 2)))',
    modelManifest: {
      parts: [
        { partId: 'a', label: 'A', parameterKeys: ['hose_od', 'length'] },
        { partId: 'b', label: 'B', parameterKeys: ['hose_od', 'length'] },
      ],
    } as any,
    uiSpec: {
      fields: [
        { type: 'number', key: 'hose_od', label: 'Hose OD' },
        { type: 'number', key: 'length', label: 'Length' },
      ],
    } as any,
    parameters: { hose_od: 16.5, length: 40 },
  });

  const shared = projection.root.children[0]!;
  assert.equal(shared.id, 'shared-params');
  assert.deepEqual(
    shared.children.map((child) => child.fieldKey),
    ['hose_od', 'length'],
  );
  // Parts keep no duplicated controls.
  for (const part of projection.root.children.slice(1)) {
    assert.equal(part.children.length, 0);
  }
});

test('attaches backend source ranges to model and part nodes', () => {
  const projection = buildMacroAstMapProjection({
    macroCode: '(model (part body (box 1 2 3)))',
    modelManifest: {
      parts: [{ partId: 'body', label: 'Body', parameterKeys: [] }],
    } as any,
    uiSpec: { fields: [] } as any,
    parameters: {},
    sourceNodes: [
      { id: 'model', kind: 'model', label: 'model', startByte: 0, endByte: 31 },
      { id: 'part:body', kind: 'part', label: 'body', startByte: 7, endByte: 30 },
    ],
  });

  assert.deepEqual(projection.root.sourceRange, { startByte: 0, endByte: 31 });
  const part = projection.root.children.find((node) => node.id === 'part:body');
  assert.deepEqual(part?.sourceRange, { startByte: 7, endByte: 30 });
});

test('leaves sourceRange undefined without backend entries', () => {
  const projection = buildMacroAstMapProjection({
    macroCode: '(model (part body (box 1 2 3)))',
    modelManifest: { parts: [{ partId: 'body', label: 'Body', parameterKeys: [] }] } as any,
    uiSpec: { fields: [] } as any,
    parameters: {},
  });
  assert.equal(projection.root.sourceRange, undefined);
});

test('Given verify source nodes When projecting New Params map Then verify clauses stay addressable by stable node id', () => {
  const projection = buildMacroAstMapProjection({
    macroCode: '(model (verify (tag step_export) (expect true)) (part body (box 1 2 3)))',
    modelManifest: { parts: [{ partId: 'body', label: 'Body', parameterKeys: [] }] } as any,
    uiSpec: { fields: [] } as any,
    parameters: {},
    sourceNodes: [
      { id: 'model', kind: 'model', label: 'model', startByte: 0, endByte: 69 },
      { id: 'verify:0', kind: 'verify', label: 'step_export', startByte: 7, endByte: 47 },
      { id: 'part:body', kind: 'part', label: 'body', startByte: 48, endByte: 68 },
    ],
  });

  // Keyed by tag so an authored verify chip (stableNodeId `verify:<tag>`)
  // focuses this exact node in the map.
  const verifyNode = projection.root.children.find((node) => node.id === 'verify:step_export');
  assert.ok(verifyNode);
  assert.equal(verifyNode?.kind, 'verify');
  assert.equal(verifyNode?.label, 'step_export');
  assert.equal(verifyNode?.syntaxLabel, 'VERIFY');
  assert.deepEqual(verifyNode?.sourceRange, { startByte: 7, endByte: 47 });
});
