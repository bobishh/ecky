import type { AutoAgent } from '../types/domain';
import { buildAgentGenieTraits } from '../genie/traits';
import type { GenieBubbleSource } from './draftFeedback';
import { promptBelongsToPrimaryAgent, usesMcpConnection } from './state';

export type RelayPresence = {
  hue: number;
  label: string;
};

/**
 * Bubble sources that carry thread-agent provenance and may therefore be
 * relayed through Ecky when they originate from a non-primary agent.
 */
const RELAY_SOURCES = new Set<GenieBubbleSource>([
  'threadAgentActivity',
  'threadAgentMascot',
  'threadError',
]);

/**
 * Decide whether the currently visible bubble is a message *relayed* through
 * Ecky from a non-primary agent (MCP only). Returns the sending agent's
 * deterministic signature hue and label, or `null` when no relay treatment
 * applies. Pure: does not touch the single-winner bubble resolver.
 */
export function resolveRelayPresence(input: {
  source: GenieBubbleSource;
  connectionType: string | null | undefined;
  autoAgents: AutoAgent[];
  primaryAgentId: string | null | undefined;
  senderLabel: string | null | undefined;
}): RelayPresence | null {
  if (!usesMcpConnection(input.connectionType)) return null;
  if (!RELAY_SOURCES.has(input.source)) return null;

  const label = (input.senderLabel ?? '').trim();
  if (!label) return null;

  if (promptBelongsToPrimaryAgent(input.autoAgents, input.primaryAgentId, label)) {
    return null;
  }

  return {
    hue: buildAgentGenieTraits(label).colorHue,
    label,
  };
}
