import { diffCode, type CodeDiffRow } from './codeDiff';
import type { SessionCodeDiffView, SessionEvent } from './sessionActivity';

export type MacroDiffPanelModel = {
  title: string;
  actorLabel: string;
  summary: string;
  timestamp: number;
  timeLabel: string;
  oldSummary: string;
  newSummary: string;
  changedLineCount: number;
  hunkCount: number;
  rows: CodeDiffRow[];
  hasDiff: boolean;
};

export function composeMacroDiffPanelModel(
  view: SessionCodeDiffView,
): MacroDiffPanelModel | null {
  const event = view.event;
  if (!event) return null;

  const diff = diffCode(view.previousCode, view.nextCode);
  return {
    title: view.title || event.title,
    actorLabel: actorLabel(event),
    summary: view.summary || event.summary,
    timestamp: event.timestamp,
    timeLabel: timeLabel(event.timestamp),
    oldSummary: lineCountLabel(diff.summary.oldLineCount),
    newSummary: `${lineCountLabel(diff.summary.newLineCount)} (+${diff.summary.insertedLineCount} / −${diff.summary.deletedLineCount})`,
    changedLineCount: diff.summary.changedLineCount,
    hunkCount: diff.summary.hunkCount,
    rows: diff.rows,
    hasDiff: !diff.summary.isEmpty,
  };
}

function actorLabel(event: SessionEvent): string {
  if (event.actor.kind === 'agent') return event.actor.label || event.actor.id;
  return event.actor.kind.toUpperCase();
}

function timeLabel(timestamp: number): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

function lineCountLabel(count: number): string {
  return count === 1 ? '1 line' : `${count} lines`;
}
