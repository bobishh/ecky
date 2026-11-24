import { toCreasedNormals } from 'three/examples/jsm/utils/BufferGeometryUtils.js';

/** Smooth curved tessellation, retain sharp joins, and orient CAD's up axis.
 * @param {import('three').BufferGeometry} geometry
 * @param {'y' | 'z'} upAxis
 */
export function preparePreviewGeometry(geometry, upAxis = 'y') {
  const prepared = toCreasedNormals(geometry, Math.PI / 3);
  if (upAxis === 'z') prepared.rotateX(-Math.PI / 2);
  return prepared;
}
