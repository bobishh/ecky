export type SketchWorkspaceLens = 'sketch' | 'draft' | 'exact';

export type SketchWorkspaceSceneStatus = 'pending' | 'fresh' | 'stale' | 'rebuildable' | 'failed' | 'committed';

export type SketchWorkspaceSceneAction =
  | {
      kind: 'previewDraft';
      label: 'BUILD DRAFT' | 'REFRESH DRAFT';
    }
  | {
      kind: 'acceptExact' | 'rebuildExact';
      label: 'ACCEPT EXACT' | 'REBUILD EXACT';
      solutionId: string;
    };

export type SketchWorkspaceSceneRow = {
  key: 'sketch' | 'draft' | 'exact';
  label: 'SketchIntent' | 'MeshDraft' | 'ExactModel';
  status: SketchWorkspaceSceneStatus;
  detail: string;
  action?: SketchWorkspaceSceneAction;
};

export type SketchWorkspaceSceneState = {
  activeLens: SketchWorkspaceLens;
  rows: SketchWorkspaceSceneRow[];
};

type SceneActionPresentationInput = {
  generating: boolean;
  acceptingSolutionId: string | null;
};

type BuildSceneStateInput = {
  currentSceneSignature: string | null;
  draftSceneSignature: string | null;
  exactSceneSignature: string | null;
  hasSketch: boolean;
  hasDraft: boolean;
  hasAcceptedExact: boolean;
  hasRebuildableExact: boolean;
  exactCandidateSolutionId: string | null;
  draftErrorText: string;
  exactErrorText: string;
  activeLens: SketchWorkspaceLens;
};

export function buildSketchWorkspaceSceneState(input: BuildSceneStateInput): SketchWorkspaceSceneState {
  const hasCurrentScene = Boolean(input.currentSceneSignature);
  const draftMatches = hasCurrentScene && input.draftSceneSignature === input.currentSceneSignature;
  const exactMatches = hasCurrentScene && input.exactSceneSignature === input.currentSceneSignature;

  const sketchRow: SketchWorkspaceSceneRow = {
    key: 'sketch',
    label: 'SketchIntent',
    status: input.hasSketch ? 'fresh' : 'pending',
    detail: input.hasSketch ? 'Structured sketch intent ready.' : 'Waiting for sketch intent.',
  };

  const draftStatus: SketchWorkspaceSceneStatus = input.draftErrorText
    ? 'failed'
    : !input.hasDraft
      ? 'pending'
      : draftMatches
        ? 'fresh'
        : 'stale';

  const draftRow: SketchWorkspaceSceneRow = {
    key: 'draft',
    label: 'MeshDraft',
    status: draftStatus,
    detail:
      draftStatus === 'failed'
        ? input.draftErrorText
        : draftStatus === 'fresh'
          ? 'Draft mesh matches current sketch intent.'
        : draftStatus === 'stale'
            ? 'Draft mesh is behind current sketch intent.'
            : 'Waiting for draft mesh.',
    ...(input.hasSketch
      ? draftStatus === 'pending' || draftStatus === 'failed' || draftStatus === 'stale'
        ? {
            action: {
              kind: 'previewDraft' as const,
              label: 'BUILD DRAFT' as const,
            },
          }
        : draftStatus === 'fresh'
          ? {
              action: {
                kind: 'previewDraft' as const,
                label: 'REFRESH DRAFT' as const,
              },
            }
          : {}
      : {}),
  };

  const exactStatus: SketchWorkspaceSceneStatus = input.exactErrorText
    ? 'failed'
    : input.hasAcceptedExact
      ? exactMatches
        ? 'committed'
        : 'stale'
      : input.hasRebuildableExact
        ? 'rebuildable'
        : 'pending';

  const exactRow: SketchWorkspaceSceneRow = {
    key: 'exact',
    label: 'ExactModel',
    status: exactStatus,
    detail:
      exactStatus === 'failed'
        ? input.exactErrorText
        : exactStatus === 'committed'
          ? 'Accepted exact model matches current scene.'
          : exactStatus === 'stale'
            ? 'Accepted exact model is behind current scene.'
            : exactStatus === 'rebuildable'
              ? 'Exact rebuild candidate is ready.'
              : 'Waiting for exact rebuild.',
    ...(input.exactCandidateSolutionId
      ? exactStatus === 'rebuildable'
        ? {
            action: {
              kind: 'acceptExact' as const,
              label: 'ACCEPT EXACT' as const,
              solutionId: input.exactCandidateSolutionId,
            },
          }
        : exactStatus === 'stale'
          ? {
              action: {
                kind: 'rebuildExact' as const,
                label: 'REBUILD EXACT' as const,
                solutionId: input.exactCandidateSolutionId,
              },
            }
          : {}
      : {}),
  };

  return {
    activeLens: input.activeLens,
    rows: [sketchRow, draftRow, exactRow],
  };
}

export function workspaceSceneActionLabel(
  action: SketchWorkspaceSceneAction,
  input: SceneActionPresentationInput,
): SketchWorkspaceSceneAction['label'] | 'BUILDING...' | 'ACCEPTING...' | 'REBUILDING...' {
  if (action.kind === 'previewDraft') {
    return input.generating ? 'BUILDING...' : action.label;
  }
  if (input.acceptingSolutionId === action.solutionId) {
    return action.kind === 'rebuildExact' ? 'REBUILDING...' : 'ACCEPTING...';
  }
  return action.label;
}

export function sceneSignatureFromStrokes(strokes: SketchStroke[]): string | null {
  if (!strokes.length) return null;
  return JSON.stringify(
    strokes.map((stroke) => ({
      primitiveId: stroke.primitiveId,
      sketchId: stroke.sketchId,
      view: stroke.view,
      closed: stroke.closed,
      kind: stroke.kind,
      points: stroke.points,
      dimensionLocks: stroke.dimensionLocks ?? null,
    })),
  );
}
import type { SketchStroke } from './sketchWorkspaceState';
