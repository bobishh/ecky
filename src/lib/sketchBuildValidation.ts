export type SketchBuildValidationStatus = 'pass' | 'pending' | 'fail';

export type SketchBuildValidationStroke = {
  closed?: boolean | null;
  points?: unknown[] | null;
  view?: string | null;
};

export type SketchBuildValidationDraft = {
  source?: string | null;
} | null;

export type SketchBuildValidationArtifactBundle = {
  previewStlPath?: string | null;
  previewGlbPath?: string | null;
  viewerAssets?: unknown[] | null;
} | null;

export type SketchBuildValidationInput = {
  strokes: SketchBuildValidationStroke[];
  draft?: SketchBuildValidationDraft;
  artifactBundle?: SketchBuildValidationArtifactBundle;
  projectionsCount: number;
  extrudeDepth?: number | null;
  errorText?: string | null;
};

export type SketchBuildValidationRow = {
  id: 'closedSketchContract' | 'previewArtifact' | 'projectionCount';
  label: string;
  status: SketchBuildValidationStatus;
  evidence: string;
};

export type SketchBuildValidationIssue = {
  id: SketchBuildValidationRow['id'];
  status: Exclude<SketchBuildValidationStatus, 'pass'>;
  evidence: string;
};

export type SketchBuildValidationSummary = {
  rows: SketchBuildValidationRow[];
  issues: SketchBuildValidationIssue[];
};

const REQUIRED_PROJECTION_COUNT = 3;

export function buildSketchBuildValidationSummary(
  input: SketchBuildValidationInput,
): SketchBuildValidationSummary {
  const closedStrokes = input.strokes.filter((stroke) => stroke.closed === true);
  const closedStrokeCount = closedStrokes.length;
  const firstClosedStroke = closedStrokes[0] ?? null;
  const draftLineCount = sourceLineCount(input.draft?.source);
  const errorText = input.errorText?.trim() ?? '';
  const extrudeDepth = Number.isFinite(input.extrudeDepth) ? Number(input.extrudeDepth) : 12;

  const closedSketchContract = closedSketchContractRow(closedStrokeCount, draftLineCount, firstClosedStroke, extrudeDepth);
  const previewArtifact = previewArtifactRow(input.artifactBundle ?? null, closedSketchContract.status, errorText);
  const projectionCount = projectionCountRow(input.projectionsCount, previewArtifact.status);
  const rows = [closedSketchContract, previewArtifact, projectionCount];

  return {
    rows,
    issues: rows.flatMap((row) =>
      row.status === 'pass' ? [] : [{ id: row.id, status: row.status, evidence: row.evidence }],
    ),
  };
}

function closedSketchContractRow(
  closedStrokeCount: number,
  draftLineCount: number,
  stroke: SketchBuildValidationStroke | null,
  extrudeDepth: number,
): SketchBuildValidationRow {
  if (closedStrokeCount === 0) {
    return {
      id: 'closedSketchContract',
      label: 'Sketch contract',
      status: 'fail',
      evidence: '0 closed strokes; closed profile required before build.',
    };
  }

  if (draftLineCount === 0) {
    return {
      id: 'closedSketchContract',
      label: 'Sketch contract',
      status: 'pending',
      evidence: `${formatCount(closedStrokeCount, 'closed stroke')}; waiting for draft source.`,
    };
  }

  const view = (stroke?.view ?? 'unknown').toString();
  const pointCount = stroke?.points?.length ?? 0;

  return {
    id: 'closedSketchContract',
    label: 'Sketch contract',
    status: 'pass',
    evidence: `${view} view; ${formatCount(pointCount, 'point')}; depth ${formatMillimeters(extrudeDepth)}; ${formatCount(draftLineCount, 'draft source line')}.`,
  };
}

function previewArtifactRow(
  artifactBundle: SketchBuildValidationArtifactBundle,
  contractStatus: SketchBuildValidationStatus,
  errorText: string,
): SketchBuildValidationRow {
  if (errorText.length > 0) {
    return {
      id: 'previewArtifact',
      label: 'Preview artifact',
      status: 'fail',
      evidence: errorText,
    };
  }

  if (contractStatus !== 'pass') {
    return {
      id: 'previewArtifact',
      label: 'Preview artifact',
      status: 'pending',
      evidence: 'Waiting for closed sketch contract.',
    };
  }

  const previewPath = artifactBundle?.previewStlPath ?? artifactBundle?.previewGlbPath ?? '';
  if (previewPath.length === 0) {
    return {
      id: 'previewArtifact',
      label: 'Preview artifact',
      status: 'pending',
      evidence: 'Waiting for preview artifact path.',
    };
  }

  return {
    id: 'previewArtifact',
    label: 'Preview artifact',
    status: 'pass',
    evidence: `${basename(previewPath)}; ${formatCount(artifactBundle?.viewerAssets?.length ?? 0, 'viewer asset')}.`,
  };
}

function projectionCountRow(
  projectionsCount: number,
  previewStatus: SketchBuildValidationStatus,
): SketchBuildValidationRow {
  if (previewStatus !== 'pass') {
    return {
      id: 'projectionCount',
      label: 'Projection count',
      status: 'pending',
      evidence: 'Waiting for preview artifact.',
    };
  }

  if (projectionsCount < REQUIRED_PROJECTION_COUNT) {
    return {
      id: 'projectionCount',
      label: 'Projection count',
      status: 'pending',
      evidence: `${projectionsCount}/${REQUIRED_PROJECTION_COUNT} projection views captured.`,
    };
  }

  return {
    id: 'projectionCount',
    label: 'Projection count',
    status: 'pass',
    evidence: `${projectionsCount}/${REQUIRED_PROJECTION_COUNT} projection views captured.`,
  };
}

function basename(path: string): string {
  return path.split('/').filter(Boolean).at(-1) ?? path;
}

function sourceLineCount(source: string | null | undefined): number {
  return source?.split('\n').filter((line) => line.trim().length > 0).length ?? 0;
}

function formatCount(count: number, noun: string): string {
  return `${count} ${count === 1 ? noun : `${noun}s`}`;
}

function formatMillimeters(value: number): string {
  return `${Number(value.toFixed(4)).toString()}mm`;
}
