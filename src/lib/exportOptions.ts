import type { ArtifactBundle, RuntimeCapabilities, ViewerAssetFormat } from './types/domain';

export type ExportMode = '3mf' | 'multipartStlZip' | 'stl' | 'fcstd' | 'step';

export type ExportChooserOption = {
  id: ExportMode;
  title: string;
  subtitle: string;
  disabled: boolean;
  disabledReason?: string;
};

export type MultipartExportPart = {
  label: string;
  path: string;
  objectName: string | null;
  partId: string | null;
  displayColor: string | null;
};

export function hasMultipartExportAssets(bundle: ArtifactBundle | null | undefined): boolean {
  return (bundle?.viewerAssets?.length ?? 0) > 1;
}

export function getStepExportPath(bundle: ArtifactBundle | null | undefined): string | undefined {
  return bundle?.exportArtifacts?.find((a) => a.format === 'step')?.path;
}

function stepExportSubtitle(bundle: ArtifactBundle | null | undefined): string {
  if (
    bundle?.productionPending &&
    !getStepExportPath(bundle) &&
    bundle.geometryProvenance?.representation !== 'meshNative'
  ) {
    return 'Materializes exact BRep STEP on demand when exported.';
  }
  const stepArtifact = bundle?.exportArtifacts?.find((artifact) => artifact.format === 'step');
  const representation =
    stepArtifact?.geometryProvenance?.representation ??
    bundle?.geometryProvenance?.representation;
  if (representation === 'facetedPolyBrep') {
    return 'Faceted poly-BRep exchange from a solidified triangle mesh; planar triangle-derived topology.';
  }
  if (representation === 'analyticBrep') {
    return 'Analytic BRep neutral CAD exchange file.';
  }
  return 'Neutral CAD exchange file.';
}

function stepExportDisabledReason(
  bundle: ArtifactBundle | null | undefined,
  runtimeCapabilities?: RuntimeCapabilities | null,
): string | undefined {
  if (getStepExportPath(bundle)) return undefined;
  if (!bundle) return 'No rendered model is available.';

  if (bundle.productionPending && bundle.geometryBackend === 'mesh') {
    if (bundle.geometryProvenance?.representation === 'meshNative') {
      return 'STEP unavailable for Ecky Native render: mesh-only geometry has no analytic BRep.';
    }
    const directOcct = runtimeCapabilities?.directOcct ?? null;
    if (directOcct && !directOcct.available) {
      return `STEP unavailable for Ecky Native render: ${directOcct.detail || 'Direct OCCT unavailable.'}`;
    }
    return undefined;
  }

  if (bundle.geometryBackend === 'mesh') {
    const directOcct = runtimeCapabilities?.directOcct ?? null;
    if (!directOcct) {
      return 'STEP unavailable for Ecky Native render: Direct OCCT capability was not probed.';
    }
    if (!directOcct.available) {
      return `STEP unavailable for Ecky Native render: ${directOcct.detail || 'Direct OCCT unavailable.'}`;
    }
    return 'STEP unavailable for Ecky Native render: no BRep STEP artifact was produced.';
  }

  return 'STEP artifact is not present in this model bundle.';
}

export function buildMultipartExportParts(
  bundle: ArtifactBundle | null | undefined,
  format?: ViewerAssetFormat,
): MultipartExportPart[] {
  const assets = bundle?.viewerAssets ?? [];
  return (format ? assets.filter((asset) => asset.format === format) : assets).map((asset) => ({
    label: asset.label || asset.objectName || asset.partId,
    path: asset.path,
    objectName: asset.objectName || null,
    partId: asset.partId || null,
    displayColor: null,
  }));
}

function sanitizeFileStem(input: string | null | undefined): string {
  const value = (input ?? '').trim().toLowerCase();
  if (!value) return 'design';
  const sanitized = value
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80);
  return sanitized || 'design';
}

export function buildExportDefaultNames(title: string | null | undefined) {
  const stem = sanitizeFileStem(title);
  return {
    threeMf: `${stem}.3mf`,
    multipartStlZip: `${stem}-parts.zip`,
    stl: `${stem}.stl`,
    fcstd: `${stem}.FCStd`,
    step: `${stem}.step`,
  };
}

export function buildExportChooserOptions(
  bundle: ArtifactBundle | null | undefined,
  runtimeCapabilities?: RuntimeCapabilities | null,
): ExportChooserOption[] {
  const isMultipart = hasMultipartExportAssets(bundle);
  const options: ExportChooserOption[] = [];

  if (bundle?.modelStlPath || bundle?.viewerAssets?.some((asset) => asset.format === 'stl')) {
    options.push({
      id: '3mf',
      title: '3MF',
      subtitle: 'Keeps bodies separate and includes any saved print settings.',
      disabled: false,
    });
  }

  if (isMultipart) {
    options.push({
      id: 'multipartStlZip',
      title: 'Multipart STL (.zip)',
      subtitle: 'Exports one STL per body inside a single zip archive.',
      disabled: false,
    });
  }

  options.push({
    id: 'stl',
    title: 'STL',
    subtitle: bundle?.productionPending
      ? 'Materializes high-resolution production STL on demand.'
      : isMultipart
        ? 'Flattened single-mesh export. Use 3MF or Multipart STL to preserve separate bodies.'
        : 'Single-mesh STL export.',
    disabled: !Boolean(bundle?.modelStlPath),
    disabledReason: bundle?.modelStlPath ? undefined : 'Model STL is not available for this model.',
  });

  options.push({
    id: 'fcstd',
    title: 'FCStd',
    subtitle: 'FreeCAD source document.',
    disabled: !Boolean(bundle?.fcstdPath),
    disabledReason: bundle?.fcstdPath ? undefined : 'FreeCAD source is not available for this model.',
  });

  const stepPath = getStepExportPath(bundle);
  const stepDisabledReason = stepExportDisabledReason(bundle, runtimeCapabilities);
  const stepAvailable = Boolean(stepPath) || Boolean(bundle?.productionPending && !stepDisabledReason);
  options.push({
    id: 'step',
    title: 'STEP',
    subtitle: stepExportSubtitle(bundle),
    disabled: !stepAvailable,
    disabledReason: stepDisabledReason,
  });

  return options;
}
