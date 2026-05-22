import assert from 'node:assert/strict';
import test from 'node:test';
import { defaultMaxVerifyAttempts } from './requestQueue';

test('request submission falls back to two screenshot verify attempts', () => {
  assert.equal(defaultMaxVerifyAttempts(undefined), 2);
  assert.equal(defaultMaxVerifyAttempts(null), 2);
  assert.equal(defaultMaxVerifyAttempts(0), 0);
});
