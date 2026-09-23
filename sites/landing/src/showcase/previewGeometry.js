import { toCreasedNormals } from 'three/examples/jsm/utils/BufferGeometryUtils.js';

/** Source bindings from bicycle-bottle-holder.ecky and its mating rail. */
export function bottleRailPlacement() {
  const holderOriginX = -25;
  const bottleDiameter = 74;
  const bottleRadialClearance = 0.5;
  const wallThickness = 4;
  const outerRadius = bottleDiameter / 2 + bottleRadialClearance + wallThickness;
  const spineInset = 2;
  const spineThickness = 8;
  const spineOuterY = -outerRadius + spineInset - spineThickness;
  const holderHeight = 130;
  const slotEntryBelowBase = 7;
  const slotEntryZ = -holderHeight / 2 - slotEntryBelowBase;
  return [holderOriginX, spineOuterY, slotEntryZ];
}

/** Smooth curved tessellation, retain sharp joins, and orient CAD's up axis.
 * @param {import('three').BufferGeometry} geometry
 * @param {'y' | 'z'} upAxis
 */
export function preparePreviewGeometry(geometry, upAxis = 'y') {
  const prepared = toCreasedNormals(geometry, Math.PI / 3);
  if (upAxis === 'z') prepared.rotateX(-Math.PI / 2);
  return prepared;
}
