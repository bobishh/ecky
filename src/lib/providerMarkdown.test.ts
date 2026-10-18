import assert from 'node:assert/strict';
import test from 'node:test';
import { renderProviderMarkdown } from './providerMarkdown';

test('renderProviderMarkdown renders GFM, LaTeX, Mermaid hosts, and Ecky source references', () => {
  const rendered = renderProviderMarkdown([
    '## Geometry',
    '',
    '**Frequency:** `3V` and $60^\\circ$.',
    '',
    '- PLAN',
    '- BUILD',
    '',
    '| Type | Count |',
    '| --- | ---: |',
    '| A | 10 |',
    '',
    '[model.ecky](/Users/test/Application%20Support/ecky/model.ecky:42)',
    '',
    '```mermaid',
    'flowchart LR',
    '  PLAN --> BUILD',
    '```',
  ].join('\n'));

  assert.match(rendered.html, /<h2>Geometry<\/h2>/);
  assert.match(rendered.html, /<strong>Frequency:<\/strong>/);
  assert.match(rendered.html, /<code>3V<\/code>/);
  assert.match(rendered.html, /class="katex"/);
  assert.match(rendered.html, /<ul>[\s\S]*<li>PLAN<\/li>[\s\S]*<li>BUILD<\/li>[\s\S]*<\/ul>/);
  assert.match(rendered.html, /<table>/);
  assert.match(rendered.html, /data-provider-code-reference="0"/);
  assert.match(rendered.html, /data-provider-mermaid-id="provider-mermaid-0"/);
  assert.deepEqual(rendered.codeReferences, [{
    kind: 'codeReference',
    label: 'model.ecky',
    path: '/Users/test/Application Support/ecky/model.ecky',
    line: 42,
  }]);
  assert.deepEqual(rendered.diagrams, [{
    id: 'provider-mermaid-0',
    source: 'flowchart LR\n  PLAN --> BUILD',
  }]);
});

test('renderProviderMarkdown escapes raw HTML and disables unsafe links', () => {
  const rendered = renderProviderMarkdown([
    '<script>globalThis.pwned = true</script>',
    '',
    '[unsafe](javascript:alert(1))',
  ].join('\n'));

  assert.doesNotMatch(rendered.html, /<script>/);
  assert.match(rendered.html, /&lt;script&gt;/);
  assert.doesNotMatch(rendered.html, /href="javascript:/);
  assert.match(rendered.html, />unsafe</);
});

test('Codex links to bundled examples retain the same download intent as book links', () => {
  const rendered = renderProviderMarkdown('[Worm source](/docs/examples/4189afc54271-01-worm.ecky)');
  assert.match(rendered.html, /href="\/docs\/examples\/4189afc54271-01-worm\.ecky" download="01-worm\.ecky"/);
});
