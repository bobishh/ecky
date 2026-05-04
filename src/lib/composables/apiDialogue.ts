import type { EngineConfig, Message, Request } from '../types/domain';

type ApiEngineLike = Pick<EngineConfig, 'enabled' | 'provider' | 'apiKey'> | null | undefined;

const TERMINAL_PHASES = new Set<Request['phase']>(['canceled']);

function imageAttachmentSource(attachment: Request['attachments'][number]): string | null {
  if (attachment.type !== 'image') return null;
  const inline = attachment.dataUrl?.trim();
  if (inline) return inline;
  const path = attachment.path?.trim();
  return path || null;
}

function requestTimestampSeconds(request: Request): number {
  return Math.max(0, Math.floor(request.createdAt / 1000));
}

function normalizeText(value: string | null | undefined): string {
  return `${value ?? ''}`.trim();
}

function hasPersistedAssistantMessage(messages: Message[], request: Request): boolean {
  const assistantMessageId = request.result?.messageId?.trim();
  if (!assistantMessageId) return false;
  return messages.some((message) => message.id === assistantMessageId);
}

function pendingAssistantCopy(request: Request): string {
  const bubble = normalizeText(request.lightResponse);
  if (bubble) return bubble;

  switch (request.phase) {
    case 'classifying':
      return 'Routing request...';
    case 'answering':
      return 'Answering question...';
    case 'repairing':
      return 'Repairing geometry...';
    case 'queued_for_render':
      return 'Preparing render...';
    case 'rendering':
      return 'Rendering model...';
    case 'committing':
      return 'Saving result...';
    case 'success':
      return request.result?.design?.response?.trim() || 'Done.';
    case 'error':
      return normalizeText(request.error) || 'Request failed.';
    case 'canceled':
      return 'Request canceled.';
    case 'generating':
    default:
      return 'Processing request...';
  }
}

function pendingAssistantStatus(request: Request): Message['status'] {
  if (request.phase === 'error') return 'error';
  if (request.phase === 'success') return 'success';
  return request.phase === 'rendering' || request.phase === 'committing' ? 'working' : 'pending';
}

export function deriveOptimisticDialogueMessages(
  messages: Message[],
  requests: Request[],
): Message[] {
  const merged = [...messages];
  const sortedRequests = [...requests].sort(
    (left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id),
  );

  for (const request of sortedRequests) {
    const persisted = hasPersistedAssistantMessage(messages, request);
    if (persisted) continue;

    const timestamp = requestTimestampSeconds(request);
    merged.push({
      id: `optimistic-user-${request.id}`,
      role: 'user',
      content: request.prompt,
      status: 'success',
      timestamp,
      imageData: request.screenshot,
      attachmentImages: request.attachments
        .map(imageAttachmentSource)
        .filter((source): source is string => Boolean(source)),
    });

    if (TERMINAL_PHASES.has(request.phase)) continue;

    merged.push({
      id: `optimistic-assistant-${request.id}`,
      role: 'assistant',
      content: pendingAssistantCopy(request),
      status: pendingAssistantStatus(request),
      timestamp: timestamp + 1,
    });
  }

  return merged;
}

export function hasLiveApiEngineConnection(
  connectionType: string | null | undefined,
  selectedEngine: ApiEngineLike,
): boolean {
  if (connectionType !== 'api_key') return false;
  if (!selectedEngine?.enabled) return false;
  if (selectedEngine.provider === 'ollama') return true;
  return normalizeText(selectedEngine.apiKey).length > 0;
}
