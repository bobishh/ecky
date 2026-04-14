import type { AgentOrigin, Message } from './types/domain';

export type TimelineVisual = {
  src: string;
  alt: string;
  label: string;
};

export function isVersionTimelineMessage(message: Message): boolean {
  return message.role === 'assistant' && Boolean(message.artifactBundle);
}

export function isRenderableVersionTimelineMessage(message: Message): boolean {
  return isVersionTimelineMessage(message) && message.status === 'success';
}

export function versionTimelineTitle(message: Message | null | undefined): string {
  if (!message) return 'this version';
  return (
    message.output?.title ||
    message.modelManifest?.document?.documentLabel ||
    message.modelManifest?.document?.documentName ||
    message.artifactBundle?.modelId ||
    'Imported Model'
  );
}

export function formatTimelineAgentOrigin(origin: AgentOrigin | null | undefined): string | null {
  if (!origin) return null;
  const host = origin.hostLabel?.trim() || origin.agentLabel?.trim() || 'Agent';
  const model = origin.llmModelLabel?.trim() || origin.llmModelId?.trim() || '';
  if (!model || model.toLowerCase() === host.toLowerCase()) {
    return host;
  }
  return `${host} · ${model}`;
}

export function threadTimelineMessages(messages: Message[]): Message[] {
  return messages
    .map((message, index) => ({ message, index }))
    .filter(
      ({ message }) => message.status !== 'discarded' || isVersionTimelineMessage(message),
    )
    .sort((left, right) => {
      if (left.message.timestamp !== right.message.timestamp) {
        return left.message.timestamp - right.message.timestamp;
      }
      return left.index - right.index;
    })
    .map(({ message }) => message);
}

export function versionTimelineMessages(messages: Message[]): Message[] {
  return threadTimelineMessages(messages).filter(
    (message) => isRenderableVersionTimelineMessage(message),
  );
}

export function activeVersionTimelineIndex(
  versionMessages: Message[],
  activeVersionId: string | null | undefined,
): number {
  if (!versionMessages.length) return -1;
  const activeIndex = versionMessages.findIndex((message) => message.id === activeVersionId);
  return activeIndex >= 0 ? activeIndex : versionMessages.length - 1;
}

export function timelineVisuals(
  message: Message,
  toAssetUrl: (path: string | null | undefined) => string,
): TimelineVisual[] {
  const visuals: TimelineVisual[] = [];
  if (message.imageData) {
    visuals.push({
      src: message.imageData,
      alt: message.role === 'user' ? 'Viewport snapshot' : 'Message image',
      label:
        message.role === 'user'
          ? 'VIEWPORT'
          : message.visualKind === 'conceptPreview'
            ? 'CONCEPT'
            : 'IMAGE',
    });
  }
  for (const image of message.attachmentImages || []) {
    const src = toAssetUrl(image);
    if (!src) continue;
    visuals.push({
      src,
      alt: 'Attached reference image',
      label: 'REFERENCE',
    });
  }
  return visuals;
}
