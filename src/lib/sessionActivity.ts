export type SessionActor =
  | { kind: 'user'; id: string }
  | { kind: 'agent'; id: string; label: string }
  | { kind: 'system'; id: string };

export type SessionSeverity = 'info' | 'success' | 'warning' | 'error' | 'question';

export type SessionEventKind =
  | 'agent_action_started'
  | 'agent_action_finished'
  | 'macro_patch_proposed'
  | 'macro_patch_applied'
  | 'params_changed'
  | 'render_started'
  | 'render_succeeded'
  | 'render_failed'
  | 'validation_reported'
  | 'preview_updated'
  | 'version_committed'
  | 'user_decision';

export type SessionEventArtifactKind =
  | 'preview_image'
  | 'preview_file'
  | 'validation_report'
  | 'raw_error'
  | 'text'
  | 'file';

export type SessionEventArtifact = {
  kind: SessionEventArtifactKind;
  label: string;
  value: string;
  href?: string | null;
  mimeType?: string | null;
  raw?: unknown;
};

export type SessionDiffKind = 'text' | 'params' | 'line';

export type SessionEventDiff = {
  kind: SessionDiffKind;
  label?: string | null;
  path?: string | null;
  key?: string | null;
  before: string | null;
  after: string | null;
};

export type SessionEvent = {
  id: string;
  sessionId: string;
  threadId: string | null;
  versionId: string | null;
  actor: SessionActor;
  kind: SessionEventKind;
  title: string;
  summary: string;
  timestamp: number;
  severity: SessionSeverity;
  artifacts?: SessionEventArtifact[];
  diffs?: SessionEventDiff[];
  raw?: unknown;
};

export type SessionActivity = {
  events: SessionEvent[];
  activeThreadId: string | null;
  activeVersionId: string | null;
  threadEvents: SessionEvent[];
  versionEvents: SessionEvent[];
  visibleEvents: SessionEvent[];
  latestEvent: SessionEvent | null;
  latestImportantEvent: SessionEvent | null;
  latestAgentActionEvent: SessionEvent | null;
  latestMacroEvent: SessionEvent | null;
  latestParamsEvent: SessionEvent | null;
  latestPreviewEvent: SessionEvent | null;
  latestValidationEvent: SessionEvent | null;
};

export type SessionBubbleEvent = {
  event: SessionEvent | null;
  title: string;
  summary: string;
  compact: boolean;
  openTarget: 'activity' | 'none';
};

export type SessionCodeDiffView = {
  event: SessionEvent | null;
  title: string;
  summary: string;
  currentCode: string;
  previousCode: string;
  nextCode: string;
  diff: SessionEventDiff | null;
  diffs: SessionEventDiff[];
  hasDiff: boolean;
};

const AGENT_ACTION_KINDS = new Set<SessionEventKind>([
  'agent_action_started',
  'agent_action_finished',
  'macro_patch_proposed',
  'macro_patch_applied',
  'params_changed',
  'render_started',
  'render_succeeded',
  'render_failed',
  'validation_reported',
  'preview_updated',
  'version_committed',
  'user_decision',
]);

const MACRO_EVENT_KINDS = new Set<SessionEventKind>([
  'macro_patch_proposed',
  'macro_patch_applied',
]);

const RENDER_CYCLE_KINDS = new Set<SessionEventKind>([
  'render_started',
  'render_succeeded',
  'render_failed',
  'validation_reported',
  'preview_updated',
]);

/// Related events for one render cycle: render/validation/preview events
/// sharing the anchor's version. Non-cycle anchors (macro, params, commit)
/// and version-less anchors have no cycle to link, so they return [].
export function relatedSessionEvents(
  events: SessionEvent[],
  eventId: string,
): SessionEvent[] {
  const anchor = events.find((event) => event.id === eventId);
  if (!anchor) return [];
  if (!anchor.versionId) return [];
  if (!RENDER_CYCLE_KINDS.has(anchor.kind)) return [];

  return sortSessionEvents(
    events.filter(
      (event) =>
        event.id !== anchor.id &&
        event.versionId === anchor.versionId &&
        RENDER_CYCLE_KINDS.has(event.kind),
    ),
  );
}

export function appendSessionEvent(
  events: SessionEvent[],
  event: SessionEvent,
): SessionEvent[] {
  return [...events, event]
    .map((item, index) => ({ item, index }))
    .sort((left, right) => {
      if (left.item.timestamp !== right.item.timestamp) {
        return left.item.timestamp - right.item.timestamp;
      }
      return left.index - right.index;
    })
    .map(({ item }) => item);
}

export function composeSessionActivity(
  events: SessionEvent[],
  activeThreadId: string | null,
  activeVersionId: string | null,
): SessionActivity {
  const sortedEvents = sortSessionEvents(events);
  const threadEvents = activeThreadId
    ? sortedEvents.filter((event) => event.threadId === activeThreadId)
    : sortedEvents;
  const versionEvents = activeVersionId
    ? threadEvents.filter((event) => event.versionId === activeVersionId)
    : [];
  const visibleEvents =
    activeVersionId && versionEvents.length > 0 ? versionEvents : threadEvents;

  return {
    events: sortedEvents,
    activeThreadId,
    activeVersionId,
    threadEvents,
    versionEvents,
    visibleEvents,
    latestEvent: latestEvent(visibleEvents),
    latestImportantEvent: latestImportantEvent(visibleEvents),
    latestAgentActionEvent: latestEvent(visibleEvents.filter((event) => AGENT_ACTION_KINDS.has(event.kind))),
    latestMacroEvent: latestEvent(visibleEvents.filter((event) => MACRO_EVENT_KINDS.has(event.kind))),
    latestParamsEvent: latestEvent(visibleEvents.filter((event) => event.kind === 'params_changed')),
    latestPreviewEvent: latestEvent(visibleEvents.filter((event) => event.kind === 'preview_updated')),
    latestValidationEvent: latestEvent(visibleEvents.filter((event) => event.kind === 'validation_reported')),
  };
}

export function composeBubbleEvent(activity: SessionActivity): SessionBubbleEvent {
  const event =
    latestEvent(
      activity.visibleEvents.filter((item) => item.severity === 'error'),
    ) ??
    latestEvent(
      activity.visibleEvents.filter((item) => item.severity === 'warning'),
    ) ??
    latestEvent(
      activity.visibleEvents.filter((item) => item.severity === 'question'),
    ) ??
    activity.latestAgentActionEvent ??
    activity.latestImportantEvent ??
    activity.latestEvent;

  if (!event) {
    return {
      event: null,
      title: '',
      summary: '',
      compact: false,
      openTarget: 'none',
    };
  }

  const compact = shouldCompactBubble(event);
  return {
    event,
    title: event.title,
    summary: compact ? compactSessionText(event.summary, 120) : event.summary,
    compact,
    openTarget: 'activity',
  };
}

export function composeCodeDiffView(
  activity: SessionActivity,
  selectedCode: string | null | undefined,
): SessionCodeDiffView {
  const event = activity.latestMacroEvent ?? latestEvent(activity.visibleEvents.filter((item) => MACRO_EVENT_KINDS.has(item.kind)));
  if (!event) {
    return {
      event: null,
      title: '',
      summary: '',
      currentCode: normalizeCode(selectedCode),
      previousCode: '',
      nextCode: normalizeCode(selectedCode),
      diff: null,
      diffs: [],
      hasDiff: false,
    };
  }

  const diff = latestCodeDiff(event);
  const previousCode = diff?.before ?? '';
  const nextCode = diff?.after ?? normalizeCode(selectedCode);
  return {
    event,
    title: event.title,
    summary: event.summary,
    currentCode: normalizeCode(selectedCode) || nextCode,
    previousCode,
    nextCode,
    diff,
    diffs: event.diffs ?? [],
    hasDiff: Boolean(diff),
  };
}

function latestEvent(events: SessionEvent[]): SessionEvent | null {
  if (events.length === 0) return null;
  return events[events.length - 1] ?? null;
}

function sortSessionEvents(events: SessionEvent[]): SessionEvent[] {
  return events
    .map((item, index) => ({ item, index }))
    .sort((left, right) => {
      if (left.item.timestamp !== right.item.timestamp) {
        return left.item.timestamp - right.item.timestamp;
      }
      return left.index - right.index;
    })
    .map(({ item }) => item);
}

function latestImportantEvent(events: SessionEvent[]): SessionEvent | null {
  let best: { event: SessionEvent; rank: number; index: number } | null = null;

  for (let index = 0; index < events.length; index += 1) {
    const event = events[index];
    const rank = importantEventRank(event);
    if (rank === null) continue;
    if (!best || rank < best.rank || (rank === best.rank && index > best.index)) {
      best = { event, rank, index };
    }
  }

  return best?.event ?? null;
}

function importantEventRank(event: SessionEvent): number | null {
  if (event.severity === 'error') return 0;
  if (event.severity === 'warning') return 1;
  if (event.severity === 'question') return 2;
  if (AGENT_ACTION_KINDS.has(event.kind)) return 3;
  return null;
}

function shouldCompactBubble(event: SessionEvent): boolean {
  const summary = normalizeText(event.summary);
  if (summary.length > 120) return true;
  if ((event.artifacts?.length ?? 0) > 0) return true;
  if ((event.diffs?.length ?? 0) > 0) return true;
  return event.title.trim().length > 48;
}

function latestCodeDiff(event: SessionEvent): SessionEventDiff | null {
  const diffs = event.diffs ?? [];
  if (diffs.length === 0) return null;

  const labeledDiff = diffs.find((diff) => Boolean(diff.path?.trim() || diff.key?.trim() || diff.label?.trim()));
  return labeledDiff ?? diffs[diffs.length - 1] ?? null;
}

function normalizeText(value: string | null | undefined): string {
  return `${value ?? ''}`.replace(/\s+/g, ' ').trim();
}

function compactSessionText(text: string, maxChars: number): string {
  const normalized = normalizeText(text);
  if (normalized.length <= maxChars) return normalized;
  const clipped = normalized.slice(0, Math.max(1, maxChars - 1)).trimEnd();
  return `${clipped}…`;
}

function normalizeCode(value: string | null | undefined): string {
  return `${value ?? ''}`;
}
