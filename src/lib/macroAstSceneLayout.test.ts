import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildMacroAstMapProjection } from './macroAstMap';
import { buildMacroAstSceneLayout } from './macroAstSceneLayout';

test('buildMacroAstSceneLayout places nodes and connectors for a source-backed scene', () => {
  const projection = buildMacroAstMapProjection({
    modelManifest: {
      modelId: 'scene-layout',
      sourceKind: 'generated',
      document: { documentName: 'Scene Layout', documentLabel: 'Scene Layout', objectCount: 1, warnings: [] },
      parts: [
        {
          partId: 'part-a',
          freecadObjectName: 'part_a',
          label: 'Part A',
          kind: 'solid',
          editable: true,
          parameterKeys: ['width_mm', 'enabled'],
        },
        {
          partId: 'part-b',
          freecadObjectName: 'part_b',
          label: 'Part B',
          kind: 'feature',
          editable: true,
          parameterKeys: ['mode'],
        },
      ],
    },
    uiSpec: {
      fields: [
        { type: 'number', key: 'width_mm', label: 'Width' },
        { type: 'checkbox', key: 'enabled', label: 'Enabled' },
        { type: 'select', key: 'mode', label: 'Mode', options: [], frozen: false },
      ],
    },
    parameters: {
      width_mm: 24,
      enabled: true,
      mode: 'auto',
    },
  });

  const layout = buildMacroAstSceneLayout(projection, { width: 1200 });

  assert.equal(layout.nodes.length > 0, true);
  assert.equal(layout.connectors.length > 0, true);

  const root = layout.nodes.find((node) => node.kind === 'model');
  const part = layout.nodes.find((node) => node.kind === 'part');
  const param = layout.nodes.find((node) => node.kind === 'param');
  const connector = layout.connectors[0];

  assert.ok(root);
  assert.ok(part);
  assert.ok(param);
  assert.ok(connector);
  assert.equal(root?.syntaxLabel, 'MODEL');
  assert.equal(root!.h <= 48, true, 'root is a slim title bar');
  assert.equal(part?.syntaxVariant, 'solid');
  assert.equal(param?.controlAnchor.x > param!.x, true);
  assert.equal(connector?.path.startsWith('M '), true);
  assert.equal(layout.width, 1200);
  assert.equal(layout.height > 0, true);
});

test('buildMacroAstSceneLayout keeps a single part compact and uses multiple port columns', () => {
  const projection = buildMacroAstMapProjection({
    modelManifest: {
      modelId: 'scene-compact',
      sourceKind: 'generated',
      document: { documentName: 'Scene Compact', documentLabel: 'Scene Compact', objectCount: 1, warnings: [] },
      parts: [
        {
          partId: 'part-a',
          freecadObjectName: 'part_a',
          label: 'Part A',
          kind: 'solid',
          editable: true,
          parameterKeys: ['width_mm', 'enabled', 'mode', 'image', 'height_mm', 'depth_mm'],
        },
      ],
    },
    uiSpec: {
      fields: [
        { type: 'number', key: 'width_mm', label: 'Width' },
        { type: 'checkbox', key: 'enabled', label: 'Enabled' },
        { type: 'select', key: 'mode', label: 'Mode', options: [], frozen: false },
        { type: 'image', key: 'image', label: 'Image' },
        { type: 'number', key: 'height_mm', label: 'Height' },
        { type: 'number', key: 'depth_mm', label: 'Depth' },
      ],
    },
    parameters: {
      width_mm: 24,
      enabled: true,
      mode: 'auto',
      image: '/tmp/mock.png',
      height_mm: 12,
      depth_mm: 6,
    },
  });

  const layout = buildMacroAstSceneLayout(projection, { width: 1400 });
  const part = layout.nodes.find((node) => node.kind === 'part');
  const paramXs = layout.nodes.filter((node) => node.kind === 'param').map((node) => node.x);

  assert.ok(part);
  assert.equal(part!.w < layout.width * 0.9, true);
  assert.equal(new Set(paramXs).size > 1, true);
  // Every param module carries a port anchor on its left edge.
  for (const node of layout.nodes.filter((entry) => entry.kind === 'param')) {
    assert.equal(node.portAnchors.length, 1);
    assert.equal(node.portAnchors[0]!.x, node.x);
  }
});
