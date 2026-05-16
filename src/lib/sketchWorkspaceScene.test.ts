import assert from 'node:assert/strict';
import test from 'node:test';

import { buildSketchWorkspaceSceneState, workspaceSceneActionLabel } from './sketchWorkspaceScene';

test('buildSketchWorkspaceSceneState reports pending shared scene before any sketch exists', () => {
  const state = buildSketchWorkspaceSceneState({
    currentSceneSignature: null,
    draftSceneSignature: null,
    exactSceneSignature: null,
    hasSketch: false,
    hasDraft: false,
    hasAcceptedExact: false,
    hasRebuildableExact: false,
    exactCandidateSolutionId: null,
    draftErrorText: '',
    exactErrorText: '',
    activeLens: 'sketch',
  });

  assert.equal(state.activeLens, 'sketch');
  assert.deepEqual(
    state.rows.map((row) => [row.label, row.status]),
    [
      ['SketchIntent', 'pending'],
      ['MeshDraft', 'pending'],
      ['ExactModel', 'pending'],
    ],
  );
});

test('buildSketchWorkspaceSceneState reports committed exact model when current scene matches accepted exact', () => {
  const state = buildSketchWorkspaceSceneState({
    currentSceneSignature: 'scene:v1',
    draftSceneSignature: 'scene:v1',
    exactSceneSignature: 'scene:v1',
    hasSketch: true,
    hasDraft: true,
    hasAcceptedExact: true,
    hasRebuildableExact: true,
    exactCandidateSolutionId: 'solution0',
    draftErrorText: '',
    exactErrorText: '',
    activeLens: 'exact',
  });

  assert.equal(state.activeLens, 'exact');
  assert.deepEqual(
    state.rows.map((row) => [row.label, row.status]),
    [
      ['SketchIntent', 'fresh'],
      ['MeshDraft', 'fresh'],
      ['ExactModel', 'committed'],
    ],
  );
});

test('buildSketchWorkspaceSceneState reports stale exact model after sketch edit but fresh rerun draft', () => {
  const state = buildSketchWorkspaceSceneState({
    currentSceneSignature: 'scene:v2',
    draftSceneSignature: 'scene:v2',
    exactSceneSignature: 'scene:v1',
    hasSketch: true,
    hasDraft: true,
    hasAcceptedExact: true,
    hasRebuildableExact: true,
    exactCandidateSolutionId: 'solution1',
    draftErrorText: '',
    exactErrorText: '',
    activeLens: 'sketch',
  });

  assert.equal(state.activeLens, 'sketch');
  assert.deepEqual(
    state.rows.map((row) => [row.label, row.status]),
    [
      ['SketchIntent', 'fresh'],
      ['MeshDraft', 'fresh'],
      ['ExactModel', 'stale'],
    ],
  );
  assert.deepEqual(state.rows.find((row) => row.key === 'exact')?.action, {
    kind: 'rebuildExact',
    label: 'REBUILD EXACT',
    solutionId: 'solution1',
  });
});

test('buildSketchWorkspaceSceneState reports exact accept action when single rebuildable candidate is ready', () => {
  const state = buildSketchWorkspaceSceneState({
    currentSceneSignature: 'scene:v1',
    draftSceneSignature: 'scene:v1',
    exactSceneSignature: null,
    hasSketch: true,
    hasDraft: true,
    hasAcceptedExact: false,
    hasRebuildableExact: true,
    exactCandidateSolutionId: 'solution0',
    draftErrorText: '',
    exactErrorText: '',
    activeLens: 'sketch',
  });

  assert.deepEqual(state.rows.find((row) => row.key === 'exact')?.action, {
    kind: 'acceptExact',
    label: 'ACCEPT EXACT',
    solutionId: 'solution0',
  });
});

test('buildSketchWorkspaceSceneState reports mesh refresh action when draft is fresh', () => {
  const state = buildSketchWorkspaceSceneState({
    currentSceneSignature: 'scene:v1',
    draftSceneSignature: 'scene:v1',
    exactSceneSignature: null,
    hasSketch: true,
    hasDraft: true,
    hasAcceptedExact: false,
    hasRebuildableExact: false,
    exactCandidateSolutionId: null,
    draftErrorText: '',
    exactErrorText: '',
    activeLens: 'draft',
  });

  assert.deepEqual(state.rows.find((row) => row.key === 'draft')?.action, {
    kind: 'previewDraft',
    label: 'REFRESH DRAFT',
  });
});

test('buildSketchWorkspaceSceneState reports mesh preview action when sketch exists but no draft exists yet', () => {
  const state = buildSketchWorkspaceSceneState({
    currentSceneSignature: 'scene:v1',
    draftSceneSignature: null,
    exactSceneSignature: null,
    hasSketch: true,
    hasDraft: false,
    hasAcceptedExact: false,
    hasRebuildableExact: false,
    exactCandidateSolutionId: null,
    draftErrorText: '',
    exactErrorText: '',
    activeLens: 'sketch',
  });

  assert.deepEqual(state.rows.find((row) => row.key === 'draft')?.action, {
    kind: 'previewDraft',
    label: 'BUILD DRAFT',
  });
});

test('workspaceSceneActionLabel reports BUILDING while draft preview is running', () => {
  assert.equal(
    workspaceSceneActionLabel(
      {
        kind: 'previewDraft',
        label: 'REFRESH DRAFT',
      },
      {
        generating: true,
        acceptingSolutionId: null,
      },
    ),
    'BUILDING...',
  );
});

test('workspaceSceneActionLabel reports ACCEPTING for active exact accept', () => {
  assert.equal(
    workspaceSceneActionLabel(
      {
        kind: 'acceptExact',
        label: 'ACCEPT EXACT',
        solutionId: 'solution0',
      },
      {
        generating: false,
        acceptingSolutionId: 'solution0',
      },
    ),
    'ACCEPTING...',
  );
});
