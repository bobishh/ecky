<script lang="ts">
  import { tick } from 'svelte';
  import StlViewer from '../StlViewer.svelte';
  import ReadOnlyEckySource from './ReadOnlyEckySource.svelte';
  import {
    modelShowcaseVariants,
  } from './modelShowcaseManifest';

  let selectedId = $state(modelShowcaseVariants[0].id);
  let codeOpen = $state(false);
  let copyState = $state<'idle' | 'copied' | 'error'>('idle');
  let codeButton: HTMLButtonElement | null = $state(null);
  let sourceDialog: HTMLDialogElement | null = $state(null);
  let closeButton: HTMLButtonElement | null = $state(null);
  let copyResetTimer: ReturnType<typeof setTimeout> | null = null;
  let sourceText = $state('');
  let sourceStatus = $state<'idle' | 'loading' | 'ready' | 'error'>('idle');
  let sourceError = $state('');
  let sourceRequest = 0;

  const selected = $derived(
    modelShowcaseVariants.find((variant) => variant.id === selectedId) ?? modelShowcaseVariants[0],
  );
  let selectedSourceIndex = $state(0);
  const selectedSources = $derived(selected ? [
    {
      label: selected.sourceLabel ?? 'MODEL',
      url: selected.sourceUrl,
      downloadName: selected.sourceDownloadName,
    },
    ...(selected.companionSources ?? []),
  ] : []);
  const selectedSource = $derived(selectedSources[selectedSourceIndex] ?? selectedSources[0]);

  $effect(() => {
    if (!codeOpen) return;
    const previousOverflow = document.documentElement.style.overflow;
    document.documentElement.style.overflow = 'hidden';
    return () => {
      document.documentElement.style.overflow = previousOverflow;
    };
  });

  function selectVariant(id: string) {
    selectedId = id;
    selectedSourceIndex = 0;
  }

  async function loadSource() {
    if (!selectedSource) return;
    copyState = 'idle';
    sourceText = '';
    sourceError = '';
    sourceStatus = 'loading';
    const request = ++sourceRequest;
    const sourceUrl = selectedSource.url;
    const sourceDownloadName = selectedSource.downloadName;
    try {
      const response = await fetch(sourceUrl);
      if (!response.ok) {
        const body = await response.text();
        throw new Error(`HTTP ${response.status}${body ? ` — ${body}` : ''}`);
      }
      const text = await response.text();
      if (request !== sourceRequest || selectedSource?.url !== sourceUrl) return;
      sourceText = text;
      sourceStatus = 'ready';
    } catch (error) {
      if (request !== sourceRequest) return;
      sourceStatus = 'error';
      sourceError = `Could not load ${sourceDownloadName}: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  async function openCode() {
    if (!selected) return;
    selectedSourceIndex = 0;
    codeOpen = true;
    await tick();
    if (sourceDialog && !sourceDialog.open) sourceDialog.showModal();
    closeButton?.focus();
    await loadSource();
  }

  function selectSource(index: number) {
    if (index === selectedSourceIndex) return;
    selectedSourceIndex = index;
    void loadSource();
  }

  function closeCode() {
    if (sourceDialog?.open) {
      sourceDialog.close();
      return;
    }
    finalizeCodeClose();
  }

  function finalizeCodeClose() {
    sourceRequest += 1;
    codeOpen = false;
    copyState = 'idle';
    void tick().then(() => codeButton?.focus());
  }

  function handleDialogClick(event: MouseEvent) {
    if (event.target === sourceDialog) closeCode();
  }

  async function copyCode() {
    if (!selected) return;
    if (copyResetTimer) clearTimeout(copyResetTimer);
    try {
      await navigator.clipboard.writeText(sourceText);
      copyState = 'copied';
    } catch {
      copyState = 'error';
    }
    copyResetTimer = setTimeout(() => (copyState = 'idle'), 1800);
  }

</script>

{#if selected}
  <div
    class="model-workbench-wrap"
    data-testid="model-workbench"
    data-selected-variant={selected.id}
  >
    <div class="model-workbench">
      <header class="workbench-header">
        <div class="workbench-file">
          <span class="workbench-file__kicker">WORKING MODEL</span>
          <strong>{selected.sourceDownloadName}</strong>
        </div>
        <div class="workbench-controls">
          <div class="source-static" aria-label="Model format">
            <span>FORMAT</span>
            <strong>.ECKY + ZIP</strong>
          </div>
          <button class="workbench-code" type="button" bind:this={codeButton} onclick={openCode}>SEE CODE</button>
        </div>
      </header>

      <div class="model-strip" role="group" aria-label="Working models">
        <span class="model-strip__label">PICK A MODEL</span>
        <div class="model-options">
          {#each modelShowcaseVariants as variant}
            <button
              type="button"
              class:model-choice--active={variant.id === selected.id}
              class="model-choice"
              aria-pressed={variant.id === selected.id}
              onclick={() => selectVariant(variant.id)}
            >
              <strong>{variant.label}</strong>
              <span>{variant.note}</span>
            </button>
          {/each}
        </div>
      </div>

      <div class="model-viewport">
        <div class="viewport-label">{selected.title.toUpperCase()}</div>
        <StlViewer
          size={620}
          initialYaw={selected.view.yaw}
          initialPitch={selected.view.pitch}
          parts={(selected.preview?.parts ?? selected.parts).map(({ url, color }) => ({ url, color }))}
          upAxis={selected.preview?.upAxis ?? 'y'}
          label={`${selected.title}${selected.preview?.layout === 'assembly' ? ', assembled' : ''} — drag to rotate`}
        />
        <div class="viewport-hint">{selected.preview?.layout === 'assembly' ? 'ASSEMBLED VIEW · ' : ''}DRAG TO ORBIT</div>
      </div>

      {#if codeOpen}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <dialog
          class="source-dialog"
          bind:this={sourceDialog}
          aria-label={`Macro Inspector: ${selected.title}`}
          onclose={finalizeCodeClose}
          onclick={handleDialogClick}
        >
          <div class="source-inspector">
            <header class="source-header">
              <strong>MACRO INSPECTOR: {selectedSource?.downloadName}</strong>
              <button type="button" bind:this={closeButton} onclick={closeCode} aria-label="CLOSE CODE">×</button>
            </header>
            {#if selectedSources.length > 1}
              <nav class="source-tabs" aria-label="Source files">
                {#each selectedSources as source, index}
                  <button
                    type="button"
                    aria-pressed={index === selectedSourceIndex}
                    onclick={() => selectSource(index)}
                  >{source.label} SOURCE</button>
                {/each}
              </nav>
            {/if}
            <div class="source-editor">
              {#if sourceStatus === 'ready'}
                <ReadOnlyEckySource code={sourceText} label={`Full source for ${selected.title}`} />
              {:else if sourceStatus === 'error'}
                <div class="source-state source-state--error" role="alert">{sourceError}</div>
              {:else}
                <div class="source-state" role="status">LOADING SAVED SOURCE…</div>
              {/if}
            </div>
            <footer class="source-footer">
              <button type="button" onclick={copyCode} disabled={sourceStatus !== 'ready'}>
                {copyState === 'copied' ? 'COPIED' : copyState === 'error' ? 'COPY FAILED' : 'COPY CODE'}
              </button>
              {#if selectedSource}
                <a href={selectedSource.url} download={selectedSource.downloadName}>DOWNLOAD {selectedSource.label} SOURCE</a>
              {/if}
            </footer>
          </div>
        </dialog>
      {/if}
    </div>

    <div class="model-downloads" aria-label="Model downloads">
      <a class="model-download model-download--primary" href={selected.archiveUrl} download={selected.archiveDownloadName}>
        DOWNLOAD ZIP
      </a>
      {#each selectedSources as source}
        <a class="model-download" href={source.url} download={source.downloadName}>
          {selectedSources.length === 1 ? 'DOWNLOAD .ECKY' : `DOWNLOAD ${source.label} SOURCE`}
        </a>
      {/each}
    </div>
  </div>
{/if}

<style>
  .model-workbench-wrap,
  .model-workbench,
  .model-viewport,
  .source-inspector,
  .source-editor {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .model-workbench-wrap { width: 100%; }

  .model-workbench {
    position: relative;
    border: 2px solid color-mix(in srgb, var(--secondary) 58%, var(--border-bright));
    background: color-mix(in srgb, var(--bg-100) 94%, #000 6%);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--bg-300) 90%, transparent), 0 20px 60px rgba(0, 0, 0, 0.36);
  }

  .workbench-header {
    min-height: 58px;
    padding: 8px 10px 8px 14px;
    border-bottom: 1px solid var(--bg-300);
    background: var(--bg-200);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    overflow: hidden;
  }

  .workbench-file { display: grid; gap: 2px; min-width: 0; }

  .workbench-file__kicker,
  .source-static span,
  .model-strip__label {
    color: var(--text-dim);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.12em;
  }

  .workbench-file strong {
    color: var(--secondary);
    font-size: 0.76rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .workbench-controls { display: flex; align-items: stretch; gap: 8px; flex: 0 0 auto; }

  .source-static {
    display: grid;
    gap: 1px;
    min-width: 142px;
    padding: 4px 8px;
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
  }

  .source-static strong {
    color: var(--text);
    font: 700 0.74rem var(--font-mono);
    text-transform: uppercase;
  }

  .workbench-code,
  .model-choice,
  .source-tabs button,
  .source-header button,
  .source-footer button,
  .source-footer a,
  .model-download {
    border: 1px solid var(--bg-400);
    background: var(--bg-100);
    color: var(--text);
    font-family: var(--font-mono);
    font-weight: 700;
    cursor: pointer;
  }

  .workbench-code { min-width: 92px; padding: 0 14px; color: var(--secondary); letter-spacing: 0.06em; }

  .workbench-code:hover,
  .workbench-code:focus-visible,
  .source-footer button:hover,
  .source-footer a:hover,
  .source-tabs button:hover,
  .source-tabs button:focus-visible { border-color: var(--secondary); outline: none; }

  .model-strip {
    padding: 10px 12px 12px;
    border-bottom: 1px solid var(--bg-300);
    display: grid;
    grid-template-columns: 74px minmax(0, 1fr);
    align-items: stretch;
    gap: 10px;
    overflow: hidden;
  }

  .model-strip__label { padding-top: 9px; color: var(--secondary); }
  .model-options { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 8px; min-width: 0; }

  .model-choice {
    min-width: 0;
    padding: 8px 10px;
    display: grid;
    gap: 3px;
    text-align: left;
    overflow: hidden;
  }

  .model-choice strong { color: var(--text); font-size: 0.74rem; letter-spacing: 0.07em; }
  .model-choice span { color: var(--text-dim); font-size: 0.7rem; font-weight: 400; line-height: 1.4; }

  .model-choice:hover,
  .model-choice:focus-visible,
  .model-choice--active { border-color: var(--primary); outline: none; }

  .model-choice--active { background: color-mix(in srgb, var(--primary) 12%, var(--bg-100)); }
  .model-choice--active strong { color: var(--primary); }

  .model-viewport {
    position: relative;
    min-height: 580px;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    justify-items: center;
    background: linear-gradient(rgba(200, 146, 79, 0.035) 1px, transparent 1px), linear-gradient(90deg, rgba(200, 146, 79, 0.035) 1px, transparent 1px), #080c17;
    background-size: 20px 20px;
  }

  .viewport-label {
    width: 100%;
    padding: 8px 10px;
    border-bottom: 1px solid var(--bg-300);
    color: var(--text-dim);
    font-size: 0.7rem;
    font-weight: 700;
    letter-spacing: 0.1em;
  }

  .model-viewport :global(.viewer) { align-self: center; }

  .viewport-hint {
    width: 100%;
    padding: 7px 10px;
    border-top: 1px solid var(--bg-300);
    color: var(--text-dim);
    font-size: 0.68rem;
    letter-spacing: 0.09em;
    text-align: right;
  }

  .model-downloads { display: flex; justify-content: center; gap: 8px; flex-wrap: wrap; padding-top: 18px; overflow: hidden; }
  .model-download { padding: 9px 13px; font-size: 0.72rem; letter-spacing: 0.06em; }
  .model-download:hover,
  .model-download:focus-visible { border-color: var(--primary); color: var(--primary); outline: none; }
  .model-download--primary { border-color: var(--primary); color: var(--primary); background: color-mix(in srgb, var(--primary) 12%, var(--bg-100)); }

  .source-dialog {
    width: min(calc(100vw - 28px), 1100px);
    height: min(calc(100dvh - 28px), 820px);
    max-width: none;
    max-height: none;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    overflow: hidden;
  }
  .source-dialog::backdrop { background: rgba(5, 7, 13, 0.88); backdrop-filter: blur(4px); }
  .source-inspector { width: 100%; height: 100%; border: 2px solid color-mix(in srgb, var(--secondary) 72%, var(--bg-300)); background: var(--bg-100); box-shadow: 0 0 32px color-mix(in srgb, var(--secondary) 18%, transparent); display: grid; grid-template-rows: auto auto minmax(0, 1fr) auto; }
  .source-header,
  .source-footer { display: flex; align-items: center; gap: 8px; padding: 7px 9px; background: var(--bg-200); overflow: hidden; }
  .source-header { grid-row: 1; justify-content: space-between; border-bottom: 1px solid var(--bg-300); }
  .source-header strong { min-width: 0; color: var(--secondary); font-size: 0.72rem; letter-spacing: 0.08em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .source-header button { border: 0; background: transparent; padding: 2px 7px; color: var(--text-dim); font-size: 1.1rem; }
  .source-tabs { grid-row: 2; display: flex; gap: 6px; padding: 6px 9px; border-bottom: 1px solid var(--bg-300); background: var(--bg-200); overflow: hidden; }
  .source-tabs button { padding: 6px 9px; font-size: 0.66rem; letter-spacing: 0.05em; }
  .source-tabs button[aria-pressed='true'] { border-color: var(--primary); color: var(--primary); }
  .source-editor { grid-row: 3; }
  .source-footer { grid-row: 4; justify-content: flex-end; border-top: 1px solid var(--bg-300); }
  .source-footer button,
  .source-footer a { padding: 7px 10px; font-size: 0.72rem; letter-spacing: 0.06em; }
  .source-footer button:disabled { cursor: wait; opacity: 0.45; }
  .source-state { padding: 14px; color: var(--text-dim); font: 0.74rem/1.5 var(--font-mono); }
  .source-state--error { color: #ef7d7d; }

  @media (max-width: 720px) {
    .workbench-header { align-items: stretch; flex-direction: column; gap: 8px; }
    .workbench-controls { width: 100%; }
    .source-static { flex: 1; min-width: 0; }
    .model-strip { grid-template-columns: 1fr; gap: 7px; }
    .model-strip__label { padding-top: 0; }
    .model-options { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .model-choice span { display: none; }
    .model-viewport { min-height: 0; aspect-ratio: 1 / 1.2; }
    .source-dialog { width: calc(100vw - 10px); height: calc(100dvh - 10px); }
    .source-footer { justify-content: stretch; }
    .source-footer button,
    .source-footer a { flex: 1; text-align: center; }
  }
</style>
