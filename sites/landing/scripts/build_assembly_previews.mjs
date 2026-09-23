/** Publish preview-only geometry; downloaded print parts remain byte-identical. */
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { bottleRailPlacement } from '../src/showcase/previewGeometry.js';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const manifestPath = join(site, 'src/showcase/models.json');
const models = JSON.parse(await readFile(manifestPath, 'utf8'));
const previewRoot = join(site, 'public/models/previews');
await mkdir(previewRoot, { recursive: true });
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
async function publish(name, bytes, color, label) {
  const filename = `${name}-${hash(bytes).slice(0, 12)}.stl`;
  await writeFile(join(previewRoot, filename), bytes);
  return { label, url: `/models/previews/${filename}`, downloadName: filename, color };
}
const bottle = models.find(model => model.id === 'bicycle-bottle-holder');
const bottleParts = [];
for (const [index, part] of bottle.parts.entries()) {
  const bytes = Buffer.from(await readFile(join(site, 'public', part.url)));
  if (index === 1) {
    const displacement = bottleRailPlacement();
    const triangleCount = bytes.readUInt32LE(80);
    if (bytes.length !== 84 + triangleCount * 50) throw new Error('Expected a binary STL');
    for (let triangle = 0; triangle < triangleCount; triangle++) {
      for (let vertex = 0; vertex < 3; vertex++) for (let axis = 0; axis < 3; axis++) {
        const offset = 84 + triangle * 50 + 12 + vertex * 12 + axis * 4;
        bytes.writeFloatLE(bytes.readFloatLE(offset) + displacement[axis], offset);
      }
    }
  }
  bottleParts.push(await publish(`bottle-${index}`, bytes, part.color, part.label));
}
bottle.preview = { layout: 'assembly', upAxis: 'z', parts: bottleParts,
  provenance: { railTranslation: bottleRailPlacement(), sourceSha256: hash(await readFile(join(site, 'public', bottle.sourceUrl))), railSourceSha256: hash(await readFile(join(site, 'public', bottle.companionSources[0].url))) } };
bottle.view = { yaw: -0.55, pitch: 0.35 };
bottle.note = 'Bottle cage with the frame rail seated in its rear dovetail. ZIP contains separate print parts.';

// Pass the native render's bundle.json. It references the exact generated meshes.
const bundlePath = process.argv[2];
if (bundlePath) {
  const bundle = JSON.parse(await readFile(bundlePath, 'utf8'));
  const dryer = models.find(model => model.id === 'filament-dryer');
  const parts = [];
  for (const part of bundle.viewerAssets) {
    // A sacrificial fit gauge is a print aid, not an assembled component.
    if (part.partId === 'bearing-print-gauge') continue;
    const original = dryer.parts.find(item => item.downloadName === `${part.partId}.stl`);
    parts.push(await publish(`dryer-${part.partId}`, await readFile(part.path), original?.color ?? '#c8924f', part.label));
  }
  dryer.preview = { layout: 'assembly', upAxis: 'z', parts,
    provenance: { modelId: bundle.modelId, sourceSha256: hash(await readFile(join(site, 'public', dryer.sourceUrl))), parameters: JSON.parse(await readFile(process.argv[3], 'utf8')) } };
  dryer.view = { yaw: -0.55, pitch: 0.28 };
  dryer.note = 'Assembled enclosure with lid, latches, and feedthrough. ZIP contains all 13 print parts.';
}
await writeFile(manifestPath, JSON.stringify(models, null, 2) + '\n');
