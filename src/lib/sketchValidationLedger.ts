import type { SketchDraftSource } from './tauri/contracts';
import type { SketchStroke } from './sketchWorkspaceState';
import { basename, sourceLineCount } from './sketchWorkspaceState';
import { buildSketchBuildValidationSummary } from './sketchBuildValidation';
import { buildSketchFitValidationSeed } from './sketchFitValidation';
import { closedStrokeBounds } from './sketchEditState';

export type SketchValidationStatus = 'pass' | 'fail' | 'pending';

export type SketchValidationRow = {
  id: string;
  label: string;
  status: SketchValidationStatus;
  detail: string;
};

export type SketchValidationArtifactBundle = {
  previewStlPath?: string | null;
  viewerAssets?: unknown[] | null;
};

export type SketchValidationLedgerInput = {
  strokes: SketchStroke[];
  draft: SketchDraftSource | null;
  artifactBundle: SketchValidationArtifactBundle | null;
  extrudeDepth?: number;
  projectionsCount: number;
  errorText: string;
};

export function buildSketchValidationRows(input: SketchValidationLedgerInput): SketchValidationRow[] {
  const closedStrokeCount = input.strokes.filter((stroke) => stroke.closed).length;
  const hasClosedProfile = closedStrokeCount > 0;
  const hasSourceDraft = Boolean(input.draft?.source);
  const hasMeshPreview = Boolean(input.artifactBundle?.previewStlPath);
  const hasBackendError = input.errorText.length > 0;

  const buildValidationRows = buildSketchBuildValidationSummary({
    strokes: input.strokes,
    draft: input.draft,
    artifactBundle: input.artifactBundle,
    projectionsCount: input.projectionsCount,
    extrudeDepth: input.extrudeDepth,
    errorText: input.errorText,
  }).rows;

  return [
    closedProfileRow(hasClosedProfile, closedStrokeCount),
    sketchContractRow(buildValidationRows[0]),
    sourceRow(hasClosedProfile, hasSourceDraft, hasBackendError, input),
    previewArtifactRow(buildValidationRows[1]),
    sourceFitCheckRow(input),
    ...constraintSolverRows(input.strokes),
    ...constraintValueRows(input.strokes),
    meshRow(hasClosedProfile, hasSourceDraft, hasMeshPreview, hasBackendError, input),
    projectionsRow(hasMeshPreview, input.projectionsCount),
  ];
}

function sketchContractRow(row: { status: SketchValidationStatus; evidence: string }): SketchValidationRow {
  return {
    id: 'sketchContract',
    label: 'Sketch contract',
    status: row.status,
    detail: row.evidence,
  };
}

function previewArtifactRow(row: { status: SketchValidationStatus; evidence: string }): SketchValidationRow {
  return {
    id: 'previewArtifact',
    label: 'Preview artifact',
    status: row.status,
    detail: row.evidence,
  };
}

function sourceFitCheckRow(input: SketchValidationLedgerInput): SketchValidationRow {
  const closedStroke = latestClosedStroke(input.strokes);
  if (!closedStroke) {
    return {
      id: 'sourceFitCheck',
      label: 'Source fit check',
      status: 'pending',
      detail: 'Waiting for closed source profile.',
    };
  }

  const seed = buildSketchFitValidationSeed({
    profilePoints: closedStroke.points.map(([x, y]) => ({ x, y })),
    view: { width: 100, height: 100 },
    extrudeDepth: input.extrudeDepth ?? 12,
    artifactEvidence: {
      ...(input.artifactBundle?.previewStlPath ? { previewArtifactPath: input.artifactBundle.previewStlPath } : {}),
      ...(input.draft?.source ? { source: input.draft.source } : {}),
    },
    ...(input.errorText ? { backendError: input.errorText } : {}),
  });

  if (seed.status === 'fail') {
    const failedMessages = seed.rows.filter((row) => row.status === 'fail').map((row) => row.message);
    return {
      id: 'sourceFitCheck',
      label: 'Source fit check',
      status: 'fail',
      detail: failedMessages.join('; ') || 'Source fit check failed.',
    };
  }

  if (seed.status === 'pending') {
    return {
      id: 'sourceFitCheck',
      label: 'Source fit check',
      status: 'pending',
      detail: 'Containment checked; tolerance checked; waiting for preview artifact.',
    };
  }

  return {
    id: 'sourceFitCheck',
    label: 'Source fit check',
    status: 'pass',
    detail: 'Containment pass; tolerance pass; preview artifact pass.',
  };
}

function latestClosedStroke(strokes: SketchStroke[]): SketchStroke | null {
  for (let index = strokes.length - 1; index >= 0; index -= 1) {
    const stroke = strokes[index];
    if (stroke?.closed) return stroke;
  }
  return null;
}

function constraintSolverRows(strokes: SketchStroke[]): SketchValidationRow[] {
  const lockedStroke = latestLockedClosedStroke(strokes);
  if (!lockedStroke) return [];

  try {
    const bounds = closedStrokeBounds(lockedStroke);
    const details = ['locked-axis translation'];
    if (lockedStroke.dimensionLocks?.width) details.push(`width ${formatNumber(bounds.width)}mm`);
    if (lockedStroke.dimensionLocks?.height) details.push(`height ${formatNumber(bounds.height)}mm`);

    return [
      {
        id: 'constraintSolver',
        label: 'Constraint solver',
        status: 'pass',
        detail: `${details.join('; ')}.`,
      },
    ];
  } catch (error) {
    return [
      {
        id: 'constraintSolver',
        label: 'Constraint solver',
        status: 'fail',
        detail: error instanceof Error ? error.message : String(error),
      },
    ];
  }
}

function constraintValueRows(strokes: SketchStroke[]): SketchValidationRow[] {
  const lockedStroke = latestLockedClosedStroke(strokes);
  if (!lockedStroke) return [];

  try {
    const bounds = closedStrokeBounds(lockedStroke);
    const details: string[] = [];
    if (lockedStroke.dimensionLocks?.width) details.push(`width ${formatNumber(bounds.width)}mm`);
    if (lockedStroke.dimensionLocks?.height) details.push(`height ${formatNumber(bounds.height)}mm`);

    return [
      {
        id: 'constraintValues',
        label: 'Constraint values',
        status: 'pass',
        detail: `${details.join('; ')}.`,
      },
    ];
  } catch (error) {
    return [
      {
        id: 'constraintValues',
        label: 'Constraint values',
        status: 'fail',
        detail: error instanceof Error ? error.message : String(error),
      },
    ];
  }
}

function latestLockedClosedStroke(strokes: SketchStroke[]): SketchStroke | null {
  for (let index = strokes.length - 1; index >= 0; index -= 1) {
    const stroke = strokes[index];
    if (!stroke?.closed) continue;
    if (stroke.dimensionLocks?.width || stroke.dimensionLocks?.height) return stroke;
  }
  return null;
}

function closedProfileRow(hasClosedProfile: boolean, closedStrokeCount: number): SketchValidationRow {
  if (!hasClosedProfile) {
    return {
      id: 'closedProfile',
      label: 'Closed profile',
      status: 'fail',
      detail: 'Close profile before preview.',
    };
  }

  return {
    id: 'closedProfile',
    label: 'Closed profile',
    status: 'pass',
    detail: `${formatCount(closedStrokeCount, 'closed stroke')}.`,
  };
}

function sourceRow(
  hasClosedProfile: boolean,
  hasSourceDraft: boolean,
  hasBackendError: boolean,
  input: SketchValidationLedgerInput,
): SketchValidationRow {
  if (!hasClosedProfile) {
    return {
      id: 'source',
      label: 'Source generated',
      status: 'pending',
      detail: 'Waiting for closed profile.',
    };
  }

  if (hasBackendError) {
    return {
      id: 'source',
      label: 'Source generated',
      status: 'fail',
      detail: input.errorText,
    };
  }

  if (!hasSourceDraft) {
    return {
      id: 'source',
      label: 'Source generated',
      status: 'pending',
      detail: 'Waiting for source draft.',
    };
  }

  return {
    id: 'source',
    label: 'Source generated',
    status: 'pass',
    detail: `${formatCount(sourceLineCount(input.draft?.source ?? ''), 'source line')}.`,
  };
}

function meshRow(
  hasClosedProfile: boolean,
  hasSourceDraft: boolean,
  hasMeshPreview: boolean,
  hasBackendError: boolean,
  input: SketchValidationLedgerInput,
): SketchValidationRow {
  if (hasBackendError && hasClosedProfile) {
    return {
      id: 'mesh',
      label: 'Mesh preview',
      status: 'fail',
      detail: input.errorText,
    };
  }

  if (!hasSourceDraft) {
    return {
      id: 'mesh',
      label: 'Mesh preview',
      status: 'pending',
      detail: 'Waiting for source draft.',
    };
  }

  if (!hasMeshPreview) {
    return {
      id: 'mesh',
      label: 'Mesh preview',
      status: 'pending',
      detail: 'Waiting for mesh preview.',
    };
  }

  return {
    id: 'mesh',
    label: 'Mesh preview',
    status: 'pass',
    detail: `${basename(input.artifactBundle?.previewStlPath ?? '')} with ${formatCount(input.artifactBundle?.viewerAssets?.length ?? 0, 'viewer asset')}.`,
  };
}

function projectionsRow(hasMeshPreview: boolean, projectionsCount: number): SketchValidationRow {
  if (!hasMeshPreview) {
    return {
      id: 'projections',
      label: 'Projections check',
      status: 'pending',
      detail: 'Waiting for mesh preview.',
    };
  }

  if (projectionsCount < 3) {
    return {
      id: 'projections',
      label: 'Projections check',
      status: 'pending',
      detail: 'Waiting for 3 projection views.',
    };
  }

  return {
    id: 'projections',
    label: 'Projections check',
    status: 'pass',
    detail: `${formatCount(projectionsCount, 'projection view')}.`,
  };
}

function formatCount(count: number, noun: string): string {
  return `${count} ${count === 1 ? noun : `${noun}s`}`;
}

function formatNumber(value: number): string {
  if (Number.isInteger(value)) return String(value);
  return value.toFixed(2).replace(/\.?0+$/, '');
}
