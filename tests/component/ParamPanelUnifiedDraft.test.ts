import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import ParamPanel from '../../src/lib/ParamPanel.svelte';
import { session } from '../../src/lib/stores/sessionStore';
import type { ArtifactBundle, DesignParams, ModelManifest, UiSpec } from '../../src/lib/types/domain';

vi.mock('../../src/lib/tauri/client', async () => {
  const actual = await vi.importActual<typeof import('../../src/lib/tauri/client')>(
    '../../src/lib/tauri/client',
  );
  return {
    ...actual,
    parseMacroParams: vi.fn(async () => ({ fields: [], params: {} })),
  };
});

afterEach(() => {
  session.clearModelRuntime();
});

function sampleBundle(): ArtifactBundle {
  return {
    schemaVersion: 1,
    modelId: 'model-1',
    sourceKind: 'generated',
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'mesh',
    contentHash: 'hash-1',
    artifactVersion: 1,
    fcstdPath: '',
    manifestPath: '/tmp/manifest.json',
    macroPath: '/tmp/model.ecky',
    previewStlPath: '/tmp/model.stl',
    modelStlPath: '/tmp/model.stl',
    viewerAssets: [],
    exportArtifacts: [],
    productionPending: false,
  };
}

function sampleManifest(): ModelManifest {
  return {
    schemaVersion: 1,
    modelId: 'model-1',
    sourceKind: 'generated',
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'mesh',
    document: {
      documentName: 'lamp',
      documentLabel: 'Lamp',
      sourcePath: 'model.ecky',
      objectCount: 2,
      warnings: [],
    },
    parts: [
      {
        partId: 'bulb_left',
        freecadObjectName: 'bulb_left',
        label: 'Left Bulb',
        kind: 'solid',
        semanticRole: 'generated',
        viewerAssetPath: 'parts/bulb_left.stl',
        viewerNodeIds: ['bulb_left'],
        parameterKeys: ['radius'],
        editable: true,
        bounds: null,
        volume: null,
        area: null,
      },
      {
        partId: 'stand',
        freecadObjectName: 'stand',
        label: 'Stand',
        kind: 'solid',
        semanticRole: 'generated',
        viewerAssetPath: 'parts/stand.stl',
        viewerNodeIds: ['stand'],
        parameterKeys: ['height'],
        editable: true,
        bounds: null,
        volume: null,
        area: null,
      },
    ],
    parameterGroups: [],
    controlPrimitives: [],
    controlRelations: [],
    controlViews: [],
    advisories: [],
    selectionTargets: [],
    measurementAnnotations: [],
    warnings: [],
    enrichmentProposals: [],
    enrichmentState: { status: 'none', proposals: [] },
  };
}

const uiSpec: UiSpec = {
  fields: [
    { type: 'number', key: 'radius', label: 'Radius', frozen: false },
    { type: 'number', key: 'height', label: 'Height', frozen: false },
  ],
};

describe('ParamPanel Unified Draft Sync', () => {
  it('Given panel edits height When viewport merges radius into externalDraftParams Then both edits persist and Cancel clears the shared draft', async () => {
    const manifest = sampleManifest();
    session.setModelRuntime(sampleBundle(), manifest);
    let sharedDraft: DesignParams | null = null;
    const onExternalDraftChange = vi.fn((next: DesignParams | null) => {
      sharedDraft = next;
    });

    const view = render(ParamPanel, {
      props: {
        uiSpec,
        parameters: { radius: 5, height: 10 },
        externalDraftParams: sharedDraft,
        onExternalDraftChange,
        showParameterGhostControls: true,
        modelManifest: manifest,
      },
    });

    // Panel edits height -> pushes to shared draft
    const heightInput = view.getByLabelText(/Height/i) as HTMLInputElement;
    await fireEvent.input(heightInput, { target: { value: '14' } });
    expect(sharedDraft).toEqual({ radius: 5, height: 14 });

    // Viewport overlay merges radius: 8 onto the same shared draft
    sharedDraft = { radius: 8, height: 14 };
    await view.rerender({
      uiSpec,
      parameters: { radius: 5, height: 10 },
      externalDraftParams: sharedDraft,
      onExternalDraftChange,
      showParameterGhostControls: true,
      modelManifest: manifest,
    });

    const radiusInput = view.getByLabelText(/Radius/i) as HTMLInputElement;
    expect(radiusInput.value).toBe('8');
    expect((view.getByLabelText(/Height/i) as HTMLInputElement).value).toBe('14');

    // Cancel clears shared draft back to saved values
    await fireEvent.click(view.getByRole('button', { name: 'CANCEL' }));
    expect(sharedDraft).toBeNull();
    await view.rerender({
      uiSpec,
      parameters: { radius: 5, height: 10 },
      externalDraftParams: sharedDraft,
      onExternalDraftChange,
      showParameterGhostControls: true,
      modelManifest: manifest,
    });
    expect((view.getByLabelText(/Radius/i) as HTMLInputElement).value).toBe('5');
    expect((view.getByLabelText(/Height/i) as HTMLInputElement).value).toBe('10');
  });
});
