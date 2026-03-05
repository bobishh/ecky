import { writable, derived, get } from 'svelte/store';

export type RequestPhase = 
  | 'classifying'
  | 'answering'
  | 'generating' 
  | 'queued_for_render'
  | 'rendering'
  | 'committing'
  | 'repairing'
  | 'success'
  | 'error'
  | 'canceled';

export interface QueuedRequest {
  id: string;
  prompt: string;
  attachments: any[];
  createdAt: number;
  phase: RequestPhase;
  attempt: number;
  maxAttempts: number;
  isQuestion: boolean;
  lightResponse: string;
  screenshot: string | null;
  threadId: string | null;
  result: {
    design: any;
    threadId: string;
    messageId: string;
    stlUrl: string;
  } | null;
  error: string | null;
  cookingStartTime: number | null;
  cookingElapsed: number;
}

function createRequestQueue() {
  const { subscribe, set, update } = writable<{
    byId: Record<string, QueuedRequest>;
    order: string[];
    activeId: string | null;
  }>({
    byId: {},
    order: [],
    activeId: null,
  });

  const MAX_CONCURRENT_LLM = 4;

  return {
    subscribe,

    submit(prompt: string, attachments: any[] = [], threadId: string | null = null): string {
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
      };
      update(q => ({
        ...q,
        byId: { ...q.byId, [id]: request },
        order: [...q.order, id],
        activeId: q.activeId || id,
      }));
      return id;
    },

    patch(id: string, changes: Partial<QueuedRequest>) {
      update(q => {
        const existing = q.byId[id];
        if (!existing) return q;
        return {
          ...q,
          byId: { ...q.byId, [id]: { ...existing, ...changes } },
        };
      });
    },

    setActive(id: string | null) {
      update(q => ({ ...q, activeId: id }));
    },

    remove(id: string) {
      update(q => {
        const { [id]: _, ...rest } = q.byId;
        return {
          byId: rest,
          order: q.order.filter(x => x !== id),
          activeId: q.activeId === id ? (q.order.find(x => x !== id) || null) : q.activeId,
        };
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
