import type { Message } from './types/domain';

type EngineLabelMessage = Pick<Message, 'output' | 'artifactBundle' | 'modelManifest'> | null | undefined;

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

  if (sourceValue === 'build123d') return 'Python + build123d';
  if (sourceValue === 'eckyIrV0') {
    return backendValue === 'build123d' ? 'IR + build123d' : 'IR + Native';
  }
  if (engineValue === 'build123d' || backendValue === 'build123d') return 'Python + build123d';
  if (engineValue === 'eckyIrV0') return backendValue === 'build123d' ? 'IR + build123d' : 'IR + Native';
  return 'FreeCAD';
}
