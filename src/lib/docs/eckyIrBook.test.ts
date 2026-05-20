import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { buildEckyIrBook } from './eckyIrBook';

function fixturePath(...parts: string[]): string {
  return path.join(process.cwd(), ...parts);
}

test('buildEckyIrBook assembles source docs and complex model walkthrough into book html', () => {
  const docsMarkdown = fs.readFileSync(fixturePath('public', 'docs', 'ecky-ir.md'), 'utf8');
  const walkthroughMarkdown = fs.readFileSync(
    fixturePath('docs', 'books', 'ecky-ir-helicoid-walkthrough.md'),
    'utf8',
  );
  const complexModelSource = fs.readFileSync(
    fixturePath('model-runtime', 'examples', 'film-scanning-adapter-helicoid.ecky'),
    'utf8',
  );

  const book = buildEckyIrBook({
    docsMarkdown,
    walkthroughMarkdown,
    complexModelSource,
  });

  assert.equal(book.title, 'Ecky IR Field Guide');
  assert.match(book.html, /Table of Contents/i);
  assert.match(book.html, /Forms and Structure/);
  assert.match(book.html, /Complex Model Walkthrough: Film Scanning Adapter Helicoid/);
  assert.match(book.html, /helical-ridge/);
  assert.match(book.html, /clip-box/);
  assert.match(book.html, /moving_lens_carrier/);
});
