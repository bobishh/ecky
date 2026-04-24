export type DialogueState =
  | { mode: 'generate' }
  | { mode: 'mcp-idle' }
  | { mode: 'agent-reply'; requestId: string; agentLabel: string };

type PendingAgentPromptLike = {
  requestId: string;
  agentLabel: string;
} | null | undefined;

export function deriveDialogueState(
  activePendingAgentPrompt: PendingAgentPromptLike,
  usesQueuedAgentDialogue: boolean,
): DialogueState {
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
