<script>
  let { onGenerate, isGenerating = false, messages = [], onShowCode, activeVersionId = $bindable(null), onVersionChange } = $props();

  let prompt = $state('');

  // Extract versions (pairs of user prompt + assistant output)
  const versions = $derived(messages.filter(m => m.role === 'assistant' && m.output));
  
  const currentVersionIndex = $derived(versions.findIndex(v => v.id === activeVersionId));
  const hasPrev = $derived(currentVersionIndex > 0);
  const hasNext = $derived(currentVersionIndex >= 0 && currentVersionIndex < versions.length - 1);

  function submit() {
    if (onGenerate && !isGenerating && prompt.trim()) {
      onGenerate(prompt);
      prompt = '';
    }
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
  const currentUserMsg = $derived(currentVersion ? messages.find(m => m.timestamp <= currentVersion.timestamp && m.role === 'user') : null);

  let detailsOpen = $state(false);

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleString();
  }
</script>

<div class="prompt-container">
  {#if versions.length > 0}
    <div class="version-nav">
      <button class="nav-btn" disabled={!hasPrev} onclick={goPrev}>&larr; PREV</button>
      
      <div class="version-info">
        <span class="version-counter">V{currentVersionIndex + 1} OF {versions.length}</span>
        {#if currentVersion}
          <button class="code-btn" onclick={() => onShowCode(currentVersion)} title="Inspect Python Code">📜 CODE</button>
          <button class="code-btn" onclick={() => onVersionChange(currentVersion)} title="Force Re-render">🔄 RENDER</button>
        {/if}
      </div>

      <button class="nav-btn" disabled={!hasNext} onclick={goNext}>NEXT &rarr;</button>
    </div>

    {#if currentUserMsg && currentVersion}
      <details class="version-details" bind:open={detailsOpen}>
        <summary>Prompt Details: {currentVersion.output.title}</summary>
        <div class="details-content">
          <div class="meta">Requested: {formatDate(currentUserMsg.timestamp)}</div>
          <div class="query">"{currentUserMsg.content}"</div>
        </div>
      </details>
    {/if}
  {/if}

  <div class="input-area">
    <textarea 
      class="input-mono prompt-input"
      bind:value={prompt}
      onkeydown={handleKeydown}
      placeholder="Type your design intent... (Cmd+Enter to send)"
      spellcheck="false"
    ></textarea>
    <div class="prompt-actions">
      <button 
        class="btn btn-primary" 
        disabled={isGenerating || !prompt.trim()} 
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
    gap: 12px;
  }

  .version-counter {
    font-size: 0.75rem;
    font-weight: bold;
    color: var(--secondary);
    letter-spacing: 0.05em;
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
</style>
