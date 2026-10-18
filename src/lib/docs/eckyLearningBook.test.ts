import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { readPublishedChapters } from '../../../scripts/ecky_ir_content';
import { parseDocsDocument } from './eckyIrGuide';

const root = process.cwd();

test('Given six source chapters When publishing Then app lessons match web order and complete examples', () => {
  const { chapters } = readPublishedChapters(root);
  const app = parseDocsDocument(fs.readFileSync('public/tutorials/ecky-book.md', 'utf8'));
  assert.equal(chapters.length, 6);
  assert.deepEqual(app.sections.map(s => s.title), chapters.map(c => c.title));
  for (const chapter of chapters) {
    assert.match(chapter.markdown, /Worm Farm/);
    assert.match(chapter.markdown, /\*\*You can now/);
    assert.match(chapter.markdown, /```scheme\n[\s\S]*?\(model/);
    assert.doesNotMatch(chapter.markdown, /<!-- example:/);
  }
});

test('Given source illustrations When publishing Then every image and download has packaged bytes', () => {
  const { chapters, exampleFiles, imageFiles } = readPublishedChapters(root);
  for (const chapter of chapters) {
    const images = [...chapter.markdown.matchAll(/!\[[^\]]*\]\(\/docs\/([^)]*)\)/g)];
    assert.ok(images.length > 0, chapter.title);
    for (const image of images) assert.ok(imageFiles.has(image[1]), image[1]);
    for (const link of chapter.markdown.matchAll(/\]\(\/docs\/([^)]*\.ecky)\)/g)) {
      assert.ok(exampleFiles.has(link[1]), link[1]);
    }
  }
  const reader = fs.readFileSync('src/lib/ChaptersReader.svelte', 'utf8');
  assert.match(reader, /ecky-book\.md\?raw/);
});

test('Given app chapters When source links render Then every example is an explicit download', () => {
  const app = parseDocsDocument(fs.readFileSync('public/tutorials/ecky-book.md', 'utf8'));
  const links = app.sections.flatMap(section => [...section.bodyHtml.matchAll(/<a href="([^\"]+\.ecky)"([^>]*)>/g)]);
  assert.ok(links.length >= 6);
  for (const [, href, attributes] of links) {
    assert.match(attributes, /\bdownload="[^\"]+\.ecky"/, href);
  }
});

test('Given committed CAD renders When source changes Then illustration manifests detect drift', () => {
  const assets = 'docs/books/ecky-ir/assets/worm-farm';
  const manifests = fs.readdirSync(assets).filter(file => file.endsWith('.manifest.json'));
  assert.equal(manifests.length, 6);
  for (const file of manifests) {
    const manifest = JSON.parse(fs.readFileSync(path.join(assets, file), 'utf8'));
    const bytes = fs.readFileSync(manifest.sourcePath);
    assert.equal(manifest.sourceSha256, createHash('sha256').update(bytes).digest('hex'));
    assert.ok(fs.readFileSync(manifest.pngPath).length > 5000);
    assert.ok(manifest.nonBackgroundCoverage > 0.01);
    assert.equal(manifest.backend, 'direct-occt');
  }
});
