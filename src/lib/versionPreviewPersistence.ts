import type { ArtifactBundle, Message } from './types/domain';

export function sameArtifactVersion(
  versionBundle: ArtifactBundle | null | undefined,
  runtimeBundle: ArtifactBundle | null | undefined,
): boolean {
  if (!versionBundle || !runtimeBundle) return false;
  return (
    versionBundle.modelId === runtimeBundle.modelId &&
    versionBundle.contentHash === runtimeBundle.contentHash &&
    (versionBundle.artifactVersion ?? null) === (runtimeBundle.artifactVersion ?? null)
  );
}

export function shouldPersistVersionPreview(
  activeVersionMessage: Message | null,
  artifactBundle: ArtifactBundle | null,
  stlUrl: string | null,
): boolean {
  if (!activeVersionMessage) return false;
  if (!artifactBundle) return false;
  if (!stlUrl?.trim()) return false;
  if (activeVersionMessage.imageData?.trim()) return false;
  return sameArtifactVersion(activeVersionMessage.artifactBundle, artifactBundle);
}
