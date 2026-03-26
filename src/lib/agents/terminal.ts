import type { AgentTerminalInput, AgentTerminalSnapshot } from '../types/domain';

const TERMINAL_REPLAY_LIMIT = 65_536;

export type TerminalKeyEventLike = {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
};

const PASSTHROUGH_KEYS = new Set([
  'Enter',
  'Tab',
  'Escape',
  'Backspace',
  'Delete',
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'Home',
  'End',
  'PageUp',
  'PageDown',
  'Insert',
]);

const IGNORED_KEYS = new Set([
  'Shift',
  'Control',
  'Alt',
  'Meta',
  'CapsLock',
  'NumLock',
  'ScrollLock',
  'Fn',
  'Hyper',
  'Super',
  'OS',
  'Compose',
]);

function blankTerminalInput(agentId: string): AgentTerminalInput {
  return {
    agentId,
    text: '',
    key: null,
    ctrl: false,
    alt: false,
    shift: false,
    meta: false,
    submit: false,
  };
}

export function buildAgentTerminalLineInput(
  agentId: string,
  text: string,
  submit = true,
): AgentTerminalInput | null {
  if (!text.length && !submit) return null;
  return {
    ...blankTerminalInput(agentId),
    text,
    submit,
  };
}

export function buildAgentTerminalKeyInput(
  agentId: string,
  event: TerminalKeyEventLike,
): AgentTerminalInput | null {
  if (!event.key || event.key === 'Unidentified' || event.key === 'Process' || event.key === 'Dead') {
    return null;
  }
  if (IGNORED_KEYS.has(event.key)) return null;
  if (event.metaKey && !event.ctrlKey && !event.altKey) return null;

  if (event.key.length === 1 && !event.ctrlKey && !event.altKey && !event.metaKey) {
    return {
      ...blankTerminalInput(agentId),
      text: event.key,
      shift: event.shiftKey,
    };
  }

  if (PASSTHROUGH_KEYS.has(event.key) || event.ctrlKey || event.altKey) {
    return {
      ...blankTerminalInput(agentId),
      key: event.key,
      ctrl: event.ctrlKey,
      alt: event.altKey,
      shift: event.shiftKey,
      meta: event.metaKey,
    };
  }

  return null;
}

function byNewest(a: AgentTerminalSnapshot, b: AgentTerminalSnapshot): number {
  return b.updatedAt - a.updatedAt;
}

export function agentTerminalSessionKey(
  snapshot: AgentTerminalSnapshot | null | undefined,
): string {
  if (!snapshot) return '';
  return snapshot.sessionId
    ? `${snapshot.agentId}:${snapshot.sessionId}:${snapshot.sessionNonce}`
    : `${snapshot.agentId}:${snapshot.sessionNonce}`;
}

export function resolveAgentTerminalReplayText(
  snapshot: AgentTerminalSnapshot | null | undefined,
): string {
  if (!snapshot) return '';
  if (snapshot.vtStream?.length) return snapshot.vtStream;
  return snapshot.active ? '' : snapshot.screenText;
}

function trimReplayTail(output: string, maxChars: number): string {
  if (output.length <= maxChars) return output;
  return output.slice(output.length - maxChars);
}

export function mergeAgentTerminalSnapshot(
  previous: AgentTerminalSnapshot | null | undefined,
  incoming: AgentTerminalSnapshot,
): AgentTerminalSnapshot {
  const incomingDelta = incoming.vtDelta ?? '';
  if (!previous) {
    return {
      ...incoming,
      vtStream: trimReplayTail(
        incoming.vtStream || incomingDelta,
        TERMINAL_REPLAY_LIMIT,
      ),
      vtDelta: null,
    };
  }

  const sameSession =
    agentTerminalSessionKey(previous) === agentTerminalSessionKey(incoming);

  const mergedStream = sameSession
    ? trimReplayTail(
        incoming.vtStream
          ? incoming.vtStream
          : `${previous.vtStream ?? ''}${incomingDelta}`,
        TERMINAL_REPLAY_LIMIT,
      )
    : trimReplayTail(incoming.vtStream || incomingDelta, TERMINAL_REPLAY_LIMIT);

  return {
    ...previous,
    ...incoming,
    vtStream: mergedStream,
    vtDelta: null,
  };
}

function largestSuffixPrefixOverlap(previous: string, next: string): number {
  const maxOverlap = Math.min(previous.length, next.length);
  for (let size = maxOverlap; size > 0; size -= 1) {
    if (previous.slice(-size) === next.slice(0, size)) {
      return size;
    }
  }
  return 0;
}

export function resolveTerminalStreamWrite(
  previous: string,
  next: string,
): { mode: 'noop' | 'append' | 'reset'; data: string } {
  if (next === previous) {
    return { mode: 'noop', data: '' };
  }
  if (!previous.length) {
    return { mode: 'reset', data: next };
  }
  if (next.startsWith(previous)) {
    return { mode: 'append', data: next.slice(previous.length) };
  }

  const overlap = largestSuffixPrefixOverlap(previous, next);
  const overlapThreshold = Math.min(512, Math.max(32, Math.floor(next.length / 4)));
  if (overlap > 0 && (overlap === next.length || overlap >= overlapThreshold)) {
    const delta = next.slice(overlap);
    return delta.length
      ? { mode: 'append', data: delta }
      : { mode: 'noop', data: '' };
  }

  return { mode: 'reset', data: next };
}

export function shouldReplayTerminalOnVisibilityRestore(input: {
  previousSessionKey: string;
  nextSessionKey: string;
  wasVisible: boolean;
  isVisible: boolean;
}): boolean {
  if (!input.isVisible) return false;
  if (input.previousSessionKey !== input.nextSessionKey) return true;
  return !input.wasVisible;
}

export function hasRenderableTerminal(snapshot: AgentTerminalSnapshot | null | undefined): boolean {
  if (!snapshot) return false;
  return snapshot.active
    || snapshot.attentionRequired
    || resolveAgentTerminalReplayText(snapshot).trim().length > 0;
}

export function pickVisibleAgentTerminal(
  snapshots: AgentTerminalSnapshot[],
  primaryAgentId: string | null | undefined,
  preferredSessionId: string | null | undefined = null,
): AgentTerminalSnapshot | null {
  const renderable = snapshots.filter(hasRenderableTerminal);
  if (!renderable.length) return null;

  const active = renderable.filter((snapshot) => snapshot.active);
  const inactive = renderable.filter((snapshot) => !snapshot.active);
  const preferSession = (items: AgentTerminalSnapshot[]) =>
    preferredSessionId
      ? items.filter((snapshot) => snapshot.sessionId === preferredSessionId)
      : items;

  const activeForSession = preferSession(active);
  const inactiveForSession = preferSession(inactive);

  const primaryAttention =
    primaryAgentId
      ? activeForSession.find(
          (snapshot) => snapshot.agentId === primaryAgentId && snapshot.attentionRequired,
        ) ?? null
      : null;
  if (primaryAttention) return primaryAttention;

  const anyAttention = [...(activeForSession.length ? activeForSession : active)]
    .filter((snapshot) => snapshot.attentionRequired)
    .sort(byNewest)[0];
  if (anyAttention) return anyAttention;

  if (primaryAgentId && activeForSession.length) {
    const primary =
      activeForSession.find((snapshot) => snapshot.agentId === primaryAgentId) ?? null;
    if (primary) return primary;
  }

  const newestActive = [...(activeForSession.length ? activeForSession : active)].sort(byNewest)[0] ?? null;
  if (newestActive) return newestActive;

  if (primaryAgentId && inactiveForSession.length) {
    const primaryFallback =
      inactiveForSession.find((snapshot) => snapshot.agentId === primaryAgentId) ?? null;
    if (primaryFallback) return primaryFallback;
  }

  const preferredInactive = inactiveForSession.length ? inactiveForSession : inactive;
  return [...preferredInactive].sort(byNewest)[0] ?? null;
}

export function pickAgentTerminalAttention(
  snapshots: AgentTerminalSnapshot[],
  primaryAgentId: string | null | undefined,
  preferredSessionId: string | null | undefined = null,
): AgentTerminalSnapshot | null {
  const visible = pickVisibleAgentTerminal(snapshots, primaryAgentId, preferredSessionId);
  return visible?.attentionRequired ? visible : null;
}
