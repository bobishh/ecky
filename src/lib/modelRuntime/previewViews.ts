import type { ImportedPreviewTransform } from './importedRuntime';
import type { ModelManifest, PreviewView } from '../types/domain';

function normalizePreviewViewId(previewViews: PreviewView[], requestedViewId: string | null): string | null {
  if (requestedViewId && previewViews.some((view) => view.viewId === requestedViewId)) {
    return requestedViewId;
  }
  return previewViews[0]?.viewId ?? null;
}

export function resolveActivePreviewView(
  manifest: ModelManifest | null,
  requestedViewId: string | null,
): PreviewView | null {
  const previewViews = manifest?.previewViews ?? [];
  const activeViewId = normalizePreviewViewId(previewViews, requestedViewId);
  if (!activeViewId) return null;
  return previewViews.find((view) => view.viewId === activeViewId) ?? null;
}

export function buildPreviewViewTransforms(
  manifest: ModelManifest | null,
  requestedViewId: string | null,
): Record<string, ImportedPreviewTransform> {
  const activeView = resolveActivePreviewView(manifest, requestedViewId);
  if (!activeView) return {};

  const transforms: Record<string, ImportedPreviewTransform> = {};
  for (const offset of activeView.offsets || []) {
    transforms[offset.partId] = {
      anchor: { x: 0, y: 0, z: 0 },
      scale: { x: 1, y: 1, z: 1 },
      translate: {
        x: offset.dx,
        y: offset.dy,
        z: offset.dz,
      },
    };
  }
  return transforms;
}

export function mergePreviewTransforms(
  base: Record<string, ImportedPreviewTransform>,
  overlay: Record<string, ImportedPreviewTransform>,
): Record<string, ImportedPreviewTransform> {
  const merged: Record<string, ImportedPreviewTransform> = { ...base };
  for (const [partId, transform] of Object.entries(overlay)) {
    const current = merged[partId];
    merged[partId] = current
      ? {
          anchor: current.anchor,
          scale: current.scale,
          translate: transform.translate,
        }
      : transform;
  }
  return merged;
}
