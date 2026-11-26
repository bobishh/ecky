export type ViewerBusyPhase = 'generating' | 'repairing' | 'rendering' | 'committing' | null;

type ViewerBusyState = {
  showViewerBusyMask: boolean;
  viewerBusyPhase: ViewerBusyPhase;
  viewerBusyText: string | null;
};

export function deriveViewerBusyState(input: {
  geometryRenderActive: boolean;
  projectFolderRenderPending: boolean;
  hasEarlyDisplayMesh?: boolean;
}): ViewerBusyState {
  if (input.geometryRenderActive && !input.hasEarlyDisplayMesh) {
    return {
      showViewerBusyMask: true,
      viewerBusyPhase: 'rendering',
      viewerBusyText: 'Rendering geometry.',
    };
  }
  if (input.projectFolderRenderPending && !input.hasEarlyDisplayMesh) {
    return {
      showViewerBusyMask: true,
      viewerBusyPhase: 'generating',
      viewerBusyText: 'Settling changed source.',
    };
  }
  return {
    showViewerBusyMask: false,
    viewerBusyPhase: null,
    viewerBusyText: null,
  };
}
