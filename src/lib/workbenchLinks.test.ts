import assert from 'node:assert/strict';
import test from 'node:test';
import { workbenchLinkIntent } from './workbenchLinks';

test('book source and EPUB links are download intents, with identical policy in dialogue and documentation', () => {
  assert.deepEqual(workbenchLinkIntent('/docs/examples/4189afc54271-01-worm.ecky'), {
    kind: 'download', assetPath: '/docs/examples/4189afc54271-01-worm.ecky', filename: '01-worm.ecky',
  });
  assert.equal(workbenchLinkIntent('/docs/ecky-ir-field-guide.epub').kind, 'download');
  assert.deepEqual(workbenchLinkIntent('/docs/chapters/01-worm/'), { kind: 'external', url: 'https://ecky-cad.com/docs/chapters/01-worm/' });
});

test('external URLs leave through the system opener and arbitrary local or executable URLs cannot navigate the app', () => {
  assert.deepEqual(workbenchLinkIntent('https://example.test/source.ecky'), { kind: 'external', url: 'https://example.test/source.ecky' });
  for (const href of ['javascript:alert(1)', 'file:///tmp/source.ecky', '/tmp/source.ecky', 'data:text/html,x']) {
    assert.equal(workbenchLinkIntent(href).kind, 'unsupported', href);
  }
});
