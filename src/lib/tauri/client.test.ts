import assert from 'node:assert/strict';
import test from 'node:test';

test('generateDesign forwards source language and geometry backend options', async () => {
  let capturedArgs: Record<string, unknown> | null = null;
  const tauriInternals = {
    invoke: async (_cmd: string, args: Record<string, unknown>) => {
      capturedArgs = args;
      return {
        threadId: 'thread-1',
        messageId: 'message-1',
        design: {
          title: 'ok',
          versionName: 'v1',
          response: 'ok',
          interactionMode: 'design',
          macroCode: '(model)',
          macroDialect: 'ecky',
          engineKind: 'ecky',
          sourceLanguage: 'ecky',
          geometryBackend: 'freecad',
          uiSpec: { fields: [] },
          initialParams: {},
          postProcessing: null,
        },
        usage: null,
      };
    },
  };
  const shimmedGlobal = globalThis as typeof globalThis & {
    window?: unknown;
    __TAURI_INTERNALS__?: unknown;
  };
  shimmedGlobal.__TAURI_INTERNALS__ = tauriInternals;
  Object.defineProperty(globalThis, 'window', {
    value: shimmedGlobal,
    configurable: true,
  });
  const { generateDesign } = await import('./client');

  await generateDesign({
    prompt: 'make a bracket',
    threadId: 'thread-1',
    parentMacroCode: null,
    workingDesign: null,
    isRetry: false,
    imageData: null,
    attachments: [],
    questionMode: false,
    followUpQuestion: null,
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'freecad',
  });

  const options = (capturedArgs as Record<string, unknown> | null)?.options as Record<string, unknown> | undefined;
  assert.deepEqual(options, {
    questionMode: false,
    followUpQuestion: null,
    engineKind: 'ecky',
    sourceLanguage: 'ecky',
    geometryBackend: 'freecad',
  });
});
