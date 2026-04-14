import assert from 'node:assert/strict';
import test from 'node:test';
import { isCurrentThreadLoad, shouldShowDialoguePreloader, shouldSkipThreadSelect } from '../threadLoading';

test('thread loading ignores stale results after a fast A to B switch', () => {
  assert.equal(isCurrentThreadLoad(1, 2, 'thread-b', 'thread-a'), false);
  assert.equal(isCurrentThreadLoad(2, 2, 'thread-b', 'thread-b'), true);
});

test('thread loading keeps the previous viewer eligible while the next thread version loads', () => {
  assert.equal(isCurrentThreadLoad(1, 2, 'thread-b', 'thread-a'), false);
});

test('thread loading blocks duplicate fetches for an active thread click', () => {
  assert.equal(
    shouldSkipThreadSelect('thread-a', {
      activeThreadId: 'thread-a',
      loadingThreadId: null,
      threadHasMessages: true,
      threadMessagesLoading: false,
    }),
    true,
  );
});

test('thread loading blocks duplicate fetches while the same thread is already loading', () => {
  assert.equal(
    shouldSkipThreadSelect('thread-a', {
      activeThreadId: 'thread-a',
      loadingThreadId: 'thread-a',
      threadHasMessages: false,
      threadMessagesLoading: true,
    }),
    true,
  );
});

test('thread loading shows dialogue preloader while messages load', () => {
  assert.equal(shouldShowDialoguePreloader(true), true);
  assert.equal(shouldShowDialoguePreloader(false), false);
});
