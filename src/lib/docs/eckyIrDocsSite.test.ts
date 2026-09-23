import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { parseDocsDocument, type DocsDocument } from './eckyIrGuide';
import {
  buildDocsClientScript,
  buildDocsSiteHtml,
  buildDocsSitePages,
  type DocsSiteOptions,
} from './eckyIrDocsSite';

function docsFixture(): string {
  return fs.readFileSync(
    path.join(process.cwd(), 'public', 'docs', 'ecky-ir.md'),
    'utf8',
  );
}

function buildSite(): string {
  const doc = parseDocsDocument(docsFixture(), { assetBasePath: '/docs' });
  const options: DocsSiteOptions = {
    basePath: '/docs',
    rawMarkdownPath: '/docs/ecky-ir.md',
    epubPath: '/docs/ecky-ir-field-guide.epub',
  };
  return buildDocsSiteHtml(doc, options, 'operation-index');
}

test('Given parsed docs When one page built Then every section title appears as a route link', () => {
  const html = buildSite();
  const doc = parseDocsDocument(docsFixture());

  for (const section of doc.sections) {
    const route = section.slug === 'operation-index'
      ? 'href="/docs/"'
      : `href="/docs/${section.slug}/"`;
    assert.ok(
      html.includes(route),
      `TOC missing route for section "${section.title}" (slug ${section.slug})`,
    );
    assert.ok(
      html.includes(section.title),
      `TOC missing title text for section "${section.title}"`,
    );
  }
});

test('Given parsed docs When one section page built Then only that section body renders', () => {
  const doc = parseDocsDocument(docsFixture(), { assetBasePath: '/docs' });
  const html = buildDocsSiteHtml(doc, {
    basePath: '/docs',
    rawMarkdownPath: '/docs/ecky-ir.md',
    epubPath: '/docs/ecky-ir-field-guide.epub',
  }, 'primitive-signatures');

  assert.ok(html.includes('Primitive Signatures'), 'missing primitive reference');
  assert.ok(/<pre><code/.test(html), 'no code blocks rendered');
  assert.ok(!html.includes('<h2 class="docs-main__heading">Language Overview</h2>'));
  assert.ok(!/<img src=/.test(html), 'dry reference should not contain tutorial renders');
});

test('Given linked operation markdown When site page built Then links resolve to signature routes', () => {
  const html = buildSite();

  assert.match(html, /href="\/docs\/primitive-signatures\/#box"/);
  assert.doesNotMatch(html, /Available backends|build123d|ecky-rust|freecad/);
});

test('Given parsed docs When all pages built Then one static html file exists per section', () => {
  const doc = parseDocsDocument(docsFixture(), { assetBasePath: '/docs' });
  const pages = buildDocsSitePages(doc, {
    basePath: '/docs',
    rawMarkdownPath: '/docs/ecky-ir.md',
    epubPath: '/docs/ecky-ir-field-guide.epub',
  });

  assert.equal(pages.size, doc.sections.length);
  assert.ok(pages.has('index.html'));
  assert.ok(pages.has('verify-clauses/index.html'));
  assert.ok(pages.has('primitive-signatures/index.html'));
});

test('Given a section page When built Then previous and next navigation is rendered', () => {
  const doc = parseDocsDocument(docsFixture(), { assetBasePath: '/docs' });
  const html = buildDocsSiteHtml(doc, {
    basePath: '/docs',
    rawMarkdownPath: '/docs/ecky-ir.md',
    epubPath: '/docs/ecky-ir-field-guide.epub',
  }, 'language-overview');

  assert.match(html, /aria-label="Section navigation"/);
  assert.match(html, /href="\/docs\/"[^>]*>[\s\S]*Operation Index/);
  assert.match(html, /href="\/docs\/forms-and-structure\/"[^>]*>[\s\S]*Forms and Structure/);
});

test('Given mobile docs shell When built Then accessible drawer controls and external script exist', () => {
  const html = buildSite();
  const script = buildDocsClientScript();

  assert.match(html, /button[^>]+class="docs-menu"[^>]+aria-expanded="false"/);
  assert.match(html, /nav class="docs-toc"[^>]+aria-label="Reference contents"/);
  assert.match(html, /script src="\/docs\/docs\.js" defer/);
  assert.match(script, /aria-expanded/);
  assert.match(script, /Escape/);
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
  // Bronze accent tokens match the application theme.
  assert.ok(/#c8924f/i.test(html), 'primary bronze token missing');
  assert.ok(/#e1b766/i.test(html), 'secondary bronze token missing');
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
