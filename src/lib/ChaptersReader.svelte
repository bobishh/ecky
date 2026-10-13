<script lang="ts">
  import { parseDocsDocument, type DocsSection } from './docs/eckyIrGuide';
  import tutorialMarkdown from '../../public/tutorials/ecky-book.md?raw';

  type OpenAttemptPayload = {
    code: string;
    title: string;
  };

  let {
    onOpenAttempt = undefined,
  }: {
    onOpenAttempt?: (payload: OpenAttemptPayload) => void;
  } = $props();

  const campaign = parseDocsDocument(tutorialMarkdown);
  let selectedSection = $state(campaign.sections[0] ?? null);

  function selectSection(section: DocsSection) {
    selectedSection = section;
  }

  function extractSnippets(markdown: string): string[] {
    const matches = markdown.matchAll(/```[a-zA-Z0-9_-]*\n([\s\S]*?)```/g);
    const snippets: string[] = [];
    for (const match of matches) {
      const snippet = match[1]?.trim();
      if (snippet) snippets.push(snippet);
    }
    return snippets;
  }

  function chooseAttemptSnippet(snippets: string[]): string | null {
    return snippets.find((snippet) => /(?:^|\n)\(model\b/.test(snippet)) ?? null;
  }

  function openAttempt() {
    if (!selectedSection) return;
    const snippetList = extractSnippets(selectedSection.bodyMarkdown);
    const snippet = chooseAttemptSnippet(snippetList);
    if (!snippet) return;
    const normalizedTitle = selectedSection.title.replace(/^\s*(?:Level|Chapter)\s+\d{1,2}:\s*/i, '');
    onOpenAttempt?.({
      code: snippet,
      title: normalizedTitle,
    });
  }
</script>

<section class="chapters" aria-label="Ecky campaign chapters">
  <header class="chapters__header">
    <p>Ecky CAD Book</p>
    <h1>Worm Farm</h1>
    {@html campaign.summaryHtml}
  </header>

  <div class="chapters__body">
    <aside class="chapters__list" aria-label="Campaign levels">
      {#each campaign.sections as section}
        <button
          type="button"
          class="chapters__level"
          class:chapters__level--active={selectedSection?.slug === section.slug}
          onclick={() => selectSection(section)}
        >
          {section.title}
        </button>
      {/each}
    </aside>

    <article class="chapters__content">
      {#if selectedSection}
        <h2>{selectedSection.title}</h2>
        {@html selectedSection.bodyHtml}
        <div class="chapters__actions">
          <button
            type="button"
            class="chapters__attempt"
            onclick={openAttempt}
            disabled={!selectedSection.snippet || !onOpenAttempt}
          >
            OPEN ATTEMPT IN CODE
          </button>
        </div>
      {:else}
        <p>No campaign lesson selected.</p>
      {/if}
    </article>
  </div>
</section>

<style>
  .chapters {
    height: 100%;
    min-height: 0;
    display: grid;
    grid-template-rows: auto 1fr;
    gap: 14px;
    padding: 14px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    color: var(--text);
    background: linear-gradient(180deg, #111524 0%, #090c14 100%);
  }

  .chapters__header p {
    margin: 0;
    color: var(--secondary);
    font-size: 11px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .chapters__header h1 {
    margin: 6px 0 0;
    font-size: 28px;
    line-height: 1;
  }

  .chapters__body {
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(240px, 280px) minmax(0, 1fr);
    gap: 14px;
    overflow: hidden;
  }

  .chapters__list {
    min-height: 0;
    display: grid;
    gap: 8px;
    align-content: start;
    overflow: auto;
    padding-right: 4px;
  }

  .chapters__content {
    min-height: 0;
    min-width: 0;
    display: grid;
    gap: 14px;
    align-content: start;
    overflow: auto;
  }

  .chapters__level,
  .chapters__attempt {
    width: 100%;
    border: 1px solid var(--bg-300);
    background: rgba(15, 19, 32, 0.92);
    color: var(--text);
    padding: 10px;
    font: inherit;
    cursor: pointer;
  }

  .chapters__level--active {
    border-color: var(--secondary);
    color: var(--secondary);
  }

  .chapters__attempt {
    width: fit-content;
    justify-self: end;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .chapters__content h2 {
    margin: 0;
    font-size: 22px;
  }

  .chapters__content :global(pre) {
    min-height: 0;
    max-height: 50%;
    overflow: auto;
    border: 1px solid var(--bg-300);
    margin: 0;
    padding: 10px;
    background: #090c14;
    color: #e0e0e0;
  }

  .chapters__content :global(img) {
    max-width: 100%;
    height: auto;
  }

  .chapters__content :global(p),
  .chapters__content :global(li) {
    color: var(--text-dim);
  }

  .chapters__content :global(ol),
  .chapters__content :global(ul) {
    margin-left: 1.25rem;
  }

  @media (max-width: 860px) {
    .chapters__body {
      grid-template-columns: 1fr;
      overflow: visible;
    }

    .chapters__list {
      grid-auto-flow: column;
      grid-auto-columns: minmax(150px, 1fr);
      overflow-x: auto;
      grid-auto-flow: column;
      grid-template-rows: repeat(auto-fill, minmax(0, 1fr));
    }

    .chapters__level {
      min-width: 150px;
    }
  }
</style>
