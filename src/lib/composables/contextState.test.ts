import assert from 'node:assert/strict';
import test from 'node:test';

import { deriveContextState } from './contextState';
import type { DesignParams, ModelManifest, UiSpec } from '../types/domain';

function manifest(): ModelManifest {
  return {
    schemaVersion: 2,
    modelId: 'model-1',
    sourceKind: 'generated',
    document: {
      documentName: 'Widget',
      documentLabel: 'Widget',
      sourcePath: null,
      objectCount: 1,
      warnings: [],
    },
    parts: [
      {
        partId: 'body',
        freecadObjectName: 'Body',
        label: 'Body',
        kind: 'Part::Feature',
        semanticRole: 'body',
        viewerAssetPath: '/tmp/body.stl',
        viewerNodeIds: ['body-node'],
        parameterKeys: ['body_height'],
        editable: true,
        bounds: null,
        volume: null,
        area: null,
      },
    ],
    parameterGroups: [],
    controlPrimitives: [
      {
        primitiveId: 'body-height',
        label: 'Body Height',
        kind: 'number',
        source: 'generated',
        partIds: ['body'],
        bindings: [{ parameterKey: 'body_height', scale: 1, offset: 0, min: null, max: null }],
        editable: true,
        order: 0,
      },
    ],
    controlRelations: [],
    controlViews: [
      {
        viewId: 'view-body',
        label: 'Body',
        scope: 'part',
        partIds: ['body'],
        primitiveIds: ['body-height'],
        sections: [
          {
            sectionId: 'body-section',
            label: 'Body',
            primitiveIds: ['body-height'],
            collapsed: false,
          },
        ],
        default: true,
        source: 'generated',
        status: 'accepted',
        order: 0,
      },
    ],
    advisories: [],
    selectionTargets: [],
    measurementAnnotations: [],
    warnings: [],
    enrichmentState: { status: 'none', proposals: [] },
  };
}

test('deriveContextState resolves imported context and semantic overlays from a manifest', () => {
  const uiSpec: UiSpec = {
    fields: [
      {
        type: 'number',
        key: 'body_height',
        label: 'Body Height',
        frozen: false,
      },
    ],
  };
  const params: DesignParams = { body_height: 12 };

  const state = deriveContextState({
    sessionModelManifest: manifest(),
    activeArtifactBundle: null,
    paramUiSpec: uiSpec,
    paramValues: params,
    selectedContextTargetId: null,
    selectedPartId: 'body',
    activeControlViewId: 'view-body',
    focusedMeasurementControl: null,
  });

  assert.equal(state.activeModelManifest?.modelId, 'model-1');
  assert.equal(state.selectedTarget?.partId, 'body');
  assert.equal(state.selectedPartId, 'body');
  assert.equal(state.resolvedActiveControlViewId, 'view-body');
  assert.equal(state.activeControlView?.viewId, 'view-body');
  assert.equal(state.overlayControls.length, 1);
  assert.equal(state.overlayAdvisories.length, 0);
});
