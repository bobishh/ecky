import type { Message } from './types/domain';

type EngineLabelMessage = Pick<Message, 'output' | 'artifactBundle' | 'modelManifest'> | null | undefined;

function backendLabel(backend: string | undefined): string | null {
  switch (backend) {
    case 'build123d':
      return 'build123d';
    case 'freecad':
      return 'freecad';
    case 'mesh':
      return 'native';
    default:
      return null;
  }
}

function authoringFileExtension(
  sourceLanguage: string | undefined,
  _geometryBackend: string | undefined,
): string {
  if (sourceLanguage === 'build123d') return '.py';
  if (sourceLanguage === 'legacyPython') return '.FCMacro';
  return '.ecky';
}

function formatEckyBackendLabel(backend: string | undefined): string {
  const backendName = backendLabel(backend);
  if (backendName === 'native') return 'Ecky Native (.ecky)';
  return backendName ? `Ecky + ${backendName} (.ecky)` : 'Ecky (.ecky)';
}

export function codeInspectorTitle(
  baseTitle: string | undefined,
  sourceLanguage: string | undefined,
  geometryBackend: string | undefined,
): string {
  const trimmed = `${baseTitle ?? ''}`.trim() || 'design';
  const extension = authoringFileExtension(sourceLanguage, geometryBackend);
  return trimmed.endsWith(extension) ? trimmed : `${trimmed}${extension}`;
}

export function modelEngineLabel(message: EngineLabelMessage): string {
  const source =
    message?.artifactBundle?.sourceLanguage ??
    message?.modelManifest?.sourceLanguage ??
    message?.output?.sourceLanguage;
  const backend =
    message?.artifactBundle?.geometryBackend ??
    message?.modelManifest?.geometryBackend ??
    message?.output?.geometryBackend;
  const engine =
    message?.artifactBundle?.engineKind ??
    message?.modelManifest?.engineKind ??
    message?.output?.engineKind;
  const sourceValue = source as string | undefined;
  const backendValue = backend as string | undefined;
  const engineValue = engine as string | undefined;

  if (sourceValue === 'build123d') return 'build123d (.py)';
  if (sourceValue === 'ecky') {
    return formatEckyBackendLabel(backendValue);
  }
  if (engineValue === 'build123d' || backendValue === 'build123d') return formatEckyBackendLabel('build123d');
  if (engineValue === 'ecky') return formatEckyBackendLabel(backendValue);
  if (backendValue === 'freecad') return formatEckyBackendLabel('freecad');
  return 'FreeCAD';
}
