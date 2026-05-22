<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import { EditorView, basicSetup } from 'codemirror';
  import { python } from '@codemirror/lang-python';
  import { oneDark } from '@codemirror/theme-one-dark';
  import type { ViewUpdate } from '@codemirror/view';
  import { eckyLanguageSupport } from './eckyLanguage';
  import { usesPythonEditorMode } from './codeEditorMode';
  import { diffCode } from './codeDiff';

  let {
    code = $bindable(''),
    sourceLanguage = null,
    onchange,
    diffBefore = null,
    diffAfter = null,
    diffTitle = 'LAST MACRO DIFF',
    diffSummary = '',
  }: {
    code?: string;
    sourceLanguage?: string | null;
    onchange?: (code: string) => void;
    diffBefore?: string | null;
    diffAfter?: string | null;
    diffTitle?: string;
    diffSummary?: string;
  } = $props();

  let editorContainer: HTMLDivElement;
  let view: EditorView | null = null;
  let activeSourceLanguage = $state<string | null>(null);
  const codeDiff = $derived.by(() => {
    if (diffBefore === null || diffAfter === null) return null;
    const result = diffCode(diffBefore, diffAfter, { contextLines: 2 });
    return result.summary.isEmpty ? null : result;
  });

  function editorExtensions(currentSourceLanguage: string | null) {
    return [
      basicSetup,
      ...(usesPythonEditorMode(currentSourceLanguage)
        ? [python()]
        : currentSourceLanguage === 'ecky'
          ? [eckyLanguageSupport()]
          : []),
      oneDark,
      EditorView.updateListener.of((update: ViewUpdate) => {
        if (update.docChanged) {
          const newCode = update.state.doc.toString();
          if (newCode !== code) {
            code = newCode;
            if (onchange) onchange(newCode);
          }
        }
      }),
      EditorView.theme({
        '&': { height: '100%', fontSize: '14px', fontFamily: 'var(--font-mono)' },
        '.cm-scroller': { overflow: 'auto' },
        '.cm-ecky-comment': { color: '#6e7b95', fontStyle: 'italic' },
        '.cm-ecky-keyword': { color: '#d4a04f', fontWeight: '700' },
        '.cm-ecky-kind': { color: '#d98f70', fontWeight: '700' },
        '.cm-ecky-op': { color: '#62b6ab' },
        '.cm-ecky-helper': { color: '#a98fd1' },
        '.cm-ecky-name': { color: '#f0d49a', fontWeight: '700' },
        '.cm-ecky-call': { color: '#e2c089' },
        '.cm-ecky-number': { color: '#7db2d7' },
        '.cm-ecky-string': { color: '#8ebf86' },
        '.cm-ecky-atom': { color: '#cf8d5a' },
        '.cm-ecky-symbol': { color: '#d7deea' },
        '.cm-ecky-paren-1': { color: '#8a93ad' },
        '.cm-ecky-paren-2': { color: '#7fa3a0' },
        '.cm-ecky-paren-3': { color: '#9d8fbd' },
      }),
    ];
  }

  onMount(() => {
    activeSourceLanguage = sourceLanguage;
    let startState = EditorState.create({
      doc: code,
      extensions: editorExtensions(sourceLanguage),
    });

    view = new EditorView({
      state: startState,
      parent: editorContainer
    });
  });

  onDestroy(() => {
    if (view) {
      view.destroy();
    }
  });

  // Watch for external code changes (e.g. loading a different version)
  $effect(() => {
    if (view && view.state.doc.toString() !== code) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: code },
      });
    }
  });

  $effect(() => {
    if (!view || activeSourceLanguage === sourceLanguage) return;
    activeSourceLanguage = sourceLanguage;
    view.setState(
      EditorState.create({
        doc: code,
        extensions: editorExtensions(sourceLanguage),
      }),
    );
  });
</script>

<div class="code-container">
  <div class="code-editor" bind:this={editorContainer}></div>
  {#if codeDiff}
    <div class="code-diff" data-testid="code-diff-panel">
      <div class="code-diff__head">
        <span>{diffTitle}</span>
        <span>
          +{codeDiff.summary.insertedLineCount} / -{codeDiff.summary.deletedLineCount}
        </span>
      </div>
      {#if diffSummary}
        <div class="code-diff__summary">{diffSummary}</div>
      {/if}
      <div class="code-diff__rows">
        {#each codeDiff.rows as row, index (`${row.hunkIndex}:${index}`)}
          <div class="code-diff__row" data-kind={row.kind}>
            <span class="code-diff__line">{row.oldLineNumber ?? ''}</span>
            <span class="code-diff__line">{row.newLineNumber ?? ''}</span>
            <code>{row.kind === 'delete' ? row.oldText : row.newText}</code>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .code-container {
    height: 100%;
    width: 100%;
    background: var(--bg-100);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .code-editor {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .code-diff {
    max-height: 34%;
    min-height: 128px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-top: 1px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    background: color-mix(in srgb, var(--bg) 84%, transparent);
    color: var(--text);
    font-family: var(--font-mono);
  }

  .code-diff__head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--bg-300);
    color: var(--secondary);
    font-size: 0.72rem;
    font-weight: 800;
    text-transform: uppercase;
  }

  .code-diff__summary {
    padding: 7px 10px;
    border-bottom: 1px solid var(--bg-300);
    color: var(--text-muted);
    font-size: 0.74rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .code-diff__rows {
    min-height: 0;
    overflow: auto;
    padding: 6px 0;
  }

  .code-diff__row {
    display: grid;
    grid-template-columns: 42px 42px minmax(0, 1fr);
    gap: 8px;
    padding: 2px 10px;
    font-size: 0.75rem;
    line-height: 1.45;
  }

  .code-diff__row[data-kind='insert'] {
    background: color-mix(in srgb, var(--primary) 18%, transparent);
  }

  .code-diff__row[data-kind='delete'] {
    background: color-mix(in srgb, #8f433d 30%, transparent);
  }

  .code-diff__line {
    color: var(--text-muted);
    text-align: right;
  }

  .code-diff code {
    min-width: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--font-mono);
  }
  
  :global(.cm-editor) {
    height: 100%;
    outline: none !important;
  }

  :global(.cm-editor .cm-ecky-comment) {
    color: #6e7b95 !important;
  }

  :global(.cm-editor .cm-ecky-keyword) {
    color: #d4a04f !important;
    font-weight: 700 !important;
  }

  :global(.cm-editor .cm-ecky-number) {
    color: #7db2d7 !important;
  }

  :global(.cm-editor .cm-ecky-string) {
    color: #8ebf86 !important;
  }

  :global(.cm-editor .cm-ecky-atom) {
    color: #cf8d5a !important;
  }

  :global(.cm-editor .cm-ecky-symbol) {
    color: #d7deea !important;
  }

  :global(.cm-editor .cm-ecky-kind) {
    color: #d98f70 !important;
    font-weight: 700 !important;
  }

  :global(.cm-editor .cm-ecky-op) {
    color: #62b6ab !important;
  }

  :global(.cm-editor .cm-ecky-helper) {
    color: #a98fd1 !important;
  }

  :global(.cm-editor .cm-ecky-name) {
    color: #f0d49a !important;
    font-weight: 700 !important;
  }

  :global(.cm-editor .cm-ecky-call) {
    color: #e2c089 !important;
  }

  :global(.cm-editor .cm-ecky-paren-1) {
    color: #8a93ad !important;
  }

  :global(.cm-editor .cm-ecky-paren-2) {
    color: #7fa3a0 !important;
  }

  :global(.cm-editor .cm-ecky-paren-3) {
    color: #9d8fbd !important;
  }
</style>
