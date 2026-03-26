import type { Message } from './types/domain';

export type ViewportPresentationMode = 'model' | 'blueprint';

export type ConceptPreviewUiState = {
  pinnedMessageId: string | null;
  lastAutoPinnedMessageId: string | null;
  mode: ViewportPresentationMode;
};

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

export function resolveEffectiveConceptPreviewMessage(
  messages: Message[],
  pinnedMessageId: string | null,
): ConceptPreviewMessage | null {
  const previews = listConceptPreviewMessages(messages);
  if (previews.length === 0) return null;
  if (pinnedMessageId) {
    const pinned = previews.find((message) => message.id === pinnedMessageId);
    if (pinned) return pinned;
  }
  return previews[previews.length - 1] ?? null;
}

export function reconcileConceptPreviewUiState(input: {
  messages: Message[];
  previous: ConceptPreviewUiState;
  hasModel: boolean;
}): { previews: ConceptPreviewMessage[]; effectiveMessage: ConceptPreviewMessage | null; nextState: ConceptPreviewUiState } {
  const previews = listConceptPreviewMessages(input.messages);
  if (previews.length === 0) {
    return {
      previews,
      effectiveMessage: null,
      nextState: {
        pinnedMessageId: null,
        lastAutoPinnedMessageId: null,
        mode: 'model',
      },
    };
  }

  const latestPreview = previews[previews.length - 1];
  let pinnedMessageId = input.previous.pinnedMessageId;
  let lastAutoPinnedMessageId = input.previous.lastAutoPinnedMessageId;
  let mode = input.previous.mode;

  if (latestPreview.id !== lastAutoPinnedMessageId) {
    pinnedMessageId = latestPreview.id;
    lastAutoPinnedMessageId = latestPreview.id;
  }

  const effectiveMessage = resolveEffectiveConceptPreviewMessage(previews, pinnedMessageId);
  pinnedMessageId = effectiveMessage?.id ?? null;

  if (!input.hasModel) {
    mode = 'blueprint';
  } else if (mode !== 'blueprint') {
    mode = 'model';
  }

  return {
    previews,
    effectiveMessage,
    nextState: {
      pinnedMessageId,
      lastAutoPinnedMessageId,
      mode,
    },
  };
}

export function cycleConceptPreviewMessageId(
  messages: Message[],
  currentMessageId: string | null,
): string | null {
  const previews = listConceptPreviewMessages(messages);
  if (previews.length === 0) return null;
  if (!currentMessageId) return previews[0]?.id ?? null;
  const currentIndex = previews.findIndex((message) => message.id === currentMessageId);
  if (currentIndex === -1) return previews[previews.length - 1]?.id ?? null;
  return previews[(currentIndex + 1) % previews.length]?.id ?? null;
}

export function buildGenerateFromConceptPrompt(message: Message | null): string {
  const note = `${message?.content ?? ''}`.trim();
  if (!note) {
    return 'Generate a manufacturable 3D CAD model from this concept preview. Use the concept image as the primary visual reference.';
  }
  return `Generate a manufacturable 3D CAD model from this concept preview.\n\nConcept note:\n${note}\n\nUse the concept image as the primary visual reference.`;
}
