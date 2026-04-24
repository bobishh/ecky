import assert from 'node:assert/strict';
import test from 'node:test';

import { needsGeneratedQuestionAnswer, pendingQuestionCopy } from './questionAnswer';

test('needsGeneratedQuestionAnswer requires full answer when classifier has no final response', () => {
  assert.equal(needsGeneratedQuestionAnswer(''), true);
  assert.equal(needsGeneratedQuestionAnswer(null), true);
  assert.equal(needsGeneratedQuestionAnswer('Full answer here.'), false);
});

test('pendingQuestionCopy avoids showing classifier routing text as final answer work', () => {
  assert.equal(pendingQuestionCopy(''), 'Answering question...');
  assert.equal(pendingQuestionCopy('Full answer here.'), 'Full answer here.');
});
