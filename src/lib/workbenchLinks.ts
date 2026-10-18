import { docsDownloadFilename } from './docs/eckyIrGuide';
import { downloadDocumentationAsset, ECKY_IR_EPUB_FILENAME, ECKY_IR_EPUB_PATH, hasTauriInvokeBridge } from './docs/downloadAsset';
import { openExternalUrl } from './tauri/client';

export type WorkbenchLinkIntent =
  | { kind: 'download'; assetPath: string; filename: string }
  | { kind: 'external'; url: string }
  | { kind: 'unsupported'; href: string };

export function workbenchLinkIntent(href: string): WorkbenchLinkIntent {
  const filename = docsDownloadFilename(href);
  if (filename) return { kind: 'download', assetPath: href, filename };
  if (href === ECKY_IR_EPUB_PATH) return { kind: 'download', assetPath: href, filename: ECKY_IR_EPUB_FILENAME };
  if (/^\/docs(?:\/|$)/.test(href)) return { kind: 'external', url: new URL(href, 'https://ecky-cad.com').href };
  try {
    const url = new URL(href);
    if (['https:', 'http:', 'mailto:'].includes(url.protocol)) return { kind: 'external', url: url.href };
  } catch { /* Local source references must use the bound Code inspector action. */ }
  return { kind: 'unsupported', href };
}

export function installWorkbenchLinkRouting(
  node: HTMLElement | Document,
  onError: (error: unknown) => void,
): () => void {
  function handleClick(event: Event) {
    if (event.defaultPrevented) return;
    if (event instanceof MouseEvent && event.button !== 0 && event.button !== 1) return;
    const link = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href]') : null;
    if (!link || !node.contains(link)) return;
    const href = link.getAttribute('href') ?? '';
    if (href.startsWith('#')) return;
    if (link.hasAttribute('download') && /^(blob:|data:)/.test(href)) return;
    // Cancel synchronously, before dialogs or IPC. No app-content link may
    // replace the WebView document and tear down Svelte's subscriptions.
    event.preventDefault();
    const intent = workbenchLinkIntent(href);
    if (intent.kind === 'download') {
      void downloadDocumentationAsset(intent.assetPath, intent.filename).catch(onError);
    } else if (intent.kind === 'external') {
      if (hasTauriInvokeBridge()) void openExternalUrl(intent.url).catch(onError);
      else window.open(intent.url, '_blank', 'noopener,noreferrer');
    } else {
      onError(new Error(`Cannot open this link inside the workbench: ${intent.href}`));
    }
  }
  node.addEventListener('click', handleClick);
  node.addEventListener('auxclick', handleClick);
  return () => {
    node.removeEventListener('click', handleClick);
    node.removeEventListener('auxclick', handleClick);
  };
}
