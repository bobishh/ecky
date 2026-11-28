import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import ParamPanel from '../../src/lib/ParamPanel.svelte';
import { session } from '../../src/lib/stores/sessionStore';
import type { ArtifactBundle, ModelManifest, UiSpec } from '../../src/lib/types/domain';

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

function multiPartManifest(): ModelManifest {
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
        viewerAssetPath: 'parts/001-bulb_left.stl',
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
        viewerAssetPath: 'parts/002-stand.stl',
        viewerNodeIds: ['stand'],
        parameterKeys: ['height'],
        editable: true,
        bounds: null,
        volume: null,
        area: null,
      },
    ],
    assemblyTree: [
      {
        id: 'part:bulb_left',
        name: 'Left Bulb',
        partKey: 'bulb_left',
        bounds: null,
        parameterKeys: ['radius'],
        children: [],
      },
      {
        id: 'part:stand',
        name: 'Stand',
        partKey: 'stand',
        bounds: null,
        parameterKeys: ['height'],
        children: [],
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

describe('ParamPanel Assembly Tab', () => {
  it('Given a multi-part model When viewing PARAMETERS Then assembly tree is in its own ASSEMBLY tab and not inline', async () => {
    const manifest = multiPartManifest();
    session.setModelRuntime(sampleBundle(), manifest);
    const onSelectPart = vi.fn();
    const onToggleIsolatePart = vi.fn();
    const view = render(ParamPanel, {
      props: {
        uiSpec,
        parameters: { radius: 5, height: 12 },
        modelManifest: manifest,
        onSelectPart,
        onToggleIsolatePart,
      },
    });

    expect(view.queryByLabelText('Assembly tree')).toBeNull();
    expect(view.getByText('Radius')).toBeTruthy();

    const assemblyTab = view.getByRole('button', { name: 'ASSEMBLY' });
    await fireEvent.click(assemblyTab);

    expect(assemblyTab.getAttribute('aria-pressed')).toBe('true');
    expect(view.getByLabelText('Assembly tree')).toBeTruthy();
    expect(view.queryByText('Radius')).toBeNull();

    await fireEvent.click(view.getByText('left bulb'));
    expect(onSelectPart).toHaveBeenCalledWith('bulb_left');

    await fireEvent.click(view.getByRole('button', { name: 'Isolate Left Bulb' }));
    expect(onToggleIsolatePart).toHaveBeenCalledWith('bulb_left');
  });

  it('Given an isolated part When switching back to PARAMETERS Then isolated scope bar filters parameters', async () => {
    const manifest = multiPartManifest();
    session.setModelRuntime(sampleBundle(), manifest);
    const view = render(ParamPanel, {
      props: {
        uiSpec,
        parameters: { radius: 5, height: 12 },
        modelManifest: manifest,
        isolatedPartId: 'bulb_left',
      },
    });

    expect(view.queryByLabelText('Assembly tree')).toBeNull();
    expect(view.getByText(/SCOPED: left bulb \(1\/2\)/i)).toBeTruthy();
    expect(view.getByText('Radius')).toBeTruthy();
    expect(view.queryByText('Height')).toBeNull();

    await fireEvent.click(view.getByRole('button', { name: 'SHOW ALL' }));
    expect(view.getByText('Height')).toBeTruthy();
  });

  it('Given ASSEMBLY tab is active When model becomes single-part without isolation Then panel falls back to PARAMETERS', async () => {
    const manifest = multiPartManifest();
    const singlePartManifest = {
      ...manifest,
      parts: [manifest.parts[0]],
      assemblyTree: [manifest.assemblyTree![0]],
    };
    session.setModelRuntime(sampleBundle(), manifest);
    const view = render(ParamPanel, {
      props: {
        uiSpec,
        parameters: { radius: 5, height: 12 },
        modelManifest: manifest,
      },
    });

    await fireEvent.click(view.getByRole('button', { name: 'ASSEMBLY' }));
    expect(view.getByLabelText('Assembly tree')).toBeTruthy();

    session.setModelRuntime(sampleBundle(), singlePartManifest);
    await view.rerender({
      uiSpec,
      parameters: { radius: 5, height: 12 },
      modelManifest: singlePartManifest,
    });

    expect(view.queryByRole('button', { name: 'ASSEMBLY' })).toBeNull();
    expect(view.queryByLabelText('Assembly tree')).toBeNull();
    expect(view.getByText('Radius')).toBeTruthy();
  });
});
