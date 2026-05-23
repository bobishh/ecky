type SaveDialog = (options: {
  filters: Array<{ name: string; extensions: string[] }>;
  defaultPath: string;
}) => Promise<string | null>;

type ExportNativeFile = (path: string) => Promise<void>;

type NativeSaveDeps = {
  saveDialog: SaveDialog;
  exportNativeFile: ExportNativeFile;
};

type TauriBridgeWindow = Window &
  typeof globalThis & {
    __TAURI_INTERNALS__?: {
      invoke?: unknown;
    };
  };

export const ECKY_IR_EPUB_PATH = '/docs/ecky-ir-field-guide.epub';
export const ECKY_IR_EPUB_FILENAME = 'ecky-ir-field-guide.epub';

export function hasTauriInvokeBridge(target: Window | undefined = globalThis.window): boolean {
  if (!target) return false;
  const bridge = (target as TauriBridgeWindow).__TAURI_INTERNALS__;
  return typeof bridge?.invoke === 'function';
}

export async function saveBookEpubNative(
  deps: NativeSaveDeps,
  fileName: string = ECKY_IR_EPUB_FILENAME,
): Promise<'saved' | 'cancelled'> {
  const targetPath = await deps.saveDialog({
    filters: [{ name: 'EPUB Book', extensions: ['epub'] }],
    defaultPath: fileName,
  });

  if (typeof targetPath !== 'string' || !targetPath.trim()) {
    return 'cancelled';
  }

  await deps.exportNativeFile(targetPath);
  return 'saved';
}

export function triggerBrowserDownload(
  target: Document,
  epubPath: string = ECKY_IR_EPUB_PATH,
  fileName: string = ECKY_IR_EPUB_FILENAME,
) {
  const anchor = target.createElement('a');
  anchor.href = epubPath;
  anchor.download = fileName;
  anchor.click();
}
