#!/usr/bin/env node
/**
 * render_ecky_preview_png.mjs
 *
 * Deterministic preview PNG pipeline for committed Ecky geometry.
 *
 *   canonical .ecky  --(native ecky/direct-occt)-->  STL
 *   STL             --(Three/WebGL, headless chromium)-->  PNG
 *
 * This is the only image-generation path for campaign assets. It never invokes
 * OpenSCAD and never hand-draws geometry: the rendered mesh is the actual
 * native-Ecky tessellation. The published PNG is bound to its canonical source
 * by a sidecar manifest recording the source sha256 and the native ecky
 * contentHash, so a source/canvas drift check can fail the build.
 *
 * Usage:
 *   node scripts/render_ecky_preview_png.mjs \
 *     --ecky docs/books/ecky-ir/examples/corner-bracket.ecky \
 *     --png  public/docs/assets/corner-bracket.png
 */
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { mkdtemp } from 'node:fs/promises';
import { chromium } from '@playwright/test';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');

function parseArgs(argv) {
  const out = { ecky: null, png: null, size: 720, yaw: 0.62, pitch: -0.92 };
  for (let i = 2; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--ecky') out.ecky = argv[++i];
    else if (a === '--png') out.png = argv[++i];
    else if (a === '--size') out.size = Number(argv[++i]);
    else if (a === '--yaw') out.yaw = Number(argv[++i]);
    else if (a === '--pitch') out.pitch = Number(argv[++i]);
    else throw new Error(`Unknown argument: ${a}`);
  }
  if (!out.ecky || !out.png) {
    throw new Error('Usage: render_ecky_preview_png.mjs --ecky <path> --png <path>');
  }
  return out;
}

/**
 * Run native ecky render and return { stlBytes, contentHash } by parsing the
 * `--json` line printed on stdout.
 */
async function nativeRender(eckyAbs, stlAbs) {
  const eckyBin = resolve(root, 'src-tauri/target/debug/ecky');
  const { execFile } = await import('node:child_process');
  return new Promise((resolveP, rejectP) => {
    execFile(eckyBin, ['render', '--backend', 'direct-occt', eckyAbs, '--stl', stlAbs, '--json'], { cwd: root, maxBuffer: 1 << 26 }, (err, stdout, stderr) => {
      if (err) rejectP(new Error(`ecky render failed: ${err.message}\n${stderr}`));
      else {
        const parsed = JSON.parse(stdout.trim().split('\n').pop());
        resolveP({ contentHash: parsed.contentHash });
      }
    });
  });
}

const HARNESS = (params) => `<!doctype html>
<html><head><meta charset="utf-8"><title>render</title>
<style>html,body{margin:0;background:#0a0d16;}canvas{display:block;}</style>
</head><body>
<script type="module">
import * as THREE from '/three/three.module.js';

async function readStl(url) {
  const buf = await (await fetch(url)).arrayBuffer();
  const bytes = new Uint8Array(buf);
  // Detect ASCII STL: the first 5 non-data bytes spell "solid" AND the file is
  // not a valid binary header. Binary STL also starts with "solid" sometimes,
  // so verify by size math first.
  const dv = new DataView(buf);
  const triCount = dv.getUint32(80, true);
  const expectedBinary = 84 + triCount * 50;
  let positions = [];
  if (expectedBinary === bytes.length && triCount > 0) {
    let off = 84;
    for (let i = 0; i < triCount; i += 1) {
      off += 12; // skip normal
      for (let v = 0; v < 3; v += 1) {
        positions.push(dv.getFloat32(off, true), dv.getFloat32(off + 4, true), dv.getFloat32(off + 8, true));
        off += 12;
      }
      off += 2; // attribute
    }
  } else {
    const text = new TextDecoder().decode(buf);
    const re = /vertex\s+([-\deE.]+)\s+([-\deE.]+)\s+([-\deE.]+)/g;
    let m;
    while ((m = re.exec(text)) !== null) {
      positions.push(Number(m[1]), Number(m[2]), Number(m[3]));
    }
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geo.computeVertexNormals();
  return geo;
}

try {
  const SIZE = ${params.size};
  const canvas = document.createElement('canvas');
  canvas.width = SIZE; canvas.height = SIZE;
  document.body.appendChild(canvas);
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: true, alpha: false });
  renderer.setPixelRatio(1);
  renderer.setSize(SIZE, SIZE, false);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.setClearColor(0x0a0d16, 1);

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x0a0d16);
  const group = new THREE.Group();
  scene.add(group);
  const camera = new THREE.PerspectiveCamera(30, 1, 0.1, 4000);
  camera.position.set(0, 0, 180);
  camera.lookAt(0, 0, 0);
  scene.add(new THREE.AmbientLight(0x9fb7a6, 1.1));
  const key = new THREE.DirectionalLight(0xdce8db, 2.2);
  key.position.set(-60, 80, 100);
  scene.add(key);
  const rim = new THREE.DirectionalLight(0x6fae8a, 1.0);
  rim.position.set(70, -40, 60);
  scene.add(rim);

  const geo = await readStl('/model.stl');
  const mat = new THREE.MeshStandardMaterial({ color: 0xc8a838, metalness: 0.18, roughness: 0.45 });
  const mesh = new THREE.Mesh(geo, mat);
  group.add(mesh);

  const box = new THREE.Box3().setFromObject(mesh);
  const center = box.getCenter(new THREE.Vector3());
  const dims = box.getSize(new THREE.Vector3());
  const maxDim = Math.max(dims.x, dims.y, dims.z) || 1;
  const scale = 64 / maxDim;
  mesh.position.copy(center).negate();
  group.scale.setScalar(scale);
  group.rotation.y = ${params.yaw};
  group.rotation.x = ${params.pitch};
  renderer.render(scene, camera);

  // Programmatic proof the mesh is actually framed (not a blank/empty canvas):
  // fraction of pixels that differ meaningfully from the background color.
  const gl = renderer.getContext();
  const fb = new Uint8Array(SIZE * SIZE * 4);
  gl.readPixels(0, 0, SIZE, SIZE, gl.RGBA, gl.UNSIGNED_BYTE, fb);
  let nonBg = 0;
  for (let i = 0; i < fb.length; i += 4) {
    const dr = Math.abs(fb[i] - 0x0a);
    const dg = Math.abs(fb[i + 1] - 0x0d);
    const db = Math.abs(fb[i + 2] - 0x16);
    if (dr + dg + db > 24) nonBg += 1;
  }
  window.__coverage = nonBg / (SIZE * SIZE);

  window.__renderPng = canvas.toDataURL('image/png');
  window.__renderDone = true;
} catch (e) {
  window.__renderError = (e && e.stack) ? e.stack : String(e);
  window.__renderDone = true;
}
</script>
</body></html>`;

function startServer({ threeBuildDir, stlPath, params }) {
  return new Promise(async (resolveP) => {
    const threeCache = new Map();
    async function readThree(name) {
      if (!threeCache.has(name)) threeCache.set(name, await readFile(join(threeBuildDir, name)));
      return threeCache.get(name);
    }
    const stlBytes = await readFile(stlPath);
    const html = HARNESS(params);
    const server = createServer(async (req, res) => {
      try {
        const url = req.url.split('?')[0];
        if (url === '/' || url === '/index.html') {
          res.writeHead(200, { 'content-type': 'text/html' });
          res.end(html);
        } else if (url.startsWith('/three/')) {
          const name = url.replace(/^\/three\//, '');
          if (/\.js$/i.test(name)) {
            res.writeHead(200, { 'content-type': 'text/javascript' });
            res.end(await readThree(name));
          } else {
            res.writeHead(404); res.end('not found');
          }
        } else if (url === '/model.stl') {
          res.writeHead(200, { 'content-type': 'model/stl' });
          res.end(stlBytes);
        } else {
          res.writeHead(404);
          res.end('not found');
        }
      } catch (e) {
        res.writeHead(500);
        res.end(String(e));
      }
    });
    server.listen(0, '127.0.0.1', () => resolveP(server));
  });
}

async function main() {
  const args = parseArgs(process.argv);
  const eckyAbs = resolve(root, args.ecky);
  const pngAbs = resolve(root, args.png);
  const sourceBytes = await readFile(eckyAbs);
  const sourceSha256 = createHash('sha256').update(sourceBytes).digest('hex');

  const workDir = await mkdtemp(join(tmpdir(), 'ecky-png-'));
  const stlPath = join(workDir, 'model.stl');
  const { contentHash } = await nativeRender(eckyAbs, stlPath);
  const stlBytes = (await readFile(stlPath)).length;

  const threeBuildDir = resolve(root, 'node_modules/three/build');
  const params = { size: args.size, yaw: args.yaw, pitch: args.pitch };
  const server = await startServer({ threeBuildDir, stlPath, params });
  const port = server.address().port;
  const baseURL = `http://127.0.0.1:${port}/`;

  const browser = await chromium.launch({ args: ['--use-gl=angle', '--use-angle=swiftshader-webgl', '--ignore-gpu-blocklist'] });
  try {
    const page = await browser.newPage({ viewport: { width: args.size, height: args.size }, deviceScaleFactor: 1 });
    const errors = [];
    page.on('response', (r) => { if (r.status() >= 400) errors.push(`HTTP ${r.status()} ${r.url()}`); });
    page.on('requestfailed', (req) => errors.push(`REQFAIL ${req.url()} ${req.failure()?.errorText || ''}`));
    page.on('console', (m) => { if (m.type() === 'error') errors.push(`console:${m.text()}`); });
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.goto(baseURL, { waitUntil: 'networkidle' });
    try {
      await page.waitForFunction(() => window.__renderDone === true, { timeout: 30000 });
    } catch (waitErr) {
      const state = await page.evaluate(() => ({ done: !!window.__renderDone, err: window.__renderError || null }));
      throw new Error(`render signal timeout: ${waitErr.message}\nstate=${JSON.stringify(state)}\nconsole/pageerr: ${errors.join(' | ')}`);
    }
    const err = await page.evaluate(() => window.__renderError);
    if (err) throw new Error(`WebGL render failed: ${err}\nconsole: ${errors.join(' | ')}`);
    const dataUrl = await page.evaluate(() => window.__renderPng);
    const coverage = await page.evaluate(() => window.__coverage);
    const b64 = dataUrl.split(',')[1];
    const pngBuffer = Buffer.from(b64, 'base64');
    await mkdir(dirname(pngAbs), { recursive: true });
    await writeFile(pngAbs, pngBuffer);

    const manifest = {
      sourcePath: args.ecky,
      sourceSha256,
      eckyContentHash: contentHash,
      stlBytes,
      pngPath: args.png,
      pngWidth: args.size,
      pngHeight: args.size,
      nonBackgroundCoverage: coverage,
      renderer: 'three-webgl-swiftshader',
      backend: 'direct-occt',
      generatedBy: 'scripts/render_ecky_preview_png.mjs',
    };
    await writeFile(`${pngAbs}.manifest.json`, `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(`rendered ${args.png} (${pngBuffer.length} bytes, coverage ${(coverage * 100).toFixed(2)}%) from ${args.ecky} (sha256 ${sourceSha256.slice(0, 12)})`);
  } finally {
    await browser.close();
    server.close();
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
