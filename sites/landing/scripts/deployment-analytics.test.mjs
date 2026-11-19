import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { deploymentAnalytics } from './deployment-analytics.mjs';

test('Given the source landing page, when built without deployment settings, then it contains no analytics bootstrap', async () => {
  const html = await readFile(new URL('../index.html', import.meta.url), 'utf8');
  assert.doesNotMatch(html, /analytics-bootstrap/);
});

test('Given no complete deployment settings, when the page builds, then it loads no external client', () => {
  assert.deepEqual(deploymentAnalytics({}), []);
  assert.deepEqual(deploymentAnalytics({ ANALYTICS_CLIENT_URL: 'https://collector.example.test/client.js' }), []);
});

test('Given explicit deployment settings, when the page builds, then it loads the external dependency with its public credential', () => {
  assert.deepEqual(deploymentAnalytics({ ANALYTICS_CLIENT_URL: 'https://collector.example.test/client.js',
    ANALYTICS_PROJECT: 'fixture', ANALYTICS_BROWSER_TOKEN: 'public-fixture-key' }), [{
    tag: 'script', attrs: { defer: true, src: 'https://collector.example.test/client.js',
      'data-project': 'fixture', 'data-token': 'public-fixture-key' }, injectTo: 'head',
  }]);
});

test('Given unsafe client URLs, when deployment is configured, then reject the build', () => {
  for (const url of ['http://collector.example.test/client.js', 'javascript:alert(1)', 'https://user:pass@collector.example.test/client.js']) {
    assert.throws(() => deploymentAnalytics({ ANALYTICS_CLIENT_URL: url,
      ANALYTICS_PROJECT: 'fixture', ANALYTICS_BROWSER_TOKEN: 'public-fixture-key' }));
  }
});
