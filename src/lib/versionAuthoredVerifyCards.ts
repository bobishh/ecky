import { deriveAuthoredVerifyChips, type AuthoredVerifyChip } from './controllers/structuralVerification';
import type { Message, Request, StructuralVerificationResult } from './types/domain';

export type VersionAuthoredVerifyChipMap = Record<string, AuthoredVerifyChip[]>;

export function buildVersionAuthoredVerifyChipMap(
  messages: Message[],
  requests: Request[],
): VersionAuthoredVerifyChipMap {
  const chipMap: VersionAuthoredVerifyChipMap = {};
  const persistedMessageIds = new Set<string>();

  for (const message of messages) {
    const chips = chipsFromResult(message.structuralVerification ?? null);
    if (chips.length === 0) continue;
    chipMap[message.id] = chips;
    persistedMessageIds.add(message.id);
  }

  for (const request of requests) {
    if (request.phase !== 'success') continue;
    const messageId = request.result?.messageId?.trim();
    if (!messageId || persistedMessageIds.has(messageId)) continue;
    const chips = chipsFromResult(request.result?.structuralVerification ?? null);
    if (chips.length === 0) continue;
    chipMap[messageId] = chips;
  }

  return chipMap;
}

function chipsFromResult(
  result: StructuralVerificationResult | null | undefined,
): AuthoredVerifyChip[] {
  return deriveAuthoredVerifyChips(result);
}
