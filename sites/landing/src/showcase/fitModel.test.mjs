import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { fitModelGroup } from './fitModel.js';

test('shared assembly center remains at the orbit origin without changing relative placement', () => {
  const group = new THREE.Group();
  const left = new THREE.Mesh(new THREE.BoxGeometry(20, 40, 10));
  const right = new THREE.Mesh(new THREE.BoxGeometry(10, 15, 20));
  left.position.set(300, 400, 100);
  right.position.set(350, 410, 130);
  group.add(left, right);
  const difference = right.position.clone().sub(left.position);
  fitModelGroup(group, [left, right]);
  assert.ok(right.position.clone().sub(left.position).distanceTo(difference) < 1e-8);
  const center = new THREE.Box3().setFromObject(group).getCenter(new THREE.Vector3());
  assert.ok(center.length() < 1e-8);
  for (const yaw of [-2.4, 0, 2.4]) {
    group.rotation.set(1.2, yaw, 0);
    group.updateMatrixWorld(true);
    assert.ok(group.localToWorld(new THREE.Vector3()).length() < 1e-8);
    const box = new THREE.Box3().setFromObject(group);
    assert.ok(box.min.length() <= 56 && box.max.length() <= 56);
  }
});

test('framing leaves an 80-unit inspection diameter for a tall model', () => {
  const group = new THREE.Group();
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(0.01, 400, 0.01));
  group.add(mesh);
  fitModelGroup(group, [mesh]);
  const height = new THREE.Box3().setFromObject(group).getSize(new THREE.Vector3()).y;
  assert.ok(height > 79.9 && height < 80.1);
});
