import type {
  AgyProviderBinding,
  CodexDialogueMessage,
  CodexProviderHistoryUpdate,
  CodexQueuedPrompt,
  CodexTakeoverRuntime,
  ProviderTurnTrace,
} from './tauri/contracts';
import type { Message } from './types/domain';
import { projectProviderTurnMessages } from './providerActivity';

export type CodexProviderUpdateEvent = Omit<CodexProviderHistoryUpdate, 'eckyThreadId'> & {
  eckyThreadId?: string;
  liveMessages?: CodexDialogueMessage[];
  turnTraces?: ProviderTurnTrace[];
  runtime?: CodexTakeoverRuntime;
};

export function codexProviderUpdateTarget(
  event: CodexProviderUpdateEvent,
  activeThreadId: string | null,
  connectionType: string | null | undefined,
  binding: { eckyThreadId: string; codexThreadId: string } | null | undefined,
): string | null {
  if (!activeThreadId || connectionType !== 'provider:codex') return null;
  if (event.eckyThreadId) return event.eckyThreadId === activeThreadId ? activeThreadId : null;
  return binding?.eckyThreadId === activeThreadId && binding.codexThreadId === event.threadId
    ? activeThreadId : null;
}

export type ProviderDialogueSnapshot = {
  providerId: 'codex' | 'agy';
  providerLabel: string;
  externalConversationId: string;
  binding?: { eckyThreadId: string } | AgyProviderBinding;
  messages: CodexDialogueMessage[];
  liveMessages: CodexDialogueMessage[];
  turnTraces: ProviderTurnTrace[];
  nextCursor: string | null;
  backwardsCursor: string | null;
  runtime: CodexTakeoverRuntime;
  queue: CodexQueuedPrompt[];
};

export type ProviderDialogueState = {
  snapshot: ProviderDialogueSnapshot | null;
  revision: number;
  loadToken: number;
  loading: boolean;
  error: string | null;
};

export type ProviderDialogueAction =
  | { type: 'loadStarted'; token: number }
  | { type: 'snapshot'; snapshot: ProviderDialogueSnapshot | null; preserveLoadedPages: boolean; token?: number }
  | { type: 'authoritativeSnapshot'; snapshot: ProviderDialogueSnapshot; preserveLoadedPages: boolean }
  | { type: 'page'; page: { messages: CodexDialogueMessage[]; nextCursor: string | null; backwardsCursor: string | null }; direction: 'older' | 'newer' }
  | { type: 'live'; liveMessages: CodexDialogueMessage[]; turnTraces?: ProviderTurnTrace[]; runtime: CodexTakeoverRuntime }
  | { type: 'error'; error: string }
  | { type: 'clearError' };

export function createProviderDialogueState(): ProviderDialogueState {
  return { snapshot: null, revision: 0, loadToken: 0, loading: false, error: null };
}

export function isProviderResultCurrent(
  snapshot: { binding?: { eckyThreadId: string } } | null,
  expectedThreadId: string,
): boolean {
  return snapshot?.binding?.eckyThreadId === expectedThreadId;
}

function mergeMessages(existing: CodexDialogueMessage[], incoming: CodexDialogueMessage[], direction: 'older' | 'newer'): CodexDialogueMessage[] {
  const ordered = direction === 'older' ? [...incoming, ...existing] : [...existing, ...incoming];
  const byId = new Map<string, CodexDialogueMessage>();
  for (const item of ordered) if (!byId.has(item.id)) byId.set(item.id, item);
  // Incoming page is authoritative for identities already present.
  for (const item of incoming) byId.set(item.id, item);
  return [...byId.values()];
}

function acceptsToken(state: ProviderDialogueState, token?: number): boolean {
  return token === undefined || token >= state.loadToken;
}

export function applyProviderDialogueAction(state: ProviderDialogueState, action: ProviderDialogueAction): ProviderDialogueState {
  if (action.type === 'loadStarted') {
    return { ...state, loadToken: Math.max(state.loadToken, action.token), loading: true, error: null };
  }
  if (action.type === 'authoritativeSnapshot') {
    const applied = applyProviderDialogueAction(state, {
      type: 'snapshot',
      snapshot: action.snapshot,
      preserveLoadedPages: action.preserveLoadedPages,
    });
    return { ...applied, loadToken: state.loadToken + 1 };
  }
  if (action.type === 'snapshot') {
    if (!acceptsToken(state, action.token)) return state;
    const next = action.snapshot && action.preserveLoadedPages && state.snapshot
      ? { ...action.snapshot, messages: mergeMessages(state.snapshot.messages, action.snapshot.messages, 'newer'), nextCursor: state.snapshot.messages.length > action.snapshot.messages.length ? state.snapshot.nextCursor : action.snapshot.nextCursor, backwardsCursor: action.snapshot.backwardsCursor ?? state.snapshot.backwardsCursor }
      : action.snapshot;
    return { ...state, snapshot: next, revision: state.revision + 1, loading: false, error: next?.runtime.error ?? null };
  }
  if (action.type === 'error') return { ...state, loading: false, error: action.error };
  if (action.type === 'clearError') return { ...state, error: null };
  if (!state.snapshot) return state;
  if (action.type === 'page') {
    return { ...state, snapshot: { ...state.snapshot, messages: mergeMessages(state.snapshot.messages, action.page.messages, action.direction), nextCursor: action.page.nextCursor, backwardsCursor: action.page.backwardsCursor }, revision: state.revision + 1, error: null };
  }
  if (action.type === 'live') {
    return { ...state, snapshot: { ...state.snapshot, liveMessages: action.liveMessages, turnTraces: action.turnTraces ?? state.snapshot.turnTraces, runtime: action.runtime }, revision: state.revision + 1, error: action.runtime.error };
  }
  return state;
}

export function projectProviderDialogue(snapshot: ProviderDialogueSnapshot): Message[] {
  const liveIds = new Set(snapshot.liveMessages.map((item) => item.id));
  const persisted: Message[] = snapshot.messages
    .filter((item) => !liveIds.has(item.id))
    .map((item, index) => ({
      id: item.id,
      role: item.role === 'assistant' ? 'assistant' : 'user',
      content: item.content,
      status: item.role === 'user' ? 'success' : (['pending', 'working', 'success', 'error', 'discarded'].includes(item.status) ? item.status as Message['status'] : 'success'),
      timestamp: item.timestamp,
      timelineOrder: index,
      attachmentImages: item.attachments
        ?.filter((attachment) => attachment.kind === 'image')
        .map((attachment) => attachment.dataUrl || attachment.path),
    }));
  const turn = (messages: CodexDialogueMessage[], phase: 'active' | 'completed' | 'interrupted' | 'error', turnId: string | null) => projectProviderTurnMessages({ providerId: snapshot.providerId, providerLabel: snapshot.providerLabel, externalConversationId: snapshot.externalConversationId, activeTurnId: turnId, phase, messages });
  const traceMessages = snapshot.turnTraces.flatMap((trace) => turn(trace.messages, trace.status === 'success' ? 'completed' : trace.status === 'interrupted' ? 'interrupted' : 'error', trace.turnId));
  const projected = [...persisted, ...traceMessages, ...turn(snapshot.liveMessages, 'active', snapshot.runtime.activeTurnId)];
  const byId = new Map<string, Message>();
  for (const item of projected) byId.set(item.id, item);
  return [...byId.values()];
}

export function mergeProviderSnapshot<T extends { binding: { eckyThreadId: string }; messages: CodexDialogueMessage[]; nextCursor: string | null; backwardsCursor: string | null }>(current: T | null, next: T, preserveLoadedPages: boolean): T {
  if (!preserveLoadedPages || !current || current.binding.eckyThreadId !== next.binding.eckyThreadId) return next;
  return {
    ...next,
    messages: mergeMessages(current.messages, next.messages, 'newer'),
    nextCursor: current.messages.length > next.messages.length ? current.nextCursor : next.nextCursor,
    backwardsCursor: next.backwardsCursor ?? current.backwardsCursor,
  };
}

export function mergeProviderPage<
  T extends { messages: CodexDialogueMessage[]; nextCursor: string | null; backwardsCursor: string | null },
  P extends { messages: CodexDialogueMessage[]; nextCursor: string | null; backwardsCursor: string | null },
>(current: T, page: P): T {
  return { ...current, messages: mergeMessages(current.messages, page.messages, 'older'), nextCursor: page.nextCursor, backwardsCursor: page.backwardsCursor };
}
