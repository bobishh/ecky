/** Publish preview-only geometry; downloaded print parts remain byte-identical. */
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

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
// Optional first two inputs regenerate the dryer preview. The third contains
// native preview STL files named like the bottle print parts.
const [dryerBundlePath, dryerParamsPath, bottlePreviewDir] = process.argv.slice(2);
const bottle = models.find(model => model.id === 'bicycle-bottle-holder');
const bottleParts = [];
for (const [index, part] of bottle.parts.entries()) {
  const path = bottlePreviewDir
    ? join(bottlePreviewDir, part.downloadName)
    : join(site, 'public', part.url);
  const bytes = await readFile(path);
  bottleParts.push(await publish(`bottle-${index}`, bytes, part.color, part.label));
}
bottle.preview = { layout: 'assembly', upAxis: 'z', parts: bottleParts,
  provenance: { sourceSha256: hash(await readFile(join(site, 'public', bottle.sourceUrl))) } };
bottle.view = { yaw: -0.7, pitch: 0.42 };
bottle.note = 'Closed hex cage, ramped dovetail, and a printed-thread handlebar clamp.';

// Pass the native render's bundle.json. It references the exact generated meshes.
if (dryerBundlePath) {
  const bundle = JSON.parse(await readFile(dryerBundlePath, 'utf8'));
  const dryer = models.find(model => model.id === 'filament-dryer');
  const parts = [];
  for (const part of bundle.viewerAssets) {
    // A sacrificial fit gauge is a print aid, not an assembled component.
    if (part.partId === 'bearing-print-gauge') continue;
    const original = dryer.parts.find(item => item.downloadName === `${part.partId}.stl`);
    parts.push(await publish(`dryer-${part.partId}`, await readFile(part.path), original?.color ?? '#c8924f', part.label));
  }
  dryer.preview = { layout: 'assembly', upAxis: 'z', parts,
    provenance: { modelId: bundle.modelId, sourceSha256: hash(await readFile(join(site, 'public', dryer.sourceUrl))), parameters: JSON.parse(await readFile(dryerParamsPath, 'utf8')) } };
  dryer.view = { yaw: -0.55, pitch: 0.28 };
  dryer.note = 'Assembled enclosure with lid, latches, and feedthrough. ZIP contains all 13 print parts.';
}
await writeFile(manifestPath, JSON.stringify(models, null, 2) + '\n');
