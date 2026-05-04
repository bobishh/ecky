import assert from 'node:assert/strict';
import test from 'node:test';

import { confirmAction } from './confirmAction';

type TestWindow = {
  confirm?: (message: string) => boolean;
  __TAURI_INTERNALS__?: {
    invoke: (cmd: string, args: Record<string, unknown>) => Promise<unknown>;
    transformCallback?: (callback: unknown) => number;
  };
};

function setWindow(value: TestWindow) {
  Object.defineProperty(globalThis, 'window', {
    value,
    configurable: true,
  });
}

test('confirmAction uses browser confirm outside Tauri', async () => {
  let message = '';
  setWindow({
    confirm: (nextMessage) => {
      message = nextMessage;
      return false;
    },
  });

  const confirmed = await confirmAction('Fork now?');

  assert.equal(confirmed, false);
  assert.equal(message, 'Fork now?');
});

test('confirmAction awaits Tauri dialog confirm when Tauri runtime exists', async () => {
  let capturedCommand = '';
  let capturedArgs: Record<string, unknown> | null = null;
  setWindow({
    __TAURI_INTERNALS__: {
      invoke: async (cmd, args) => {
        capturedCommand = cmd;
        capturedArgs = args;
        return false;
      },
      transformCallback: () => 0,
    },
  });

  const confirmed = await confirmAction('Fork now?', 'Ecky CAD');

  assert.equal(confirmed, false);
  assert.equal(capturedCommand, 'plugin:dialog|confirm');
  assert.deepEqual(capturedArgs, {
    message: 'Fork now?',
    title: 'Ecky CAD',
    kind: 'warning',
    okButtonLabel: 'OK',
    cancelButtonLabel: 'Cancel',
  });
});

test('confirmAction treats undefined Tauri confirmation as cancelled', async () => {
  setWindow({
    __TAURI_INTERNALS__: {
      invoke: async () => undefined,
      transformCallback: () => 0,
    },
  });

  const confirmed = await confirmAction('Fork now?', 'Ecky CAD');

  assert.equal(confirmed, false);
});
