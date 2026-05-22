<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { EditorState, StateEffect, StateField } from '@codemirror/state';
  import { Decoration, type DecorationSet } from '@codemirror/view';
  import { EditorView, basicSetup } from 'codemirror';
  import { oneDark } from '@codemirror/theme-one-dark';
  import { eckyLanguageSupport } from './eckyLanguage';

  let {
    code = '',
    scopeLabel = '',
    scopeStart = 0,
    scopeEnd = 0,
    busy = false,
    error = null,
    onApply,
    onCancel,
  }: {
    code?: string;
    scopeLabel?: string;
    scopeStart?: number;
    scopeEnd?: number;
    busy?: boolean;
    error?: string | null;
    onApply?: (code: string) => void;
    onCancel?: () => void;
  } = $props();

  let editorContainer: HTMLDivElement;
  let view: EditorView | null = null;

  const setScopeEffect = StateEffect.define<{ from: number; to: number }>();
  const scopeMark = Decoration.mark({ class: 'cm-ecky-scope' });
  const scopeField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(decorations, tr) {
      let next = decorations.map(tr.changes);
      for (const effect of tr.effects) {
        if (effect.is(setScopeEffect)) {
          const { from, to } = effect.value;
          next =
            to > from
              ? Decoration.set([scopeMark.range(from, to)])
              : Decoration.none;
        }
      }
      return next;
    },
    provide: (field) => EditorView.decorations.from(field),
  });

  function clampedScope(docLength: number): { from: number; to: number } {
    const from = Math.max(0, Math.min(scopeStart, docLength));
    const to = Math.max(from, Math.min(scopeEnd, docLength));
    return { from, to };
  }

  function applyScope() {
    if (!view) return;
    const { from, to } = clampedScope(view.state.doc.length);
    view.dispatch({
      effects: [setScopeEffect.of({ from, to }), EditorView.scrollIntoView(from, { y: 'start', yMargin: 24 })],
    });
  }

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: code,
        extensions: [
          basicSetup,
          eckyLanguageSupport(),
          oneDark,
          scopeField,
          EditorView.theme({
            '&': { height: '100%', fontSize: '13px', fontFamily: 'var(--font-mono)' },
            '.cm-scroller': { overflow: 'auto' },
            '.cm-ecky-scope': {
              backgroundColor: 'color-mix(in srgb, #d4a04f 14%, transparent)',
              outline: '1px solid color-mix(in srgb, #d4a04f 35%, transparent)',
            },
          }),
        ],
      }),
      parent: editorContainer,
    });
    applyScope();
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });

  // Scope moves when the author dblclicks another node; the draft text stays.
  $effect(() => {
    void scopeStart;
    void scopeEnd;
    applyScope();
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
