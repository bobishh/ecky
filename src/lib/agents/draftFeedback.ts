export type AgentDraftFeedbackStatus = 'checking' | 'passed' | 'failed' | 'warning';

export type AgentDraftFeedbackSource =
  | 'structuralVerification'
  | 'renderError'
  | 'toolError'
  | 'visualRepair';

export type AgentDraftFeedbackItem = {
  code: string;
  message: string;
};

export type AgentAuthoringLint = {
  kind?: string | null;
  partKey?: string | null;
  paramKey?: string | null;
  suggestedParamKey?: string | null;
  occurrenceCount?: number | null;
  message: string;
};

export type AgentDraftFeedback = {
  status: AgentDraftFeedbackStatus;
  summary: string;
  items: AgentDraftFeedbackItem[];
  source: AgentDraftFeedbackSource;
  authoringLints?: AgentAuthoringLint[];
  threadId: string;
  previewId: string;
  sessionId: string;
};

function normalizeText(value: string | null | undefined): string {
  return `${value ?? ''}`.replace(/\s+/g, ' ').trim();
}

function truncateSummary(text: string, maxChars: number): string {
  if (text.length <= maxChars) return text;
  const softLimit = Math.max(32, maxChars - 1);
  const sliced = text.slice(0, softLimit);
  const boundary = Math.max(
    sliced.lastIndexOf('. '),
    sliced.lastIndexOf('; '),
    sliced.lastIndexOf(', '),
    sliced.lastIndexOf(' '),
  );
  const compact = (boundary >= 48 ? sliced.slice(0, boundary) : sliced).trimEnd();
  return `${compact}…`;
}

export function summarizeAgentDraftFeedback(
  feedback: AgentDraftFeedback | null | undefined,
  maxChars = 140,
): string {
  const summary = normalizeText(feedback?.summary);
  if (!summary) return '';
  return truncateSummary(summary, maxChars);
}

export function summarizeAgentAuthoringLints(
  lints: AgentAuthoringLint[] | null | undefined,
  maxChars = 160,
): string {
  const normalized = (lints ?? [])
    .map((lint) => normalizeText(lint.message))
    .filter(Boolean);
  if (!normalized.length) return '';
  const lead = normalized[0];
  const label =
    normalized.length > 1
      ? `Authoring lints (${normalized.length}): ${lead} (+${normalized.length - 1} more)`
      : `Authoring lint: ${lead}`;
  return truncateSummary(label, maxChars);
}

export function composeAgentDraftFeedbackBubbleText(input: {
  feedback: AgentDraftFeedback | null | undefined;
  fallbackAuthoringLints?: AgentAuthoringLint[] | null | undefined;
  summaryMaxChars?: number;
  lintMaxChars?: number;
}): string {
  const summary = summarizeAgentDraftFeedback(input.feedback, input.summaryMaxChars ?? 140);
  const lints =
    input.feedback?.authoringLints?.length
      ? input.feedback.authoringLints
      : (input.fallbackAuthoringLints ?? []);
  const lintSummary = summarizeAgentAuthoringLints(lints, input.lintMaxChars ?? 160);
  if (summary && lintSummary) return `${summary} ${lintSummary}`;
  return summary || lintSummary;
}

export function isVisibleAgentDraftFeedback(
  feedback: AgentDraftFeedback | null | undefined,
  activeThreadId: string | null | undefined,
  activeVersionId: string | null | undefined,
): boolean {
  if (!feedback) return false;
  return feedback.threadId === (activeThreadId ?? '') && feedback.previewId === (activeVersionId ?? '');
}
