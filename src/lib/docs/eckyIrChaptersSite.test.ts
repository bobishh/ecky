import assert from 'node:assert/strict';
import test from 'node:test';

import { buildChaptersSitePages, type StaticChapter } from './eckyIrChaptersSite';

const chapters: StaticChapter[] = [
  {
    id: 'mission-01',
    sectionSlug: 'level-01-corner-bracket',
    title: 'Bracket into enclosure',
    markdown: '# Mission 01: Bracket into enclosure\n\nBuild it.\n\n## Checkpoints\n\n### Worked bracket',
    checkpoints: [{ id: 'worked-bracket', source: 'docs/books/example.ecky', code: '(box 1 2 3)' }],
  },
];

test('Given canonical mission files When static chapters build Then index and exact chapter source routes exist', () => {
  const pages = buildChaptersSitePages(chapters, {
    basePath: '/docs',
    epubPath: '/docs/ecky-ir-field-guide.epub',
  });

  const index = pages.get('chapters/index.html') ?? '';
  const chapter = pages.get('chapters/level-01-corner-bracket/index.html') ?? '';

  assert.match(index, /href="\/docs\/chapters\/level-01-corner-bracket\/"/);
  assert.match(index, /href="\/docs\/"[^>]*>Reference/);
  assert.match(index, /href="\/docs\/ecky-ir-field-guide\.epub"[^>]*>EPUB/);
  assert.match(chapter, /Build it\./);
  assert.match(chapter, /<h3 id="checkpoints">Checkpoints<\/h3>/);
  assert.match(chapter, /<h4 id="worked-bracket">Worked bracket<\/h4>/);
  assert.match(chapter, /data-checkpoint-source="docs\/books\/example\.ecky"/);
  assert.match(chapter, /\(box 1 2 3\)/);
  assert.doesNotMatch(chapter, /OPEN IN CODE|mission state|live render/i);
});

test('Given authored heading anchors When a chapter renders Then its local contents targets clean headings', () => {
  const pages = buildChaptersSitePages([{ ...chapters[0], markdown: '# Bracket\n\n## Join the parts {#join}\n\nChange the width.', checkpoints: [] }], { basePath: '/docs', epubPath: '/docs/book.epub' });
  const html = pages.get('chapters/level-01-corner-bracket/index.html')!;
  assert.match(html, /aria-label="On this page"/);
  assert.match(html, /href="#join"/);
  assert.match(html, /id="join">Join the parts/);
  assert.doesNotMatch(html, /\{#join\}|Canonical sources|manifest checkpoints/);
});
