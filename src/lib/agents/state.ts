import type { GenieMode } from '../genie/traits';
import type { AgentSession, AutoAgent, McpMode } from '../types/domain';
import type { ThreadAgentState } from '../tauri/client';

export type PendingThreadPrompt = {
  requestId: string;
  agentLabel: string;
  threadId?: string | null;
};

export type PendingThreadScreenshot = {
  requestId: string;
  threadId: string;
};

export function normalizeMcpMode(mode: McpMode | null | undefined, autoAgents: AutoAgent[]): McpMode {
  if (mode === 'active' || mode === 'passive') return mode;
  return autoAgents.length > 0 ? 'active' : 'passive';
}

export function usesActiveMcpMode(
  connectionType: string | null | undefined,
  mode: McpMode | null | undefined,
): boolean {
  return connectionType === 'mcp' && mode === 'active';
}

export function usesMcpConnection(
  connectionType: string | null | undefined,
): boolean {
  return connectionType === 'mcp';
}

export function usesAgentDialogueMode(
  connectionType: string | null | undefined,
  threadAgentState:
    | {
        connectionState?: string | null | undefined;
      }
    | null
    | undefined,
): boolean {
  if (connectionType === 'mcp') return true;
  const state = threadAgentState?.connectionState?.trim() ?? '';
  return ['sleeping', 'waking', 'waiting', 'active', 'error'].includes(state);
}

export function hasLiveAgentSession(
  sessions: AgentSession[] | null | undefined,
): boolean {
  return Array.isArray(sessions) && sessions.length > 0;
}

export function shouldAutoFocusAgentWorkingVersion(input: {
  currentView: string | null | undefined;
  activeThreadId: string | null | undefined;
  eventThreadId: string | null | undefined;
}): boolean {
  return (
    input.currentView === 'workbench' &&
    !!input.activeThreadId &&
    !!input.eventThreadId &&
    input.activeThreadId === input.eventThreadId
  );
}

export function derivePrimaryAgentId(
  autoAgents: AutoAgent[],
  primaryAgentId: string | null | undefined,
): string | null {
  const enabledAgents = autoAgents.filter((agent) => agent.enabled);
  if (!enabledAgents.length) return null;
  if (primaryAgentId && enabledAgents.some((agent) => agent.id === primaryAgentId)) {
    return primaryAgentId;
  }
  return enabledAgents[0]?.id ?? null;
}

export function derivePrimaryAgentLabel(
  autoAgents: AutoAgent[],
  primaryAgentId: string | null | undefined,
): string | null {
  const resolvedPrimaryAgentId = derivePrimaryAgentId(autoAgents, primaryAgentId);
  if (!resolvedPrimaryAgentId) return null;
  return autoAgents.find((agent) => agent.id === resolvedPrimaryAgentId)?.label ?? null;
}

export function promptBelongsToPrimaryAgent(
  autoAgents: AutoAgent[],
  primaryAgentId: string | null | undefined,
  agentLabel: string | null | undefined,
): boolean {
  const primaryAgentLabel = derivePrimaryAgentLabel(autoAgents, primaryAgentId);
  if (!primaryAgentLabel) return true;
  return (agentLabel ?? '').trim() === primaryAgentLabel.trim();
}

export function phaseLabelForThreadAgentState(state: ThreadAgentState): string {
  if (state.activityLabel?.trim()) return state.activityLabel;
  if (state.statusText?.trim()) return state.statusText;
  switch (state.phase) {
    case 'rendering':
      return 'rendering model...';
    case 'restoring_version':
      return 'restoring version...';
    case 'saving_version':
      return 'saving version...';
    case 'patching_params':
      return 'tuning parameters...';
    case 'patching_macro':
      return 'editing macro...';
    case 'reading':
      return 'reading thread...';
    case 'resolving':
      return 'resolving...';
    case 'waiting_for_user':
      return 'waiting for your next message...';
    case 'error':
      return 'error';
    default:
      return '...';
  }
}

export function resolveActivePendingPrompt(input: {
  prompts: PendingThreadPrompt[];
  currentThreadId: string | null | undefined;
  connectionType: string | null | undefined;
  mode: McpMode | null | undefined;
  autoAgents: AutoAgent[];
  primaryAgentId: string | null | undefined;
}): PendingThreadPrompt | null {
  const filtered = input.prompts.filter((prompt) => {
    if (input.connectionType !== 'mcp' || input.mode !== 'active') {
      return true;
    }
    return promptBelongsToPrimaryAgent(
      input.autoAgents,
      input.primaryAgentId,
      prompt.agentLabel,
    );
  });
  if (!filtered.length) return null;

  const currentThreadId = input.currentThreadId?.trim() || null;
  if (currentThreadId) {
    return (
      filtered.find((prompt) => (prompt.threadId?.trim() || null) === currentThreadId) ??
      filtered.find((prompt) => !(prompt.threadId?.trim() || null)) ??
      null
    );
  }

  return filtered.find((prompt) => !(prompt.threadId?.trim() || null)) ?? null;
}

export function deriveThreadAttentionIds(input: {
  prompts: PendingThreadPrompt[];
  screenshots: PendingThreadScreenshot[];
  activePromptRequestId: string | null | undefined;
  currentThreadId: string | null | undefined;
}): string[] {
  const currentThreadId = input.currentThreadId?.trim() || null;
  const attention = new Set<string>();

  for (const prompt of input.prompts) {
    const promptThreadId = prompt.threadId?.trim() || null;
    if (!promptThreadId) continue;
    if (prompt.requestId === input.activePromptRequestId) continue;
    if (currentThreadId && promptThreadId === currentThreadId) continue;
    attention.add(promptThreadId);
  }

  for (const screenshot of input.screenshots) {
    const screenshotThreadId = screenshot.threadId?.trim() || null;
    if (!screenshotThreadId) continue;
    if (currentThreadId && screenshotThreadId === currentThreadId) continue;
    attention.add(screenshotThreadId);
  }

  return [...attention];
}

export function deriveMascotStateForThreadAgent(
  state: ThreadAgentState | null | undefined,
): { connected: boolean; mode: GenieMode; bubble: string } {
  if (!state || state.connectionState === 'none') {
    return { connected: false, mode: 'idle', bubble: '' };
  }

  const fallbackLabel = state.agentLabel || 'Agent';
  switch (state.connectionState) {
    case 'sleeping':
      return {
        connected: false,
        mode: 'idle',
        bubble: '',
      };
    case 'waking':
      return {
        connected: true,
        mode: 'waking',
        bubble: state.statusText || `Waking ${fallbackLabel}...`,
      };
    case 'waiting':
      return {
        connected: true,
        mode: 'light',
        bubble: phaseLabelForThreadAgentState(state),
      };
    case 'active':
      if (state.phase === 'rendering' || state.phase === 'restoring_version') {
        return { connected: true, mode: 'rendering', bubble: phaseLabelForThreadAgentState(state) };
      }
      if (state.phase === 'saving_version') {
        return { connected: true, mode: 'light', bubble: phaseLabelForThreadAgentState(state) };
      }
      return {
        connected: true,
        mode: state.busy ? 'thinking' : 'light',
        bubble: phaseLabelForThreadAgentState(state),
      };
    case 'error':
      return {
        connected: true,
        mode: 'error',
        bubble: state.statusText || `${fallbackLabel} hit an error.`,
      };
    case 'disconnected':
      return {
        connected: false,
        mode: 'idle',
        bubble: state.statusText || `${fallbackLabel} disconnected.`,
      };
    default:
      return { connected: false, mode: 'idle', bubble: '' };
  }
}
