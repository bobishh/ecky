import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { buildEckyIrBook } from './eckyIrBook';

function fixturePath(...parts: string[]): string {
  return path.join(process.cwd(), ...parts);
}

test('buildEckyIrBook assembles source docs and complex model walkthrough into book html', () => {
  const docsMarkdown = bookMarkdownFixture();

  const book = buildEckyIrBook({
    docsMarkdown,
  });

  assert.equal(book.title, 'Ecky IR Field Guide');
  assert.match(book.html, /Table of Contents/i);
  assert.equal(book.chapters[0]?.title, 'First Solid: Ball on a Base');
  assert.ok(book.chapters.some((chapter) => chapter.title === 'Final Model: Integrated Film Adapter Open Helicoid v9'));
  assert.ok(book.chapters.some((chapter) => chapter.title === 'Appendix: Language Reference'));
  assert.match(book.html, /Sketch to Solid: Plate from a Profile/);
  assert.match(book.html, /<code>assembly<\/code> \(planned\)/i);
  assert.match(book.html, /planned top-level clause for explicit multi-part assembly recipes/i);
  assert.match(book.html, /runtime\/compiler support deferred/i);
  assert.match(book.html, /views prove the display\/manufacturing split/i);
  assert.match(book.html, /formalize what component packages already do at the package layer/i);
  assert.match(book.html, /assemblies stay placement-based as today/i);
  assert.match(book.html, /examples here mark intent only, not accepted source today/i);
  assert.match(book.html, /use <code>view<\/code> for preview-only offsets/i);
  assert.match(book.html, /<code>export<\/code> \(planned\)/i);
  assert.match(book.html, /planned top-level clause for authored export\/manufacturing policy/i);
  assert.match(book.html, /preview transforms never affect STL or STEP artifacts/i);
  assert.match(book.html, /artifact manifests, and package output modes outside <code>\.ecky<\/code> source/i);
  assert.match(book.html, /model-level <code>let\*<\/code>/i);
  assert.match(book.html, /helper <code>define<\/code>/i);
  assert.match(book.html, /<code>define-component<\/code>/i);
  assert.match(book.html, /<code>divider-depth<\/code> owns the wall-offset math once/i);
  assert.match(book.html, /Verification: State What Must Stay True/);
  assert.match(book.html, /generation should emit suffixed literals like mm\/cm\/in\/deg\/rad/i);
  assert.match(book.html, /emit suffixed literals for lengths and angles/i);
  assert.match(book.html, /bare numbers only for counts, ratios, and unitless math/i);
  assert.match(book.html, /author verify clauses from requirements/i);
  assert.match(book.html, /expect the first run to go red/i);
  assert.match(book.html, /verify_generated_model/);
  assert.match(book.html, /fix the model and re-render/i);
  assert.match(book.html, /Real Model Patterns: Procedural Cuts and Arrayed Frames/);
  assert.match(book.html, /assets\/10-real-model-patterns-01\.png/);
  assert.match(book.html, /assets\/10-real-model-patterns-02\.png/);
  assert.match(book.html, /assets\/10-real-model-patterns-03\.png/);
  assert.match(book.html, /Woodlouse hotel/);
  assert.match(book.html, /voronoi2/);
  assert.match(book.html, /linear-array/);
  assert.match(book.html, /radial-array/);
  assert.match(book.html, /helical-ridge/);
  assert.match(book.html, /clip-box/);
  assert.match(book.html, /tunnel_female_bottom_male_top/);
  assert.match(book.html, /base_recessed_male_rails/);
  assert.match(book.html, /Tag any fit-critical selector\./);
  assert.match(book.html, /:created-by pocket/);
  assert.match(book.html, /limits face candidates to the cavity created from <code>pocket<\/code>/);
  assert.doesNotMatch(book.html, /render-source/);
  assert.ok(book.assets.length > 0);
  assert.match(book.html, /assets\/01-first-solid-01\.png/);
  assert.deepEqual(book.assets[0], {
    sourcePath: 'target/book/public/docs/assets/01-first-solid-01.png',
    outputPath: 'assets/01-first-solid-01.png',
    mediaType: 'image/png',
  });
});

function bookMarkdownFixture(): string {
  return fs.readFileSync(fixturePath('public', 'docs', 'ecky-ir.md'), 'utf8');
}
