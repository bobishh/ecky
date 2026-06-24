import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildMacroAstMapProjection } from './macroAstMap';
import { buildMacroAstSceneLayout, PART_COLLAPSE_THRESHOLD } from './macroAstSceneLayout';

function densePartProjection(paramCount: number) {
  const keys = Array.from({ length: paramCount }, (_, i) => `param_${i}`);
  return buildMacroAstMapProjection({
    modelManifest: {
      modelId: 'scene-density',
      sourceKind: 'generated',
      document: { documentName: 'Scene Density', documentLabel: 'Scene Density', objectCount: 1, warnings: [] },
      parts: [
        {
          partId: 'part-dense',
          freecadObjectName: 'part_dense',
          label: 'Part Dense',
          kind: 'solid',
          editable: true,
          parameterKeys: keys,
        },
      ],
    },
    uiSpec: {
      fields: keys.map((key) => ({ type: 'number' as const, key, label: key })),
    },
    parameters: Object.fromEntries(keys.map((key, i) => [key, i])),
  });
}

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

test('a part with more than the collapse threshold params collapses by default', () => {
  const projection = densePartProjection(PART_COLLAPSE_THRESHOLD + 1);
  const layout = buildMacroAstSceneLayout(projection, { width: 1200 });
  const part = layout.nodes.find((node) => node.kind === 'part');
  const paramNodes = layout.nodes.filter((node) => node.kind === 'param');

  assert.ok(part);
  assert.equal(part!.collapsed, true);
  assert.equal(part!.paramCount, PART_COLLAPSE_THRESHOLD + 1);
  assert.equal(paramNodes.length, 0, 'collapsed part emits no param module nodes');
  assert.equal(layout.connectors.some((connector) => connector.fromId === part?.id), false, 'no param connectors originate from the collapsed part');

  // Root -> part connector must still exist.
  const rootPartConnector = layout.connectors.find((connector) => connector.toId === part?.id);
  assert.ok(rootPartConnector, 'root->part connector stays for a collapsed part');
});

test('collapsed part height is constant regardless of param count', () => {
  const layoutSeven = buildMacroAstSceneLayout(densePartProjection(7), { width: 1200 });
  const layoutThirty = buildMacroAstSceneLayout(densePartProjection(30), { width: 1200 });
  const partSeven = layoutSeven.nodes.find((node) => node.kind === 'part');
  const partThirty = layoutThirty.nodes.find((node) => node.kind === 'part');

  assert.ok(partSeven);
  assert.ok(partThirty);
  assert.equal(partSeven!.collapsed, true);
  assert.equal(partThirty!.collapsed, true);
  assert.equal(partSeven!.h, partThirty!.h, 'collapsed height is param-count independent');
  assert.equal(partSeven!.h <= 80, true, 'collapsed height stays compact (~64px contract)');
});

test('a dense part listed in expandedPartIds renders its full param grid', () => {
  const projection = densePartProjection(PART_COLLAPSE_THRESHOLD + 1);
  const layout = buildMacroAstSceneLayout(projection, {
    width: 1200,
    expandedPartIds: new Set(['part:part-dense']),
  });
  const part = layout.nodes.find((node) => node.kind === 'part');
  const paramNodes = layout.nodes.filter((node) => node.kind === 'param');

  assert.ok(part);
  assert.equal(Boolean(part!.collapsed), false, 'expanded part is not collapsed');
  assert.equal(paramNodes.length, PART_COLLAPSE_THRESHOLD + 1, 'expanded part emits all param nodes');
});

test('a part with exactly the threshold count of params is expanded by default', () => {
  const projection = densePartProjection(PART_COLLAPSE_THRESHOLD);
  const layout = buildMacroAstSceneLayout(projection, { width: 1200 });
  const part = layout.nodes.find((node) => node.kind === 'part');
  const paramNodes = layout.nodes.filter((node) => node.kind === 'param');

  assert.ok(part);
  assert.equal(Boolean(part!.collapsed), false);
  assert.equal(paramNodes.length, PART_COLLAPSE_THRESHOLD);
});

test('a part with threshold + 1 params is collapsed by default (boundary)', () => {
  const projection = densePartProjection(PART_COLLAPSE_THRESHOLD + 1);
  const layout = buildMacroAstSceneLayout(projection, { width: 1200 });
  const part = layout.nodes.find((node) => node.kind === 'part');

  assert.ok(part);
  assert.equal(part!.collapsed, true);
});
