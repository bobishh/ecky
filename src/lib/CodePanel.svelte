<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import { EditorView, basicSetup } from 'codemirror';
  import { python } from '@codemirror/lang-python';
  import { oneDark } from '@codemirror/theme-one-dark';
  import type { ViewUpdate } from '@codemirror/view';

  let {
    code = $bindable(''),
    onchange,
  }: {
    code?: string;
    onchange?: (code: string) => void;
  } = $props();

  let editorContainer: HTMLDivElement;
  let view: EditorView | null = null;

  onMount(() => {
    let startState = EditorState.create({
      doc: code,
      extensions: [
        basicSetup,
        python(),
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
          "&": { height: "100%", fontSize: "14px", fontFamily: "var(--font-mono)" },
          ".cm-scroller": { overflow: "auto" }
        })
      ]
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
        changes: { from: 0, to: view.state.doc.length, insert: code }
      });
    }
  });
</script>

<div class="code-container" bind:this={editorContainer}></div>

<style>
  .code-container {
    height: 100%;
    width: 100%;
    background: var(--bg-100);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  
  :global(.cm-editor) {
    height: 100%;
    outline: none !important;
  }
</style>
