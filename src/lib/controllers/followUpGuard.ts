import type { Attachment, Message, Thread } from '../types/domain';

export interface FollowUpAnswerGuardInput {
  promptText: string;
  attachments: Attachment[];
  activeThread: Thread | null;
  explicitQuestionOnly: boolean;
}

export interface FollowUpAnswerGuardResult {
  matched: boolean;
  question: string | null;
  messageId: string | null;
  reason: string;
}

const CLARIFICATION_TAIL_WINDOW = 220;
const CLARIFICATION_SIGNAL_PATTERNS = [
  /\?/,
  /\b(which|what|where|when|choose|pick|select|confirm|specify|clarify|tell me)\b/i,
  /\b(какой|какая|какие|куда|где|выбери|уточни|подтверди|скажи|нужно уточнить)\b/i,
];

function findLastPersistedMessage(messages: Message[]): Message | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const candidate = messages[index];
    if (candidate?.status && candidate.status !== 'pending') {
      return candidate;
    }
  }
  return null;
}

function isClarificationQuestion(message: Message | null): boolean {
  if (!message) return false;
  if (message.role !== 'assistant' || message.status !== 'success') return false;
  if (message.output || message.artifactBundle || message.modelManifest) return false;
  const normalized = message.content.trim();
  if (!normalized) return false;
  const tail = normalized.slice(-CLARIFICATION_TAIL_WINDOW);
  return CLARIFICATION_SIGNAL_PATTERNS.some((pattern) => pattern.test(tail));
}

export function detectFollowUpAnswer(
  input: FollowUpAnswerGuardInput,
): FollowUpAnswerGuardResult {
  const trimmedPrompt = `${input.promptText ?? ''}`.trim();
  if (!trimmedPrompt) {
    return {
      matched: false,
      question: null,
      messageId: null,
      reason: 'empty prompt',
    };
  }

  if (input.explicitQuestionOnly) {
    return {
      matched: false,
      question: null,
      messageId: null,
      reason: 'explicit question-only request',
    };
  }

  if (input.attachments.length > 0) {
    return {
      matched: false,
      question: null,
      messageId: null,
      reason: 'attachments present',
    };
  }

  if (trimmedPrompt.length > 220) {
    return {
      matched: false,
      question: null,
      messageId: null,
      reason: 'prompt exceeds narrow-answer limit',
    };
  }

  if (!input.activeThread?.messages?.length) {
    return {
      matched: false,
      question: null,
      messageId: null,
      reason: 'no persisted thread messages',
    };
  }

  const lastMessage = findLastPersistedMessage(input.activeThread.messages);
  if (!lastMessage) {
    return {
      matched: false,
      question: null,
      messageId: null,
      reason: 'no persisted thread messages',
    };
  }

  if (!isClarificationQuestion(lastMessage)) {
    return {
      matched: false,
      question: null,
      messageId: null,
      reason: 'last persisted message is not an assistant clarification question',
    };
  }

  return {
    matched: true,
    question: lastMessage.content.trim(),
    messageId: lastMessage.id,
    reason: 'matched',
  };
}
