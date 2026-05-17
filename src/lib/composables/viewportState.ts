import { viewportCameraKey, viewportTargetKey } from '../agents/screenshot';
import { formatAgentOriginLabel } from './agentOps';
import { listConceptPreviewMessages, resolveLatestConceptPreviewMessage } from '../viewportBlueprint';
import type { ArtifactBundle, Message, ViewportCameraState, ViewerAsset } from '../types/domain';

export type ViewportStateInput = {
  activeArtifactBundle: ArtifactBundle | null;
  activeThreadId: string | null;
  activeThreadMessages: Message[];
  activeVersionId: string | null;
  activeVersionMessage: Message | null;
  cameraStateByTarget: Record<string, ViewportCameraState>;
  runtimeRevision?: number;
  stlUrl: string | null;
  toAssetUrl: (path: string | null | undefined) => string;
};

export type ViewportState = {
  viewerAssets: ViewerAsset[];
  hasRenderableModel: boolean;
  conceptPreviewMessages: Message[];
  effectiveConceptPreviewMessage: Message | null;
  currentViewportTargetKey: string | null;
  currentViewerModelKey: string | null;
  persistedViewportCameraState: ViewportCameraState | null;
  activeVersionAgentLabel: string | null;
};

export function deriveViewportState(input: ViewportStateInput): ViewportState {
  const viewerAssets = (input.activeArtifactBundle?.viewerAssets || []).map((asset) => ({
    ...asset,
    path: input.toAssetUrl(asset.path),
  }));
  const hasRenderableModel = Boolean(
    input.activeThreadId && ((input.stlUrl || '').trim() || viewerAssets.length > 0),
  );
  const conceptPreviewMessages = listConceptPreviewMessages(input.activeThreadMessages);
  const effectiveConceptPreviewMessage = resolveLatestConceptPreviewMessage(
    input.activeThreadMessages,
  );
  const currentViewportTargetKey =
    input.activeThreadId && input.activeVersionId
      ? viewportCameraKey(
          input.activeThreadId,
          input.activeVersionId,
          input.activeArtifactBundle?.modelId ?? null,
          input.activeArtifactBundle?.artifactVersion ?? null,
          input.activeArtifactBundle?.contentHash ?? null,
        )
      : null;
  const runtimeRevision =
    typeof input.runtimeRevision === 'number' && input.runtimeRevision > 0
      ? `r${input.runtimeRevision}`
      : '';
  const currentViewerModelKey = input.activeArtifactBundle
    ? [
        input.activeThreadId ?? '',
        input.activeArtifactBundle.modelId,
        input.activeArtifactBundle.artifactVersion ?? '',
        input.activeArtifactBundle.contentHash ?? '',
        input.stlUrl ?? '',
        ...(runtimeRevision ? [runtimeRevision] : []),
      ].join(':')
    : input.stlUrl
      ? [input.activeThreadId ?? '', input.activeVersionId ?? '', input.stlUrl].join(':')
      : input.activeThreadId && input.activeVersionId
        ? viewportTargetKey(input.activeThreadId, input.activeVersionId)
        : null;
  const persistedViewportCameraState =
    currentViewportTargetKey ? input.cameraStateByTarget[currentViewportTargetKey] ?? null : null;
  const activeVersionAgentLabel = formatAgentOriginLabel(input.activeVersionMessage?.agentOrigin);

  return {
    viewerAssets,
    hasRenderableModel,
    conceptPreviewMessages,
    effectiveConceptPreviewMessage,
    currentViewportTargetKey,
    currentViewerModelKey,
    persistedViewportCameraState,
    activeVersionAgentLabel,
  };
}
