import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { parseDocsDocument, type DocsDocument } from './eckyIrGuide';
import { buildDocsSiteHtml, type DocsSiteOptions } from './eckyIrDocsSite';

function docsFixture(): string {
  return fs.readFileSync(
    path.join(process.cwd(), 'public', 'docs', 'ecky-ir.md'),
    'utf8',
  );
}

function buildSite(): string {
  const doc = parseDocsDocument(docsFixture(), { assetBasePath: '/docs' });
  const options: DocsSiteOptions = {
    rawMarkdownPath: '/docs/ecky-ir.md',
    epubPath: '/docs/ecky-ir-field-guide.epub',
  };
  return buildDocsSiteHtml(doc, options);
}

test('Given parsed docs When site html built Then every section title appears as a TOC anchor link', () => {
  const html = buildSite();
  const doc = parseDocsDocument(docsFixture());

  for (const section of doc.sections) {
    const anchor = `href="#${section.slug}"`;
    assert.ok(
      html.includes(anchor),
      `TOC missing anchor for section "${section.title}" (slug ${section.slug})`,
    );
    assert.ok(
      html.includes(section.title),
      `TOC missing title text for section "${section.title}"`,
    );
  }
});

test('Given parsed docs When site html built Then section bodies render in the page', () => {
  const html = buildSite();

  // A real section with code + content: the first solid guide.
  assert.ok(html.includes('First Solid'), 'missing first section heading');
  // Code blocks from the markdown must survive into the HTML.
  assert.ok(/<pre><code/.test(html), 'no code blocks rendered');
  // Images (figures) from rendered examples must reference /docs/assets.
  assert.ok(
    /<img src="\/docs\/assets\//.test(html),
    'images not resolved under /docs/assets',
  );
});

test('Given parsed docs When site html built Then agent markdown and epub download links present', () => {
  const html = buildSite();

  assert.ok(
    html.includes('href="/docs/ecky-ir.md"'),
    'raw markdown link for agents missing',
  );
  assert.ok(
    html.includes('href="/docs/ecky-ir-field-guide.epub"'),
    'epub download link missing',
  );
});

test('Given parsed docs When site html built Then midnight tactical theme applies', () => {
  const html = buildSite();

  // Dark background token from the app theme.
  assert.ok(/#1a1a2e/i.test(html), 'dark bg token missing');
  // Primary green + secondary bronze accent tokens.
  assert.ok(/#4a8c5c/i.test(html), 'primary green token missing');
  assert.ok(/#c8a620/i.test(html), 'secondary bronze token missing');
  // Mono font for code.
  assert.ok(/monospace/i.test(html), 'mono font family missing');
  // Square borders — every border-radius declaration must be zero.
  const radiusDeclarations = html.match(/border-radius\s*:\s*[^;}]+/gi) ?? [];
  const allZero = radiusDeclarations.every((decl) => /border-radius\s*:\s*0/.test(decl));
  assert.ok(
    allZero,
    'theme must use square borders (found non-zero border-radius)',
  );
});

test('Given pending sections When site html built Then they are marked so they stand out', () => {
  const html = buildSite();
  const doc = parseDocsDocument(docsFixture());

  const pending = doc.sections.filter((section) => section.status === 'pending');
  if (pending.length === 0) return; // no pending sections in the corpus yet

  for (const section of pending) {
    assert.ok(
      html.toLowerCase().includes(`${section.slug}`),
      `pending section ${section.slug} should be identifiable in markup`,
    );
  }
  // A status label for pending sections must exist somewhere.
  assert.ok(/pending/i.test(html), 'pending status label missing');
});

test('Given parsed docs When site html built Then output is a complete standalone html document', () => {
  const html = buildSite();

  assert.ok(html.startsWith('<!doctype html>'), 'must start with doctype');
  assert.ok(/<html[^>]*lang="en"/.test(html), 'html lang attr missing');
  assert.ok(/<meta[^>]*charset="utf-8"/.test(html), 'charset meta missing');
  assert.ok(/<meta[^>]*name="viewport"/.test(html), 'viewport meta missing');
  assert.ok(html.includes('</html>'), 'must close html');
});
