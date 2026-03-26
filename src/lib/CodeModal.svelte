<script lang="ts">
  import Window from './Window.svelte';
  import CodePanel from './CodePanel.svelte';
  import { fromMirrorIr, isEckyIrSource, toMirrorIr } from './eckyIrMirror';
  let {
    code = $bindable(''),
    title,
    onclose,
    onCommit,
    onFork,
  }: {
    code?: string;
    title: string;
    onclose: () => void;
    onCommit?: (code: string) => Promise<void> | void;
    onFork?: (code: string) => Promise<void> | void;
  } = $props();

  let x = $state(100);
  let y = $state(100);
  let width = $state(1000);
  let height = $state(700);

  let copyState = $state<'idle' | 'copied'>('idle');
  let commitState = $state<'idle' | 'committing' | 'forking'>('idle');
  let commitError = $state('');
  let editorViewMode = $state<'canon' | 'mirror'>('canon');

  const isEckyIr = $derived(isEckyIrSource(code));
  const displayCode = $derived(
    isEckyIr ? (editorViewMode === 'mirror' ? toMirrorIr(code) : code) : code,
  );

  function formatCommitError(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === 'string') return error;
    try {
      return JSON.stringify(error);
    } catch {
      return String(error);
    }
  }

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(displayCode);
      copyState = 'copied';
      setTimeout(() => copyState = 'idle', 2000);
    } catch (e: unknown) {
      console.error('Failed to copy code:', e);
    }
  }

  async function handleCommit() {
    if (!onCommit || commitState !== 'idle') return;
    commitState = 'committing';
    commitError = '';
    try {
      await onCommit(code);
    } catch (e: unknown) {
      console.error('Failed to commit code:', e);
      commitError = formatCommitError(e);
    } finally {
      commitState = 'idle';
    }
  }

  async function handleFork() {
    if (!onFork || commitState !== 'idle') return;
    commitState = 'forking';
    commitError = '';
    try {
      await onFork(code);
    } catch (e: unknown) {
      console.error('Failed to fork code:', e);
      commitError = formatCommitError(e);
    } finally {
      commitState = 'idle';
    }
  }

  function handleCodeChange(nextCode: string) {
    code = isEckyIr && editorViewMode === 'mirror' ? fromMirrorIr(nextCode) : nextCode;
  }
</script>

<Window 
  title={`MACRO INSPECTOR: ${title}`} 
  {onclose} 
  bind:x 
  bind:y 
  bind:width 
  bind:height
>
  <div class="code-modal-content">
    {#if isEckyIr}
      <div class="code-mode-tabs">
        <button
          class="code-mode-tab {editorViewMode === 'canon' ? 'active' : ''}"
          type="button"
          onclick={() => (editorViewMode = 'canon')}
        >
          CANON
        </button>
        <button
          class="code-mode-tab {editorViewMode === 'mirror' ? 'active' : ''}"
          type="button"
          onclick={() => (editorViewMode = 'mirror')}
        >
          MIRROR
        </button>
        <div class="code-mode-help">
          MIRROR is an alias view over canonical Ecky IR v0. Saves still commit canonical source.
        </div>
      </div>
    {/if}
    <div class="code-editor-area">
      <CodePanel code={displayCode} onchange={handleCodeChange} />
    </div>
    <div class="code-modal-footer">
      <div class="footer-left">
        <button class="btn btn-secondary" onclick={copyCode}>
          {copyState === 'copied' ? 'COPIED!' : 'COPY CODE'}
        </button>
        {#if commitError}
          <div class="commit-error" title={commitError}>{commitError}</div>
        {/if}
      </div>
      <div class="footer-actions">
        <button
          class="btn btn-secondary"
          onclick={handleFork}
          disabled={!onFork || commitState !== 'idle'}
          title="Fork these code changes into a new thread"
        >
          {#if commitState === 'forking'}
            FORKING...
          {:else}
            FORK TO NEW THREAD
          {/if}
        </button>
        <button
          class="btn btn-primary"
          onclick={handleCommit}
          disabled={!onCommit || commitState !== 'idle'}
          title="Save changes as a new version in history"
        >
          {#if commitState === 'committing'}
            COMMITTING...
          {:else}
            COMMIT AS NEW VERSION
          {/if}
        </button>
      </div>
    </div>
  </div>
</Window>

<style>
  .code-modal-content {
    width: 100%;
    height: 100%;
    background: var(--bg);
    display: flex;
    flex-direction: column;
  }

  .code-editor-area {
    flex: 1;
    min-height: 0;
  }

  .code-mode-tabs {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 0 12px;
    background: var(--bg);
    flex-shrink: 0;
  }

  .code-mode-tab {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text-dim);
    padding: 4px 10px;
    font-family: var(--font-mono);
    font-size: 0.68rem;
    cursor: pointer;
  }

  .code-mode-tab.active {
    border-color: var(--primary);
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
  }

  .code-mode-help {
    min-width: 0;
    color: var(--text-dim);
    font-size: 0.66rem;
    font-family: var(--font-mono);
  }

  .code-modal-footer {
    padding: 12px;
    background: var(--bg-100);
    border-top: 1px solid var(--bg-300);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  .footer-left {
    display: flex;
    gap: 8px;
    align-items: center;
    min-width: 0;
  }

  .commit-error {
    max-width: 480px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--red) 72%, var(--bg-300));
    background: color-mix(in srgb, var(--red) 14%, var(--bg-100));
    color: var(--text);
    font-size: 0.72rem;
    line-height: 1.35;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .footer-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: flex-end;
  }
</style>
