import assert from 'node:assert/strict';
import { test } from 'node:test';

import { exportDocsBookEpub, formatBackendError } from './client';
import { commands, type AppError, type Result } from './contracts';

test('formatBackendError includes raw backend details', () => {
  const message = formatBackendError({
    code: 'validation',
    message: 'Direct OCCT native shim compile failed.',
    details: 'stdout: raw compiler stdout\nstderr: raw compiler stderr',
  });

  assert.match(message, /Direct OCCT native shim compile failed\./);
  assert.match(message, /raw compiler stdout/);
  assert.match(message, /raw compiler stderr/);
});

test('formatBackendError lifts structured diagnostic tail into visible context', () => {
  const message = formatBackendError({
    code: 'render',
    message: 'Boolean failed.',
    details: 'kernel body mismatch\npart=female_rail op=difference rail_tip_w=8 rail_h=2',
    stableNodeKey: 'part:female_rail',
    startLine: 12,
    endLine: 14,
  });

  assert.match(message, /Boolean failed\./);
  assert.match(message, /kernel body mismatch/);
  assert.match(message, /Context: part=female_rail \| op=difference \| rail_tip_w=8 \| rail_h=2 \| lines=12-14/);
});

test('formatBackendError prefers diagnosticContext span without duplicating line fields', () => {
  const message = formatBackendError({
    code: 'render',
    message: 'Fillet failed.',
    details: 'kernel body mismatch\npart=body op=fillet lines=3 width=12',
    diagnosticContext: {
      partKey: 'body',
      opName: 'fillet',
      startLine: 3,
      endLine: 3,
      resolvedParams: [{ key: 'width', value: 12 }],
    },
  });

  assert.match(message, /Fillet failed\./);
  assert.match(message, /Context: part=body \| width=12 \| op=fillet \| lines=3/);
  assert.doesNotMatch(message, /lines=3 \| lines=3/);
});

test('exportDocsBookEpub routes through generated Tauri command wrapper', async () => {
  const original = commands.exportDocsBookEpub;
  const calls: string[] = [];
  commands.exportDocsBookEpub = async (targetPath: string): Promise<Result<null, AppError>> => {
    calls.push(targetPath);
    return { status: 'ok', data: null };
  };

  try {
    await exportDocsBookEpub('/tmp/ecky-ir.epub');
  } finally {
    commands.exportDocsBookEpub = original;
  }

  assert.deepEqual(calls, ['/tmp/ecky-ir.epub']);
});
