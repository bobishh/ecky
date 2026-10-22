import assert from 'node:assert/strict';
import test from 'node:test';

import { manualModelTitle, manualVersionLabel } from './manualVersionLabels';

test('Given a meaningful manual version label When applying Then it is preserved', () => {
  assert.equal(manualVersionLabel('V2 · fit adjustment'), 'V2 · fit adjustment');
});

test('Given no manual version label When applying Then it gets a distinct timestamped label', () => {
  assert.equal(
    manualVersionLabel('', new Date('2026-10-20T12:34:56.000Z')),
    'Manual edit 2026-10-20 12:34:56 UTC',
  );
});

test('Given a legacy generic manual label When applying Then it is replaced with an identifiable label', () => {
  assert.equal(
    manualVersionLabel('V Manual', new Date('2026-10-20T12:34:56.000Z')),
    'Manual edit 2026-10-20 12:34:56 UTC',
  );
  assert.equal(
    manualVersionLabel('Manual edit', new Date('2026-10-20T12:34:56.000Z')),
    'Manual edit 2026-10-20 12:34:56 UTC',
  );
});

test('Given blank working-copy title and named project When manual edit opens Then project identity is retained', () => {
  assert.equal(manualModelTitle('', 'Hydrant cap project', 'Old version title'), 'Hydrant cap project');
});

test('Given meaningful working-copy title When manual edit opens Then explicit title wins', () => {
  assert.equal(manualModelTitle('Pump housing', 'Hydrant cap project', null), 'Pump housing');
});

test('Given inherited generic title and named project When manual edit opens Then project identity wins', () => {
  assert.equal(manualModelTitle('Manual Edit', 'Hydrant cap project', null), 'Hydrant cap project');
});
