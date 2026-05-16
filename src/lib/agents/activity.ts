import type { AgentTerminalSnapshot } from '../types/domain';
import type { ThreadAgentState } from '../tauri/client';

export type GenieBubbleSource =
  | 'none'
  | 'onboarding'
  | 'viewportScreenshot'
  | 'confirm'
  | 'terminalAttention'
  | 'pendingPrompt'
  | 'draftFeedback'
  | 'queuedMessage'
  | 'threadAgentActivity'
  | 'threadAgentMascot'
  | 'threadError'
  | 'repair'
  | 'cooking'
  | 'assistant';

export type GenieBubblePresentation = {
  text: string;
  source: GenieBubbleSource;
  compact: boolean;
  badge: string | null;
  contextLabel: string | null;
};

const PREVIEW_VALIDATION_RE =
  /\b(validation|validated|preview|projection|hidden-?line|containment|topology|brep|step)\b/i;
const PREVIEW_REPAIR_RE = /\b(repair|repaired|rerun|rerunning|snap|bounds|patch)\b/i;

function normalizeBubbleText(text: string | null | undefined): string {
  return `${text ?? ''}`.replace(/\s+/g, ' ').trim();
}

function compactBubbleSources(): Set<GenieBubbleSource> {
  return new Set<GenieBubbleSource>([
    'threadAgentActivity',
    'threadAgentMascot',
    'repair',
    'cooking',
  ]);
}

function previewBadgeFor(input: {
  source: GenieBubbleSource;
  text: string;
  hasPreviewArtifact: boolean;
}): string | null {
  if (!input.hasPreviewArtifact || !input.text) return null;
  if (!compactBubbleSources().has(input.source)) return null;
  if (input.source !== 'repair' && PREVIEW_VALIDATION_RE.test(input.text)) {
    return 'PREVIEW CHECK';
  }
  if (input.source === 'repair' || PREVIEW_REPAIR_RE.test(input.text)) {
    return 'PREVIEW REPAIR';
  }
  return null;
}

export function resolveGenieBubblePresentation(input: {
  onboardingText?: string | null;
  viewportScreenshotMessage?: string | null;
  confirmMessage?: string | null;
  terminalAttentionSummary?: string | null;
  pendingAgentPrompt?: { message?: string | null; agentLabel: string } | null;
  draftFeedbackSummary?: string | null;
  hasQueuedAgentMessageWithoutPrompt?: boolean;
  threadAgentState?: ThreadAgentState | null | undefined;
  activeMcpBubbleSummary?: string | null;
  threadAgentMascotBubble?: string | null;
  threadError?: string | null;
  repairMessage?: string | null;
  cookingPhrase?: string | null;
  assistantBubble?: string | null;
  dismissedBubbleText?: string | null;
  hasPreviewArtifact?: boolean;
  previewArtifactName?: string | null;
}): GenieBubblePresentation {
  let source: GenieBubbleSource = 'none';
  let raw = '';

  if (input.onboardingText) {
    source = 'onboarding';
    raw = input.onboardingText;
  } else if (input.viewportScreenshotMessage) {
    source = 'viewportScreenshot';
    raw = input.viewportScreenshotMessage;
  } else if (input.confirmMessage) {
    source = 'confirm';
    raw = input.confirmMessage;
  } else if (input.terminalAttentionSummary) {
    source = 'terminalAttention';
    raw = input.terminalAttentionSummary;
  } else if (input.pendingAgentPrompt) {
    source = 'pendingPrompt';
    raw =
      input.pendingAgentPrompt.message ||
      `${input.pendingAgentPrompt.agentLabel} is waiting for your input`;
  } else if (input.draftFeedbackSummary) {
    source = 'draftFeedback';
    raw = input.draftFeedbackSummary;
  } else if (input.hasQueuedAgentMessageWithoutPrompt) {
    source = 'queuedMessage';
    raw = 'Your message is queued. The agent has not requested the next prompt yet.';
  } else if (input.threadAgentState?.connectionState === 'active') {
    source = 'threadAgentActivity';
    raw = input.activeMcpBubbleSummary ?? '';
  } else if (input.threadAgentMascotBubble) {
    source = 'threadAgentMascot';
    raw = input.threadAgentMascotBubble;
  } else if (input.threadError) {
    source = 'threadError';
    raw = input.threadError;
  } else if (input.repairMessage) {
    source = 'repair';
    raw = input.repairMessage;
  } else if (input.cookingPhrase) {
    source = 'cooking';
    raw = input.cookingPhrase;
  } else if (input.assistantBubble) {
    source = 'assistant';
    raw = input.assistantBubble;
  }

  const normalized = normalizeBubbleText(raw);
  if (!normalized || normalizeBubbleText(input.dismissedBubbleText) === normalized) {
    return {
      text: '',
      source: 'none',
      compact: false,
      badge: null,
      contextLabel: null,
    };
  }

  const badge = previewBadgeFor({
    source,
    text: normalized,
    hasPreviewArtifact: Boolean(input.hasPreviewArtifact),
  });
  const compact = Boolean(badge);

  return {
    text: compact ? compactThreadActivitySummary(normalized, 104) : normalized,
    source,
    compact,
    badge,
    contextLabel: compact ? normalizeBubbleText(input.previewArtifactName) || null : null,
  };
}

export function isThreadAgentBusy(
  state: ThreadAgentState | null | undefined,
): boolean {
  return Boolean(
    state &&
      state.connectionState === 'active' &&
      state.busy &&
      !state.waitingOnPrompt,
  );
}

export function formatAgentActivityElapsed(
  activityStartedAt: number | null | undefined,
  nowSecs: number,
): string | null {
  if (!activityStartedAt) return null;
  const elapsed = Math.max(0, nowSecs - activityStartedAt);
  const hours = Math.floor(elapsed / 3600);
  const minutes = Math.floor((elapsed % 3600) / 60);
  const seconds = elapsed % 60;
  if (hours > 0) {
    return `${hours}h ${String(minutes).padStart(2, '0')}m`;
  }
  return `${minutes}m ${String(seconds).padStart(2, '0')}s`;
}

function withElapsed(
  text: string | null | undefined,
  activityStartedAt: number | null | undefined,
  nowSecs: number,
): string {
  const normalized = `${text ?? ''}`.replace(/\s+/g, ' ').trim();
  if (!normalized) return '';
  const elapsed = formatAgentActivityElapsed(activityStartedAt, nowSecs);
  return elapsed ? `${normalized} · ${elapsed}` : normalized;
}

export function resolveActiveMcpBubble(input: {
  threadAgentState: ThreadAgentState | null | undefined;
  visibleAgentTerminal: AgentTerminalSnapshot | null | undefined;
  cookingPhrase: string | null | undefined;
  nowSecs: number;
}): string {
  const state = input.threadAgentState;
  if (!state) return '';

  const activityStartedAt =
    state.activityStartedAt ?? null;
  const activityLabel = state.activityLabel?.trim() || '';
  if (activityLabel) {
    return withElapsed(activityLabel, activityStartedAt, input.nowSecs);
  }

  const cookingPhrase = `${input.cookingPhrase ?? ''}`.trim();
  if (isThreadAgentBusy(state) && cookingPhrase) {
    return withElapsed(cookingPhrase, activityStartedAt, input.nowSecs);
  }

  return (
    `${state.statusText ?? ''}`.trim()
  );
}

export function resolveTerminalActivityMeta(input: {
  threadAgentState: ThreadAgentState | null | undefined;
  visibleAgentTerminal: AgentTerminalSnapshot | null | undefined;
  nowSecs: number;
}): string {
  const state = input.threadAgentState;
  const base =
    state?.activityLabel?.trim() ||
    input.visibleAgentTerminal?.summary?.trim() ||
    state?.statusText?.trim() ||
    '';
  return withElapsed(
    base,
    state?.activityStartedAt ?? null,
    input.nowSecs,
  );
}

export function compactThreadActivitySummary(
  text: string | null | undefined,
  maxChars = 180,
): string {
  const raw = `${text ?? ''}`.trim();
  if (!raw) return '';
  const firstParagraph = raw
    .split(/\n\s*\n/)
    .map((chunk) => chunk.trim())
    .find(Boolean) ?? raw;
  const normalized = firstParagraph.replace(/\s+/g, ' ').trim();
  if (normalized.length <= maxChars) return normalized;
  const softLimit = Math.max(32, maxChars - 1);
  const sliced = normalized.slice(0, softLimit);
  const boundary = Math.max(
    sliced.lastIndexOf('. '),
    sliced.lastIndexOf('! '),
    sliced.lastIndexOf('? '),
    sliced.lastIndexOf('; '),
    sliced.lastIndexOf(', '),
    sliced.lastIndexOf(' '),
  );
  const compact = (boundary >= 48 ? sliced.slice(0, boundary) : sliced).trimEnd();
  return `${compact}…`;
}
