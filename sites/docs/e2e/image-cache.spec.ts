import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

test('Given an old immutable illustration in cache When a new chapter is published Then the new worm loads', async ({ page }) => {
  const assetRoot = 'docs/books/ecky-ir/assets/worm-farm';
  // Distinct valid PNG stands in for bytes cached before deployment.
  const staleImage = fs.readFileSync(`${assetRoot}/03-rake.png`);
  const currentImage = fs.readFileSync(`${assetRoot}/01-worm.png`);
  const expectedHash = createHash('sha256').update(currentImage).digest('hex');
  const siteRoot = path.resolve('target/book/dist/docs-site');
  const currentHtml = fs.readFileSync(path.join(siteRoot, 'chapters/01-worm/index.html'));
  const legacyPath = '/docs/assets/worm-farm/01-worm.png';
  let published = false;
  let legacyRequests = 0;
  const server = createServer((req, res) => {
    const pathname = new URL(req.url!, 'http://localhost').pathname;
    if (pathname === '/docs/chapters/01-worm/') {
      res.writeHead(200, { 'content-type': 'text/html', 'cache-control': 'no-cache' });
      res.end(published ? currentHtml : `<html><body><img src="${legacyPath}" alt="Old worm"></body></html>`);
    } else if (pathname === legacyPath) {
      legacyRequests++;
      res.writeHead(200, { 'content-type': 'image/png', 'cache-control': 'public, max-age=31536000, immutable' });
      res.end(published ? currentImage : staleImage);
    } else if (pathname.startsWith('/docs/assets/')) {
      const file = path.join(siteRoot, pathname.slice('/docs/'.length));
      if (!fs.existsSync(file)) { res.writeHead(404).end(); return; }
      res.writeHead(200, { 'content-type': 'image/png', 'cache-control': 'public, max-age=31536000, immutable' });
      res.end(fs.readFileSync(file));
    } else res.writeHead(404).end();
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Missing test server port');
  const base = `http://127.0.0.1:${address.port}`;
  try {
    await page.goto(`${base}/docs/chapters/01-worm/?before=deploy`);
    await expect.poll(() => page.locator('img').evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
    expect(legacyRequests).toBe(1);
    published = true;
    // Keep the same browser cache: request interception would disable it.
    await page.goto(`${base}/docs/chapters/01-worm/?after=deploy`);
    const image = page.locator('.docs-main figure img').first();
    await expect(image).toHaveAttribute('src', /\/docs\/assets\/worm-farm\/[a-f\d]{12}-01-worm\.png$/);
    const hash = await image.evaluate(async (img: HTMLImageElement) => {
      const bytes = await (await fetch(img.src)).arrayBuffer();
      const digest = await crypto.subtle.digest('SHA-256', bytes);
      return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('');
    });
    expect(hash).toBe(expectedHash);
    expect(legacyRequests).toBe(1);
  } finally {
    await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  }
});
