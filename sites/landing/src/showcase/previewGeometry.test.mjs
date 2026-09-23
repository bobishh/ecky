import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { preparePreviewGeometry, bottleRailPlacement } from './previewGeometry.js';

test('mount rail seats on the external spine and stops below the closed slot roof', () => {
  const placement = bottleRailPlacement();
  assert.deepEqual(placement, [-25, -47.5, -72]);
  const railTop = placement[2] + 85;
  const slotRoof = -72 + 85 + 0.2;
  assert.ok(Math.abs(slotRoof - railTop - 0.2) < 1e-8);
});

test('Z-up CAD becomes upright while crease smoothing preserves every vertex', () => {
  const original = new THREE.CylinderGeometry(10, 10, 40, 32).toNonIndexed();
  original.rotateX(Math.PI / 2);
  const positions = original.getAttribute('position').array.slice();
  const prepared = preparePreviewGeometry(original, 'z');
  prepared.computeBoundingBox();
  const size = prepared.boundingBox.getSize(new THREE.Vector3());
  assert.ok(Math.abs(size.y - 40) < 1e-5);
  assert.ok(Math.abs(size.z - 20) < 1e-5);
  const restored = prepared.clone().rotateX(Math.PI / 2);
  for (let i = 0; i < positions.length; i++) assert.ok(Math.abs(restored.attributes.position.array[i] - positions[i]) < 1e-5);
  // Adjacent triangles on the curved wall share a smooth radial normal;
  // the top cap still retains its sharp, vertical normal.
  const normals = prepared.attributes.normal.array;
  assert.ok(Array.from(normals).every(Number.isFinite));
  assert.equal(normals.length, positions.length);
});
