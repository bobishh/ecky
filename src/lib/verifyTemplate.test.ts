import assert from 'node:assert/strict';
import test from 'node:test';

import {
  canInsertVerifyTemplate,
  hasVerifyClause,
  insertVerifyTemplate,
  looksLikeEckyModelSource,
} from './verifyTemplate';

test('detects ecky model sources only', () => {
  assert.equal(looksLikeEckyModelSource('(model (part body (box 1 1 1)))'), true);
  assert.equal(looksLikeEckyModelSource('print("box")'), false);
});

test('detects existing verify clauses', () => {
  assert.equal(hasVerifyClause('(model (verify (tag body_shell)))'), true);
  assert.equal(hasVerifyClause('(model (part body (box 1 1 1)))'), false);
});

test('inserts verify template before closing model paren', () => {
  const source = '(model\n  (part body (box 1 1 1)))\n';
  const inserted = insertVerifyTemplate(source);

  assert.equal(
    inserted,
    [
      '(model',
      '  (part body (box 1 1 1))',
      '  (verify',
      '    (tag body_shell)',
      '    (metric check (manifest has-step))',
      '    (expect check (= true)))',
      ')',
      '',
    ].join('\n'),
  );
});

test('inserts clearance verify template when model already has two parts', () => {
  const source = '(model\n  (part body (box 1 1 1))\n  (part lid (box 1 1 1)))\n';
  const inserted = insertVerifyTemplate(source);

  assert.match(inserted, /\(tag body_lid_gap\)/);
  assert.match(inserted, /\(metric gap \(clearance min-distance body lid\)\)/);
  assert.match(inserted, /\(expect gap \(>= 3\)\)/);
});

test('does not insert duplicate verify template', () => {
  const source = '(model\n  (verify\n    (tag body_shell)\n    (metric check (manifest has-step))\n    (expect check (= true)))\n  (part body (box 1 1 1)))\n';
  assert.equal(canInsertVerifyTemplate(source), false);
  assert.equal(insertVerifyTemplate(source), source);
});
