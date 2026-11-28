import test from 'node:test';
import assert from 'node:assert/strict';
import { createParameterGhostScheduler } from './parameterGhostScheduler';
import type { DesignParams } from './types/domain';

test('Given continuous drag input When values stream in Then non-resetting leading window fires promptly and newer values coalesce to latest-wins', async () => {
  const dispatched: Array<{ requestId: string; params: DesignParams }> = [];
  const cancelled: string[] = [];
  const resolvers: Array<() => void> = [];
  let seq = 0;
  let clock = 1000;

  const scheduler = createParameterGhostScheduler({
    leadingDelayMs: 0,
    throttleMs: 60,
    now: () => clock,
    createRequestId: () => `req-${++seq}`,
    dispatch: (params, requestId) =>
      new Promise<void>((resolve) => {
        dispatched.push({ requestId, params });
        resolvers.push(resolve);
      }),
    cancelRequest: (requestId) => cancelled.push(requestId),
  });

  // First drag tick dispatches promptly (no 250ms pause)
  scheduler.schedule({ radius: 6 });
  assert.equal(dispatched.length, 1);
  assert.deepEqual(dispatched[0], { requestId: 'req-1', params: { radius: 6 } });
  assert.equal(scheduler.isBusy(), true);

  // Rapid intermediate ticks coalesce into latest queued value without cancelling req-1
  clock += 20;
  scheduler.schedule({ radius: 7 });
  clock += 20;
  scheduler.schedule({ radius: 8 });
  clock += 20;
  scheduler.schedule({ radius: 9 });
  assert.equal(dispatched.length, 1);
  assert.deepEqual(cancelled, []);

  // When req-1 completes mid-drag, latest queued value (radius: 9) dispatches immediately
  resolvers[0]();
  await Promise.resolve();
  assert.equal(dispatched.length, 2);
  assert.deepEqual(dispatched[1], { requestId: 'req-2', params: { radius: 9 } });

  // Finish req-2
  resolvers[1]();
  await Promise.resolve();
  assert.equal(scheduler.isBusy(), false);
});

test('Given a synchronous burst of input events When leadingDelayMs is set Then burst coalesces into one dispatch of the latest value', async () => {
  const dispatched: Array<{ requestId: string; params: DesignParams }> = [];
  let seq = 0;

  const scheduler = createParameterGhostScheduler({
    leadingDelayMs: 25,
    throttleMs: 60,
    createRequestId: () => `req-${++seq}`,
    dispatch: async (params, requestId) => {
      dispatched.push({ requestId, params });
    },
    cancelRequest: () => {},
  });

  scheduler.schedule({ radius: 6 });
  scheduler.schedule({ radius: 7 });
  scheduler.schedule({ radius: 8 });
  assert.equal(dispatched.length, 0);

  await new Promise((resolve) => setTimeout(resolve, 40));
  assert.equal(dispatched.length, 1);
  assert.deepEqual(dispatched[0], { requestId: 'req-1', params: { radius: 8 } });
});

test('Given an in-flight preview When cancel is called Then native request is cancelled and queued work is dropped', async () => {
  const dispatched: string[] = [];
  const cancelled: string[] = [];
  const resolvers: Array<() => void> = [];
  let seq = 0;

  const scheduler = createParameterGhostScheduler({
    leadingDelayMs: 0,
    throttleMs: 60,
    now: () => 1000,
    createRequestId: () => `req-${++seq}`,
    dispatch: (_params, requestId) =>
      new Promise<void>((resolve) => {
        dispatched.push(requestId);
        resolvers.push(resolve);
      }),
    cancelRequest: (requestId) => cancelled.push(requestId),
  });

  scheduler.schedule({ radius: 6 });
  scheduler.schedule({ radius: 10 });
  assert.equal(scheduler.isCurrentRequest('req-1'), true);

  scheduler.cancel();
  assert.deepEqual(cancelled, ['req-1']);
  assert.equal(scheduler.isCurrentRequest('req-1'), false);
  assert.equal(scheduler.isBusy(), false);

  // Resolving stale req-1 must NOT dispatch queued radius: 10
  resolvers[0]();
  await Promise.resolve();
  assert.deepEqual(dispatched, ['req-1']);
});
