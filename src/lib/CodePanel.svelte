<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import { EditorView, basicSetup } from 'codemirror';
  import { python } from '@codemirror/lang-python';
  import { oneDark } from '@codemirror/theme-one-dark';
  import type { ViewUpdate } from '@codemirror/view';
  import { eckyLanguageSupport } from './eckyLanguage';
  import { usesPythonEditorMode } from './codeEditorMode';

  let {
    code = $bindable(''),
    sourceLanguage = null,
    onchange,
  }: {
    code?: string;
    sourceLanguage?: string | null;
    onchange?: (code: string) => void;
  } = $props();

  let editorContainer: HTMLDivElement;
  let view: EditorView | null = null;
  let activeSourceLanguage = $state<string | null>(null);

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
