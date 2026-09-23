import { Box3, Vector3 } from 'three';

/**
 * Center the assembly in local space so its pivot stays fixed while orbiting.
 * Every part receives the same offset, preserving the exported relationships.
 * @param {import('three').Group} group
 * @param {import('three').Mesh[]} meshes
 */
export function fitModelGroup(group, meshes) {
  const box = new Box3();
  for (const mesh of meshes) {
    mesh.geometry.computeBoundingBox();
    mesh.updateMatrix();
    box.union(mesh.geometry.boundingBox.clone().applyMatrix4(mesh.matrix));
  }
  const center = box.getCenter(new Vector3());
  for (const mesh of meshes) mesh.position.sub(center);
  group.position.set(0, 0, 0);
  // Fit the bounding sphere, keeping all orbit angles within the camera frame.
  group.scale.setScalar(80 / (box.getSize(new Vector3()).length() || 1));
}
