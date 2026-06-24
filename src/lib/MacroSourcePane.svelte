<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import { EditorView, basicSetup } from 'codemirror';
  import { oneDark } from '@codemirror/theme-one-dark';
  import { eckyLanguageSupport } from './eckyLanguage';

  let {
    code = '',
    scopeLabel = '',
    busy = false,
    error = null,
    onApply,
    onCancel,
    onDirtyChange,
  }: {
    code?: string;
    scopeLabel?: string;
    busy?: boolean;
    error?: string | null;
    onApply?: (code: string) => void;
    onCancel?: () => void;
    onDirtyChange?: (dirty: boolean) => void;
  } = $props();

  let editorContainer: HTMLDivElement;
  let view: EditorView | null = null;
  let dirty = false;

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: code,
        extensions: [
          basicSetup,
          eckyLanguageSupport(),
          oneDark,
          EditorView.updateListener.of((update) => {
            if (!update.docChanged) return;
            const nextDirty = update.state.doc.toString() !== code;
            if (nextDirty !== dirty) {
              dirty = nextDirty;
              onDirtyChange?.(dirty);
            }
          }),
          EditorView.theme({
            '&': { height: '100%', fontSize: '13px', fontFamily: 'var(--font-mono)' },
            '.cm-scroller': { overflow: 'auto' },
          }),
        ],
      }),
      parent: editorContainer,
    });
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });

  export function currentCode(): string {
    return view?.state.doc.toString() ?? code;
  }
</script>

<div class="macro-source-pane" data-testid="macro-source-pane">
  <div class="macro-source-pane__head">
    <span class="section-label">EDIT SOURCE / {scopeLabel.toUpperCase()}</span>
    <div class="macro-source-pane__actions">
      <button class="btn btn-xs" onclick={() => onApply?.(currentCode())} disabled={busy}>
        {busy ? 'APPLYING…' : 'APPLY'}
      </button>
      <button class="btn btn-xs btn-ghost" onclick={() => onCancel?.()} disabled={busy}>
        CLOSE
      </button>
    </div>
  </div>
  <div class="macro-source-pane__editor" bind:this={editorContainer}></div>
  {#if error}
    <div class="macro-source-pane__error">{error}</div>
  {/if}
</div>

<style>
  .macro-source-pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--secondary) 45%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-primary, #0b0e13) 94%, transparent);
  }

  .macro-source-pane__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid color-mix(in srgb, var(--secondary) 30%, transparent);
    flex-shrink: 0;
  }

  .macro-source-pane__actions {
    display: flex;
    gap: 6px;
  }

  .macro-source-pane__editor {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .macro-source-pane__error {
    flex-shrink: 0;
    padding: 6px 10px;
    color: var(--error, #e06c5a);
    font-size: 11px;
    white-space: pre-wrap;
    border-top: 1px solid color-mix(in srgb, var(--error, #e06c5a) 35%, transparent);
  }
</style>
