import { hasLiveAgentSession, deriveThreadAttentionIds, resolveActivePendingPrompt, type PendingThreadPrompt, type PendingThreadScreenshot } from '../agents/state';
import { isThreadAgentBusy, resolveActiveMcpBubble, resolveTerminalActivityMeta } from '../agents/activity';
import { mapThreadAgentStateToViewerBusy } from './viewerBusyState';
import type { AgentSession, AutoAgent, Request, ViewerAsset, AgentTerminalSnapshot, ViewportCameraState } from '../types/domain';
import type { ThreadAgentState } from '../tauri/client';

export type PendingViewportScreenshotChoice = PendingThreadScreenshot & {
  messageId: string;
  modelId?: string | null;
  previewStlPath: string;
  viewerAssets: ViewerAsset[];
  includeOverlays: boolean;
  message: string;
  buttons: string[];
  camera?: ViewportCameraState | null;
};

export type PendingAgentPrompt = PendingThreadPrompt & {
  message?: string | null;
  sessionId?: string;
  messageId?: string | null;
  modelId?: string | null;
};

export type AgentOpsInput = {
  activeAgentSessions: AgentSession[];
  activeThreadId: string | null;
  activeThreadRequests: Request[];
  autoAgents: AutoAgent[];
  connectionType: string | null | undefined;
  cookingPhrase: string | null;
  hasRenderableModel: boolean;
  mcpMode: 'passive' | 'active' | null | undefined;
  nowSecs: number;
  pendingAgentPrompts: PendingAgentPrompt[];
  pendingViewportScreenshotChoices: PendingViewportScreenshotChoice[];
  primaryAgentId: string | null | undefined;
  primaryAgentLabel: string | null;
  suppressViewportBusyUi: boolean;
  threadAgentState: ThreadAgentState | null;
  visibleAgentTerminal: AgentTerminalSnapshot | null;
  activeVersionId: string | null;
};

export type AgentOpsState = {
  activePendingAgentPrompt: PendingAgentPrompt | null;
  threadAttentionIds: string[];
  activeViewportScreenshotChoice: PendingViewportScreenshotChoice | null;
  activeMcpBusy: boolean;
  activeMcpRenderBusy: boolean;
  activeMcpBubbleSummary: string;
  activeAgentTerminalMetaSummary: string;
  activeMascotAgentIdentity: string;
  hasLiveMcpSession: boolean;
};

export function formatAgentPhase(phase: string): string {
  return phase.replace(/_/g, ' ').toUpperCase();
}

export function formatAgentOriginLabel(origin: { hostLabel?: string | null; agentLabel?: string | null; llmModelLabel?: string | null; llmModelId?: string | null } | null | undefined): string | null {
  if (!origin) return null;
  const host = origin.hostLabel?.trim() || origin.agentLabel?.trim() || 'Agent';
  const model = origin.llmModelLabel?.trim() || origin.llmModelId?.trim() || '';
  if (!model || model.toLowerCase() === host.toLowerCase()) {
    return host;
  }
  return `${host} · ${model}`;
}

export function shouldSuppressOnboardingForAutomation(): boolean {
  if (typeof navigator === 'undefined') return false;
  return Boolean(navigator.webdriver);
}

export function deriveAgentOpsState(input: AgentOpsInput): AgentOpsState {
  const activePendingAgentPrompt = resolveActivePendingPrompt({
    prompts: input.pendingAgentPrompts as PendingThreadPrompt[],
    currentThreadId: input.activeThreadId,
    connectionType: input.connectionType,
    mode: input.mcpMode,
    autoAgents: input.autoAgents,
    primaryAgentId: input.primaryAgentId,
  });

  const threadAttentionIds = deriveThreadAttentionIds({
    prompts: input.pendingAgentPrompts,
    screenshots: input.pendingViewportScreenshotChoices.map((choice) => ({
      requestId: choice.requestId,
      threadId: choice.threadId,
    })),
    activePromptRequestId: activePendingAgentPrompt?.requestId ?? null,
    currentThreadId: input.activeThreadId,
  });

  const activeViewportScreenshotChoice =
    input.pendingViewportScreenshotChoices.find((choice) => choice.threadId === (input.activeThreadId ?? '')) ??
    null;

  const activeMcpBusy = Boolean(input.connectionType === 'mcp' && isThreadAgentBusy(input.threadAgentState));
  const activeMcpRenderBusy = Boolean(
    input.connectionType === 'mcp' && mapThreadAgentStateToViewerBusy(input.threadAgentState) !== null,
  );
  const activeMcpBubbleSummary = resolveActiveMcpBubble({
    threadAgentState: input.threadAgentState,
    visibleAgentTerminal: input.visibleAgentTerminal,
    cookingPhrase: input.cookingPhrase,
    nowSecs: input.nowSecs,
  });
  const activeAgentTerminalMetaSummary = resolveTerminalActivityMeta({
    threadAgentState: input.threadAgentState,
    visibleAgentTerminal: input.visibleAgentTerminal,
    nowSecs: input.nowSecs,
  });
  const hasLiveMcpSession = hasLiveAgentSession(input.activeAgentSessions);

  const activeMascotAgentIdentity = (() => {
    if (activePendingAgentPrompt?.agentLabel?.trim()) {
      return activePendingAgentPrompt.agentLabel.trim();
    }
    if (input.visibleAgentTerminal?.active && input.visibleAgentTerminal.agentLabel?.trim()) {
      return input.visibleAgentTerminal.agentLabel.trim();
    }
    const connectedThreadAgent =
      input.threadAgentState?.agentLabel?.trim() &&
      ['waking', 'waiting', 'active', 'error'].includes(input.threadAgentState.connectionState)
        ? input.threadAgentState.agentLabel.trim()
        : null;
    if (connectedThreadAgent) {
      return connectedThreadAgent;
    }
    const primaryLiveSession =
      input.primaryAgentLabel?.trim()
        ? input.activeAgentSessions.find((session) => {
            const agentLabel = session.agentLabel?.trim() ?? '';
            const hostLabel = session.hostLabel?.trim() ?? '';
            const primaryLabel = input.primaryAgentLabel?.trim() ?? '';
            return agentLabel === primaryLabel || hostLabel === primaryLabel;
          }) ?? null
        : null;
    if (primaryLiveSession?.agentLabel?.trim()) {
      return primaryLiveSession.agentLabel.trim();
    }
    if (input.activeAgentSessions[0]?.agentLabel?.trim()) {
      return input.activeAgentSessions[0].agentLabel.trim();
    }
    if (input.primaryAgentLabel?.trim()) {
      return input.primaryAgentLabel.trim();
    }
    return 'Ecky';
  })();

  return {
    activePendingAgentPrompt,
    threadAttentionIds,
    activeViewportScreenshotChoice,
    activeMcpBusy,
    activeMcpRenderBusy,
    activeMcpBubbleSummary,
    activeAgentTerminalMetaSummary,
    activeMascotAgentIdentity,
    hasLiveMcpSession,
  };
}
