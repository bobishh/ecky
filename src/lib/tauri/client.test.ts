import assert from 'node:assert/strict';
import { test } from 'node:test';

import { formatBackendError } from './client';

test('formatBackendError includes raw backend details', () => {
  const message = formatBackendError({
    code: 'validation',
    message: 'Direct OCCT native shim compile failed.',
    details: 'stdout: raw compiler stdout\nstderr: raw compiler stderr',
  });

  assert.match(message, /Direct OCCT native shim compile failed\./);
  assert.match(message, /raw compiler stdout/);
  assert.match(message, /raw compiler stderr/);
});
