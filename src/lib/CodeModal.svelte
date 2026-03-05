<script>
  import Window from './Window.svelte';
  import CodePanel from './CodePanel.svelte';
  let { code = $bindable(), title, onclose, onCommit } = $props();

  let x = $state(100);
  let y = $state(100);
  let width = $state(1000);
  let height = $state(700);

  function handleCommit() {
    if (onCommit) onCommit(code);
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
      <CodePanel bind:code />
    </div>
    <div class="code-modal-footer">
      <button class="btn btn-primary" onclick={handleCommit} title="Save changes as a new version in history">
        COMMIT AS NEW VERSION
      </button>
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
    justify-content: flex-end;
  }
</style>
