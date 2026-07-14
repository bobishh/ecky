// Tiny static file server for the docs-site e2e suite.
//
// Stages a production-mirroring directory under target/www/ from the already-built
// artifacts, then serves it. Run from repo root after `build:docs-site` + `build:book`:
//
//   npm run build:docs-site && npm run build:book && node sites/docs/serve.mjs
//
// Serves /docs/ exactly as nginx does in production.

import { createServer } from 'node:http';
import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import { extname, join, normalize } from 'node:path';

const port = Number(process.env.DOCS_E2E_PORT ?? 4245);
const root = process.cwd();
const wwwRoot = join(root, 'target', 'www');
const docsRoot = join(wwwRoot, 'docs');

function stage() {
  mkdirSync(docsRoot, { recursive: true });

  const docsHtml = join(root, 'target', 'book', 'dist', 'docs-site', 'index.html');
  const bookDir = join(root, 'target', 'book', 'dist', 'books');
  const mdSource = join(root, 'public', 'docs', 'ecky-ir.md');
  const favicon = join(root, 'sites', 'landing', 'public', 'favicon.svg');

  if (!existsSync(docsHtml)) {
    throw new Error(`Build the docs site first: expected ${docsHtml} (run: npm run build:docs-site)`);
  }
  if (!existsSync(bookDir)) {
    throw new Error(`Build the EPUB first: expected ${bookDir}/ (run: npm run build:book)`);
  }

  copyFileSync(docsHtml, join(docsRoot, 'index.html'));
  cpSync(join(bookDir, 'assets'), join(docsRoot, 'assets'), { recursive: true });
  copyFileSync(join(bookDir, 'ecky-ir-field-guide.epub'), join(docsRoot, 'ecky-ir-field-guide.epub'));
  copyFileSync(mdSource, join(docsRoot, 'ecky-ir.md'));
  if (existsSync(favicon)) copyFileSync(favicon, join(wwwRoot, 'favicon.svg'));

  console.log(`[docs-serve] staged ${docsRoot}`);
}

stage();

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8',
  '.epub': 'application/epub+zip',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.jpg': 'image/jpeg',
  '.css': 'text/css',
  '.js': 'text/javascript',
};

const server = createServer((req, res) => {
  const urlPath = decodeURIComponent(new URL(req.url, `http://localhost`).pathname);
  let filePath = normalize(join(wwwRoot, urlPath));
  if (!filePath.startsWith(wwwRoot)) {
    res.writeHead(403).end('forbidden');
    return;
  }
  if (urlPath.endsWith('/')) filePath = join(filePath, 'index.html');
  if (!existsSync(filePath) || !statSync(filePath).isFile()) {
    res.writeHead(404).end('not found');
    return;
  }
  const mime = MIME[extname(filePath).toLowerCase()] ?? 'application/octet-stream';
  res.writeHead(200, { 'content-type': mime });
  res.end(readFileSync(filePath));
});

server.listen(port, () => {
  console.log(`[docs-serve] serving http://localhost:${port}/docs/`);
});
