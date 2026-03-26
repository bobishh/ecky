import assert from 'node:assert/strict';
import test from 'node:test';

import type {
  Advisory,
  ArtifactBundle,
  MeasurementAnnotation,
  ModelManifest,
  SelectionTarget,
} from '../types/domain';
import type { MaterializedSemanticControl, MaterializedSemanticView } from './semanticControls';
import {
  buildContextSelectionTargets,
  filterFieldsBySearch,
  pickContextAdvisories,
  pickContextControls,
  resolveMeasurementCallout,
  resolveActiveContextViewId,
  resolveContextSections,
  resolveTargetParameterKeys,
  resolveViewerNodeTarget,
  shouldDisplayViewportControlList,
  type ContextSelectionTarget,
} from './contextualEditing';

function selectionTarget(target: Partial<SelectionTarget> & Pick<SelectionTarget, 'partId' | 'viewerNodeId' | 'label' | 'kind' | 'editable'>): SelectionTarget {
  return {
    targetId: null,
    parameterKeys: [],
    primitiveIds: [],
    viewIds: [],
    ...target,
  };
}

function manifest(overrides: Partial<ModelManifest> = {}): ModelManifest {
  return {
    schemaVersion: 2,
    modelId: 'model-1',
    sourceKind: 'generated',
    document: {
      documentName: 'Vessel',
      documentLabel: 'Vessel',
      sourcePath: null,
      objectCount: 1,
      warnings: [],
    },
    parts: [
      {
        partId: 'body',
        freecadObjectName: 'Body001',
        label: 'Body',
        kind: 'Part::Feature',
        semanticRole: 'body',
        viewerAssetPath: '/tmp/body.stl',
        viewerNodeIds: ['Body001'],
        parameterKeys: ['body_height', 'wall_thickness'],
        editable: true,
        bounds: null,
        volume: null,
        area: null,
      },
      {
        partId: 'rim',
        freecadObjectName: 'Rim001',
        label: 'Rim',
        kind: 'Part::Feature',
        semanticRole: 'ornament',
        viewerAssetPath: '/tmp/rim.stl',
        viewerNodeIds: ['Rim001'],
        parameterKeys: ['rim_diameter'],
        editable: true,
        bounds: null,
        volume: null,
        area: null,
      },
    ],
    parameterGroups: [
      {
        groupId: 'group-body',
        label: 'Body',
        parameterKeys: ['body_height', 'wall_thickness'],
        partIds: ['body'],
        editable: true,
        presentation: 'primary',
        order: 0,
      },
    ],
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
      {
        primitiveId: 'wall-thickness',
        label: 'Wall Thickness',
        kind: 'number',
        source: 'generated',
        partIds: ['body'],
        bindings: [{ parameterKey: 'wall_thickness', scale: 1, offset: 0, min: null, max: null }],
        editable: true,
        order: 1,
      },
      {
        primitiveId: 'global-finish',
        label: 'Global Finish',
        kind: 'choice',
        source: 'generated',
        partIds: [],
        bindings: [{ parameterKey: 'surface_finish', scale: 1, offset: 0, min: null, max: null }],
        editable: true,
        order: 2,
      },
    ],
    controlRelations: [],
    controlViews: [
      {
        viewId: 'view-body',
        label: 'Body',
        scope: 'part',
        partIds: ['body'],
        primitiveIds: ['body-height', 'wall-thickness'],
        sections: [
          {
            sectionId: 'body-primary',
            label: 'Primary',
            primitiveIds: ['body-height', 'wall-thickness'],
            collapsed: false,
          },
        ],
        default: false,
        source: 'generated',
        status: 'accepted',
        order: 1,
      },
      {
        viewId: 'view-model',
        label: 'Model',
        scope: 'global',
        partIds: [],
        primitiveIds: ['global-finish'],
        sections: [
          {
            sectionId: 'global-primary',
            label: 'Primary',
            primitiveIds: ['global-finish'],
            collapsed: false,
          },
        ],
        default: true,
        source: 'generated',
        status: 'accepted',
        order: 0,
      },
    ],
    advisories: [
      {
        advisoryId: 'body-note',
        label: 'Body note',
        severity: 'info',
        primitiveIds: ['body-height'],
        viewIds: [],
        message: 'Body height drives the silhouette.',
        condition: 'always',
        threshold: null,
      },
    ],
    selectionTargets: [],
    measurementAnnotations: [],
    warnings: [],
    enrichmentState: { status: 'none', proposals: [] },
    ...overrides,
  };
}

function bundle(overrides: Partial<ArtifactBundle> = {}): ArtifactBundle {
  return {
    schemaVersion: 2,
    modelId: 'model-1',
    sourceKind: 'generated',
    contentHash: 'hash-1',
    artifactVersion: 1,
    fcstdPath: '/tmp/model.FCStd',
    manifestPath: '/tmp/model.json',
    macroPath: '/tmp/model.py',
    previewStlPath: '/tmp/model.stl',
    viewerAssets: [],
    edgeTargets: [],
    calloutAnchors: [],
    measurementGuides: [],
    ...overrides,
  };
}

function control(
  primitiveId: string,
  label = primitiveId,
  partIds: string[] = [],
  parameterKey = primitiveId,
): MaterializedSemanticControl {
  return {
    primitiveId,
    label,
    kind: 'number',
    source: 'generated',
    editable: true,
    partIds,
    order: 0,
    rawField: {
      type: 'number',
      key: parameterKey,
      label,
      frozen: false,
    },
    bindings: [{ parameterKey, scale: 1, offset: 0, min: null, max: null }],
    value: 0,
  };
}

function view(sections: MaterializedSemanticView['sections'], advisories = [] as MaterializedSemanticView['advisories']): MaterializedSemanticView {
  return {
    viewId: 'view-main',
    label: 'Main',
    scope: 'global',
    partIds: [],
    isDefault: true,
    source: 'generated',
    status: 'none',
    order: 0,
    sections,
    advisories,
  };
}

test('buildContextSelectionTargets preserves exact scoping metadata and synthesizes missing part targets', () => {
  const targets = buildContextSelectionTargets(
    manifest({
      selectionTargets: [
        selectionTarget({
          targetId: 'target-body-object',
          partId: 'body',
          viewerNodeId: 'Body001',
          label: 'Body Object',
          kind: 'object',
          editable: true,
          primitiveIds: ['body-height'],
          parameterKeys: ['body_height'],
          viewIds: ['view-body'],
        }),
      ],
    }),
  );

  assert.deepEqual(
    targets.map((target) => target.targetId),
    ['target-body-object', 'part:body', 'part:rim'],
  );
  assert.deepEqual(targets[0].primitiveIds, ['body-height']);
});

test('resolveViewerNodeTarget prefers exact object targets over broader part targets', () => {
  const targets = buildContextSelectionTargets(
    manifest({
      selectionTargets: [
        selectionTarget({
          targetId: 'target-body-part',
          partId: 'body',
          viewerNodeId: 'Body001',
          label: 'Body',
          kind: 'part',
          editable: true,
        }),
        selectionTarget({
          targetId: 'target-body-object',
          partId: 'body',
          viewerNodeId: 'Body001',
          label: 'Body Loft',
          kind: 'object',
          editable: true,
        }),
      ],
    }),
  );

  const resolved = resolveViewerNodeTarget(targets, 'Body001', 'body');
  assert.equal(resolved?.targetId, 'target-body-object');
});

test('resolveMeasurementCallout prefers parameter-key matches over primitive and target matches', () => {
  const manifestValue = manifest({
    selectionTargets: [
      selectionTarget({
        targetId: 'target-body-object',
        partId: 'body',
        viewerNodeId: 'Body001',
        label: 'Body Object',
        kind: 'object',
        editable: true,
        primitiveIds: ['body-height'],
        parameterKeys: ['body_height'],
        viewIds: ['view-body'],
      }),
    ],
    measurementAnnotations: [
      {
        annotationId: 'target-match',
        label: 'Target Match',
        basis: 'outer',
        axis: 'x',
        parameterKeys: [],
        primitiveIds: [],
        targetIds: ['target-body-object'],
        guideId: null,
        explanation: null,
        formulaHint: null,
        source: 'manual',
      } satisfies MeasurementAnnotation,
      {
        annotationId: 'primitive-match',
        label: 'Primitive Match',
        basis: 'inner',
        axis: 'z',
        parameterKeys: [],
        primitiveIds: ['body-height'],
        targetIds: [],
        guideId: null,
        explanation: null,
        formulaHint: null,
        source: 'manual',
      } satisfies MeasurementAnnotation,
      {
        annotationId: 'parameter-match',
        label: 'Outer Height',
        basis: 'outer',
        axis: 'z',
        parameterKeys: ['body_height'],
        primitiveIds: [],
        targetIds: [],
        guideId: null,
        explanation: 'Measures the outer wall height.',
        formulaHint: null,
        source: 'manual',
      } satisfies MeasurementAnnotation,
    ],
  });
  const targets = buildContextSelectionTargets(manifestValue);
  const selectedTarget = targets[0];

  const callout = resolveMeasurementCallout(
    manifestValue,
    bundle(),
    targets,
    { primitiveId: 'body-height', parameterKey: 'body_height' },
    selectedTarget,
  );

  assert.equal(callout?.annotationId, 'parameter-match');
  assert.equal(callout?.badgeLabel, 'Outer Height');
});

test('resolveMeasurementCallout prefers guide-backed annotations within the same rank', () => {
  const manifestValue = manifest({
    measurementAnnotations: [
      {
        annotationId: 'no-guide',
        label: 'Wall Thickness',
        basis: 'wall',
        axis: 'normal',
        parameterKeys: ['wall_thickness'],
        primitiveIds: [],
        targetIds: [],
        guideId: null,
        explanation: null,
        formulaHint: null,
        source: 'manual',
      } satisfies MeasurementAnnotation,
      {
        annotationId: 'with-guide',
        label: 'Wall Thickness Guide',
        basis: 'wall',
        axis: 'normal',
        parameterKeys: ['wall_thickness'],
        primitiveIds: [],
        targetIds: [],
        guideId: 'guide-wall',
        explanation: null,
        formulaHint: null,
        source: 'manual',
      } satisfies MeasurementAnnotation,
    ],
  });

  const callout = resolveMeasurementCallout(
    manifestValue,
    bundle({
      calloutAnchors: [
        { anchorId: 'anchor-a', position: [0, 0, 0], normal: null },
        { anchorId: 'anchor-b', position: [10, 0, 0], normal: null },
      ],
      measurementGuides: [
        {
          guideId: 'guide-wall',
          kind: 'linear',
          anchorIds: ['anchor-a', 'anchor-b'],
          labelAnchorId: null,
          targetIds: [],
        },
      ],
    }),
    buildContextSelectionTargets(manifestValue),
    { primitiveId: null, parameterKey: 'wall_thickness' },
    null,
  );

  assert.equal(callout?.annotationId, 'with-guide');
  assert.equal(callout?.guide?.guideId, 'guide-wall');
  assert.deepEqual(callout?.guide?.points, [
    [0, 0, 0],
    [10, 0, 0],
  ]);
});

test('resolveMeasurementCallout falls back to selected target matching when no focused control exists', () => {
  const manifestValue = manifest({
    selectionTargets: [
      selectionTarget({
        targetId: 'target-body-edge',
        partId: 'body',
        viewerNodeId: 'Body001',
        label: 'Body Edge',
        kind: 'edge',
        editable: true,
        primitiveIds: [],
        parameterKeys: [],
        viewIds: [],
      }),
    ],
    measurementAnnotations: [
      {
        annotationId: 'edge-clearance',
        label: 'Edge Clearance',
        basis: 'clearance',
        axis: 'path',
        parameterKeys: [],
        primitiveIds: [],
        targetIds: ['target-body-edge'],
        guideId: null,
        explanation: null,
        formulaHint: null,
        source: 'manual',
      } satisfies MeasurementAnnotation,
    ],
  });
  const targets = buildContextSelectionTargets(manifestValue);
  const edgeTarget = targets.find((target) => target.targetId === 'target-body-edge') ?? null;

  const callout = resolveMeasurementCallout(
    manifestValue,
    bundle(),
    targets,
    null,
    edgeTarget,
  );

  assert.equal(callout?.annotationId, 'edge-clearance');
  assert.deepEqual(callout?.targetIds, ['target-body-edge']);
});

test('pickContextControls orders exact target controls before part and global controls', () => {
  const target: ContextSelectionTarget = {
    targetId: 'target-edge',
    kind: 'edge',
    partId: 'body',
    label: 'Body Edge',
    editable: true,
    viewerNodeId: 'Body001',
    parameterKeys: [],
    primitiveIds: ['wall-thickness'],
    viewIds: [],
  };
  const result = pickContextControls(
    view([
      {
        sectionId: 'main',
        label: 'Main',
        collapsed: false,
        controls: [
          control('body-height', 'Body Height', ['body'], 'body_height'),
          control('global-finish', 'Global Finish', [], 'surface_finish'),
          control('wall-thickness', 'Wall Thickness', ['body'], 'wall_thickness'),
        ],
      },
    ]),
    target,
  );

  assert.deepEqual(
    result.map((entry) => entry.primitiveId),
    ['wall-thickness', 'body-height', 'global-finish'],
  );
});

test('resolveContextSections applies target scoping and shared search query together', () => {
  const target: ContextSelectionTarget = {
    targetId: 'target-body',
    kind: 'object',
    partId: 'body',
    label: 'Body',
    editable: true,
    viewerNodeId: 'Body001',
    parameterKeys: ['body_height'],
    primitiveIds: [],
    viewIds: [],
  };
  const sections = resolveContextSections(
    view([
      {
        sectionId: 'main',
        label: 'Main',
        collapsed: false,
        controls: [
          control('body-height', 'Body Height', ['body'], 'body_height'),
          control('wall-thickness', 'Wall Thickness', ['body'], 'wall_thickness'),
        ],
      },
    ]),
    target,
    'height',
  );

  assert.equal(sections.length, 1);
  assert.deepEqual(
    sections[0].controls.map((entry) => entry.primitiveId),
    ['body-height'],
  );
});

test('pickContextAdvisories follows currently visible contextual controls', () => {
  const advisory: Advisory = {
    advisoryId: 'body-note',
    label: 'Body note',
    severity: 'info',
    primitiveIds: ['body-height'] as string[],
    viewIds: [] as string[],
    message: 'Body height drives the silhouette.',
    condition: 'always',
    threshold: null,
  };
  const result = pickContextAdvisories(
    view(
      [
        {
          sectionId: 'main',
          label: 'Main',
          collapsed: false,
          controls: [
            control('body-height', 'Body Height', ['body'], 'body_height'),
            control('wall-thickness', 'Wall Thickness', ['body'], 'wall_thickness'),
          ],
        },
      ],
      [advisory],
    ),
    {
      targetId: 'target-body',
      kind: 'object',
      partId: 'body',
      label: 'Body',
      editable: true,
      viewerNodeId: 'Body001',
      parameterKeys: ['body_height'],
      primitiveIds: [],
      viewIds: [],
    },
  );

  assert.deepEqual(result.map((entry) => entry.advisoryId), ['body-note']);
});

test('resolveTargetParameterKeys prefers exact target keys before broader part keys', () => {
  const keys = resolveTargetParameterKeys(
    manifest(),
    {
      targetId: 'target-body',
      kind: 'object',
      partId: 'body',
      label: 'Body',
      editable: true,
      viewerNodeId: 'Body001',
      parameterKeys: ['wall_thickness'],
      primitiveIds: [],
      viewIds: [],
    },
  );

  assert.deepEqual([...keys], ['wall_thickness']);
});

test('resolveActiveContextViewId prefers target-scoped views before global defaults', () => {
  const views = [
    {
      viewId: 'view-model',
      label: 'Model',
      scope: 'global',
      partIds: [],
      isDefault: true,
      source: 'generated',
      status: 'accepted',
      order: 0,
      sections: [],
      advisories: [],
    },
    {
      viewId: 'view-body',
      label: 'Body',
      scope: 'part',
      partIds: ['body'],
      isDefault: false,
      source: 'generated',
      status: 'accepted',
      order: 1,
      sections: [],
      advisories: [],
    },
  ] as MaterializedSemanticView[];

  const result = resolveActiveContextViewId(views, {
    targetId: 'target-body',
    kind: 'object',
    partId: 'body',
    label: 'Body',
    editable: true,
    viewerNodeId: 'Body001',
    parameterKeys: [],
    primitiveIds: [],
    viewIds: [],
  }, null);

  assert.equal(result, 'view-body');
});

test('filterFieldsBySearch matches both parameter key and label', () => {
  const result = filterFieldsBySearch(
    [
      { key: 'wall_thickness', label: 'Wall Thickness' },
      { key: 'body_height', label: 'Body Height' },
    ],
    'thick',
  );

  assert.deepEqual(result.map((entry) => entry.key), ['wall_thickness']);
});

test('shouldDisplayViewportControlList hides the duplicated global control list', () => {
  assert.equal(
    shouldDisplayViewportControlList({
      targetId: 'global',
      kind: 'global',
      partId: null,
      label: 'Model',
      editable: true,
      viewerNodeId: null,
      parameterKeys: [],
      primitiveIds: [],
      viewIds: [],
    }),
    false,
  );

  assert.equal(
    shouldDisplayViewportControlList({
      targetId: 'target-body',
      kind: 'object',
      partId: 'body',
      label: 'Body',
      editable: true,
      viewerNodeId: 'Body001',
      parameterKeys: ['body_height'],
      primitiveIds: [],
      viewIds: [],
    }),
    true,
  );
});
