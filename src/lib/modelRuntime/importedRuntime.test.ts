import assert from 'node:assert/strict';
import test from 'node:test';

import type { ModelManifest } from '../types/domain';
import { buildImportedUiSpec } from './importedRuntime';

test('buildImportedUiSpec defaults imported numeric controls to number inputs', () => {
  const manifest = {
    modelId: 'imported-shell',
    sourceKind: 'importedFcstd',
    document: {
      documentName: 'Imported Shell',
      documentLabel: 'Imported Shell',
    },
    parameterGroups: [
      {
        groupId: 'group-shell',
        label: 'Shell',
        parameterKeys: ['outer_shell_width', 'outer_shell_height'],
        partIds: [],
        editable: true,
      },
    ],
    parts: [
      {
        partId: 'outer-shell',
        freecadObjectName: 'OuterShell',
        label: 'Outer Shell',
        kind: 'solid',
        editable: true,
        parameterKeys: ['outer_shell_width', 'outer_shell_depth'],
      },
    ],
  } as ModelManifest;

  assert.deepEqual(
    buildImportedUiSpec(manifest).fields,
    [
      {
        type: 'number',
        key: 'outer_shell_depth',
        label: 'Outer Shell Depth',
        min: 0,
        max: undefined,
        step: 1,
        frozen: false,
      },
      {
        type: 'number',
        key: 'outer_shell_height',
        label: 'Outer Shell Height',
        min: 0,
        max: undefined,
        step: 1,
        frozen: false,
      },
      {
        type: 'number',
        key: 'outer_shell_width',
        label: 'Outer Shell Width',
        min: 0,
        max: undefined,
        step: 1,
        frozen: false,
      },
    ],
  );
});
