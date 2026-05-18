import { get, writable } from 'svelte/store';
import { appendSessionEvent, type SessionActor, type SessionEvent } from '../sessionActivity';

export const sessionActivityEvents = writable<SessionEvent[]>([]);

let eventSeq = 0;

export type SessionActivityEventInput = Omit<SessionEvent, 'id' | 'sessionId' | 'timestamp' | 'actor'> &
  Partial<Pick<SessionEvent, 'id' | 'sessionId' | 'timestamp' | 'actor'>>;

export function recordSessionActivityEvent(input: SessionActivityEventInput): SessionEvent {
  const now = input.timestamp ?? Date.now();
  const actor: SessionActor = input.actor ?? { kind: 'system', id: 'ecky' };
  const event: SessionEvent = {
    ...input,
    id: input.id ?? `session-event:${now}:${eventSeq++}`,
    sessionId: input.sessionId ?? 'local-session',
    timestamp: now,
    actor,
  };
  sessionActivityEvents.update((events) => appendSessionEvent(events, event));
  return event;
}

export function clearSessionActivityEvents() {
  eventSeq = 0;
  sessionActivityEvents.set([]);
}

export function currentSessionActivityEvents(): SessionEvent[] {
  return get(sessionActivityEvents);
}
