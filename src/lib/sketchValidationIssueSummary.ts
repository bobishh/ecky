import type { SketchValidationIssue, SketchValidationIssueKind } from './tauri/contracts';

const ISSUE_KIND_LABELS: Record<SketchValidationIssueKind, string> = {
  missingClosedProfile: 'missing closed profile',
  missingProjectionEdges: 'missing projection edges',
  boundsMismatch: 'bounds mismatch',
  containmentMismatch: 'containment mismatch',
  topologyMismatch: 'topology mismatch',
  concavityMismatch: 'concavity mismatch',
  projectionReplayCoverageGap: 'projection replay coverage gap',
  candidateGraphNoVertices: 'candidate graph no vertices',
  candidateGraphNoEdges: 'candidate graph no edges',
};

export function summarizeSketchValidationIssue(issue: SketchValidationIssue): string {
  const structured = issueStructuredParts(issue);
  const message = issue.message.trim();

  if (structured.length === 0) {
    return message || 'validation issue';
  }

  if (!message) {
    return structured.join(' / ');
  }

  return `${structured.join(' / ')} / ${message}`;
}

export function summarizeSketchValidationIssues(issues: SketchValidationIssue[] | null | undefined): string {
  return (issues ?? []).map(summarizeSketchValidationIssue).filter(Boolean).join('; ');
}

function issueStructuredParts(issue: SketchValidationIssue): string[] {
  const parts: string[] = [];

  if (issue.kind) {
    parts.push(ISSUE_KIND_LABELS[issue.kind] ?? issue.kind);
  }
  if (issue.view && issue.view !== 'custom') {
    parts.push(issue.view.toUpperCase());
  }
  if (issue.topology?.loopRole) {
    parts.push(issue.topology.loopRole.toUpperCase());
  }
  if (issue.edgeId) {
    parts.push(issue.edgeId);
  } else if (issue.primitiveId) {
    parts.push(issue.primitiveId);
  }

  return parts;
}
