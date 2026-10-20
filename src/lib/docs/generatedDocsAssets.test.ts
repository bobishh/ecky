import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { after, before, test } from 'node:test';
import { createServer, type ViteDevServer } from 'vite';

const root = path.resolve(import.meta.dirname, '../../..');
const assets = path.join(root, 'target/book/public/docs/assets');
const id = randomUUID();
const assetName = `asset-check-${id}.png`;
const outsideName = `outside-check-${id}.txt`;
const linkName = `link-check-${id}.png`;
const outsidePath = path.join(assets, '..', outsideName);
let server: ViteDevServer;
let port: number;

function request(rawPath: string): Promise<{ status: number; body: string }> {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port, path: rawPath }, (response) => {
      const chunks: Buffer[] = [];
      response.on('data', (chunk: Buffer) => chunks.push(chunk));
      response.on('end', () => resolve({
        status: response.statusCode ?? 0,
        body: Buffer.concat(chunks).toString(),
      }));
    }).on('error', reject);
  });
}

before(async () => {
  await fs.mkdir(assets, { recursive: true });
  await fs.writeFile(path.join(assets, assetName), 'public-fixture');
  await fs.writeFile(outsidePath, 'outside-fixture');
  await fs.symlink(outsidePath, path.join(assets, linkName));
  server = await createServer({
    root,
    configFile: path.join(root, 'vite.config.ts'),
    logLevel: 'silent',
    server: { host: '127.0.0.1', port: 0, strictPort: false, watch: null },
  });
  await server.listen();
  const address = server.httpServer?.address();
  assert.ok(address && typeof address !== 'string');
  port = address.port;
});

after(async () => {
  await server?.close();
  await Promise.all([
    fs.rm(path.join(assets, assetName), { force: true }),
    fs.rm(path.join(assets, linkName), { force: true }),
    fs.rm(outsidePath, { force: true }),
  ]);
});

test('Given a generated asset, when requested, then its public bytes are served', async () => {
  assert.deepEqual(await request(`/docs/assets/${assetName}`), {
    status: 200, body: 'public-fixture',
  });
});

test('Given a raw parent path, when requested, then outside bytes are refused', async () => {
  const response = await request(`/docs/assets/../${outsideName}`);
  assert.equal(response.status, 403);
  assert.ok(!response.body.includes('outside-fixture'));
});

test('Given an encoded parent path, when requested, then outside bytes are refused', async () => {
  const response = await request(`/docs/assets/%2e%2e%2f${outsideName}`);
  assert.equal(response.status, 403);
  assert.ok(!response.body.includes('outside-fixture'));
});

test('Given a symlink outside the asset root, when requested, then outside bytes are refused', async () => {
  const response = await request(`/docs/assets/${linkName}`);
  assert.equal(response.status, 403);
  assert.ok(!response.body.includes('outside-fixture'));
});
