<script>
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Modal from './Modal.svelte';

  let { onGenerate, isGenerating = false, messages = [], onShowCode, activeVersionId = $bindable(null), onVersionChange, onDeleteVersion } = $props();

  let prompt = $state('');
  let attachments = $state([]); // { path: string, name: string, explanation: string, type: string }
  let isDragging = $state(false);
  let showDeleteConfirm = $state(false);

  function processPaths(paths) {
    const newAttachments = paths.map(path => {
      const name = path.split(/[\/\\]/).pop();
      const ext = name.split('.').pop().toLowerCase();
      return {
        path,
        name,
        explanation: '',
        type: ['png', 'jpg', 'jpeg'].includes(ext) ? 'image' : 'cad'
      };
    });
    attachments = [...attachments, ...newAttachments];
  }

  onMount(() => {
    let unlisten = null;
    const tauriBridge = typeof window !== 'undefined' ? window.__TAURI_INTERNALS__ : null;
    const hasTauriWindow = tauriBridge && typeof tauriBridge.metadata === 'object';
    if (!hasTauriWindow) {
      return () => {};
    }
    // 1. Native Tauri Drag & Drop (for absolute paths)
    try {
      getCurrentWindow()
        .onDragDropEvent((event) => {
          if (event.payload.type === 'hover') {
            isDragging = true;
          } else if (event.payload.type === 'drop') {
            isDragging = false;
            processPaths(event.payload.paths);
          } else if (event.payload.type === 'cancel') {
            isDragging = false;
          }
        })
        .then((cleanup) => {
          unlisten = cleanup;
        })
        .catch((e) => {
          console.error('Failed to wire Tauri drag-drop listener:', e);
        });
    } catch (e) {
      console.warn('Tauri drag-drop bridge unavailable:', e);
    }

    return () => {
      unlisten?.();
    };
  });

  // 2. Web Drag & Drop Fallback (mainly for E2E testing in browser environments)
  function handleWebDragOver(e) {
    e.preventDefault();
    isDragging = true;
  }

  function handleWebDragLeave() {
    isDragging = false;
  }

  function handleWebDrop(e) {
    e.preventDefault();
    isDragging = false;
    
    // In a real browser, we don't get absolute paths, but for E2E tests 
    // we can simulate the 'paths' if needed or just test the UI reaction.
    if (e.dataTransfer.files.length > 0) {
      const files = Array.from(e.dataTransfer.files);
      const mockPaths = files.map(f => f.name); // Fallback to names
      processPaths(mockPaths);
    }
  }

  // Extract versions (pairs of user prompt + assistant output)
  const versions = $derived(messages.filter(m => m.role === 'assistant' && m.output));
  
  const currentVersionIndex = $derived(versions.findIndex(v => v.id === activeVersionId));
  const hasPrev = $derived(currentVersionIndex > 0);
  const hasNext = $derived(currentVersionIndex >= 0 && currentVersionIndex < versions.length - 1);

  let isSubmitting = $state(false);

  function submit() {
    if (onGenerate && !isGenerating && !isSubmitting && (prompt.trim() || attachments.length > 0)) {
      isSubmitting = true;
      const currentPrompt = prompt;
      const currentAttachments = [...attachments];
      
      prompt = '';
      attachments = [];
      
      onGenerate(currentPrompt, currentAttachments).finally(() => {
        isSubmitting = false;
      });
    }
  }

  function retryFailedPrompt() {
    if (!onGenerate || !failedPromptForRetry || isGenerating || isSubmitting) return;
    isSubmitting = true;
    onGenerate(failedPromptForRetry, []).finally(() => {
      isSubmitting = false;
    });
  }

  async function addAttachment() {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          { name: 'Images, CAD & Macros', extensions: ['png', 'jpg', 'jpeg', 'stl', 'step', 'stp', 'py', 'fcmacro'] }
        ]
      });

      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        const newAttachments = paths.map(path => {
          const name = path.split(/[\/\\]/).pop();
          const ext = name.split('.').pop().toLowerCase();
          return {
            path,
            name,
            explanation: '',
            type: ['png', 'jpg', 'jpeg'].includes(ext) ? 'image' : 'cad'
          };
        });
        attachments = [...attachments, ...newAttachments];
      }
    } catch (e) {
      console.error('Failed to open file dialog:', e);
    }
  }

  function removeAttachment(index) {
    attachments = attachments.filter((_, i) => i !== index);
  }

  function handleKeydown(e) {
    if (e.key === 'Enter' && e.metaKey) {
      submit();
    }
  }

  function goPrev() {
    if (hasPrev && onVersionChange) onVersionChange(versions[currentVersionIndex - 1]);
  }

  function goNext() {
    if (hasNext && onVersionChange) onVersionChange(versions[currentVersionIndex + 1]);
  }

  function executeDelete() {
    if (onDeleteVersion && currentVersion) {
      onDeleteVersion(currentVersion.id);
      showDeleteConfirm = false;
    }
  }

  const currentVersion = $derived(currentVersionIndex >= 0 ? versions[currentVersionIndex] : null);
  const promptTrail = $derived.by(() => {
    if (!currentVersion) return [];
    const isLatest = currentVersion.id === versions[versions.length - 1]?.id;
    if (isLatest) return messages;
    return messages.filter(m => m.timestamp <= currentVersion.timestamp);
  });
  const currentUserMsg = $derived.by(() => {
    const userMsgs = promptTrail.filter(m => m.role === 'user');
    return userMsgs.length > 0 ? userMsgs[userMsgs.length - 1] : null;
  });

  let detailsOpen = $state(false);
  let trailListEl = $state(null);

  // Auto-scroll to bottom when opened or messages change
  $effect(() => {
    if (detailsOpen && trailListEl) {
      // Use requestAnimationFrame or setTimeout to ensure DOM is updated
      requestAnimationFrame(() => {
        if (trailListEl) trailListEl.scrollTop = trailListEl.scrollHeight;
      });
    }
  });

  // Also watch for message changes while the panel is open
  $effect(() => {
    if (detailsOpen && messages.length && trailListEl) {
      trailListEl.scrollTop = trailListEl.scrollHeight;
    }
  });

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleString();
  }

  const lastMessage = $derived(messages.length > 0 ? messages[messages.length - 1] : null);
  const failedPromptForRetry = $derived.by(() => {
    if (!lastMessage || lastMessage.role !== 'assistant' || lastMessage.status !== 'error') return null;
    for (let i = messages.length - 1; i >= 0; i--) {
      const msg = messages[i];
      if (msg.role === 'user' && msg.timestamp <= lastMessage.timestamp) {
        const text = `${msg.content ?? ''}`.trim();
        if (text) return text;
      }
    }
    return null;
  });
</script>

<div 
  class="prompt-container" 
  class:is-dragging={isDragging}
  ondragover={handleWebDragOver}
  ondragleave={handleWebDragLeave}
  ondrop={handleWebDrop}
  role="region"
  aria-label="Prompt panel"
>
  {#if isDragging}
    <div class="drag-overlay">
      <div class="drag-msg">DROP TO ATTACH REFERENCES</div>
    </div>
  {/if}
  {#if versions.length > 0}
    {#if showDeleteConfirm}
      <Modal title="Confirm Version Purge" onclose={() => showDeleteConfirm = false}>
        <div class="confirm-delete-body">
          <p>Are you sure you want to delete <strong>{currentVersion?.output?.title || 'this version'}</strong>?</p>
          <p class="warning">This specific iteration will be removed from the thread's timeline.</p>
          <div class="confirm-actions">
            <button class="btn btn-secondary" onclick={() => showDeleteConfirm = false}>CANCEL</button>
            <button class="btn btn-danger" onclick={executeDelete}>DELETE VERSION</button>
          </div>
        </div>
      </Modal>
    {/if}
    <div class="version-nav">
      <div class="nav-controls">
        <button class="nav-btn" disabled={!hasPrev} onclick={goPrev}>&larr;</button>
        <button class="nav-btn" disabled={!hasNext} onclick={goNext}>&rarr;</button>
      </div>
      
      <div class="version-info">
        <div class="version-counter-group">
          <span class="version-counter">V {currentVersionIndex + 1} OF {versions.length}</span>
          {#if currentVersion && currentVersion.output?.versionName}
            <span class="version-name">{currentVersion.output.versionName}</span>
          {/if}
        </div>
        {#if currentVersion}
          <div class="version-actions">
            <button class="code-btn" onclick={() => onShowCode(currentVersion)} title="Inspect Python Code">📜 CODE</button>
            <button class="code-btn delete-btn" onclick={() => showDeleteConfirm = true} title="Delete Version">🗑️ DEL</button>
          </div>
        {/if}
      </div>
    </div>

    {#if lastMessage && lastMessage.status === 'error'}
      <div class="error-msg-box">
        <div class="error-header">LLM GENERATION ERROR</div>
        <div class="error-content">{lastMessage.content}</div>
        {#if failedPromptForRetry}
          <div class="error-actions">
            <button class="btn btn-danger" onclick={retryFailedPrompt} disabled={isGenerating || isSubmitting}>
              {isSubmitting ? 'RETRYING...' : 'RETRY LAST FAILED REQUEST'}
            </button>
          </div>
        {/if}
      </div>
    {/if}

    {#if currentUserMsg && currentVersion}
      <details class="version-details" bind:open={detailsOpen}>
        <summary>Dialogue History for {currentVersion.output.title}</summary>
        <div class="details-content">
          {#if promptTrail.length > 0}
            <div class="trail-list" bind:this={trailListEl}>
              {#each promptTrail as msg, i}
                <div class="trail-item {msg.role === 'assistant' ? 'trail-assistant' : 'trail-user'}">
                  <div class="trail-header-row">
                    <span class="trail-role">{msg.role === 'assistant' ? 'ECKY' : 'YOU'}</span>
                    <span class="trail-time">{formatDate(msg.timestamp)}</span>
                  </div>
                  <div class="trail-content">
                    {#if msg.imageData}
                      <div class="trail-image-wrapper">
                        <img src={msg.imageData} alt="Viewport snapshot" class="trail-image" />
                      </div>
                    {/if}
                    {#if msg.role === 'assistant' && msg.output}
                      <i>[{msg.output.interactionMode.toUpperCase()}] {msg.output.title} ({msg.output.versionName})</i>
                      <br/>
                      {msg.output.response || msg.content}
                    {:else}
                      {msg.content}
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </details>
    {/if}
  {/if}

  <div class="input-area">
    {#if attachments.length > 0}
      <div class="attachments-list">
        {#each attachments as att, i}
          <div class="attachment-item">
            <div class="att-header">
              <span class="att-type">{att.type === 'image' ? '🖼️ IMG' : '📐 CAD'}</span>
              <span class="att-name">{att.name}</span>
              <button class="btn-remove" onclick={() => removeAttachment(i)}>✕</button>
            </div>
            <input 
              class="input-mono att-explanation" 
              placeholder="Explain this context (e.g. 'This is my base sketch')"
              bind:value={att.explanation}
            />
          </div>
        {/each}
      </div>
    {/if}

    <textarea
      class="input-mono prompt-input"
      bind:value={prompt}
      onkeydown={handleKeydown}
      placeholder="Type a question or design change... (Cmd+Enter to process)"
      spellcheck="false"
    ></textarea>
    <div class="prompt-actions">
      <button class="btn btn-xs btn-ghost" onclick={addAttachment} title="Attach images or reference CAD files">
        📎 ATTACH REFERENCE
      </button>
      <button
        class="btn btn-primary"
        disabled={isGenerating || isSubmitting || (!prompt.trim() && attachments.length === 0)}
        onclick={submit}
      >
        {#if isGenerating || isSubmitting}
          PROCESSING...
        {:else}
          PROCESS
        {/if}
      </button>
    </div>  </div>
</div>

<style>
  .prompt-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    position: relative;
  }

  .drag-overlay {
    position: absolute;
    inset: 0;
    background: color-mix(in srgb, var(--primary) 15%, transparent);
    border: 3px dashed var(--primary);
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    backdrop-filter: blur(2px);
  }

  .drag-msg {
    background: var(--bg);
    color: var(--primary);
    padding: 12px 24px;
    font-family: var(--font-mono);
    font-weight: bold;
    border: 1px solid var(--primary);
    letter-spacing: 0.1em;
    box-shadow: 0 0 20px rgba(0,0,0,0.5);
  }

  .attachments-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 8px;
    max-height: 160px;
    overflow-y: auto;
    padding: 4px;
    background: var(--bg-100);
    border: 1px dashed var(--bg-300);
  }

  .attachment-item {
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 240px;
    flex-shrink: 0;
  }

  .att-header {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.6rem;
    font-weight: bold;
  }

  .att-type {
    color: var(--secondary);
    background: rgba(0,0,0,0.2);
    padding: 1px 4px;
  }

  .att-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-dim);
  }

  .btn-remove {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.8rem;
  }

  .btn-remove:hover {
    color: var(--red);
  }

  .att-explanation {
    background: var(--bg);
    border: 1px solid var(--bg-400);
    color: var(--text);
    padding: 2px 4px;
    font-size: 0.65rem;
  }

  .version-nav {
    display: flex;
    justify-content: flex-start;
    align-items: center;
    padding: 8px 12px;
    background: var(--bg-100);
    border-bottom: 1px solid var(--bg-300);
    gap: 12px;
  }

  .nav-controls {
    display: flex;
    gap: 4px;
  }

  .nav-btn {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    padding: 4px 8px;
    font-size: 0.7rem;
    cursor: pointer;
    font-family: var(--font-mono);
  }

  .nav-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .nav-btn:not(:disabled):hover {
    border-color: var(--primary);
    color: var(--primary);
  }

  .version-info {
    display: flex;
    align-items: center;
    gap: 16px;
    flex: 1;
    min-width: 0;
  }

  .version-counter-group {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-shrink: 0;
  }

  .version-counter {
    font-size: 0.7rem;
    font-weight: bold;
    color: var(--secondary);
    font-family: var(--font-mono);
    white-space: nowrap;
  }

  .version-name {
    font-size: 0.65rem;
    color: var(--text-dim);
    text-transform: uppercase;
    font-weight: 500;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .version-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }

  .code-btn {
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    font-size: 0.6rem;
    padding: 2px 6px;
    cursor: pointer;
    font-weight: bold;
  }

  .code-btn:hover {
    color: var(--primary);
  }

  .delete-btn:hover {
    color: var(--danger, #ff4444);
    border-color: var(--danger, #ff4444);
  }

  .version-details {
    padding: 8px 12px;
    background: var(--bg-100);
    border-bottom: 1px solid var(--bg-300);
    font-size: 0.75rem;
  }

  .version-details summary {
    cursor: text;
    color: var(--text-dim);
    -webkit-user-select: text;
    user-select: text;
    font-weight: bold;
  }

  .version-details summary:hover {
    color: var(--text);
  }

  .details-content {
    margin-top: 8px;
    padding-left: 16px;
    border-left: 2px solid var(--bg-300);
    -webkit-user-select: text;
    user-select: text;
  }

  .trail-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 8px;
    max-height: 300px;
    overflow-y: auto;
    padding-right: 4px;
  }

  .trail-item {
    border: 1px solid var(--bg-300);
    padding: 6px 10px;
    max-width: min(800px, 90%);
    width: fit-content;
    min-width: 200px;
    cursor: text;
    -webkit-user-select: text;
    user-select: text;
  }

  .trail-user {
    background: var(--bg-200);
    border-left: 2px solid var(--text-dim);
    align-self: flex-start;
  }

  .trail-assistant {
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
    border-left: 2px solid var(--primary);
    align-self: flex-start;
  }

  .trail-header-row {
    display: flex;
    justify-content: space-between;
    margin-bottom: 4px;
    font-size: 0.6rem;
  }

  .trail-role {
    font-weight: bold;
    color: var(--secondary);
  }

  .trail-time {
    color: var(--text-dim);
    font-family: var(--font-mono);
  }

  .trail-content {
    font-size: 0.7rem;
    color: var(--text);
    white-space: pre-wrap;
    line-height: 1.4;
    -webkit-user-select: text;
    user-select: text;
  }

  .trail-image-wrapper {
    margin-bottom: 8px;
    border: 1px solid var(--bg-400);
    max-width: 320px;
    background: #000;
  }

  .trail-image {
    display: block;
    width: 100%;
    height: auto;
    max-height: 200px;
    object-fit: contain;
  }

  .input-area {
    flex: 1;
    padding: 12px;
    background: var(--bg-100);
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
  }

  .prompt-input {
    flex: 1;
    width: 100%;
    padding: 12px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.8rem;
    resize: none;
    outline: none;
  }

  .prompt-input:focus {
    border-color: var(--primary);
  }

  .prompt-actions {
    display: flex;
    justify-content: flex-end;
  }

  .btn-primary {
    padding: 8px 16px;
    font-weight: bold;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .error-msg-box {
    margin: 8px 12px;
    padding: 12px;
    background: rgba(220, 38, 38, 0.1);
    border: 1px solid var(--red);
    color: var(--red);
    font-size: 0.75rem;
    overflow: hidden;
  }

  .error-header {
    font-weight: bold;
    margin-bottom: 8px;
    font-size: 0.65rem;
    letter-spacing: 0.1em;
  }

  .error-content {
    font-family: var(--font-mono);
    white-space: pre-wrap;
    max-height: 200px;
    overflow-y: auto;
    word-break: break-all;
  }

  .error-actions {
    margin-top: 10px;
    display: flex;
    justify-content: flex-end;
  }

  /* Shared Confirmation Styles (matching HistoryPanel) */
  .confirm-delete-body {
    padding: 20px;
    font-size: 0.85rem;
    color: var(--text);
  }

  .confirm-delete-body p {
    margin-bottom: 12px;
  }

  .confirm-delete-body .warning {
    color: var(--red);
    font-weight: bold;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }

  .btn {
    padding: 6px 16px;
    font-size: 0.75rem;
    font-weight: bold;
    cursor: pointer;
    border: 1px solid transparent;
  }

  .btn-secondary {
    background: var(--bg-300);
    color: var(--text);
    border-color: var(--bg-400);
  }

  .btn-secondary:hover {
    background: var(--bg-400);
  }

  .btn-danger {
    background: var(--red);
    color: white;
  }

  .btn-danger:hover {
    background: color-mix(in srgb, var(--red) 80%, black);
  }
</style>
