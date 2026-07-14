/**
 * Build the Ecky IR Field Guide as a standalone, server-rendered HTML page.
 *
 * Output: target/book/dist/docs-site/index.html
 *
 * Run:  npm run build:docs-site
 *
 * This replaces the old parchment-style field-guide HTML for the web. The
 * EPUB builder (build_ecky_ir_book.ts) still handles the EPUB artifact; this
 * script handles the themed web page served at /docs/.
 */
import fs from 'node:fs';
import path from 'node:path';

import { parseDocsDocument } from '../src/lib/docs/eckyIrGuide';
import { buildDocsSiteHtml } from '../src/lib/docs/eckyIrDocsSite';

const root = process.cwd();
const docsSourcePath = path.join(root, 'public', 'docs', 'ecky-ir.md');
const outputDir = path.join(root, 'target', 'book', 'dist', 'docs-site');
const outputPath = path.join(outputDir, 'index.html');

const docsMarkdown = fs.readFileSync(docsSourcePath, 'utf8');
const doc = parseDocsDocument(docsMarkdown, { assetBasePath: '/docs' });

const html = buildDocsSiteHtml(doc, {
  rawMarkdownPath: '/docs/ecky-ir.md',
  epubPath: '/docs/ecky-ir-field-guide.epub',
});

fs.mkdirSync(outputDir, { recursive: true });
fs.writeFileSync(outputPath, html);

console.log(`Docs site HTML: ${outputPath}`);
console.log(`  ${doc.sections.length} sections rendered`);
console.log(`  ${(html.length / 1024).toFixed(0)} KB`);
