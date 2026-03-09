<script>
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { readTextFile } from '@tauri-apps/plugin-fs';
  import Modal from './Modal.svelte';

  let { show = $bindable(false), onImport } = $props();

  let macroCode = $state('');
  let fileName = $state('');

  async function handleFileImport() {
    try {
      const selected = await openDialog({
        multiple: false,
        filters: [{ name: 'FreeCAD Macro', extensions: ['FCMacro', 'py'] }]
      });
      if (selected) {
        const content = await readTextFile(selected);
        macroCode = content;
        fileName = selected.split(/[\\/]/).pop() || 'Imported Macro';
      }
    } catch (e) {
      console.error('Failed to read file:', e);
    }
  }

  function submit() {
    onImport({ code: macroCode, title: fileName || 'Manual Import' });
    show = false;
    reset();
  }

  function reset() {
    macroCode = '';
    fileName = '';
  }
</script>

<Modal bind:show title="IMPORT EXISTING MACRO" onclose={reset}>
  <div class="import-container">
    <div class="input-header">
      <span>PASTE PYTHON CODE OR</span>
      <button class="btn btn-xs btn-ghost" onclick={handleFileImport}>📂 UPLOAD FILE</button>
    </div>
    <textarea 
      bind:value={macroCode} 
      placeholder="Paste FreeCAD macro (Python) here..."
      class="macro-textarea input-mono"
    ></textarea>
    {#if fileName}
      <div class="file-indicator">📎 {fileName}</div>
    {/if}

    <div class="modal-actions">
      <button class="btn btn-ghost" onclick={() => show = false}>CANCEL</button>
      <button 
        class="btn btn-primary" 
        onclick={submit}
        disabled={!macroCode.trim()}
      >
        CREATE THREAD
      </button>
    </div>
  </div>
</Modal>

<style>
  .import-container {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 10px;
  }

  .input-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.6rem;
    color: var(--text-dim);
    font-weight: bold;
  }

  .macro-textarea {
    width: 100%;
    height: 300px;
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    color: var(--text);
    padding: 10px;
    resize: none;
    font-size: 0.75rem;
    font-family: var(--font-mono);
  }

  .file-indicator {
    font-size: 0.65rem;
    color: var(--secondary);
    font-family: var(--font-mono);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    margin-top: 8px;
  }
</style>
