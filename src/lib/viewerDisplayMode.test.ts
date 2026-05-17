import test from 'node:test';
import assert from 'node:assert/strict';

import {
  cycleTopologyMode,
  meshTopologyOpacity,
  meshTopologyVisible,
  topologyModeLabel,
} from './viewerDisplayMode';

test('cycleTopologyMode walks through off, feature, and mesh', () => {
  assert.equal(cycleTopologyMode('off'), 'feature');
  assert.equal(cycleTopologyMode('feature'), 'mesh');
  assert.equal(cycleTopologyMode('mesh'), 'off');
});

test('topologyModeLabel returns stable button copy', () => {
  assert.equal(topologyModeLabel('off'), 'TOPOLOGY: OFF');
  assert.equal(topologyModeLabel('feature'), 'TOPOLOGY: FEATURE');
  assert.equal(topologyModeLabel('mesh'), 'TOPOLOGY: MESH');
});

test('mesh topology is scoped to an active part, not the whole model', () => {
  assert.equal(meshTopologyVisible('off', true), false);
  assert.equal(meshTopologyVisible('feature', true), false);
  assert.equal(meshTopologyVisible('mesh', false), false);
  assert.equal(meshTopologyVisible('mesh', true), true);
  assert.equal(meshTopologyOpacity('mesh', true), 0.28);
  assert.equal(meshTopologyOpacity('mesh', false), 0);
});
