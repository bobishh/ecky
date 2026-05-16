import type { BrepHiddenLineProjectionResponse } from './tauri/contracts';
import type { ArtifactBundle } from './types/domain';
import type { SketchValidationRow } from './sketchValidationLedger';
import { summarizeSketchValidationIssues } from './sketchValidationIssueSummary';

export type SketchAcceptedCadInput = {
  artifactBundle: ArtifactBundle | null;
  hiddenLineResponse: BrepHiddenLineProjectionResponse | null;
  hiddenLineErrorText: string;
  hiddenLineLoading: boolean;
};

export function buildSketchAcceptedCadRow(input: SketchAcceptedCadInput): SketchValidationRow | null {
  if (!input.artifactBundle && !input.hiddenLineResponse && !input.hiddenLineErrorText && !input.hiddenLineLoading) {
    return null;
  }

  if (input.hiddenLineLoading) {
    return acceptedCadRow('pending', 'Extracting exact BRep/STEP hidden-line validation before CAD acceptance.');
  }

  if (input.hiddenLineResponse?.validation) {
    const validation = input.hiddenLineResponse.validation;
    const issueText = summarizeSketchValidationIssues(validation.issues);

    if (!validation.passed || (validation.issues?.length ?? 0) > 0) {
      return acceptedCadRow('fail', issueText || 'Exact BRep/sketch validation failed.');
    }

    const viewCount = input.hiddenLineResponse.views?.length ?? 0;
    const evidenceText = validation.evidence?.filter(Boolean).join('; ') ?? '';
    return acceptedCadRow(
      'pass',
      [`Accepted BRep`, `${formatCount(viewCount, 'view')} validated`, basename(input.hiddenLineResponse.sourceArtifactPath), evidenceText]
        .filter(Boolean)
        .join('; '),
    );
  }

  if (input.hiddenLineResponse) {
    return acceptedCadRow('pending', 'Hidden-line projection exists; waiting for explicit BRep/sketch validation.');
  }

  if (input.hiddenLineErrorText) {
    return acceptedCadRow('fail', input.hiddenLineErrorText);
  }

  if (input.artifactBundle) {
    if (hasBrepArtifact(input.artifactBundle)) {
      return acceptedCadRow('pending', 'BRep/STEP artifact exists; waiting for exact hidden-line validation.');
    }
    return acceptedCadRow('pending', 'Preview artifact only; accepted CAD requires exact BRep/STEP validation.');
  }

  return null;
}

function acceptedCadRow(status: SketchValidationRow['status'], detail: string): SketchValidationRow {
  return {
    id: 'acceptedCad',
    label: 'Accepted CAD',
    status,
    detail,
  };
}

function hasBrepArtifact(bundle: ArtifactBundle): boolean {
  if (bundle.fcstdPath) return true;
  return Boolean(bundle.exportArtifacts?.some((artifact) => artifact.format === 'step' && artifact.path));
}

function basename(path: string | null | undefined): string {
  const value = path ?? '';
  return value.split(/[\\/]/).filter(Boolean).pop() ?? value;
}

function formatCount(count: number, singular: string): string {
  return `${count} ${count === 1 ? singular : `${singular}s`}`;
}
