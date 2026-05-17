import type { Message } from './types/domain';

export type ProjectPreviewImage = {
  messageId: string;
  imageData: string | null;
};

function cleanPreviewImage(raw: string | null | undefined): string | null {
  const value = raw?.trim();
  return value ? value : null;
}

export function selectThreadPreviewImage(
  thread: { messages?: Array<Pick<Message, 'imageData'>> | null },
  latest: Pick<Message, 'id' | 'imageData'> | null | undefined,
  freshPreview: ProjectPreviewImage | null | undefined,
): string | null {
  const latestId = latest?.id ?? null;
  if (freshPreview && latestId && freshPreview.messageId === latestId) {
    return cleanPreviewImage(freshPreview.imageData);
  }
  const latestPreview = cleanPreviewImage(latest?.imageData);
  if (latestPreview) return latestPreview;
  if (latest !== undefined) return null;
  const fallback = [...(thread.messages || [])].reverse().find((message) => cleanPreviewImage(message.imageData));
  return cleanPreviewImage(fallback?.imageData);
}
