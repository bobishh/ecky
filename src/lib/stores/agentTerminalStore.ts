import { derived, writable } from 'svelte/store';

import {
  agentTerminalSessionKey,
  hasRenderableTerminal,
  mergeAgentTerminalSnapshot,
  pickAgentTerminalAttention,
  pickVisibleAgentTerminal,
} from '../agents/terminal';
import type { AgentTerminalSnapshot } from '../types/domain';

type TerminalSelection = {
  primaryAgentId: string | null;
  preferredSessionId: string | null;
};

const snapshotMap = writable<Record<string, AgentTerminalSnapshot>>({});
const selection = writable<TerminalSelection>({
  primaryAgentId: null,
  preferredSessionId: null,
});

let pendingSnapshots: Record<string, AgentTerminalSnapshot> = {};
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function upsertSnapshot(snapshot: AgentTerminalSnapshot) {
  const snapshotKey = agentTerminalSessionKey(snapshot);
  snapshotMap.update((current) => {
    if (!hasRenderableTerminal(snapshot)) {
      const { [snapshotKey]: _discarded, ...rest } = current;
      return rest;
    }
    const previous = current[snapshotKey] ?? null;
    const next = Object.fromEntries(
      Object.entries(current).filter(([key, existing]) => {
        if (key === snapshotKey) return true;
        if (existing.agentId !== snapshot.agentId) return true;
        return existing.sessionId !== snapshot.sessionId;
      }),
    );
    return {
      ...next,
      [snapshotKey]: mergeAgentTerminalSnapshot(previous, snapshot),
    };
  });
}

function flushPendingSnapshots() {
  flushTimer = null;
  const pending = pendingSnapshots;
  pendingSnapshots = {};
  for (const snapshot of Object.values(pending)) {
    upsertSnapshot(snapshot);
  }
}

export function replaceAgentTerminalSnapshots(snapshots: AgentTerminalSnapshot[]) {
  const next = Object.fromEntries(
    snapshots
      .filter(hasRenderableTerminal)
      .map((snapshot) => [agentTerminalSessionKey(snapshot), snapshot]),
  );
  snapshotMap.set(next);
}

export function enqueueAgentTerminalSnapshot(snapshot: AgentTerminalSnapshot) {
  const snapshotKey = agentTerminalSessionKey(snapshot);
  pendingSnapshots = {
    ...pendingSnapshots,
    [snapshotKey]: snapshot,
  };
  if (flushTimer) return;
  flushTimer = setTimeout(flushPendingSnapshots, 75);
}

export function setAgentTerminalSelection(
  primaryAgentId: string | null | undefined,
  preferredSessionId: string | null | undefined,
) {
  selection.set({
    primaryAgentId: primaryAgentId ?? null,
    preferredSessionId: preferredSessionId ?? null,
  });
}

export function resetAgentTerminalStore() {
  if (flushTimer) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  pendingSnapshots = {};
  snapshotMap.set({});
  selection.set({ primaryAgentId: null, preferredSessionId: null });
}

const snapshotList = derived(snapshotMap, ($snapshotMap) => Object.values($snapshotMap));

export const visibleAgentTerminalStore = derived(
  [snapshotList, selection],
  ([$snapshotList, $selection]) =>
    pickVisibleAgentTerminal(
      $snapshotList,
      $selection.primaryAgentId,
      $selection.preferredSessionId,
    ),
);

export const agentTerminalAttentionStore = derived(
  [snapshotList, selection],
  ([$snapshotList, $selection]) =>
    pickAgentTerminalAttention(
      $snapshotList,
      $selection.primaryAgentId,
      $selection.preferredSessionId,
    ),
);
