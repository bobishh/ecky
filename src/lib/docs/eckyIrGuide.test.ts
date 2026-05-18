import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { isDocsRoute, parseDocsDocument, resolveSection } from './eckyIrGuide';

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
  assert.equal(isDocsRoute('/learn/ecky-ir/intro'), true);
  assert.equal(isDocsRoute('/'), false);
  assert.equal(isDocsRoute('/docs/direct-occt'), false);
});

test('parseDocsDocument reads markdown corpus into title and sections', () => {
  const parsed = parseDocsDocument(docsFixture());

  assert.equal(parsed.title, 'Ecky Language Docs');
  assert.ok(parsed.summaryHtml.includes('Single-source reference'));
  assert.equal(parsed.sections[0]?.title, 'Language Overview');
  assert.equal(parsed.sections[1]?.title, 'Forms and Structure');
  assert.ok(parsed.sections.some((section) => section.title === 'Verify Clauses'));
});

test('parseDocsDocument marks pending sections and extracts snippets', () => {
  const parsed = parseDocsDocument(docsFixture());
  const pending = resolveSection(parsed.sections, 'constraint-dojo');
  const forms = resolveSection(parsed.sections, 'forms-and-structure');
  const verify = resolveSection(parsed.sections, 'verify-clauses');

  assert.equal(pending?.status, 'pending');
  assert.ok(pending?.bodyHtml.includes('fit/tolerance tutorial'));
  assert.match(forms?.snippet ?? '', /\(model/);
  assert.match(forms?.bodyHtml ?? '', /top-level authoring grammar/i);
  assert.match(verify?.snippet ?? '', /\(verify/);
  assert.match(verify?.bodyHtml ?? '', /manifest has-step/i);
});
