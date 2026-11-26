import { writable } from 'svelte/store';

export type LocalNotificationAction = {
  label: string;
  onclick: () => void;
};

export type LocalNotificationCard = {
  eventId: string;
  activityEventId?: string | null;
  threadId: string | null;
  actorLabel: string;
  summary: string;
  detail: string | null;
  severity: 'info' | 'success' | 'warning' | 'error' | 'question';
  state: 'active' | 'resolved' | 'failed';
  requiresAttention: boolean;
  persistUntilDismissed?: boolean;
  actions: LocalNotificationAction[];
};

type LocalNotificationTimerHandle = ReturnType<typeof globalThis.setTimeout> | number;

export type LocalNotificationClock = {
  setTimeout: (callback: () => void, delayMs: number) => LocalNotificationTimerHandle;
  clearTimeout: (handle: LocalNotificationTimerHandle) => void;
};

const systemClock: LocalNotificationClock = {
  setTimeout(callback, delayMs) {
    const handle = globalThis.setTimeout(callback, delayMs);
    if (typeof handle === 'object' && typeof handle.unref === 'function') handle.unref();
    return handle;
  },
  clearTimeout: (handle) => globalThis.clearTimeout(handle),
};

function notificationDurationMs(card: LocalNotificationCard): number | null {
  if (card.persistUntilDismissed) return null;
  if (card.severity === 'warning') return 12_000;
  return 8_000;
}

function cardFingerprint(card: LocalNotificationCard): string {
  return `${card.eventId}:${card.state}:${card.severity}:${card.summary}:${card.detail ?? ''}`;
}

export function createLocalNotificationStore(clock: LocalNotificationClock = systemClock) {
  const store = writable<LocalNotificationCard | null>(null);
  let timer: LocalNotificationTimerHandle | null = null;
  let currentEventId: string | null = null;
  let currentFingerprint: string | null = null;
  const dismissedFingerprints = new Set<string>();

  function hide() {
    if (timer !== null) {
      clock.clearTimeout(timer);
      timer = null;
    }
    currentEventId = null;
    currentFingerprint = null;
    store.set(null);
  }

  function set(card: LocalNotificationCard | null) {
    if (!card) {
      if (currentFingerprint !== null) {
        dismissedFingerprints.add(currentFingerprint);
      }
      hide();
      return;
    }

    const fingerprint = cardFingerprint(card);
    if (dismissedFingerprints.has(fingerprint)) {
      return;
    }

    if (card.eventId === currentEventId) {
      currentFingerprint = fingerprint;
      store.set(card);
      return;
    }

    if (timer !== null) {
      clock.clearTimeout(timer);
      timer = null;
    }

    currentEventId = card.eventId;
    currentFingerprint = fingerprint;
    store.set(card);

    const ttlMs = notificationDurationMs(card);
    if (ttlMs === null) return;

    timer = clock.setTimeout(() => {
      timer = null;
      if (currentFingerprint !== null) {
        dismissedFingerprints.add(currentFingerprint);
      }
      hide();
    }, ttlMs);
  }

  function dismiss(eventId?: string) {
    if (!eventId || eventId === currentEventId) {
      set(null);
    }
  }

  function clear() {
    dismissedFingerprints.clear();
    hide();
  }

  return { subscribe: store.subscribe, set, dismiss, hide, clear };
}

export const localNotificationActionsStore = createLocalNotificationStore();

