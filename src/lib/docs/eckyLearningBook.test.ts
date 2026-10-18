import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { readPublishedChapters } from '../../../scripts/ecky_ir_content';
import { parseDocsDocument } from './eckyIrGuide';

const root = process.cwd();

test('Given a beginner When opening the book Then the rake precedes axles and articulated joints', () => {
  const { chapters } = readPublishedChapters(root);
  assert.deepEqual(chapters.map(chapter => chapter.sectionSlug), [
    '03-rake', '02-mushroom-house', '05-garden', '04-tractor', '01-worm', '06-farm',
  ]);
  chapters.forEach((chapter, index) => assert.match(chapter.title, new RegExp(`^Chapter ${index + 1}:`)));
  assert.doesNotMatch(chapters[0].markdown, /bezier-path|worm-link/);
});

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

test('Given published chapter images When building the default docs edition Then URLs are keyed by their PNG bytes', () => {
  const { chapters, imageFiles } = readPublishedChapters(root);
  const publishedPaths = new Set<string>();

  for (const chapter of chapters) {
    for (const [, relativePath] of chapter.markdown.matchAll(/!\[[^\]]*\]\(\/docs\/([^)]*)\)/g)) {
      const bytes = imageFiles.get(relativePath);
      assert.ok(bytes, `missing image bytes for ${relativePath}`);
      const hash = createHash('sha256').update(bytes).digest('hex').slice(0, 12);
      const basename = path.basename(relativePath);
      assert.equal(path.basename(relativePath), `${hash}-${basename.replace(/^[a-f0-9]{12}-/, '')}`, relativePath);
      publishedPaths.add(relativePath);
    }
  }

  assert.equal(publishedPaths.size, imageFiles.size);
  for (const [relativePath, bytes] of imageFiles) {
    const hash = createHash('sha256').update(bytes).digest('hex').slice(0, 12);
    assert.match(path.basename(relativePath), new RegExp(`^${hash}-.+\\.png$`), relativePath);
  }
});

test('Given an illustration changes When rebuilding in an isolated book copy Then its image cache key changes', () => {
  const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'ecky-book-images-'));
  const sourceBook = path.join(root, 'docs/books/ecky-ir');
  const targetBook = path.join(temporaryRoot, 'docs/books/ecky-ir');
  fs.mkdirSync(path.dirname(targetBook), { recursive: true });
  for (const directory of ['missions', 'examples/worm-farm', 'assets/worm-farm']) {
    fs.mkdirSync(path.join(targetBook, directory), { recursive: true });
    fs.cpSync(path.join(sourceBook, directory), path.join(targetBook, directory), { recursive: true });
  }

  try {
    const imageSource = path.join(targetBook, 'assets/worm-farm/01-worm.png');
    const originalBytes = fs.readFileSync(imageSource);
    const before = readPublishedChapters(temporaryRoot);
    const wormImagePath = (file: string) => path.basename(file) === '01-worm.png' || file.endsWith('-01-worm.png');
    const oldPath = [...before.imageFiles.keys()].find(wormImagePath);
    assert.ok(oldPath, 'initial worm image path missing');

    const replacementBytes = Buffer.concat([originalBytes, Buffer.from([0x45, 0x43, 0x4b, 0x59])]);
    fs.writeFileSync(imageSource, replacementBytes);
    const after = readPublishedChapters(temporaryRoot);
    const newPath = [...after.imageFiles.keys()].find(wormImagePath);
    assert.ok(newPath, 'updated worm image path missing');

    const hash = createHash('sha256').update(replacementBytes).digest('hex').slice(0, 12);
    assert.notEqual(newPath, oldPath);
    assert.equal(path.basename(newPath), `${hash}-01-worm.png`);
    assert.deepEqual(after.imageFiles.get(newPath), replacementBytes);
  } finally {
    fs.rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test('Given local EPUB images When packaging with no image base Then source asset paths stay unchanged', () => {
  const { chapters, imageFiles } = readPublishedChapters(root, '/docs', '');
  const referencedPaths = new Set<string>();

  for (const chapter of chapters) {
    for (const [, relativePath] of chapter.markdown.matchAll(/!\[[^\]]*\]\((assets\/worm-farm\/[^)]+\.png)\)/g)) {
      assert.match(relativePath, /^assets\/worm-farm\/(?![a-f0-9]{12}-)[a-z0-9-]+\.png$/);
      const canonicalPath = path.join(root, 'docs/books/ecky-ir', relativePath);
      const bytes = fs.readFileSync(canonicalPath);
      const hash = createHash('sha256').update(bytes).digest('hex').slice(0, 12);
      const publishedPath = `${path.posix.dirname(relativePath)}/${hash}-${path.posix.basename(relativePath)}`;
      assert.deepEqual(imageFiles.get(publishedPath), bytes, publishedPath);
      referencedPaths.add(relativePath);
    }
  }

  assert.equal(referencedPaths.size, imageFiles.size);
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
  assert.equal(manifests.length, 9);
  for (const file of manifests) {
    const manifest = JSON.parse(fs.readFileSync(path.join(assets, file), 'utf8'));
    const bytes = fs.readFileSync(manifest.sourcePath);
    assert.equal(manifest.sourceSha256, createHash('sha256').update(bytes).digest('hex'));
    assert.ok(fs.readFileSync(manifest.pngPath).length > 5000);
    assert.ok(manifest.nonBackgroundCoverage > 0.01);
    assert.equal(manifest.backend, 'direct-occt');
  }
});


test('Given lesson reference links When packaging Then every route and fragment names the actual operation', () => {
  const reference = parseDocsDocument(fs.readFileSync('public/docs/ecky-ir.md', 'utf8'));
  const { chapters } = readPublishedChapters(root);
  for (const chapter of chapters) {
    for (const link of chapter.markdown.matchAll(/\]\(https:\/\/ecky-cad\.com\/docs\/([^/#]+)\/#([^)]+)\)/g)) {
      const section = reference.sections.find(section => section.slug === link[1]);
      assert.ok(section, link[0]);
      assert.ok(section.bodyHtml.includes(`id="${link[2]}"`), link[0]);
    }
  }
});

test('Given chapter mechanics When composing samples and farm Then generated sources remain synchronized', async () => {
  const { syncWormFarmSources, sourceForms } = await import('../../../scripts/worm_farm_sources');
  syncWormFarmSources(root, true);
  assert.deepEqual(sourceForms('; (ignored)\n(meta "a ) bracket")\n(model (part a (box 1 2 3)))'), [
    '(meta "a ) bracket")', '(model (part a (box 1 2 3)))',
  ]);
  assert.throws(() => sourceForms('(model (part'), /Unterminated/);
});
