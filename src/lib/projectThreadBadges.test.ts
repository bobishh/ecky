import assert from 'node:assert/strict';
import test from 'node:test';
import { deriveProjectThreadBadges } from './projectThreadBadges';

test('deriveProjectThreadBadges returns no badges when thread is idle', () => {
  assert.deepEqual(
    deriveProjectThreadBadges({
      queuedCount: 0,
      pendingConfirm: null,
    }),
    [],
  );
});

test('deriveProjectThreadBadges returns inbox badge for queued user messages', () => {
  assert.deepEqual(
    deriveProjectThreadBadges({
      queuedCount: 2,
      pendingConfirm: null,
    }),
    [
      {
        label: 'INBOX 2',
        className: 'queued',
        title: '2 queued user messages waiting in this thread',
      },
    ],
  );
});

test('deriveProjectThreadBadges returns confirm badge for pending confirmation', () => {
  assert.deepEqual(
    deriveProjectThreadBadges({
      queuedCount: 0,
      pendingConfirm: 'review-fit',
    }),
    [
      {
        label: 'CONFIRM',
        className: 'confirm',
        title: 'Pending confirmation: review-fit',
      },
    ],
  );
});
