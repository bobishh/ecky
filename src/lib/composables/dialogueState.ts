export type DialogueState =
  | { mode: 'generate' }
  | { mode: 'mcp-idle' }
  | {
      mode: 'provider';
      providerId: string;
      externalConversationId: string | null;
      label: string;
      supportsSteer: boolean;
      supportsStop: boolean;
    }
  | { mode: 'agent-reply'; requestId: string; agentLabel: string };

type PendingAgentPromptLike = {
  requestId: string;
  agentLabel: string;
} | null | undefined;

type CodexDialogueBindingLike = {
  codexThreadId?: string;
  providerId?: string;
  externalConversationId?: string;
  label: string;
  supportsSteer?: boolean;
  supportsStop?: boolean;
} | null | undefined;

export function deriveDialogueState(
  activePendingAgentPrompt: PendingAgentPromptLike,
  usesQueuedAgentDialogue: boolean,
  connectionType?: string | null,
  codexBinding?: CodexDialogueBindingLike,
): DialogueState {
  if (connectionType?.startsWith('provider:')) {
    const providerId = connectionType.slice('provider:'.length) || 'codex';
    return {
      mode: 'provider',
      providerId,
      externalConversationId: codexBinding?.externalConversationId
        ?? codexBinding?.codexThreadId
        ?? null,
      label: providerId === 'agy' ? 'Agy' : providerId === 'grok' ? 'Grok' : providerId === 'codex' ? 'Codex' : providerId,
      supportsSteer: codexBinding?.supportsSteer ?? providerId === 'codex',
      supportsStop: codexBinding?.supportsStop ?? true,
    };
  }
  if (activePendingAgentPrompt) {
    return {
      mode: 'agent-reply',
      requestId: activePendingAgentPrompt.requestId,
      agentLabel: activePendingAgentPrompt.agentLabel,
    };
  }
  if (usesQueuedAgentDialogue) return { mode: 'mcp-idle' };
  return { mode: 'generate' };
}
