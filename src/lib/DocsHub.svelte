<script lang="ts">
  import ChaptersReader from './ChaptersReader.svelte';
  import DocsSite from './DocsSite.svelte';
  import { formatBackendError } from './tauri/client';
  import { installWorkbenchLinkRouting } from './workbenchLinks';
  import { downloadDocumentationAsset, ECKY_IR_EPUB_FILENAME, ECKY_IR_EPUB_PATH } from './docs/downloadAsset';

  type OpenAttemptPayload = {
    code: string;
    title: string;
  };

  type DocsTab = 'chapters' | 'reference';
  let activeTab = $state<DocsTab>('chapters');
  let epubState = $state<'idle' | 'saving' | 'failed'>('idle');
  let epubError = $state('');
  let {
    onOpenAttempt,
  }: {
    onOpenAttempt?: (payload: OpenAttemptPayload) => void;
  } = $props();

  async function downloadAsset(assetPath: string, fileName: string) {
    epubState = 'saving';
    epubError = '';
    try {
      await downloadDocumentationAsset(assetPath, fileName);
      epubState = 'idle';
    } catch (error) {
      epubError = formatBackendError(error);
      epubState = 'failed';
    }
  }

  // File-backed documentation shares HTML with the website. Its links are
  // intents inside the desktop shell; they must never replace the workbench.
  function documentationLinks(node: HTMLElement) {
    return { destroy: installWorkbenchLinkRouting(node, error => { epubError = formatBackendError(error); }) };
  }
</script>

<div class="docs-hub" use:documentationLinks>
  <header class="docs-hub__header">
    <div>
      <p>Ecky CAD / documentation</p>
      <h1>Docs</h1>
    </div>
    <nav aria-label="Documentation sections">
      <button type="button" class:docs-hub__tab--active={activeTab === 'chapters'} class="docs-hub__tab" onclick={() => activeTab = 'chapters'}>Chapters</button>
      <button type="button" class:docs-hub__tab--active={activeTab === 'reference'} class="docs-hub__tab" onclick={() => activeTab = 'reference'}>Function Reference</button>
      <button type="button" class="docs-hub__tab" onclick={() => void downloadAsset(ECKY_IR_EPUB_PATH, ECKY_IR_EPUB_FILENAME)}>{epubState === 'saving' ? 'Saving EPUB…' : 'EPUB'}</button>
    </nav>
    {#if epubError}<p class="docs-hub__error" role="alert">{epubError}</p>{/if}
  </header>
  <main class="docs-hub__content">
    {#if activeTab === 'chapters'}
      <ChaptersReader {onOpenAttempt} />
    {:else}
      <DocsSite showHead={false} />
    {/if}
  </main>
</div>

<style>
  .docs-hub { height: 100%; min-height: 0; display: grid; grid-template-rows: auto minmax(0, 1fr); gap: 14px; padding: 14px; overflow: hidden; background: linear-gradient(180deg, #111524 0%, #090c14 100%); }
  .docs-hub__header { display:flex; align-items:center; gap:18px; padding:14px 18px; overflow:hidden; border:1px solid var(--bg-300); background:rgba(15,19,32,.94); }
  .docs-hub__header p { margin:0; color:var(--secondary); font-size:11px; letter-spacing:.14em; text-transform:uppercase; }
  .docs-hub__header h1 { margin:5px 0 0; font-size:26px; line-height:1; }
  .docs-hub__header nav { margin-left:auto; display:flex; gap:8px; overflow:auto; }
  .docs-hub__tab { flex:0 0 auto; border:1px solid var(--bg-300); background:rgba(17,21,36,.92); color:var(--text); padding:8px 10px; font:inherit; font-size:11px; letter-spacing:.08em; text-transform:uppercase; cursor:pointer; }
  .docs-hub__tab:hover, .docs-hub__tab--active { border-color:var(--secondary); color:var(--secondary); }
  .docs-hub__error { color:#f2a3a3 !important; text-transform:none !important; letter-spacing:normal !important; }
  .docs-hub__content { min-height:0; overflow:hidden; }
  @media (max-width:720px) { .docs-hub__header { align-items:flex-start; flex-direction:column; gap:12px; } .docs-hub__header nav { width:100%; margin-left:0; } }
</style>
