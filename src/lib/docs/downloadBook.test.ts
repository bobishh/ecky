import assert from 'node:assert/strict';
import test from 'node:test';

import { ECKY_IR_EPUB_FILENAME, hasTauriInvokeBridge, saveBookEpubNative } from './downloadBook';

test('hasTauriInvokeBridge detects tauri invoke bridge only when present', () => {
  assert.equal(hasTauriInvokeBridge(undefined), false);
  assert.equal(hasTauriInvokeBridge({} as Window), false);
  assert.equal(
    hasTauriInvokeBridge({
      __TAURI_INTERNALS__: {
        invoke() {
          return undefined;
        },
      },
    } as unknown as Window),
    true,
  );
});

test('saveBookEpubNative exports epub to chosen path', async () => {
  let savedPath = '';

  const result = await saveBookEpubNative({
    async saveDialog(options) {
      assert.deepEqual(options, {
        filters: [{ name: 'EPUB Book', extensions: ['epub'] }],
        defaultPath: ECKY_IR_EPUB_FILENAME,
      });
      return '/tmp/ecky-ir-field-guide.epub';
    },
    async exportNativeFile(path) {
      savedPath = path;
    },
  });

  assert.equal(result, 'saved');
  assert.equal(savedPath, '/tmp/ecky-ir-field-guide.epub');
});

test('saveBookEpubNative exits clean when user cancels save dialog', async () => {
  let exportCalls = 0;

  const result = await saveBookEpubNative({
    async saveDialog() {
      return null;
    },
    async exportNativeFile() {
      exportCalls += 1;
    },
  });

  assert.equal(result, 'cancelled');
  assert.equal(exportCalls, 0);
});
