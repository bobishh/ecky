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
        {
          partId: 'input-port',
          freecadObjectName: 'input_port_inlet',
          label: 'Input Port',
          kind: 'solid',
          editable: true,
          parameterKeys: ['input_port_diameter_mm'],
        },
        {
          partId: 'inline-anchor',
          freecadObjectName: 'inline_param_anchor',
          label: 'Inline Param Anchor',
          kind: 'solid',
          editable: true,
          parameterKeys: ['inline_anchor_width_mm'],
        },
      ],
    },
    uiSpec: {
      fields: [
        { type: 'number', key: 'model_size_mm', label: 'Model Size' },
        { type: 'number', key: 'part_region_mm', label: 'Part Region' },
        { type: 'number', key: 'input_port_diameter_mm', label: 'Input Port Diameter' },
        { type: 'number', key: 'inline_anchor_width_mm', label: 'Inline Anchor Width' },
      ],
    },
    parameters: {
      model_size_mm: 10,
      part_region_mm: 12,
      input_port_diameter_mm: 8,
      inline_anchor_width_mm: 4,
    },
  };

  const projectionA = buildMacroAstMapProjection(input);
  const projectionB = buildMacroAstMapProjection({
    ...input,
    macroCode: '(model\n  (part region\n    (input port)\n    (param anchor)))',
  });

  assert.equal(projectionA.root.id, 'macro-root');
  assert.equal(projectionA.root.label, 'Macro Root');
  assert.equal(projectionA.root.syntaxVariant, 'model');
  assert.equal(projectionA.root.syntaxLabel, 'MODEL');
  assert.equal(projectionA.root.children.length, 4);
  assert.equal(projectionA.root.children[0]?.label, 'Model');
  assert.equal(projectionA.root.children[0]?.syntaxVariant, 'solid');
  assert.equal(projectionA.root.children[0]?.syntaxLabel, 'SOLID');
  assert.equal(projectionA.root.children[0]?.children[0]?.label.includes('Model Size'), true);
  assert.equal(projectionA.root.children[0]?.children[0]?.syntaxVariant, 'number');
  assert.equal(projectionA.root.children[0]?.children[0]?.syntaxLabel, 'PORT');
  assert.equal(projectionA.root.children[0]?.children[0]?.children[0]?.syntaxVariant, 'number');
  assert.equal(projectionA.root.children[0]?.children[0]?.children[0]?.syntaxLabel, 'NUMBER');
  assert.equal(projectionA.root.children[1]?.label, 'Part/Region');
  assert.equal(projectionA.root.children[1]?.syntaxVariant, 'solid');
  assert.equal(projectionA.root.children[1]?.syntaxLabel, 'SOLID');
  assert.equal(projectionA.root.children[1]?.children[0]?.label.includes('Part Region'), true);
  assert.equal(projectionA.root.children[1]?.children[0]?.syntaxVariant, 'number');
  assert.equal(projectionA.root.children[2]?.label, 'Input Port');
  assert.equal(projectionA.root.children[2]?.syntaxVariant, 'solid');
  assert.equal(projectionA.root.children[2]?.children[0]?.label.includes('Input Port Diameter'), true);
  assert.equal(projectionA.root.children[2]?.children[0]?.syntaxVariant, 'number');
  assert.equal(projectionA.root.children[3]?.label, 'Inline Param Anchor');
  assert.equal(projectionA.root.children[3]?.syntaxVariant, 'solid');
  assert.equal(projectionA.root.children[3]?.children[0]?.label.includes('Inline Anchor Width'), true);
  assert.equal(projectionA.root.children[3]?.children[0]?.syntaxVariant, 'number');
  assert.equal(projectionA.root.children[3]?.children[0]?.value, 4);

  assert.deepEqual(
    projectionB.root.children.map((child) => child.id),
    projectionA.root.children.map((child) => child.id),
  );
  assert.equal(projectionB.root.children[1]?.children[0]?.id, projectionA.root.children[1]?.children[0]?.id);
});
