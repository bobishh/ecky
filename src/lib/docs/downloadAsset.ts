import { exportDocsAsset } from '../tauri/client';
import { safeSaveDialog } from '../safeSaveDialog';

type SaveDialog = (options: {
  filters: Array<{ name: string; extensions: string[] }>;
  defaultPath: string;
}) => Promise<string | null>;

type NativeSaveDeps = {
  saveDialog: SaveDialog;
  exportNativeFile: (assetPath: string, targetPath: string) => Promise<void>;
};

type TauriBridgeWindow = Window &
  typeof globalThis & {
    __TAURI_INTERNALS__?: {
      invoke?: unknown;
    };
  };

export const ECKY_IR_EPUB_PATH = '/docs/ecky-ir-field-guide.epub';
export const ECKY_IR_EPUB_FILENAME = 'ecky-ir-field-guide.epub';

export async function downloadDocumentationAsset(assetPath: string, fileName: string): Promise<void> {
  if (hasTauriInvokeBridge()) {
    await saveDocumentationAssetNative({ saveDialog: safeSaveDialog, exportNativeFile: exportDocsAsset }, assetPath, fileName);
  } else {
    triggerBrowserDownload(document, assetPath, fileName);
  }
}

export function hasTauriInvokeBridge(target: Window | undefined = globalThis.window): boolean {
  if (!target) return false;
  const bridge = (target as TauriBridgeWindow).__TAURI_INTERNALS__;
  return typeof bridge?.invoke === 'function';
}

export async function saveDocumentationAssetNative(
  deps: NativeSaveDeps,
  assetPath: string,
  fileName: string,
): Promise<'saved' | 'cancelled'> {
  const extension = fileName.split('.').at(-1) ?? '';
  const targetPath = await deps.saveDialog({
    filters: [{ name: extension === 'epub' ? 'EPUB Book' : 'Ecky Source', extensions: [extension] }],
    defaultPath: fileName,
  });

  if (typeof targetPath !== 'string' || !targetPath.trim()) {
    return 'cancelled';
  }

  await deps.exportNativeFile(assetPath, targetPath);
  return 'saved';
}

export function triggerBrowserDownload(
  target: Document,
  assetPath: string = ECKY_IR_EPUB_PATH,
  fileName: string = ECKY_IR_EPUB_FILENAME,
) {
  const anchor = target.createElement('a');
  anchor.href = assetPath;
  anchor.download = fileName;
  anchor.click();
}
