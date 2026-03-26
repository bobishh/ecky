import { exists, size } from '@tauri-apps/plugin-fs';

import {
  type ArtifactBundle,
  type DesignParams,
  type PostProcessingSpec,
  normalizePostProcessing,
} from '../types/domain';

export type RuntimeBundleAvailability = {
  bundle: ArtifactBundle | null;
  previewAvailable: boolean;
  degradedToPreview: boolean;
  skippedOversizedPreview: boolean;
};

type PathExists = (path: string) => Promise<boolean>;
type PathSize = (path: string) => Promise<number>;

const MAX_SAFE_VIEWER_PREVIEW_BYTES = 64 * 1024 * 1024;

async function defaultPathExists(path: string): Promise<boolean> {
  return exists(path);
}

async function defaultPathSize(path: string): Promise<number> {
  return size(path);
}

async function safePathExists(path: string, pathExists: PathExists): Promise<boolean> {
  try {
    return await pathExists(path);
  } catch {
    return false;
  }
}

async function safePathSize(
  path: string,
  pathSize: PathSize,
): Promise<number | null> {
  try {
    const bytes = await pathSize(path);
    return Number.isFinite(bytes) && bytes >= 0 ? bytes : null;
  } catch {
    return null;
  }
}

function hasDisplacementPostProcessing(
  postProcessing: PostProcessingSpec | null | undefined,
  params: DesignParams | null | undefined = null,
): boolean {
  const normalized = normalizePostProcessing(postProcessing);
  if (!normalized) return false;

  return (normalized.lithophaneAttachments ?? []).some((attachment) => {
    if (attachment.enabled === false) return false;
    if (attachment.source.kind === 'file') {
      return attachment.source.imagePath.trim().length > 0;
    }
    const parameterKey = attachment.source.imageParam.trim();
    if (!parameterKey) return false;
    const parameterValue = params?.[parameterKey];
    return typeof parameterValue === 'string' && parameterValue.trim().length > 0;
  });
}

export function getRenderableRuntimeBundle(
  bundle: ArtifactBundle | null | undefined,
  postProcessing: PostProcessingSpec | null | undefined = null,
  params: DesignParams | null | undefined = null,
): ArtifactBundle | null {
  if (!bundle) return null;
  if (!hasDisplacementPostProcessing(postProcessing, params)) return bundle;
  if (!(bundle.viewerAssets?.length ?? 0)) return bundle;
  return {
    ...bundle,
    viewerAssets: [],
  };
}

export async function inspectRuntimeBundle(
  bundle: ArtifactBundle | null | undefined,
  pathExists: PathExists = defaultPathExists,
  pathSize: PathSize = defaultPathSize,
  postProcessing: PostProcessingSpec | null | undefined = null,
  params: DesignParams | null | undefined = null,
): Promise<RuntimeBundleAvailability> {
  if (!bundle?.previewStlPath) {
    return {
      bundle: null,
      previewAvailable: false,
      degradedToPreview: false,
      skippedOversizedPreview: false,
    };
  }

  const previewAvailable = await safePathExists(bundle.previewStlPath, pathExists);
  if (!previewAvailable) {
    return {
      bundle: null,
      previewAvailable: false,
      degradedToPreview: false,
      skippedOversizedPreview: false,
    };
  }

  const previewBytes = await safePathSize(bundle.previewStlPath, pathSize);
  const oversizedLithophanePreview =
    hasDisplacementPostProcessing(postProcessing, params) &&
    typeof previewBytes === 'number' &&
    previewBytes > MAX_SAFE_VIEWER_PREVIEW_BYTES;
  if (oversizedLithophanePreview) {
    if ((bundle.viewerAssets?.length ?? 0) > 0) {
      return {
        bundle,
        previewAvailable: true,
        degradedToPreview: false,
        skippedOversizedPreview: true,
      };
    }
    return {
      bundle: null,
      previewAvailable: true,
      degradedToPreview: false,
      skippedOversizedPreview: true,
    };
  }

  const renderableBundle = getRenderableRuntimeBundle(bundle, postProcessing, params);
  const viewerAssets = renderableBundle?.viewerAssets ?? [];
  const degradedToPreview = Boolean(
    renderableBundle &&
      (bundle.viewerAssets?.length ?? 0) > 0 &&
      (renderableBundle.viewerAssets?.length ?? 0) === 0,
  );

  if (!viewerAssets.length) {
    return {
      bundle: renderableBundle,
      previewAvailable: true,
      degradedToPreview,
      skippedOversizedPreview: false,
    };
  }

  const viewerAssetChecks = await Promise.all(
    viewerAssets.map((asset) => safePathExists(asset.path, pathExists)),
  );

  if (viewerAssetChecks.every(Boolean)) {
    return {
      bundle: renderableBundle,
      previewAvailable: true,
      degradedToPreview,
      skippedOversizedPreview: false,
    };
  }

  const previewOnlyBundle = renderableBundle
    ? {
        ...renderableBundle,
        viewerAssets: [],
      }
    : null;

  return {
    bundle: previewOnlyBundle,
    previewAvailable: true,
    degradedToPreview: true,
    skippedOversizedPreview: false,
  };
}
