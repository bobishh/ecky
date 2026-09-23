import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import {
  docsSourcePath,
  isDocsRoute,
  parseDocsDocument,
  renderMarkdownFragment,
  resolveSection,
} from './eckyIrGuide';

function docsFixture(): string {
  const fixturePath = path.join(
    process.cwd(),
    'public',
    'docs',
    'ecky-ir.md',
  );
  return fs.readFileSync(fixturePath, 'utf8');
}

function campaignFixture(): string {
  return fs.readFileSync(
    path.join(process.cwd(), 'public', 'tutorials', 'ecky-campaign.md'),
    'utf8',
  );
}

test('isDocsRoute matches docs and learn guide paths only', () => {
  assert.equal(isDocsRoute('/docs/ecky-ir'), true);
  assert.equal(isDocsRoute('/ecky-ir/'), true);
  assert.equal(isDocsRoute('/learn/ecky-ir/intro'), true);
  assert.equal(isDocsRoute('/'), false);
  assert.equal(isDocsRoute('/docs/direct-occt'), false);
});

test('docsSourcePath always resolves the canonical reference, never a campaign fallback', () => {
  assert.equal(docsSourcePath('/learn/ecky-ir'), '/docs/ecky-ir.md');
  assert.equal(docsSourcePath('/learn/ecky-ir/level-04'), '/docs/ecky-ir.md');
  assert.equal(docsSourcePath('/docs/ecky-ir'), '/docs/ecky-ir.md');
  assert.equal(docsSourcePath('/docs/ecky-ir/union'), '/docs/ecky-ir.md');
  assert.equal(docsSourcePath('/ecky-ir'), '/docs/ecky-ir.md');
  assert.equal(docsSourcePath('/'), '/docs/ecky-ir.md');
});

test('parseDocsDocument reads markdown corpus into title and sections', () => {
  const parsed = parseDocsDocument(docsFixture());

  assert.equal(parsed.title, 'Ecky Language Reference');
  assert.ok(parsed.summaryHtml.includes('Exact forms'));
  assert.equal(parsed.sections[0]?.title, 'Operation Index');
  assert.ok(parsed.sections.some((section) => section.title === 'Language Overview'));
  assert.ok(parsed.sections.some((section) => section.title === 'Verify Clauses'));
  assert.ok(parsed.sections.some((section) => section.title === 'Primitive Signatures'));
  assert.ok(!parsed.sections.some((section) => section.title.startsWith('Level ')));
});

test('parseDocsDocument reads tutorial campaign as six ordered levels', () => {
  const parsed = parseDocsDocument(campaignFixture());

  assert.equal(parsed.title, 'Ecky Campaign');
  assert.equal(parsed.sections.length, 6);
  assert.equal(parsed.sections[0]?.title, 'Level 01: Corner Bracket');
  assert.equal(parsed.sections[4]?.title, 'Level 05: Perforated Toothbrush Holder');
  assert.equal(parsed.sections[5]?.title, 'Level 06: Film Adapter');
  assert.ok(parsed.sections.every((section) => section.bodyHtml.includes('<strong>Mission:</strong>')));
  assert.ok(parsed.sections.every((section) => section.bodyHtml.includes('<strong>Clear condition:</strong>')));
});

test('parseDocsDocument reads section status and extracts snippets', () => {
  const parsed = parseDocsDocument(docsFixture());
  const forms = resolveSection(parsed.sections, 'forms-and-structure');
  const params = resolveSection(parsed.sections, 'params-and-controls');
  const verify = resolveSection(parsed.sections, 'verify-clauses');

  assert.ok(!parsed.sections.some((section) => section.slug === 'constraint-dojo'));
  assert.match(forms?.snippet ?? '', /\(model/);
  assert.match(forms?.bodyHtml ?? '', /These forms organize a model/);
  assert.match(forms?.bodyHtml ?? '', /<code>assembly<\/code> \(planned\)/i);
  assert.match(forms?.bodyHtml ?? '', /not accepted model syntax yet/i);
  assert.match(forms?.bodyHtml ?? '', /<code>export<\/code> \(planned\)/i);
  assert.match(forms?.bodyHtml ?? '', /only a <code>view<\/code> offset is preview-only/i);
  assert.match(params?.bodyHtml ?? '', /Bare lengths are millimetres/i);
  assert.match(params?.bodyHtml ?? '', /Use bare numbers for counts/i);
  assert.match(verify?.snippet ?? '', /\(verify/);
  assert.match(verify?.snippet ?? '', /connected-component-count/i);
  assert.match(verify?.bodyHtml ?? '', /clearance min-distance/i);
});

test('renderMarkdownFragment renders block images as figures', () => {
  const html = renderMarkdownFragment('![Rendered output](assets/example.png)', { assetBasePath: '/docs' });

  assert.match(html, /<figure>/);
  assert.match(html, /<img src="\/docs\/assets\/example\.png" alt="Rendered output" \/>/);
  assert.match(html, /<figcaption>Rendered output<\/figcaption>/);
});

test('renderMarkdownFragment omits hidden render-source comments', () => {
  const html = renderMarkdownFragment('before\n\n<!-- render-source: ../examples/final.ecky -->\n\nafter');

  assert.match(html, /before/);
  assert.match(html, /after/);
  assert.doesNotMatch(html, /render-source/);
});

test('renderMarkdownFragment gives signature headings anchors and renders markdown links', () => {
  const html = renderMarkdownFragment(
    '### `box`\n\nSee [`cylinder`](#cylinder).',
  );

  assert.match(html, /<h3 id="box"><code>box<\/code><\/h3>/);
  assert.match(html, /<a href="#cylinder"><code>cylinder<\/code><\/a>/);
});

test('renderMarkdownFragment renders generated operation tables as linked semantic tables', () => {
  const html = renderMarkdownFragment(
    '| Form | Reference |\n| --- | --- |\n| [`box`](#box) | Primitive Signatures |',
  );

  assert.match(html, /<table>/);
  assert.match(html, /<th>Form<\/th>/);
  assert.match(html, /<th>Reference<\/th>/);
  assert.match(html, /<td><a href="#box"><code>box<\/code><\/a><\/td>/);
  assert.doesNotMatch(html, /build123d|ecky-rust|freecad/);
});

test('reader preserves links, quoted code, escaped table pipes and numbered steps', () => {
  const html = renderMarkdownFragment('[Download source](/docs/examples/bracket.ecky)\n\n`(select mode "snap")`\n\n1. Open source.\n2. Render it.\n\n| Form | Choices |\n| --- | --- |\n| select | `a\\|b` |');
  assert.match(html, /href="\/docs\/examples\/bracket.ecky"/);
  assert.doesNotMatch(html, /&amp;quot;/);
  assert.match(html, /<ol><li>Open source\.<\/li><li>Render it\.<\/li><\/ol>/);
  assert.match(html, /<td><code>a\|b<\/code><\/td>/);
  assert.doesNotMatch(renderMarkdownFragment('[bad](javascript:alert(1))'), /href="javascript/);
});
