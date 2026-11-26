import assert from 'node:assert/strict';
import test from 'node:test';
import { get } from 'svelte/store';

import { createLocalNotificationStore, type LocalNotificationCard } from './localNotificationActions';

function card(
  state: LocalNotificationCard['state'],
  patch: Partial<LocalNotificationCard> = {},
): LocalNotificationCard {
  return {
    eventId: `local-${state}`,
    threadId: 'thread-1',
    actorLabel: 'ECKY',
    summary: state,
    detail: null,
    severity: state === 'active' ? 'question' : 'info',
    state,
    requiresAttention: state === 'active',
    actions: [],
    ...patch,
  };
}

test('resolved local notifications expire after eight seconds and warnings after twelve', () => {
  const scheduled: Array<{ callback: () => void; delayMs: number }> = [];
  const store = createLocalNotificationStore({
    setTimeout: (callback, delayMs) => {
      scheduled.push({ callback, delayMs });
      return 1;
    },
    clearTimeout: () => {
      scheduled.length = 0;
    },
  });

  store.set(card('resolved'));
  assert.equal(get(store)?.eventId, 'local-resolved');
  assert.equal(scheduled[0]?.delayMs, 8_000);
  scheduled.shift()?.callback();
  assert.equal(get(store), null);

  store.set(card('resolved', { severity: 'warning' }));
  assert.equal(scheduled[0]?.delayMs, 12_000);
});

test('active, failed, question, and attention local notifications auto-expire', () => {
  const scheduled: Array<{ callback: () => void; delayMs: number }> = [];
  const store = createLocalNotificationStore({
    setTimeout: (callback, delayMs) => {
      scheduled.push({ callback, delayMs });
      return 1;
    },
    clearTimeout: () => {
      scheduled.length = 0;
    },
  });

  for (const notification of [
    card('active'),
    card('failed', { severity: 'error', requiresAttention: true }),
    card('resolved', { severity: 'question' }),
    card('resolved', { requiresAttention: true }),
  ]) {
    store.set(notification);
    assert.equal(get(store)?.eventId, notification.eventId);
    assert.equal(scheduled[0]?.delayMs, 8_000);
    scheduled.shift()?.callback();
    assert.equal(get(store), null);
  }
});

test('same local event refresh updates copy without restarting its ttl', () => {
  const scheduled: Array<{ callback: () => void; delayMs: number }> = [];
  let clearCount = 0;
  const store = createLocalNotificationStore({
    setTimeout: (callback, delayMs) => {
      scheduled.push({ callback, delayMs });
      return 1;
    },
    clearTimeout: () => {
      clearCount += 1;
    },
  });

  store.set(card('resolved'));
  const originalExpiry = scheduled[0]?.callback;
  store.set(card('resolved', { detail: 'updated detail' }));

  assert.equal(get(store)?.detail, 'updated detail');
  assert.equal(clearCount, 0);
  assert.equal(scheduled.length, 1);
  assert.equal(scheduled[0]?.callback, originalExpiry);
});

test('dismissed or expired local notification does not immediately reappear when passed again', () => {
  const scheduled: Array<{ callback: () => void; delayMs: number }> = [];
  const store = createLocalNotificationStore({
    setTimeout: (callback, delayMs) => {
      scheduled.push({ callback, delayMs });
      return 1;
    },
    clearTimeout: () => {
      scheduled.length = 0;
    },
  });

  const notification = card('resolved');
  store.set(notification);
  assert.equal(get(store)?.eventId, 'local-resolved');

  // Timer expires (after 8s)
  scheduled.shift()?.callback();
  assert.equal(get(store), null);

  // App effect re-runs and calls store.set with the identical notification
  store.set(notification);
  // It MUST stay dismissed and not resurrect!
  assert.equal(get(store), null);
  assert.equal(scheduled.length, 0);
});

test('manually dismissed local notification does not immediately reappear when effect re-runs', () => {
  const scheduled: Array<{ callback: () => void; delayMs: number }> = [];
  const store = createLocalNotificationStore({
    setTimeout: (callback, delayMs) => {
      scheduled.push({ callback, delayMs });
      return 1;
    },
    clearTimeout: () => {
      scheduled.length = 0;
    },
  });

  const notification = card('resolved');
  store.set(notification);
  assert.equal(get(store)?.eventId, 'local-resolved');

  // User clicks DISMISS (store.set(null))
  store.set(null);
  assert.equal(get(store), null);

  // App effect re-runs with the same notification
  store.set(notification);
  // It MUST stay dismissed and not resurrect!
  assert.equal(get(store), null);
  assert.equal(scheduled.length, 0);
});

test('a temporarily hidden version error returns without a dismissal timer', () => {
  const store = createLocalNotificationStore({
    setTimeout() {
      assert.fail('A version error stays visible until dismissed.');
    },
    clearTimeout() {},
  });
  const notification = card('failed', { severity: 'error', persistUntilDismissed: true });

  store.set(notification);
  store.hide();
  assert.equal(get(store), null);
  store.set(notification);
  assert.equal(get(store)?.eventId, notification.eventId);
  store.dismiss();
  store.set(notification);
  assert.equal(get(store), null);
});

