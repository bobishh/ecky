import type { ArtifactBundle, Message } from './types/domain';

export function shouldPersistVersionPreview(
  activeVersionMessage: Message | null,
  artifactBundle: ArtifactBundle | null,
  stlUrl: string | null,
): boolean {
  if (!activeVersionMessage) return false;
  if (!artifactBundle) return false;
  if (!stlUrl?.trim()) return false;
  return !activeVersionMessage.imageData?.trim();
}
