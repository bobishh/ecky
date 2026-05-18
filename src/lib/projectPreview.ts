import type { Message } from './types/domain';

export type ProjectPreviewImage = {
  messageId: string;
  imageData: string | null;
};

type PreviewFallbackMessage = Pick<Message, 'imageData'> &
  Partial<Pick<Message, 'role' | 'status'>> & {
    artifactBundle?: unknown;
  };

function cleanPreviewImage(raw: string | null | undefined): string | null {
  const value = raw?.trim();
  return value ? value : null;
}

export function selectThreadPreviewImage(
  thread: { messages?: Array<PreviewFallbackMessage> | null },
  latest: Pick<Message, 'id' | 'imageData'> | null | undefined,
  freshPreview: ProjectPreviewImage | null | undefined,
): string | null {
  const latestId = latest?.id ?? null;
  const fresh = cleanPreviewImage(freshPreview?.imageData);
  const latestPreview = cleanPreviewImage(latest?.imageData);
  if (freshPreview && latestId && freshPreview.messageId === latestId) {
    if (fresh) return fresh;
  }
  if (fresh && (!latestId || !latestPreview)) return fresh;
  if (latestPreview) return latestPreview;
  const fallback = [...(thread.messages || [])]
    .reverse()
    .find(
      (message) =>
        message.role === 'assistant' &&
        message.status === 'success' &&
        message.artifactBundle &&
        cleanPreviewImage(message.imageData),
    );
  return cleanPreviewImage(fallback?.imageData);
}
