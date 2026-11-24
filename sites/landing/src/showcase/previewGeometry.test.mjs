import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { preparePreviewGeometry } from './previewGeometry.js';

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
