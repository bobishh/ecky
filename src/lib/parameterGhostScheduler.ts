import type { DesignParams } from './types/domain';

export type ParameterGhostSchedulerOptions = {
  leadingDelayMs?: number;
  throttleMs?: number;
  now?: () => number;
  createRequestId?: () => string;
  dispatch: (params: DesignParams, requestId: string) => Promise<void>;
  cancelRequest: (requestId: string) => void;
};

export type ParameterGhostScheduler = {
  schedule: (params: DesignParams) => void;
  cancel: () => void;
  isBusy: () => boolean;
  isCurrentRequest: (requestId: string) => boolean;
  currentRequestId: () => string | null;
};

function stableSignature(params: DesignParams): string {
  const ordered: Record<string, unknown> = {};
  for (const key of Object.keys(params).sort()) {
    ordered[key] = params[key];
  }
  return JSON.stringify(ordered);
}

/**
 * Leading-edge + latest-wins scheduler for interactive ghost previews.
 *
 * Unlike a pure trailing debounce (which stays frozen throughout a continuous
 * slider drag), this dispatches immediately on the leading edge, coalesces
 * intermediate edits while a render is in flight so mid-drag frames actually
 * finish and display, and immediately dispatches the newest queued value on
 * completion. Explicit `cancel()` aborts any in-flight native preview.
 */
export function createParameterGhostScheduler(
  options: ParameterGhostSchedulerOptions,
): ParameterGhostScheduler {
  const leadingDelayMs = Math.max(0, options.leadingDelayMs ?? 30);
  const throttleMs = Math.max(leadingDelayMs, options.throttleMs ?? 60);
  const now = options.now ?? (() => Date.now());
  const createRequestId = options.createRequestId ?? (() => crypto.randomUUID());

  let inFlightRequestId: string | null = null;
  let inFlightSignature: string | null = null;
  let queued: { params: DesignParams; signature: string } | null = null;
  let lastCompletedSignature: string | null = null;
  let throttleTimer: ReturnType<typeof setTimeout> | null = null;
  let lastDispatchAt = Number.NEGATIVE_INFINITY;

  function clearThrottleTimer() {
    if (throttleTimer !== null) {
      clearTimeout(throttleTimer);
      throttleTimer = null;
    }
  }

  function startDispatch(params: DesignParams, signature: string) {
    clearThrottleTimer();
    const requestId = createRequestId();
    inFlightRequestId = requestId;
    inFlightSignature = signature;
    lastDispatchAt = now();

    void Promise.resolve(options.dispatch({ ...params }, requestId)).finally(() => {
      if (inFlightRequestId !== requestId) return;
      inFlightRequestId = null;
      lastCompletedSignature = inFlightSignature;
      inFlightSignature = null;
      if (queued) {
        const next = queued;
        queued = null;
        if (next.signature !== lastCompletedSignature) {
          startDispatch(next.params, next.signature);
        }
      }
    });
  }

  function schedule(params: DesignParams) {
    const signature = stableSignature(params);
    if (inFlightRequestId !== null) {
      if (signature === inFlightSignature) {
        queued = null;
      } else {
        queued = { params: { ...params }, signature };
      }
      return;
    }

    if (signature === lastCompletedSignature && queued === null && throttleTimer === null) {
      return;
    }

    const elapsed = now() - lastDispatchAt;
    const delay = Math.max(leadingDelayMs, throttleMs - elapsed);
    queued = { params: { ...params }, signature };
    if (delay === 0) {
      queued = null;
      startDispatch(params, signature);
      return;
    }
    if (throttleTimer === null) {
      // Non-resetting leading window: same-tick bursts coalesce into one request,
      // while continuous drags fire promptly instead of waiting for a pause.
      throttleTimer = setTimeout(() => {
        throttleTimer = null;
        if (inFlightRequestId !== null || !queued) return;
        const next = queued;
        queued = null;
        if (next.signature !== lastCompletedSignature) {
          startDispatch(next.params, next.signature);
        }
      }, delay);
    }
  }

  function cancel() {
    clearThrottleTimer();
    queued = null;
    lastCompletedSignature = null;
    inFlightSignature = null;
    const active = inFlightRequestId;
    inFlightRequestId = null;
    if (active) {
      options.cancelRequest(active);
    }
  }

  return {
    schedule,
    cancel,
    isBusy: () => inFlightRequestId !== null || queued !== null || throttleTimer !== null,
    isCurrentRequest: (requestId: string) => inFlightRequestId === requestId,
    currentRequestId: () => inFlightRequestId,
  };
}
