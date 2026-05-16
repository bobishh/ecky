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

export type AgentDraftFeedback = {
  status: AgentDraftFeedbackStatus;
  summary: string;
  items: AgentDraftFeedbackItem[];
  source: AgentDraftFeedbackSource;
  threadId: string;
  previewId: string;
  sessionId: string;
};

function normalizeText(value: string | null | undefined): string {
  return `${value ?? ''}`.replace(/\s+/g, ' ').trim();
}

export function summarizeAgentDraftFeedback(
  feedback: AgentDraftFeedback | null | undefined,
  maxChars = 140,
): string {
  const summary = normalizeText(feedback?.summary);
  if (!summary) return '';
  if (summary.length <= maxChars) return summary;
  const softLimit = Math.max(32, maxChars - 1);
  const sliced = summary.slice(0, softLimit);
  const boundary = Math.max(
    sliced.lastIndexOf('. '),
    sliced.lastIndexOf('; '),
    sliced.lastIndexOf(', '),
    sliced.lastIndexOf(' '),
  );
  const compact = (boundary >= 48 ? sliced.slice(0, boundary) : sliced).trimEnd();
  return `${compact}…`;
}

export function isVisibleAgentDraftFeedback(
  feedback: AgentDraftFeedback | null | undefined,
  activeThreadId: string | null | undefined,
  activeVersionId: string | null | undefined,
): boolean {
  if (!feedback) return false;
  return feedback.threadId === (activeThreadId ?? '') && feedback.previewId === (activeVersionId ?? '');
}
