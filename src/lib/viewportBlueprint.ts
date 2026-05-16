import type { Message } from './types/domain';

type ConceptPreviewMessage = Message & {
  role: 'assistant';
  imageData: string;
  visualKind: 'conceptPreview';
};

export function isConceptPreviewMessage(message: Message): message is ConceptPreviewMessage {
  return (
    message.role === 'assistant' &&
    message.visualKind === 'conceptPreview' &&
    typeof message.imageData === 'string' &&
    message.imageData.trim().length > 0
  );
}

export function listConceptPreviewMessages(messages: Message[]): ConceptPreviewMessage[] {
  return messages.filter(isConceptPreviewMessage);
}

export function resolveLatestConceptPreviewMessage(messages: Message[]): ConceptPreviewMessage | null {
  const previews = listConceptPreviewMessages(messages);
  if (previews.length === 0) return null;
  return previews[previews.length - 1] ?? null;
}
