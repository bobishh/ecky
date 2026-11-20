import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Vector3, Euler, Quaternion } from 'three';
import { movePivot, parentChoices, reparent } from './puppetTransforms';
import type { PuppetDocument, PuppetPart } from '../tauri/contracts';
const body: PuppetPart = { id: 'body', name: 'Body', assetId: 'a'.repeat(64), parentId: null, positionMm: [10, 20, 0], pivotMm: [0, 0, 0], rotationDeg: [0, 0, 90], scale: 2, axis: 'z', amplitudeDeg: 0, phaseDeg: 0, triangleCount: 1, textured: false };
const head: PuppetPart = { ...body, id: 'head', parentId: 'body', positionMm: [0, 10, 0], rotationDeg: [0, 0, 0], scale: 1 };
const document: PuppetDocument = { schemaVersion: 1, name: 'Unicorn', durationSecs: 2, stepped: false, parts: [body, head] };
function close(a: number[], b: number[]) { a.forEach((x, i) => assert.ok(Math.abs(x-b[i]) < 1e-7, `${a} != ${b}`)); }
test('moving a rotated, scaled joint preserves the neutral surface and child joint', () => {
  const moved = movePivot(document, 'body', [3, 0, 0]);
  close(moved.parts[0].positionMm, [10, 26, 0]);
  close(moved.parts[1].positionMm, [-6, 10, 0]);
  const rotation = new Quaternion().setFromEuler(new Euler(0, 0, Math.PI/2));
  const surface = new Vector3(5, 0, 0).sub(new Vector3(...moved.parts[0].pivotMm)).multiplyScalar(2).applyQuaternion(rotation).add(new Vector3(...moved.parts[0].positionMm));
  close(surface.toArray(), [10, 30, 0]);
});
test('reparenting preserves the world pose and rejects cycles', () => {
  const detached = reparent(document, 'head', null);
  close(detached.parts[1].positionMm, [0, 20, 0]);
  close(detached.parts[1].rotationDeg, [0, 0, 90]);
  close(reparent(detached, 'head', 'body').parts[1].positionMm, head.positionMm);
  assert.deepEqual(parentChoices(document.parts, 'body'), []);
  assert.throws(() => reparent(document, 'body', 'head'));
});
