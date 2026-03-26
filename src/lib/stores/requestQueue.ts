import { writable, derived, get } from 'svelte/store';
import { estimateBase64Bytes, profileLog } from '../debug/profiler';
import type { Attachment, Request, RequestPhase } from '../types/domain';

export interface QueuedRequest extends Request {}

type RequestQueueState = {
  byId: Record<string, QueuedRequest>;
  order: string[];
  activeId: string | null;
};

const TERMINAL_PHASES: RequestPhase[] = ['success', 'error', 'canceled'];
const MODEL_ACTIVE_PHASES: RequestPhase[] = [
  'generating',
  'repairing',
  'queued_for_render',
  'rendering',
  'committing',
];

function isTerminalPhase(phase: RequestPhase): boolean {
  return TERMINAL_PHASES.includes(phase);
}

function isModelActivePhase(phase: RequestPhase): boolean {
  return MODEL_ACTIVE_PHASES.includes(phase);
}

function queueStats(byId: Record<string, QueuedRequest>) {
  const requests = Object.values(byId);
  const terminal = requests.filter(r => isTerminalPhase(r.phase)).length;
  const active = requests.length - terminal;
  const screenshotBytes = requests.reduce((sum, r) => sum + estimateBase64Bytes(r.screenshot), 0);
  return {
    requests: requests.length,
    active,
    terminal,
    screenshotMb: Number((screenshotBytes / (1024 * 1024)).toFixed(2)),
  };
}

function createRequestQueue() {
  const { subscribe, set, update } = writable<RequestQueueState>({
    byId: {},
    order: [],
    activeId: null,
  });

  const MAX_CONCURRENT_LLM = 4;

  return {
    subscribe,

    submit(
      prompt: string,
      attachments: Attachment[] = [],
      threadId: string | null = null,
      baseMessageId: string | null = null,
      baseModelId: string | null = null,
    ): string {
      const id = `req-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
      const request: QueuedRequest = {
        id,
        prompt,
        attachments,
        createdAt: Date.now(),
        phase: 'classifying',
        attempt: 1,
        maxAttempts: 3,
        isQuestion: false,
        lightResponse: '',
        screenshot: null,
        result: null,
        error: null,
        cookingStartTime: null,
        cookingElapsed: 0,
        threadId,
        baseMessageId,
        baseModelId,
      };
      update(q => ({
        ...q,
        byId: { ...q.byId, [id]: request },
        order: [...q.order, id],
        activeId: id,
      }));
      const snapshot = get(requestQueue);
      profileLog('queue.submit', {
        requestId: id,
        threadId,
        ...queueStats(snapshot.byId),
      });
      return id;
    },

    patch(id: string, changes: Partial<QueuedRequest>) {
      update(q => {
        const existing = q.byId[id];
        if (!existing) return q;
        const merged: QueuedRequest = { ...existing, ...changes };
        // Auto-compute cookingElapsed when transitioning to a terminal phase
        if (changes.phase && isTerminalPhase(changes.phase) && merged.cookingStartTime && !changes.cookingElapsed) {
          merged.cookingElapsed = Math.max(0, Math.floor(Date.now() / 1000) - Math.floor(merged.cookingStartTime / 1000));
        }
        const next: RequestQueueState = {
          ...q,
          byId: { ...q.byId, [id]: merged },
        };
        if (changes.phase && (changes.phase !== existing.phase || isTerminalPhase(changes.phase))) {
          profileLog('queue.phase', {
            requestId: id,
            from: existing.phase,
            to: changes.phase,
            ...queueStats(next.byId),
          });
        }
        return next;
      });
    },

    setActive(id: string | null) {
      update(q => ({ ...q, activeId: id }));
    },

    cancel(id: string) {
      update(q => {
        const existing = q.byId[id];
        if (!existing || isTerminalPhase(existing.phase)) return q;
        const elapsed = existing.cookingStartTime
          ? Math.max(0, Math.floor(Date.now() / 1000) - Math.floor(existing.cookingStartTime / 1000))
          : 0;
        const canceledRequest: QueuedRequest = {
          ...existing,
          phase: 'canceled',
          cookingElapsed: elapsed,
        };
        const next: RequestQueueState = {
          ...q,
          byId: { ...q.byId, [id]: canceledRequest },
        };
        profileLog('queue.cancel', {
          requestId: id,
          ...queueStats(next.byId),
        });
        return next;
      });
    },

    remove(id: string) {
      update(q => {
        const { [id]: _, ...rest } = q.byId;
        const next = {
          byId: rest,
          order: q.order.filter(x => x !== id),
          activeId: q.activeId === id ? (q.order.find(x => x !== id) || null) : q.activeId,
        };
        profileLog('queue.remove', {
          requestId: id,
          ...queueStats(next.byId),
        });
        return next;
      });
    },

    clear() {
      set({ byId: {}, order: [], activeId: null });
    },

    MAX_CONCURRENT_LLM,
  };
}

export const requestQueue = createRequestQueue();

// Derived stores for UI

// All requests in submission order (for the cafeteria strip)
export const allRequests = derived(requestQueue, $q =>
  $q.order.map(id => $q.byId[id]).filter(Boolean)
);

// Requests belonging to the currently active thread
export const activeThreadRequests = derived(
  [requestQueue, activeThreadId],
  ([$q, $tid]) => {
    return $q.order
      .map(id => $q.byId[id])
      .filter(r => r && r.threadId === $tid);
  }
);

// Only in-flight requests
export const activeRequests = derived(requestQueue, $q => 
  $q.order.map(id => $q.byId[id]).filter(r => r && !['success', 'error', 'canceled'].includes(r.phase))
);

export const activeRequestCount = derived(activeRequests, $r => $r.length);

export const llmInFlightCount = derived(requestQueue, $q =>
  Object.values($q.byId).filter(r => r.phase === 'classifying' || r.phase === 'generating').length
);

export const renderQueueCount = derived(requestQueue, $q =>
  Object.values($q.byId).filter(r => r.phase === 'queued_for_render' || r.phase === 'rendering').length
);

export const completedRequests = derived(requestQueue, $q =>
  $q.order.map(id => $q.byId[id]).filter(r => r && r.phase === 'success')
);

export const errorRequests = derived(requestQueue, $q =>
  $q.order.map(id => $q.byId[id]).filter(r => r && r.phase === 'error')
);

export const currentActiveRequest = derived(requestQueue, $q =>
  $q.activeId ? $q.byId[$q.activeId] : null
);

import { activeThreadId } from './domainState';

/**
 * Returns true if the current active thread has an in-flight (active) request.
 */
export const activeThreadBusy = derived(
  [requestQueue, activeThreadId],
  ([$q, $tid]) => {
    return Object.values($q.byId).some(r => 
      r.threadId === $tid && !['success', 'error', 'canceled'].includes(r.phase)
    );
  }
);

export const activeThreadModelBusy = derived(
  [requestQueue, activeThreadId],
  ([$q, $tid]) => {
    return Object.values($q.byId).some((r) =>
      r.threadId === $tid && isModelActivePhase(r.phase),
    );
  },
);
