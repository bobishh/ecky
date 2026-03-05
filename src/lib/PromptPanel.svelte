<script>
  import { open } from '@tauri-apps/plugin-dialog';

  let { onGenerate, isGenerating = false, messages = [], onShowCode, activeVersionId = $bindable(null), onVersionChange } = $props();

  let prompt = $state('');
  let attachments = $state([]); // { path: string, name: string, explanation: string, type: string }

  // Extract versions (pairs of user prompt + assistant output)
  const versions = $derived(messages.filter(m => m.role === 'assistant' && m.output));
  
  const currentVersionIndex = $derived(versions.findIndex(v => v.id === activeVersionId));
  const hasPrev = $derived(currentVersionIndex > 0);
  const hasNext = $derived(currentVersionIndex >= 0 && currentVersionIndex < versions.length - 1);

  function submit() {
    if (onGenerate && !isGenerating && prompt.trim()) {
      onGenerate(prompt, attachments);
      prompt = '';
      attachments = [];
    }
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

  const currentVersion = $derived(currentVersionIndex >= 0 ? versions[currentVersionIndex] : null);
  const promptTrail = $derived.by(() => {
    if (!currentVersion) return [];
    return messages.filter(m => m.role === 'user' && m.timestamp <= currentVersion.timestamp);
  });
  const currentUserMsg = $derived(promptTrail.length > 0 ? promptTrail[promptTrail.length - 1] : null);

  let detailsOpen = $state(false);

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleString();
  }

  const lastMessage = $derived(messages.length > 0 ? messages[messages.length - 1] : null);
</script>

<div class="prompt-container">
  {#if versions.length > 0}
    <div class="version-nav">
      <button class="nav-btn" disabled={!hasPrev} onclick={goPrev}>&larr; PREV</button>
      
      <div class="version-info">
        <div class="version-counter-group">
          <span class="version-counter">V{currentVersionIndex + 1} OF {versions.length}</span>
          {#if currentVersion && currentVersion.output?.version_name}
            <span class="version-name">{currentVersion.output.version_name}</span>
          {/if}
        </div>
        {#if currentVersion}
          <div class="version-actions">
            <button class="code-btn" onclick={() => onShowCode(currentVersion)} title="Inspect Python Code">📜 CODE</button>
          </div>
        {/if}
      </div>

      <button class="nav-btn" disabled={!hasNext} onclick={goNext}>NEXT &rarr;</button>
    </div>

    {#if lastMessage && lastMessage.status === 'error'}
      <div class="error-msg-box">
        <div class="error-header">LLM GENERATION ERROR</div>
        <div class="error-content">{lastMessage.content}</div>
      </div>
    {/if}

    {#if currentUserMsg && currentVersion}
      <details class="version-details" bind:open={detailsOpen}>
        <summary>Prompt Details: {currentVersion.output.title}</summary>
        <div class="details-content">
          <div class="meta">Latest request in this version path: {formatDate(currentUserMsg.timestamp)}</div>
          <div class="query">"{currentUserMsg.content}"</div>
          {#if promptTrail.length > 1}
            <div class="trail-header">Prompt Trail</div>
            <div class="trail-list">
              {#each promptTrail as msg, i}
                <div class="trail-item">
                  <span class="trail-index">#{i + 1}</span>
                  <span class="trail-time">{formatDate(msg.timestamp)}</span>
                  <div class="trail-query">"{msg.content}"</div>
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
      placeholder="Type your design intent... (Cmd+Enter to send)"
      spellcheck="false"
    ></textarea>
    <div class="prompt-actions">
      <button class="btn btn-xs btn-ghost" onclick={addAttachment} title="Attach images or reference CAD files">
        📎 ATTACH REFERENCE
      </button>
      <button 
        class="btn btn-primary" 
        disabled={isGenerating || (!prompt.trim() && attachments.length === 0)} 
        onclick={submit}
      >
        {#if isGenerating}
          GENERATING...
        {:else if versions.length > 0}
          ITERATE DESIGN
        {:else}
          GENERATE
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .prompt-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
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
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: var(--bg-100);
    border-bottom: 1px solid var(--bg-300);
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

  .version-details {
    padding: 8px 12px;
    background: var(--bg-100);
    border-bottom: 1px solid var(--bg-300);
    font-size: 0.75rem;
  }

  .version-details summary {
    cursor: pointer;
    color: var(--text-dim);
    user-select: none;
    font-weight: bold;
  }

  .version-details summary:hover {
    color: var(--text);
  }

  .details-content {
    margin-top: 8px;
    padding-left: 16px;
    border-left: 2px solid var(--bg-300);
  }

  .meta {
    font-size: 0.65rem;
    color: var(--text-dim);
    margin-bottom: 4px;
  }

  .query {
    color: var(--text);
    white-space: pre-wrap;
    font-style: italic;
  }

  .trail-header {
    margin-top: 10px;
    margin-bottom: 6px;
    font-size: 0.62rem;
    color: var(--secondary);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: bold;
  }

  .trail-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .trail-item {
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    padding: 6px 8px;
  }

  .trail-index {
    font-size: 0.6rem;
    color: var(--secondary);
    margin-right: 8px;
  }

  .trail-time {
    font-size: 0.6rem;
    color: var(--text-dim);
  }

  .trail-query {
    margin-top: 4px;
    font-size: 0.7rem;
    color: var(--text);
    white-space: pre-wrap;
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
</style>
