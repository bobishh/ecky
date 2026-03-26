import type { ViewportCameraState } from '../types/domain';

export type ViewportCaptureMode = 'visible-live' | 'hidden-target' | 'needs-user-choice';

export type ViewportScreenshotCapture = {
  dataUrl: string;
  width: number;
  height: number;
  camera: ViewportCameraState;
  capturedAt: number;
  source?: string;
  threadId?: string;
  messageId?: string;
  modelId?: string | null;
  includeOverlays?: boolean;
};

export type ViewportCameraStateCache = Record<string, ViewportCameraState>;
export type ViewportScreenshotCache = Record<string, ViewportScreenshotCapture>;

type ChooseViewportCaptureModeInput = {
  currentView: string | null | undefined;
  currentThreadId: string | null | undefined;
  currentMessageId: string | null | undefined;
  requestedThreadId: string | null | undefined;
  requestedMessageId: string | null | undefined;
  cameraOverride: ViewportCameraState | null | undefined;
  hasVisibleViewer: boolean;
};

function cloneCamera(camera: ViewportCameraState): ViewportCameraState {
  return {
    position: [...camera.position] as [number, number, number],
    target: [...camera.target] as [number, number, number],
    zoom: camera.zoom ?? null,
    fov: camera.fov ?? null,
  };
}

export function viewportTargetKey(threadId: string, messageId: string): string {
  return `${threadId}:${messageId}`;
}

export function chooseViewportCaptureMode(
  input: ChooseViewportCaptureModeInput,
): ViewportCaptureMode {
  const matchesVisibleTarget =
    input.currentView === 'workbench' &&
    input.hasVisibleViewer &&
    !!input.currentThreadId &&
    !!input.currentMessageId &&
    input.currentThreadId === input.requestedThreadId &&
    input.currentMessageId === input.requestedMessageId;

  if (matchesVisibleTarget) {
    return input.cameraOverride ? 'hidden-target' : 'visible-live';
  }

  return 'needs-user-choice';
}

export function rememberTargetCameraState(
  cache: ViewportCameraStateCache,
  key: string,
  camera: ViewportCameraState,
  persist: boolean,
): ViewportCameraStateCache {
  if (!persist) return cache;
  return {
    ...cache,
    [key]: cloneCamera(camera),
  };
}

export function rememberTargetScreenshot(
  cache: ViewportScreenshotCache,
  key: string,
  capture: ViewportScreenshotCapture,
): ViewportScreenshotCache {
  return {
    ...cache,
    [key]: {
      ...capture,
      camera: cloneCamera(capture.camera),
    },
  };
}

export function resolveFallbackScreenshotSource(
  cache: ViewportScreenshotCache,
  key: string,
): { kind: 'cached-live'; capture: ViewportScreenshotCapture } | { kind: 'hidden-preview' } {
  const capture = cache[key];
  if (capture) {
    return { kind: 'cached-live', capture };
  }
  return { kind: 'hidden-preview' };
}
