import type { DesignOutput, DesignParams, ManifestBounds, ModelManifest, UiSpec } from '../types/domain';

export type ImportedPreviewTransform = {
  anchor: {
    x: number;
    y: number;
    z: number;
  };
  scale: {
    x: number;
    y: number;
    z: number;
  };
  translate?: {
    x: number;
    y: number;
    z: number;
  };
};

function humanizeParameterKey(key: string): string {
  return key
    .split(/[_\-.]+/)
    .filter(Boolean)
    .map((token) => token.charAt(0).toUpperCase() + token.slice(1))
    .join(' ');
}

function inferImportedDimensionValue(
  key: string,
  bounds: ManifestBounds | null | undefined,
): number {
  if (!bounds) return 0;
  if (key.endsWith('_height')) return Math.max(0, bounds.zMax - bounds.zMin);
  if (key.endsWith('_depth')) return Math.max(0, bounds.yMax - bounds.yMin);
  return Math.max(0, bounds.xMax - bounds.xMin);
}

export function buildImportedUiSpec(manifest: ModelManifest | null): UiSpec {
  if (!isImportedModel(manifest)) {
    return { fields: [] };
  }

  const keys = new Set<string>();
  for (const group of manifest.parameterGroups || []) {
    if (!group.editable) continue;
    for (const key of group.parameterKeys || []) {
      keys.add(key);
    }
  }
  for (const part of manifest.parts || []) {
    if (!part.editable) continue;
    for (const key of part.parameterKeys || []) {
      keys.add(key);
    }
  }

  return {
    fields: [...keys].sort().map((key) => ({
      type: 'number' as const,
      key,
      label: humanizeParameterKey(key),
      min: 0,
      max: undefined,
      step: 1,
      frozen: false,
    })),
  };
}

export function buildImportedParams(
  manifest: ModelManifest | null,
  currentParams: DesignParams,
  uiSpec: UiSpec,
): DesignParams {
  if (!isImportedModel(manifest)) {
    return currentParams;
  }

  const next: DesignParams = { ...currentParams };
  for (const field of uiSpec.fields || []) {
    if (next[field.key] !== undefined) continue;
    const sourcePart =
      (manifest.parts || []).find((part) => (part.parameterKeys || []).includes(field.key)) ?? null;
    next[field.key] = inferImportedDimensionValue(field.key, sourcePart?.bounds);
  }
  return next;
}

export function buildImportedSyntheticDesign(
  manifest: ModelManifest | null,
  currentParams: DesignParams,
  uiSpecOverride?: UiSpec | null,
): DesignOutput | null {
  if (!isImportedModel(manifest)) {
    return null;
  }

  const uiSpec = uiSpecOverride && (uiSpecOverride.fields || []).length > 0
    ? uiSpecOverride
    : buildImportedUiSpec(manifest);
  const initialParams = buildImportedParams(manifest, currentParams, uiSpec);
  const title =
    manifest.document.documentLabel ||
    manifest.document.documentName ||
    (manifest.sourceKind === 'importedMesh' ? 'Imported Mesh Model' : 'Imported FreeCAD Model');

  return {
    title,
    versionName: 'Imported',
    response: manifest.sourceKind === 'importedMesh' ? 'Imported mesh reference.' : 'Imported FreeCAD model.',
    interactionMode: 'design',
    macroCode: '',
    uiSpec,
    initialParams,
    sourceLanguage: manifest.sourceLanguage ?? (manifest.sourceKind === 'importedMesh' ? 'ecky' : 'legacyPython'),
    geometryBackend: manifest.geometryBackend ?? (manifest.sourceKind === 'importedMesh' ? 'mesh' : 'freecad'),
    postProcessing: null,
  };
}

function clampScale(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 1;
  return Math.max(0.05, Math.min(value, 20));
}

export function buildImportedPreviewTransforms(
  manifest: ModelManifest | null,
  currentParams: DesignParams,
): Record<string, ImportedPreviewTransform> {
  if (!isImportedModel(manifest)) {
    return {};
  }

  const transforms: Record<string, ImportedPreviewTransform> = {};

  for (const part of manifest.parts || []) {
    if (!part.editable || !part.bounds) continue;

    const { xMin, xMax, yMin, yMax, zMin, zMax } = part.bounds;
    const width = Math.max(0, xMax - xMin);
    const depth = Math.max(0, yMax - yMin);
    const height = Math.max(0, zMax - zMin);
    let scaleX = 1;
    let scaleY = 1;
    let scaleZ = 1;

    for (const key of part.parameterKeys || []) {
      const rawValue = currentParams[key];
      const numericValue = Number(rawValue);
      if (!Number.isFinite(numericValue)) continue;

      if (key.endsWith('_height') && height > 0) {
        scaleZ = clampScale(numericValue / height);
      } else if (key.endsWith('_depth') && depth > 0) {
        scaleY = clampScale(numericValue / depth);
      } else if (width > 0) {
        scaleX = clampScale(numericValue / width);
      }
    }

    transforms[part.partId] = {
      anchor: {
        x: (xMin + xMax) * 0.5,
        y: (yMin + yMax) * 0.5,
        z: zMin,
      },
      scale: {
        x: scaleX,
        y: scaleY,
        z: scaleZ,
      },
    };
  }

  return transforms;
}

function isImportedModel(manifest: ModelManifest | null): manifest is ModelManifest {
  return Boolean(
    manifest &&
      (manifest.sourceKind === 'importedFcstd' ||
        manifest.sourceKind === 'importedStep' ||
        manifest.sourceKind === 'importedMesh'),
  );
}
