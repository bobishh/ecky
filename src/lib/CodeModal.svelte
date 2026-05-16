<script lang="ts">
  import Window from './Window.svelte';
  import CodePanel from './CodePanel.svelte';

  type CodeModalCommitPayload = {
    code: string;
    title: string;
    versionName: string;
  };

  let {
    code = $bindable(''),
    title,
    defaultTitle = '',
    defaultVersionName = '',
    onclose,
    onApply,
    onCommit,
    onFork,
  }: {
    code?: string;
    title: string;
    defaultTitle?: string;
    defaultVersionName?: string;
    onclose: () => void;
    onApply?: (code: string) => Promise<unknown> | unknown;
    onCommit?: (payload: CodeModalCommitPayload) => Promise<void> | void;
    onFork?: (payload: CodeModalCommitPayload) => Promise<void> | void;
  } = $props();

  let x = $state(60);
  let y = $state(40);
  let width = $state(960);
  let height = $state(620);

  let copyState = $state<'idle' | 'copied'>('idle');
  let commitState = $state<'idle' | 'applying' | 'committing' | 'forking'>('idle');
  let commitError = $state('');
  let draftTitle = $state('');
  let draftVersionName = $state('');
  let initializedDraftFields = $state(false);

  $effect(() => {
    if (commitState !== 'idle') return;
    if (!initializedDraftFields) {
      draftTitle = defaultTitle || title;
      draftVersionName = defaultVersionName || 'V-manual';
      initializedDraftFields = true;
      return;
    }
  });

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
      await navigator.clipboard.writeText(code);
      copyState = 'copied';
      setTimeout(() => copyState = 'idle', 2000);
    } catch (e: unknown) {
      console.error('Failed to copy code:', e);
    }
  }

  async function handleApply() {
    if (!onApply || commitState !== 'idle') return;
    commitState = 'applying';
    commitError = '';
    try {
      await onApply(code);
    } catch (e: unknown) {
      console.error('Failed to apply code:', e);
      commitError = formatCommitError(e);
    } finally {
      commitState = 'idle';
    }
  }

  function commitPayload(): CodeModalCommitPayload {
    return {
      code,
      title: draftTitle.trim() || defaultTitle || title || 'Manual Edit',
      versionName: draftVersionName.trim() || defaultVersionName || 'V-manual',
    };
  }

  async function handleCommit() {
    if (!onCommit || commitState !== 'idle') return;
    commitState = 'committing';
    commitError = '';
    try {
      await onCommit(commitPayload());
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
      await onFork(commitPayload());
    } catch (e: unknown) {
      console.error('Failed to fork code:', e);
      commitError = formatCommitError(e);
    } finally {
      commitState = 'idle';
    }
  }

  function handleCodeChange(nextCode: string) {
    code = nextCode;
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
    <div class="code-editor-area">
      <CodePanel code={code} onchange={handleCodeChange} />
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
        <div class="commit-fields">
          <input
            class="commit-input"
            aria-label="Version title"
            bind:value={draftTitle}
            placeholder="Title"
            disabled={commitState !== 'idle'}
          />
          <input
            class="commit-input commit-input-version"
            aria-label="Version name"
            bind:value={draftVersionName}
            placeholder="Version"
            disabled={commitState !== 'idle'}
          />
        </div>
        <button
          class="btn btn-secondary"
          onclick={handleApply}
          disabled={!onApply || commitState !== 'idle'}
          title="Render code changes without creating a history version"
        >
          {#if commitState === 'applying'}
            APPLYING...
          {:else}
            APPLY
          {/if}
        </button>
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
            COMMIT VERSION
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
    min-width: 0;
    flex-wrap: wrap;
  }

  .commit-fields {
    display: flex;
    gap: 8px;
    min-width: 260px;
  }

  .commit-input {
    min-width: 0;
    width: 170px;
    height: 34px;
    border: 1px solid var(--bg-300);
    background: var(--bg);
    color: var(--text);
    padding: 0 10px;
    font-size: 0.72rem;
    font-family: inherit;
  }

  .commit-input-version {
    width: 110px;
  }
</style>
