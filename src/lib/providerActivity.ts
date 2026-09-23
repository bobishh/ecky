import type { Message, ProviderActivity } from './types/domain';

type ProviderLiveMessage = {
  id: string;
  role: string;
  content: string;
  status: string;
  timestamp: number;
  providerEventKind?: 'assistant' | 'activity' | null;
};

export type ProviderActivityPhase = 'active' | 'completed' | 'interrupted' | 'error';

type ProviderTurnProjectionInput = {
  providerId: string;
  providerLabel: string;
  externalConversationId: string;
  activeTurnId: string | null | undefined;
  phase: ProviderActivityPhase;
  messages: ProviderLiveMessage[];
};

const LEGACY_ACTIVITY_PREFIX = /^(?:THINKING|PLAN|WORKING|USING TOOL|RUNNING|EDITING|WRITING|VIEWING|SEARCHING|DELEGATING|TASK|FAILED)\s*·/i;
const TURN_RECEIVED_ACTIVITY = 'THINKING · Message received. Starting work.';

function isActivityMessage(message: ProviderLiveMessage): boolean {
  if (message.providerEventKind) return message.providerEventKind === 'activity';
  return LEGACY_ACTIVITY_PREFIX.test(message.content.trim());
}

function projectSpeechMessage(
  message: ProviderLiveMessage,
  phase: ProviderActivityPhase,
): Message {
  return {
    id: message.id,
    role: 'assistant',
    content: message.content,
    status: phase === 'active'
      ? 'working'
      : phase === 'error'
          ? 'error'
          : 'success',
    timestamp: message.timestamp,
  };
}

export function collapseProviderActivity(input: {
  providerId: string;
  providerLabel: string;
  externalConversationId: string;
  activeTurnId: string | null | undefined;
  phase?: ProviderActivityPhase;
  messages: ProviderLiveMessage[];
}): Message | null {
  const items: string[] = [];
  for (const message of input.messages) {
    const content = message.content.trim();
    if (!content) continue;
    items.push(content);
  }
  if (items.length === 0) return null;

  const summary = items[items.length - 1];
  const phase = input.phase ?? 'active';
  const activity: ProviderActivity = {
    providerLabel: input.providerLabel,
    summary,
    phase,
    items,
  };
  return {
    id: [
      'provider-working',
      input.providerId,
      input.externalConversationId,
      input.activeTurnId || 'active',
    ].join(':'),
    role: 'assistant',
    content: summary,
    status: phase === 'active'
      ? 'working'
      : phase === 'error'
        ? 'error'
        : 'success',
    timestamp: Math.max(...input.messages.map((message) => message.timestamp), 0),
    providerActivity: activity,
  };
}

export function projectProviderTurnMessages(input: ProviderTurnProjectionInput): Message[] {
  if (input.phase === 'active' && input.activeTurnId && input.messages.length === 0) {
    const receipt = collapseProviderActivity({
      ...input,
      messages: [{
        id: `${input.activeTurnId}:received`,
        role: 'assistant',
        content: TURN_RECEIVED_ACTIVITY,
        status: 'working',
        timestamp: 0,
        providerEventKind: 'activity',
      }],
    });
    return receipt ? [receipt] : [];
  }

  const activityMessages = input.messages.filter(isActivityMessage);
  if (input.phase === 'completed') {
    const activity = collapseProviderActivity({ ...input, messages: activityMessages });
    return activity ? [activity] : [];
  }

  if (input.phase === 'active') {
    const projected = input.messages
      .filter((message) => !isActivityMessage(message))
      .map((message) => projectSpeechMessage(message, input.phase));
    const activity = collapseProviderActivity({ ...input, messages: activityMessages });
    if (activity) projected.push(activity);
    return projected.sort((left, right) => left.timestamp - right.timestamp);
  }

  const projected: Message[] = [];
  let activityRun: ProviderLiveMessage[] = [];
  const flushActivityRun = () => {
    if (activityRun.length === 0) return;
    const activity = collapseProviderActivity({
      ...input,
      activeTurnId: `${input.activeTurnId ?? 'turn'}:${activityRun[0].id}`,
      messages: activityRun,
    });
    if (activity) projected.push(activity);
    activityRun = [];
  };
  for (const message of input.messages) {
    if (isActivityMessage(message)) {
      activityRun.push(message);
      continue;
    }
    flushActivityRun();
    projected.push(projectSpeechMessage(message, input.phase));
  }
  flushActivityRun();
  return projected;
}
