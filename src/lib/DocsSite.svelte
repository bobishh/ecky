<script lang="ts">
  import { tick } from 'svelte';
  import referenceMarkdown from '../../public/docs/ecky-ir.md?raw';
  import {
    parseDocsDocument,
    resolveSection,
    resolveDocsAnchor,
    type DocsSection,
  } from './docs/eckyIrGuide';

  let {
    showHead = true,
    onOpenSnippet: _onOpenSnippet,
  }: {
    showHead?: boolean;
    /** Compatibility only. Reference docs never launch source into a project. */
    onOpenSnippet?: ((snippet: string, title: string) => void) | undefined;
  } = $props();

  $effect(() => {
    // Compatibility prop accepted while App's shell migrates to DocsHub.
    void _onOpenSnippet;
  });

  // The desktop reference is bundled from the exact Markdown used by the site.
  // It deliberately has no route-dependent campaign fallback or project actions.
  const documentData = parseDocsDocument(referenceMarkdown, { assetBasePath: '/docs' });
  let activeSlug = $state<string | null>(documentData.sections[0]?.slug ?? null);
  let activeSection = $derived(resolveSection(documentData.sections, activeSlug));

  let article = $state<HTMLElement>();
  let linkError = $state('');

  async function followAnchor(event: MouseEvent) {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const link = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href^="#"]') : null;
    if (!link || !article?.contains(link)) return;
    event.preventDefault();
    const target = resolveDocsAnchor(documentData.sections, link.getAttribute('href') ?? '', activeSlug);
    if (!target) {
      linkError = `Reference heading not found: ${link.getAttribute('href')}`;
      return;
    }
    linkError = '';
    activeSlug = target.section.slug;
    await tick();
    if (!article || activeSlug !== target.section.slug) return;
    const heading = [...article.querySelectorAll<HTMLElement>('[id]')].find(node => node.id === target.id);
    if (heading) heading.scrollIntoView({ block: 'start' });
    else article.scrollTop = 0;
  }

  function selectSection(section: DocsSection) {
    linkError = '';
    activeSlug = section.slug;
  }
</script>

<svelte:window onclick={followAnchor} />

<svelte:head>
  {#if showHead}
    <title>{documentData?.title ?? 'Ecky Language'}</title>
  {/if}
</svelte:head>

<div class="docs-shell">
  {#if activeSection}
    <header class="docs-header">
      <div class="docs-header__kicker">Ecky language / reference</div>
      <h1>{documentData.title}</h1>
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

        <article class="docs-article" bind:this={article}>
          {#if linkError}<p role="alert">{linkError}</p>{/if}
          <div class="docs-article__meta">
          {#if activeSection.status === 'pending'}
            <span class="docs-status docs-status--pending">Pending</span>
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
  .docs-article {
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

  .docs-article__body :global(table) {
    width: 100%;
    margin: 14px 0;
    border-collapse: collapse;
    font-size: 13px;
  }

  .docs-article__body :global(th),
  .docs-article__body :global(td) {
    border: 1px solid var(--bg-300);
    padding: 8px 10px;
    text-align: left;
    vertical-align: top;
  }

  .docs-article__body :global(th) {
    color: var(--secondary);
    background: rgba(108, 80, 8, 0.24);
  }

  @media (max-width: 980px) {
    .docs-layout {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(0, 200px) minmax(0, 1fr);
    }

    .docs-sidebar {
      min-height: 0;
    }
  }
</style>
