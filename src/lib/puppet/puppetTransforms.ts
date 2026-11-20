import { Euler, Matrix4, Quaternion, Vector3 } from 'three';
import type { PuppetDocument, PuppetPart } from '../tauri/contracts';

const radians = Math.PI / 180;
function rotation(part: PuppetPart) {
  return new Quaternion().setFromEuler(new Euler(...part.rotationDeg.map(x => x * radians) as [number, number, number], 'XYZ'));
}
function worldMatrix(parts: PuppetPart[], id: string, visited = new Set<string>()): Matrix4 {
  if (visited.has(id)) throw new Error('Part parents form a cycle.');
  visited.add(id);
  const part = parts.find(p => p.id === id);
  if (!part) throw new Error('Parent part is missing.');
  const local = new Matrix4().compose(new Vector3(...part.positionMm), rotation(part), new Vector3(1, 1, 1));
  return part.parentId ? worldMatrix(parts, part.parentId, visited).multiply(local) : local;
}
export function parentChoices(parts: PuppetPart[], id: string): PuppetPart[] {
  return parts.filter(candidate => {
    let cursor: PuppetPart | undefined = candidate;
    const seen = new Set<string>();
    while (cursor) {
      if (cursor.id === id || seen.has(cursor.id)) return false;
      seen.add(cursor.id);
      cursor = parts.find(p => p.id === cursor?.parentId);
    }
    return true;
  });
}
/** Move the joint while keeping the neutral mesh and its child joints in place. */
export function movePivot(document: PuppetDocument, id: string, pivot: [number, number, number]): PuppetDocument {
  const part = document.parts.find(p => p.id === id)!;
  const delta = new Vector3(...pivot).sub(new Vector3(...part.pivotMm)).multiplyScalar(part.scale);
  const offset = delta.clone().applyQuaternion(rotation(part));
  return { ...document, parts: document.parts.map(p => p.id === id
    ? { ...p, pivotMm: pivot, positionMm: new Vector3(...p.positionMm).add(offset).toArray() }
    : p.parentId === id ? { ...p, positionMm: new Vector3(...p.positionMm).sub(delta).toArray() } : p) };
}
/** Changing parents preserves the neutral pose, including the part's rotation. */
export function reparent(document: PuppetDocument, id: string, parentId: string | null): PuppetDocument {
  if (parentId && !parentChoices(document.parts, id).some(p => p.id === parentId)) throw new Error('Choose a parent outside this part’s descendants.');
  const world = worldMatrix(document.parts, id);
  const local = parentId ? worldMatrix(document.parts, parentId).invert().multiply(world) : world;
  const position = new Vector3(), quaternion = new Quaternion(), scale = new Vector3();
  local.decompose(position, quaternion, scale);
  const euler = new Euler().setFromQuaternion(quaternion, 'XYZ');
  return { ...document, parts: document.parts.map(p => p.id === id
    ? { ...p, parentId, positionMm: position.toArray(), rotationDeg: [euler.x / radians, euler.y / radians, euler.z / radians] } : p) };
}
