import type { AgentSession, Request } from '../types/domain';
import type { ThreadAgentState } from '../tauri/client';

export type ViewerBusyPhase = 'generating' | 'repairing' | 'rendering' | 'committing' | null;

type ViewerBusyInput = {
  activeThreadId: string | null;
  activeVersionId: string | null;
  activeModelId: string | null;
  activeThreadRequests: Request[];
  activeAgentSessions: AgentSession[];
  threadAgentState: ThreadAgentState | null | undefined;
  phase: Request['phase'] | 'idle' | 'booting';
  isManual: boolean;
  manualThreadId: string | null;
  manualMessageId: string | null;
  repairMessage: string | null;
  cookingPhrase: string | null;
  hasRenderableModel: boolean;
  suppressViewportBusyUi: boolean;
};

type ViewerBusyState = {
  showViewerBusyMask: boolean;
  viewerBusyPhase: ViewerBusyPhase;
  viewerBusyText: string | null;
};

function mapAgentPhaseToViewerBusy(session: AgentSession | null): ViewerBusyPhase {
  switch (session?.phase) {
    case 'rendering':
    case 'restoring_version':
      return 'rendering';
    case 'saving_version':
      return 'committing';
    case 'patching_params':
    case 'patching_macro':
    case 'reading':
    case 'resolving':
      return 'generating';
    default:
      return null;
  }
}

export function mapThreadAgentStateToViewerBusy(
  state: ThreadAgentState | null | undefined,
): ViewerBusyPhase {
  if (!state || state.connectionState !== 'active' || state.waitingOnPrompt || !state.busy) return null;
  switch (state.phase) {
    case 'rendering':
    case 'restoring_version':
      return 'rendering';
    case 'saving_version':
      return 'committing';
    default:
      return null;
  }
}

function isActiveRequestPhase(phase: Request['phase']): boolean {
  return !['success', 'error', 'canceled'].includes(phase);
}

function isModelBusyRequestPhase(phase: Request['phase']): boolean {
  return ['generating', 'repairing', 'queued_for_render', 'rendering', 'committing'].includes(phase);
}

function requestMatchesViewerTarget(
  request: Request,
  threadId: string | null,
  messageId: string | null,
  modelId: string | null,
): boolean {
  if (!threadId || request.threadId !== threadId) return false;
  if (modelId) {
    if (request.baseModelId) return request.baseModelId === modelId;
    if (messageId && request.baseMessageId) return request.baseMessageId === messageId;
    return false;
  }
  if (messageId) {
    if (request.baseMessageId) return request.baseMessageId === messageId;
    return false;
  }
  return true;
}

function sessionMatchesViewerTarget(
  candidate: AgentSession,
  threadId: string | null,
  messageId: string | null,
  modelId: string | null,
): boolean {
  if (!threadId || candidate.threadId !== threadId) return false;
  if (modelId) {
    if (candidate.modelId) return candidate.modelId === modelId;
    if (messageId && candidate.messageId) return candidate.messageId === messageId;
    return false;
  }
  if (messageId) {
    if (candidate.messageId) return candidate.messageId === messageId;
    return false;
  }
  return true;
}

function viewerBusyTextForAgent(
  session: { agentLabel: string | null; statusText: string | null },
  phase: ViewerBusyPhase,
): string | null {
  const agentLabel = session.agentLabel?.trim() || 'external agent';
  const statusText = session.statusText?.trim() ?? '';
  if (statusText) return statusText;
  switch (phase) {
    case 'rendering':
      return `External agent ${agentLabel} is updating the model.`;
    case 'committing':
      return `External agent ${agentLabel} is saving a version.`;
    case 'generating':
      return `External agent ${agentLabel} is preparing an update.`;
    default:
      return null;
  }
}

export function deriveViewerBusyState(input: ViewerBusyInput): ViewerBusyState {
  const localViewportRequests = input.activeThreadRequests.filter(
    (request) =>
      isActiveRequestPhase(request.phase) &&
      requestMatchesViewerTarget(
        request,
        input.activeThreadId,
        input.activeVersionId,
        input.activeModelId,
      ),
  );

  const externalViewerSession =
    input.activeAgentSessions.find((candidate) =>
      sessionMatchesViewerTarget(
        candidate,
        input.activeThreadId,
        input.activeVersionId,
        input.activeModelId,
      ),
    ) ?? null;

  const externalViewerBusyPhase = mapAgentPhaseToViewerBusy(externalViewerSession);
  const threadScopedAgentBusyPhase = mapThreadAgentStateToViewerBusy(input.threadAgentState);
  const manualViewerBusyPhase =
    input.isManual &&
    input.phase === 'rendering' &&
    input.manualThreadId === input.activeThreadId &&
    (input.manualMessageId ?? null) === (input.activeVersionId ?? null)
      ? 'rendering'
      : null;

  const localViewerBusyPhase: ViewerBusyPhase =
    localViewportRequests.some((request) => request.phase === 'committing')
      ? 'committing'
      : localViewportRequests.some((request) =>
            ['queued_for_render', 'rendering'].includes(request.phase),
          )
        ? 'rendering'
        : localViewportRequests.some((request) => request.phase === 'repairing')
          ? 'repairing'
          : localViewportRequests.some((request) => request.phase === 'generating')
            ? 'generating'
            : manualViewerBusyPhase === 'rendering'
              ? 'rendering'
              : null;

  const showViewerBusyMask =
    !input.suppressViewportBusyUi &&
    (localViewportRequests.some((request) => isModelBusyRequestPhase(request.phase)) ||
      manualViewerBusyPhase === 'rendering' ||
      Boolean(threadScopedAgentBusyPhase && input.hasRenderableModel) ||
      externalViewerBusyPhase === 'rendering' ||
      externalViewerBusyPhase === 'committing');

  const viewerBusyPhase =
    localViewerBusyPhase ?? threadScopedAgentBusyPhase ?? externalViewerBusyPhase;

  let viewerBusyText: string | null = null;
  switch (localViewerBusyPhase) {
    case 'repairing':
      viewerBusyText = input.repairMessage || 'Reweaving the geometry lattice.';
      break;
    case 'rendering':
      viewerBusyText = 'Stabilizing the geometry into manufacturable solids.';
      break;
    case 'committing':
      viewerBusyText = 'Finalizing the artifact and sealing it into the thread.';
      break;
    case 'generating':
      viewerBusyText = input.cookingPhrase || 'Preparing the next transformation.';
      break;
    default:
      if (threadScopedAgentBusyPhase && input.threadAgentState) {
        viewerBusyText = viewerBusyTextForAgent(input.threadAgentState, threadScopedAgentBusyPhase);
      } else if (externalViewerSession && externalViewerBusyPhase) {
        viewerBusyText = viewerBusyTextForAgent(externalViewerSession, externalViewerBusyPhase);
      }
      break;
  }

  return {
    showViewerBusyMask,
    viewerBusyPhase,
    viewerBusyText,
  };
}
