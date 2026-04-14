import test from 'node:test';
import assert from 'node:assert/strict';

import { cycleTopologyMode, topologyModeLabel } from './viewerDisplayMode';

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
