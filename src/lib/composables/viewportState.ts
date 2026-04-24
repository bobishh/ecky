import { viewportTargetKey } from '../agents/screenshot';
import { formatAgentOriginLabel } from './agentOps';
import { listConceptPreviewMessages, resolveEffectiveConceptPreviewMessage, type ConceptPreviewUiState, type ViewportPresentationMode } from '../viewportBlueprint';
import type { ArtifactBundle, Message, ViewportCameraState, ViewerAsset } from '../types/domain';

export type ViewportStateInput = {
  activeArtifactBundle: ArtifactBundle | null;
  activeThreadId: string | null;
  activeThreadMessages: Message[];
  activeVersionId: string | null;
  activeVersionMessage: Message | null;
  cameraStateByTarget: Record<string, ViewportCameraState>;
  conceptPreviewUiByThread: Record<string, ConceptPreviewUiState>;
  stlUrl: string | null;
  toAssetUrl: (path: string | null | undefined) => string;
};

export type ViewportState = {
  viewerAssets: ViewerAsset[];
  hasRenderableModel: boolean;
  activeThreadConceptPreviewState: ConceptPreviewUiState;
  conceptPreviewMessages: Message[];
  effectiveConceptPreviewMessage: Message | null;
  viewportPresentationMode: ViewportPresentationMode;
  showBlueprintViewport: boolean;
  blueprintAttentionVisible: boolean;
  currentViewportTargetKey: string | null;
  currentViewerModelKey: string | null;
  persistedViewportCameraState: ViewportCameraState | null;
  activeVersionAgentLabel: string | null;
};

export function defaultConceptPreviewUiState(): ConceptPreviewUiState {
  return {
    pinnedMessageId: null,
    lastAutoPinnedMessageId: null,
    mode: 'model',
  };
}

export function sameConceptPreviewUiState(left: ConceptPreviewUiState, right: ConceptPreviewUiState): boolean {
  return (
    left.pinnedMessageId === right.pinnedMessageId &&
    left.lastAutoPinnedMessageId === right.lastAutoPinnedMessageId &&
    left.mode === right.mode
  );
}

export function deriveViewportState(input: ViewportStateInput): ViewportState {
  const viewerAssets = (input.activeArtifactBundle?.viewerAssets || []).map((asset) => ({
    ...asset,
    path: input.toAssetUrl(asset.path),
  }));
  const hasRenderableModel = Boolean(
    input.activeThreadId && ((input.stlUrl || '').trim() || viewerAssets.length > 0),
  );
  const activeThreadConceptPreviewState =
    input.activeThreadId
      ? input.conceptPreviewUiByThread[input.activeThreadId] ?? defaultConceptPreviewUiState()
      : defaultConceptPreviewUiState();
  const conceptPreviewMessages = listConceptPreviewMessages(input.activeThreadMessages);
  const effectiveConceptPreviewMessage = resolveEffectiveConceptPreviewMessage(
    input.activeThreadMessages,
    activeThreadConceptPreviewState.pinnedMessageId,
  );
  const viewportPresentationMode = activeThreadConceptPreviewState.mode;
  const showBlueprintViewport =
    viewportPresentationMode === 'blueprint' && Boolean(effectiveConceptPreviewMessage);
  const blueprintAttentionVisible =
    hasRenderableModel &&
    Boolean(effectiveConceptPreviewMessage) &&
    viewportPresentationMode !== 'blueprint';
  const currentViewportTargetKey =
    input.activeThreadId && input.activeVersionId
      ? viewportTargetKey(input.activeThreadId, input.activeVersionId)
      : null;
  const currentViewerModelKey = currentViewportTargetKey
    ? [
        currentViewportTargetKey,
        input.activeArtifactBundle?.modelId ?? '',
        input.activeArtifactBundle?.artifactVersion ?? '',
        input.activeArtifactBundle?.contentHash ?? '',
      ].join(':')
    : null;
  const persistedViewportCameraState =
    currentViewportTargetKey ? input.cameraStateByTarget[currentViewportTargetKey] ?? null : null;
  const activeVersionAgentLabel = formatAgentOriginLabel(input.activeVersionMessage?.agentOrigin);

  return {
    viewerAssets,
    hasRenderableModel,
    activeThreadConceptPreviewState,
    conceptPreviewMessages,
    effectiveConceptPreviewMessage,
    viewportPresentationMode,
    showBlueprintViewport,
    blueprintAttentionVisible,
    currentViewportTargetKey,
    currentViewerModelKey,
    persistedViewportCameraState,
    activeVersionAgentLabel,
  };
}
