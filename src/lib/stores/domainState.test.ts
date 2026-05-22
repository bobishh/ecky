import assert from 'node:assert/strict';
import test from 'node:test';
import { get } from 'svelte/store';

import { config } from './domainState';

test('config store defaults to two screenshot verify attempts', () => {
  assert.equal(get(config).maxVerifyAttempts, 2);
});
