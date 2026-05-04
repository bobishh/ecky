import type { Message } from '../types/domain';

export type GenieSpeechCueInput = {
  latestAssistantMessage?: Message | null;
  assistantFresh: boolean;
  visibleBubble?: string | null;
  activeErrorId?: string | null;
  activeErrorText?: string | null;
};

export type GenieSpeechCue = {
  key: string;
  text: string;
};

function normalizeSpeechCandidate(value: string | null | undefined): string {
  return `${value ?? ''}`.replace(/\s+/g, ' ').trim();
}

function isAgentWorkingVersionMessage(message: Message): boolean {
  return Boolean(
    message.agentOrigin &&
      (message.output || message.artifactBundle || message.modelManifest),
  );
}

export function resolveAssistantSpeechText(message: Message | null | undefined): string {
  if (!message || message.role !== 'assistant') return '';

  if (message.status === 'error') {
    return normalizeSpeechCandidate(message.content);
  }

  if (message.status !== 'success') return '';
  if (message.visualKind === 'conceptPreview') return '';
  if (isAgentWorkingVersionMessage(message)) return '';

  const responseText = normalizeSpeechCandidate(message.output?.response);
  if (responseText) return responseText;

  if (message.output || message.artifactBundle || message.modelManifest) return '';

  return normalizeSpeechCandidate(message.content);
}

export function resolveGenieSpeechCue(input: GenieSpeechCueInput): GenieSpeechCue | null {
  const visibleBubble = normalizeSpeechCandidate(input.visibleBubble);
  if (!visibleBubble) return null;

  const activeErrorText = normalizeSpeechCandidate(input.activeErrorText);
  if (activeErrorText && visibleBubble === activeErrorText) {
    const errorKey = normalizeSpeechCandidate(input.activeErrorId) || activeErrorText;
    return {
      key: `error:${errorKey}:${activeErrorText}`,
      text: activeErrorText,
    };
  }

  if (!input.assistantFresh) return null;

  const message = input.latestAssistantMessage;
  const assistantText = resolveAssistantSpeechText(message);
  if (!message || !assistantText || visibleBubble !== assistantText) return null;

  return {
    key: `assistant:${message.id}:${assistantText}`,
    text: assistantText,
  };
}
