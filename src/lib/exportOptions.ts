import type { ArtifactBundle } from './types/domain';

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

export function buildMultipartExportParts(
  bundle: ArtifactBundle | null | undefined,
): MultipartExportPart[] {
  return (bundle?.viewerAssets ?? []).map((asset) => ({
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
): ExportChooserOption[] {
  const isMultipart = hasMultipartExportAssets(bundle);
  const options: ExportChooserOption[] = [];

  if (isMultipart) {
    options.push({
      id: '3mf',
      title: '3MF',
      subtitle: 'Best for Bambu Studio / Orca. Keeps separate bodies in one file.',
      disabled: false,
    });
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
    subtitle: isMultipart
      ? 'Flattened single-mesh export. Use 3MF or Multipart STL to preserve separate bodies.'
      : 'Single-mesh STL export.',
    disabled: !Boolean(bundle?.previewStlPath),
    disabledReason: bundle?.previewStlPath ? undefined : 'Preview STL is not available for this model.',
  });

  options.push({
    id: 'fcstd',
    title: 'FCStd',
    subtitle: 'FreeCAD source document.',
    disabled: !Boolean(bundle?.fcstdPath),
    disabledReason: bundle?.fcstdPath ? undefined : 'FreeCAD source is not available for this model.',
  });

  const stepPath = getStepExportPath(bundle);
  options.push({
    id: 'step',
    title: 'STEP',
    subtitle: 'Neutral CAD exchange file.',
    disabled: !Boolean(stepPath),
    disabledReason: stepPath ? undefined : 'STEP export is pending for this model.',
  });

  return options;
}
