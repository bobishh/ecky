import { inspectRuntimeBundle, getRenderableRuntimeBundle } from './modelRuntime/runtimeBundle';
import { ensureSemanticManifest } from './modelRuntime/semanticControls';
import {
  getModelManifest,
  getThreadMessageVersion,
  renderModel,
  updateVersionRuntime,
} from './tauri/client';
import type { ArtifactBundle, DesignParams, Message, ModelManifest, ViewerAsset } from './types/domain';

type VersionLoupeMessage = Pick<Message, 'id' | 'artifactBundle' | 'modelManifest' | 'output'>;

export type VersionLoupeRuntime = {
  previewUrl: string | null;
  viewerAssets: ViewerAsset[];
  available: boolean;
};

type RuntimeDeps = {
  inspectRuntime?: typeof inspectRuntimeBundle;
  getThreadMessageVersion?: typeof getThreadMessageVersion;
  renderModel?: typeof renderModel;
  getModelManifest?: typeof getModelManifest;
  updateVersionRuntime?: typeof updateVersionRuntime;
};

async function hydrateVersionMessage(
  message: VersionLoupeMessage,
  threadId: string | null,
  loadVersionMessage: typeof getThreadMessageVersion,
): Promise<VersionLoupeMessage> {
  if (!threadId) return message;
  if (message.output && message.artifactBundle && message.modelManifest) return message;
  const hydrated = await loadVersionMessage(threadId, message.id);
  if (!hydrated) return message;
  return hydrated;
}

async function rebuildVersionRuntime(
  message: VersionLoupeMessage,
  inspectRuntime: typeof inspectRuntimeBundle,
  renderVersion: typeof renderModel,
  loadManifest: typeof getModelManifest,
  persistRuntime: typeof updateVersionRuntime,
): Promise<{ artifactBundle: ArtifactBundle; modelManifest: ModelManifest } | null> {
  if (!message.output) return null;

  const params = (message.output.initialParams ?? {}) as DesignParams;
  const rebuiltBundle = await renderVersion(
    message.output.macroCode,
    params,
    message.output.macroDialect ?? null,
    message.output.geometryBackend ?? null,
    message.output.postProcessing ?? null,
  );
  const runtime = await inspectRuntime(
    rebuiltBundle,
    undefined,
    undefined,
    message.output.postProcessing ?? null,
    params,
  );
  const renderableBundle =
    runtime.bundle ??
    getRenderableRuntimeBundle(rebuiltBundle, message.output.postProcessing ?? null, params) ??
    rebuiltBundle;
  const rawManifest = await loadManifest(rebuiltBundle.modelId);
  const modelManifest =
    ensureSemanticManifest(rawManifest, message.output.uiSpec, params, message.modelManifest ?? null) ??
    rawManifest;
  await persistRuntime(message.id, renderableBundle, modelManifest);
  return { artifactBundle: renderableBundle, modelManifest };
}

export async function resolveVersionLoupeRuntime(
  message: VersionLoupeMessage,
  threadId: string | null,
  toAssetUrl: (path: string | null | undefined) => string,
  deps: RuntimeDeps = {},
): Promise<VersionLoupeRuntime> {
  const inspectRuntime = deps.inspectRuntime ?? inspectRuntimeBundle;
  const loadVersionMessage = deps.getThreadMessageVersion ?? getThreadMessageVersion;
  const renderVersion = deps.renderModel ?? renderModel;
  const loadManifest = deps.getModelManifest ?? getModelManifest;
  const persistRuntime = deps.updateVersionRuntime ?? updateVersionRuntime;

  const hydratedMessage = await hydrateVersionMessage(message, threadId, loadVersionMessage);
  const params = (hydratedMessage.output?.initialParams ?? {}) as DesignParams;
  const bundle = hydratedMessage.artifactBundle ?? null;
  if (!bundle) {
    return {
      previewUrl: null,
      viewerAssets: [],
      available: false,
    };
  }

  let runtime = await inspectRuntime(
    bundle,
    undefined,
    undefined,
    hydratedMessage.output?.postProcessing ?? null,
    params,
  );
  let renderableBundle: ArtifactBundle | null = runtime.bundle ?? null;

  if (!renderableBundle?.previewStlPath) {
    const rebuilt = await rebuildVersionRuntime(
      hydratedMessage,
      inspectRuntime,
      renderVersion,
      loadManifest,
      persistRuntime,
    );
    renderableBundle = rebuilt?.artifactBundle ?? null;
  }

  if (!renderableBundle?.previewStlPath) {
    return {
      previewUrl: null,
      viewerAssets: [],
      available: false,
    };
  }

  return {
    previewUrl: toAssetUrl(renderableBundle.previewStlPath),
    viewerAssets: (renderableBundle.viewerAssets ?? []).map((asset) => ({
      ...asset,
      path: toAssetUrl(asset.path),
    })),
    available: true,
  };
}
