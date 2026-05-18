import assert from 'node:assert/strict';
import test from 'node:test';

import { diffCode } from './codeDiff';

test('diffCode returns empty diff for identical text', () => {
  const diff = diffCode('alpha\nbeta', 'alpha\nbeta');

  assert.deepEqual(diff.summary, {
    oldLineCount: 2,
    newLineCount: 2,
    unchangedLineCount: 2,
    insertedLineCount: 0,
    deletedLineCount: 0,
    changedLineCount: 0,
    hunkCount: 0,
    isEmpty: true,
  });
  assert.deepEqual(diff.hunks, []);
  assert.deepEqual(diff.rows, []);
});

test('diffCode keeps a single hunk for a line insertion', () => {
  const diff = diffCode('alpha\nbeta', 'alpha\ngamma\nbeta', { contextLines: 1 });

  assert.deepEqual(diff.summary, {
    oldLineCount: 2,
    newLineCount: 3,
    unchangedLineCount: 2,
    insertedLineCount: 1,
    deletedLineCount: 0,
    changedLineCount: 1,
    hunkCount: 1,
    isEmpty: false,
  });

  assert.deepEqual(
    diff.rows.map((row) => ({
      kind: row.kind,
      oldLineNumber: row.oldLineNumber,
      newLineNumber: row.newLineNumber,
      oldText: row.oldText,
      newText: row.newText,
      hunkIndex: row.hunkIndex,
    })),
    [
      { kind: 'context', oldLineNumber: 1, newLineNumber: 1, oldText: 'alpha', newText: 'alpha', hunkIndex: 0 },
      { kind: 'insert', oldLineNumber: null, newLineNumber: 2, oldText: '', newText: 'gamma', hunkIndex: 0 },
      { kind: 'context', oldLineNumber: 2, newLineNumber: 3, oldText: 'beta', newText: 'beta', hunkIndex: 0 },
    ],
  );
  assert.equal(diff.hunks[0]?.oldStartLine, 1);
  assert.equal(diff.hunks[0]?.oldEndLine, 2);
  assert.equal(diff.hunks[0]?.newStartLine, 1);
  assert.equal(diff.hunks[0]?.newEndLine, 3);
});

test('diffCode emits delete and insert rows for a replacement', () => {
  const diff = diffCode('alpha\nbeta\ndelta', 'alpha\ngamma\ndelta', { contextLines: 1 });

  assert.deepEqual(
    diff.rows.map((row) => row.kind),
    ['context', 'delete', 'insert', 'context'],
  );
  assert.deepEqual(
    diff.rows.map((row) => ({
      kind: row.kind,
      oldLineNumber: row.oldLineNumber,
      newLineNumber: row.newLineNumber,
      oldText: row.oldText,
      newText: row.newText,
    })),
    [
      { kind: 'context', oldLineNumber: 1, newLineNumber: 1, oldText: 'alpha', newText: 'alpha' },
      { kind: 'delete', oldLineNumber: 2, newLineNumber: null, oldText: 'beta', newText: '' },
      { kind: 'insert', oldLineNumber: null, newLineNumber: 2, oldText: '', newText: 'gamma' },
      { kind: 'context', oldLineNumber: 3, newLineNumber: 3, oldText: 'delta', newText: 'delta' },
    ],
  );
  assert.equal(diff.summary.insertedLineCount, 1);
  assert.equal(diff.summary.deletedLineCount, 1);
  assert.equal(diff.summary.changedLineCount, 2);
});

test('diffCode splits distant edits into separate hunks', () => {
  const diff = diffCode('a\nb\nc\nd\ne', 'a\nx\nc\nd\ny', { contextLines: 0 });

  assert.equal(diff.hunks.length, 2);
  assert.deepEqual(
    diff.hunks.map((hunk) => hunk.rows.map((row) => row.kind)),
    [
      ['delete', 'insert'],
      ['delete', 'insert'],
    ],
  );
});
