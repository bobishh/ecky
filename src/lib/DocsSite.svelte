<script lang="ts">
  import { onMount } from 'svelte';
  import { exportDocsBookEpub } from './tauri/client';
  import {
    docsSourcePath,
    parseDocsDocument,
    resolveSection,
    type DocsDocument,
    type DocsSection,
  } from './docs/eckyIrGuide';
  import {
    ECKY_IR_EPUB_FILENAME,
    ECKY_IR_EPUB_PATH,
    hasTauriInvokeBridge,
    saveBookEpubNative,
    triggerBrowserDownload,
  } from './docs/downloadBook';

  let {
    showHead = true,
    onOpenSnippet,
  }: {
    showHead?: boolean;
    onOpenSnippet?: ((snippet: string, title: string) => void) | undefined;
  } = $props();

  let documentData = $state<DocsDocument | null>(null);
  let activeSlug = $state<string | null>(null);
  let activeSection = $derived(resolveSection(documentData?.sections ?? [], activeSlug));
  let loading = $state(true);
  let error = $state('');
  let copyState = $state<'idle' | 'copied' | 'failed'>('idle');
  let epubState = $state<'idle' | 'saving' | 'saved' | 'failed'>('idle');
  let epubError = $state('');

  onMount(() => {
    void loadDocs();
  });

  async function loadDocs() {
    loading = true;
    error = '';

    try {
      const response = await fetch(docsSourcePath(), { cache: 'no-store' });
      if (!response.ok) {
        throw new Error(`Docs request failed: ${response.status}`);
      }

      const markdown = await response.text();
      const parsed = parseDocsDocument(markdown, { assetBasePath: '/docs' });
      documentData = parsed;
      activeSlug = window.location.hash.replace(/^#/, '') || parsed.sections[0]?.slug || null;
    } catch (nextError) {
      error = nextError instanceof Error ? nextError.message : String(nextError);
    } finally {
      loading = false;
    }
  }

  function selectSection(section: DocsSection) {
    activeSlug = section.slug;
    history.replaceState(null, '', `${window.location.pathname}#${section.slug}`);
  }

  async function copySnippet() {
    if (!activeSection?.snippet) return;

    try {
      await navigator.clipboard.writeText(activeSection.snippet);
      copyState = 'copied';
    } catch {
      copyState = 'failed';
    }

    setTimeout(() => {
      copyState = 'idle';
    }, 1500);
  }

  function downloadSnippet() {
    if (!activeSection?.snippet) return;
    const blob = new Blob([activeSection.snippet], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `${activeSection.slug}.ecky`;
    anchor.click();
    URL.revokeObjectURL(url);
  }

  function openSnippetInCode() {
    if (!activeSection?.snippet) return;
    onOpenSnippet?.(activeSection.snippet, activeSection.title);
  }

  async function downloadEpub() {
    epubState = 'saving';
    epubError = '';

    try {
      if (!hasTauriInvokeBridge()) {
        triggerBrowserDownload(document, ECKY_IR_EPUB_PATH, ECKY_IR_EPUB_FILENAME);
        epubState = 'saved';
        return;
      }

      const [{ save }] = await Promise.all([import('@tauri-apps/plugin-dialog')]);
      const result = await saveBookEpubNative({
        saveDialog: save,
        exportNativeFile: exportDocsBookEpub,
      });
      epubState = result === 'saved' ? 'saved' : 'idle';
    } catch (nextError) {
      epubError = nextError instanceof Error ? nextError.message : String(nextError);
      epubState = 'failed';
    }
  }
</script>

<svelte:head>
  {#if showHead}
    <title>Ecky IR Field Guide</title>
  {/if}
</svelte:head>

<div class="docs-shell">
  {#if loading}
    <div class="docs-state">Loading docs...</div>
  {:else if error}
    <div class="docs-state docs-state--error">{error}</div>
  {:else if documentData && activeSection}
    <header class="docs-header">
      <div class="docs-header__kicker">Ecky language / docs</div>
      <h1>{documentData.title}</h1>
      <div class="docs-header__summary">
        {@html documentData.summaryHtml}
      </div>
      <div class="docs-actions docs-actions--header">
        <button type="button" class="docs-action docs-action--primary" onclick={() => void downloadEpub()}>
          {epubState === 'saving' ? 'SAVING EPUB...' : 'DOWNLOAD EPUB'}
        </button>
      </div>
      {#if epubError}
        <div class="docs-inline-error">{epubError}</div>
      {/if}
    </header>

    <div class="docs-layout">
      <aside class="docs-sidebar">
        <div class="docs-sidebar__title">Index</div>
        <div class="docs-sidebar__list" role="tablist" aria-label="Docs sections">
          {#each documentData.sections as section}
            <button
              type="button"
              class="docs-nav-button"
              class:docs-nav-button--active={section.slug === activeSection.slug}
              onclick={() => selectSection(section)}
            >
              <span class="docs-nav-button__label">{section.title}</span>
              {#if section.status === 'pending'}
                <span class="docs-status">pending</span>
              {/if}
            </button>
          {/each}
        </div>
      </aside>

      <article class="docs-article">
        <div class="docs-article__meta">
          {#if activeSection.status === 'pending'}
            <span class="docs-status docs-status--pending">Pending</span>
          {/if}
          {#if activeSection.snippet}
            <div class="docs-actions">
              {#if onOpenSnippet}
                <button type="button" class="docs-action docs-action--primary" onclick={openSnippetInCode}>
                  OPEN IN CODE
                </button>
              {/if}
              <button type="button" class="docs-action" onclick={() => void copySnippet()}>
                {copyState === 'copied' ? 'COPIED' : copyState === 'failed' ? 'COPY FAILED' : 'COPY'}
              </button>
              <button type="button" class="docs-action" onclick={downloadSnippet}>
                DOWNLOAD .ECKY
              </button>
            </div>
          {/if}
        </div>

        <h2>{activeSection.title}</h2>
        <div class="docs-article__body">
          {@html activeSection.bodyHtml}
        </div>
      </article>
    </div>
  {/if}
</div>

<style>
  .docs-shell {
    height: 100%;
    display: grid;
    grid-template-rows: auto 1fr;
    gap: 14px;
    padding: 14px;
    overflow: hidden;
    background:
      radial-gradient(circle at top left, rgba(200, 166, 32, 0.16), transparent 24%),
      linear-gradient(180deg, #111524 0%, #090c14 100%);
    color: var(--text);
  }

  .docs-header,
  .docs-sidebar,
  .docs-article,
  .docs-state {
    border: 1px solid var(--bg-300);
    background:
      linear-gradient(rgba(255, 255, 255, 0.03) 1px, transparent 1px),
      linear-gradient(90deg, rgba(255, 255, 255, 0.03) 1px, transparent 1px),
      rgba(15, 19, 32, 0.94);
    background-size: 20px 20px;
  }

  .docs-header {
    padding: 18px;
    overflow: hidden;
  }

  .docs-header__kicker,
  .docs-sidebar__title {
    color: var(--secondary);
    font-size: 11px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .docs-header h1,
  .docs-article h2 {
    margin: 8px 0 0;
    font-size: clamp(28px, 3vw, 42px);
    line-height: 1;
  }

  .docs-header__summary :global(p) {
    margin: 10px 0 0;
    color: var(--text-dim);
    line-height: 1.6;
    max-width: 90ch;
  }

  .docs-inline-error {
    margin-top: 10px;
    color: #f2a3a3;
    font-size: 12px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .docs-layout {
    min-height: 0;
    display: grid;
    grid-template-columns: 320px minmax(0, 1fr);
    gap: 14px;
    overflow: hidden;
  }

  .docs-sidebar {
    min-height: 0;
    display: grid;
    grid-template-rows: auto 1fr;
    gap: 12px;
    padding: 16px;
    overflow: hidden;
  }

  .docs-sidebar__list {
    min-height: 0;
    display: grid;
    gap: 8px;
    align-content: start;
    overflow: auto;
    padding-right: 4px;
  }

  .docs-nav-button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    width: 100%;
    border: 1px solid var(--bg-300);
    background: rgba(17, 21, 36, 0.92);
    color: var(--text);
    padding: 12px 14px;
    text-align: left;
    font: inherit;
    cursor: pointer;
  }

  .docs-nav-button:hover,
  .docs-nav-button--active {
    border-color: var(--secondary);
    background: linear-gradient(180deg, rgba(58, 45, 12, 0.8), rgba(23, 30, 49, 0.96));
  }

  .docs-nav-button__label {
    line-height: 1.4;
  }

  .docs-article {
    min-height: 0;
    overflow: auto;
    padding: 18px 20px 48px;
  }

  .docs-article__meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 14px;
  }

  .docs-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .docs-actions--header {
    margin-top: 14px;
  }

  .docs-action,
  .docs-status {
    border: 1px solid color-mix(in srgb, var(--secondary) 45%, var(--bg-300));
    background: rgba(17, 21, 36, 0.92);
    color: var(--text);
    padding: 7px 10px;
    font: inherit;
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .docs-action {
    cursor: pointer;
  }

  .docs-action--primary,
  .docs-status--pending {
    background: linear-gradient(180deg, rgba(108, 80, 8, 0.92), rgba(62, 43, 3, 0.95));
    color: #f6eed4;
  }

  .docs-article__body {
    color: var(--text);
    line-height: 1.7;
  }

  .docs-article__body :global(h3),
  .docs-article__body :global(h4) {
    margin: 22px 0 10px;
    color: var(--secondary);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .docs-article__body :global(p),
  .docs-article__body :global(li) {
    color: var(--text-dim);
    font-size: 14px;
  }

  .docs-article__body :global(ul) {
    margin: 0 0 14px;
    padding-left: 20px;
  }

  .docs-article__body :global(pre) {
    overflow: auto;
    margin: 14px 0;
    padding: 14px;
    border: 1px solid color-mix(in srgb, var(--secondary) 28%, var(--bg-300));
    background: rgba(10, 13, 22, 0.96);
  }

  .docs-article__body :global(code) {
    font-family: 'SFMono-Regular', ui-monospace, monospace;
    color: var(--text);
  }

  .docs-state {
    display: grid;
    place-items: center;
    padding: 24px;
    min-height: 220px;
  }

  .docs-state--error {
    color: #ff9d9d;
  }

  @media (max-width: 980px) {
    .docs-layout {
      grid-template-columns: 1fr;
    }

    .docs-sidebar {
      max-height: 220px;
    }
  }
</style>
