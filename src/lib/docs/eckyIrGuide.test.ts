import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { isDocsRoute, parseDocsDocument, renderMarkdownFragment, resolveSection } from './eckyIrGuide';

function docsFixture(): string {
  const fixturePath = path.join(
    process.cwd(),
    'public',
    'docs',
    'ecky-ir.md',
  );
  return fs.readFileSync(fixturePath, 'utf8');
}

test('isDocsRoute matches docs and learn guide paths only', () => {
  assert.equal(isDocsRoute('/docs/ecky-ir'), true);
  assert.equal(isDocsRoute('/ecky-ir/'), true);
  assert.equal(isDocsRoute('/learn/ecky-ir/intro'), true);
  assert.equal(isDocsRoute('/'), false);
  assert.equal(isDocsRoute('/docs/direct-occt'), false);
});

test('parseDocsDocument reads markdown corpus into title and sections', () => {
  const parsed = parseDocsDocument(docsFixture());

  assert.equal(parsed.title, 'Ecky IR Field Guide');
  assert.ok(parsed.summaryHtml.includes('building models in order'));
  assert.equal(parsed.sections[0]?.title, 'First Solid: Ball on a Base');
  assert.equal(parsed.sections[1]?.title, 'Sketch to Solid: Plate from a Profile');
  assert.ok(parsed.sections.some((section) => section.title === 'Final Model: Integrated Film Adapter Open Helicoid v9'));
  assert.ok(parsed.sections.some((section) => section.title === 'Appendix: Language Reference'));
  assert.ok(parsed.sections.some((section) => section.title === 'Language Overview'));
  assert.ok(parsed.sections.some((section) => section.title === 'Verify Clauses'));
});

test('parseDocsDocument marks pending sections and extracts snippets', () => {
  const parsed = parseDocsDocument(docsFixture());
  const pending = resolveSection(parsed.sections, 'constraint-dojo');
  const forms = resolveSection(parsed.sections, 'forms-and-structure');
  const params = resolveSection(parsed.sections, 'params-and-controls');
  const repetition = resolveSection(parsed.sections, 'repetition-ribs-slots-and-patterns');
  const verify = resolveSection(parsed.sections, 'verify-clauses');
  const verificationChapter = resolveSection(parsed.sections, 'verification-state-what-must-stay-true');
  const selectors = resolveSection(parsed.sections, 'round-chamfer-shell-select-edges-and-faces');

  assert.equal(pending?.status, 'pending');
  assert.ok(pending?.bodyHtml.includes('fit/tolerance tutorial'));
  assert.match(forms?.snippet ?? '', /\(model/);
  assert.match(forms?.bodyHtml ?? '', /top-level authoring grammar/i);
  assert.match(forms?.bodyHtml ?? '', /<code>assembly<\/code> \(planned\)/i);
  assert.match(forms?.bodyHtml ?? '', /planned top-level clause for explicit multi-part assembly recipes/i);
  assert.match(forms?.bodyHtml ?? '', /runtime\/compiler support deferred/i);
  assert.match(forms?.bodyHtml ?? '', /views prove the display\/manufacturing split/i);
  assert.match(forms?.bodyHtml ?? '', /formalize what component packages already do at the package layer/i);
  assert.match(forms?.bodyHtml ?? '', /assemblies stay placement-based as today/i);
  assert.match(forms?.bodyHtml ?? '', /examples here mark intent only, not accepted source today/i);
  assert.match(forms?.bodyHtml ?? '', /use <code>view<\/code> for preview-only offsets/i);
  assert.match(forms?.bodyHtml ?? '', /<code>export<\/code> \(planned\)/i);
  assert.match(forms?.bodyHtml ?? '', /planned top-level clause for authored export\/manufacturing policy/i);
  assert.match(forms?.bodyHtml ?? '', /preview transforms never affect STL or STEP artifacts/i);
  assert.match(forms?.bodyHtml ?? '', /artifact manifests, and package output modes outside <code>\.ecky<\/code> source/i);
  assert.match(params?.bodyHtml ?? '', /generation should emit suffixed literals like mm\/cm\/in\/deg\/rad/i);
  assert.match(params?.bodyHtml ?? '', /emit suffixed literals for lengths and angles/i);
  assert.match(params?.bodyHtml ?? '', /bare numbers only for counts, ratios, and unitless math/i);
  assert.match(repetition?.bodyHtml ?? '', /model-level <code>let\*<\/code>/i);
  assert.match(repetition?.bodyHtml ?? '', /helper <code>define<\/code>/i);
  assert.match(repetition?.bodyHtml ?? '', /<code>define-component<\/code>/i);
  assert.match(repetition?.bodyHtml ?? '', /<code>divider-depth<\/code> owns the wall-offset math once/i);
  assert.match(verify?.snippet ?? '', /\(verify/);
  assert.match(verify?.snippet ?? '', /clearance min-distance/i);
  assert.match(verify?.bodyHtml ?? '', /clearance min-distance/i);
  assert.match(verificationChapter?.bodyHtml ?? '', /author verify clauses from requirements/i);
  assert.match(verificationChapter?.bodyHtml ?? '', /expect the first run to go red/i);
  assert.match(verificationChapter?.bodyHtml ?? '', /verify_generated_model/i);
  assert.match(verificationChapter?.bodyHtml ?? '', /fix the model and re-render/i);
  assert.match(selectors?.bodyHtml ?? '', /Tag any fit-critical selector\./);
  assert.match(selectors?.bodyHtml ?? '', /:created-by pocket/);
  assert.match(
    selectors?.bodyHtml ?? '',
    /limits face candidates to the cavity created from <code>pocket<\/code>/,
  );
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
