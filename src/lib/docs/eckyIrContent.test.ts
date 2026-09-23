import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import { projectEckyIrContent } from '../../../scripts/ecky_ir_content';

const root = process.cwd();

test('canonical corpus keeps only a notation note before the first Corner Bracket lesson', () => {
  const corpus = fs.readFileSync(
    path.join(root, 'docs/books/ecky-ir/ecky-ir-corpus.md'),
    'utf8',
  );

  assert.match(corpus, /^# Ecky IR Field Guide\n\nParenthesized `.ecky` forms compile to a fixed operation set; default rendering is exact B-rep\.\n\n## First Solid: Corner Bracket/);
  assert.doesNotMatch(corpus, /## How Ecky Thinks/);
  assert.doesNotMatch(corpus, /This Markdown file is canonical/);
  assert.doesNotMatch(corpus, /OPEN IN CODE/);
  assert.doesNotMatch(corpus, /^## Projects as Folders\b/im);
});

test('campaign Level 01 projects as Corner Bracket with no ball-on-base teaching content', () => {
  const corpus = fs.readFileSync(
    path.join(root, 'docs/books/ecky-ir/ecky-ir-corpus.md'),
    'utf8',
  );
  const projected = projectEckyIrContent(corpus);

  // The renamed first mission renders as a connected corner bracket.
  assert.match(projected.campaign, /^## Level 01: Corner Bracket$/m);
  assert.match(projected.campaign, /corner bracket/i);
  assert.match(projected.campaign, /\(part bracket/);
  assert.match(projected.campaign, /\(union/);

  // No ball-on-base / sphere-on-platform marker teaching remains in canonical
  // prose or its campaign projection. (sphere as a language primitive may still
  // appear in the dry reference, which is not a campaign teaching artifact.)
  assert.doesNotMatch(corpus, /First Solid: Ball on a Base/);
  assert.doesNotMatch(corpus, /ball on a base/i);
  assert.doesNotMatch(corpus, /\(part marker[\s)]/);
  assert.doesNotMatch(projected.campaign, /First Solid: Ball on a Base/);
  assert.doesNotMatch(projected.campaign, /ball on a base/i);
  assert.doesNotMatch(projected.campaign, /\(part marker[\s)]/);
});

test('canonical corpus projects campaign, human reference, and LLM reference without overlap', () => {
  const corpus = fs.readFileSync(
    path.join(root, 'docs/books/ecky-ir/ecky-ir-corpus.md'),
    'utf8',
  );
  const projected = projectEckyIrContent(corpus);
  const compactReference = projected.reference.replace(/\s+/g, ' ');
  const compactAgentReference = projected.agentReference.replace(/\s+/g, ' ');

  assert.match(projected.campaign, /^# Ecky Campaign$/m);
  assert.match(projected.campaign, /^## Level 01:/m);
  assert.match(projected.campaign, /^## Level 05: Perforated Toothbrush Holder$/m);
  assert.doesNotMatch(projected.campaign, /Appendix: Language Reference/);
  assert.doesNotMatch(projected.campaign, /ECKY_AGENT_REFERENCE/);

  assert.match(projected.reference, /^# Ecky Language Reference$/m);
  assert.match(projected.reference, /^## Operation Index$/m);
  assert.match(projected.reference, /^## Primitive Signatures$/m);
  assert.match(projected.reference, /\[`box`\]\(#box\)/);
  assert.doesNotMatch(projected.reference, /Available backends|build123d, ecky-rust, freecad/);
  assert.doesNotMatch(projected.reference, /First Solid: Ball on a Base/);
  assert.doesNotMatch(projected.reference, /ECKY_AGENT_REFERENCE/);
  assert.match(projected.reference, /^### Live package references$/m);
  assert.match(projected.reference, /\(import-component/);
  assert.match(compactReference, /application-global content-addressed store/);
  assert.match(compactReference, /never calls FreeCAD, converts through STL, invokes `solidify`/);

  assert.match(projected.agentReference, /^# Ecky language reference$/m);
  assert.match(projected.agentReference, /Return one complete `\(model \.\.\.\)` program/);
  assert.match(compactAgentReference, /`component_get` is vendor mode/);
  assert.match(compactAgentReference, /committed exact dependency lock/);
  assert.match(compactAgentReference, /STEP-backed live components/);
  assert.doesNotMatch(projected.agentReference, /Rendered output/);
});

test('published campaign and references equal their canonical projections', () => {
  const corpus = fs.readFileSync(
    path.join(root, 'docs/books/ecky-ir/ecky-ir-corpus.md'),
    'utf8',
  );
  const projected = projectEckyIrContent(corpus);

  assert.equal(
    fs.readFileSync(path.join(root, 'public/tutorials/ecky-campaign.md'), 'utf8'),
    projected.campaign,
  );
  assert.equal(
    fs.readFileSync(path.join(root, 'public/docs/ecky-ir.md'), 'utf8'),
    projected.reference,
  );
  assert.equal(
    fs.readFileSync(path.join(root, 'public/docs/ecky-agent-reference.md'), 'utf8'),
    projected.agentReference,
  );
});

test('human reference keeps additional functions without repeating compiler metadata or documented signatures', () => {
  const corpus = fs.readFileSync(path.join(root, 'docs/books/ecky-ir/ecky-ir-corpus.md'), 'utf8');
  const ref = projectEckyIrContent(corpus).reference;
  const appendix = ref.split('## Complete Compiler Surface')[1];
  assert.ok(appendix);
  assert.doesNotMatch(appendix, /\| Kind \||\| Backends \||\| `box` \||legacy-build123d/);
  assert.match(appendix, /### `torus`/);
  assert.match(appendix, /\(torus 20 5\)/);
});
