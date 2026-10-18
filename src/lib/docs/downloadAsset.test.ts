import assert from 'node:assert/strict';
import test from 'node:test';

import { ECKY_IR_EPUB_FILENAME, ECKY_IR_EPUB_PATH, hasTauriInvokeBridge, saveDocumentationAssetNative } from './downloadAsset';

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

test('native documentation download exports the EPUB to its chosen path', async () => {
  let savedPath = '';

  const result = await saveDocumentationAssetNative({
    async saveDialog(options) {
      assert.deepEqual(options, {
        filters: [{ name: 'EPUB Book', extensions: ['epub'] }],
        defaultPath: ECKY_IR_EPUB_FILENAME,
      });
      return '/tmp/ecky-ir-field-guide.epub';
    },
    async exportNativeFile(asset, path) {
      assert.equal(asset, ECKY_IR_EPUB_PATH);
      savedPath = path;
    },
  }, ECKY_IR_EPUB_PATH, ECKY_IR_EPUB_FILENAME);

  assert.equal(result, 'saved');
  assert.equal(savedPath, '/tmp/ecky-ir-field-guide.epub');
});

test('native documentation download never exports after cancellation', async () => {
  let exportCalls = 0;

  const result = await saveDocumentationAssetNative({
    async saveDialog() {
      return null;
    },
    async exportNativeFile() {
      exportCalls += 1;
    },
  }, '/docs/examples/abc-worm.ecky', 'worm.ecky');

  assert.equal(result, 'cancelled');
  assert.equal(exportCalls, 0);
});

test('native example download keeps the exact catalog path and exposes export failures', async () => {
  await assert.rejects(saveDocumentationAssetNative({
    async saveDialog(options) {
      assert.deepEqual(options, { filters: [{ name: 'Ecky Source', extensions: ['ecky'] }], defaultPath: 'worm.ecky' });
      return '/tmp/worm.ecky';
    },
    async exportNativeFile(assetPath, targetPath) {
      assert.equal(assetPath, '/docs/examples/abc-worm.ecky');
      assert.equal(targetPath, '/tmp/worm.ecky');
      throw new Error('destination denied (raw filesystem error)');
    },
  }, '/docs/examples/abc-worm.ecky', 'worm.ecky'), /destination denied/);
});
