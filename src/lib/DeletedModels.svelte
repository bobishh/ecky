<script>
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { restoreVersion } from './stores/history';

  let deletedMessages = $state([]);
  let isLoading = $state(true);

  async function loadDeleted() {
    isLoading = true;
    try {
      deletedMessages = await invoke('get_deleted_messages');
    } catch (e) {
      console.error('Failed to load deleted messages:', e);
    } finally {
      isLoading = false;
    }
  }

  async function handleRestore(id) {
    await restoreVersion(id);
    await loadDeleted();
  }

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleString();
  }

  onMount(loadDeleted);
</script>

<div class="deleted-models-page">
  <div class="page-header">
    <div class="title-group">
      <h2>Trash</h2>
      <p class="subtitle">Individual iterations you've discarded from your design threads.</p>
    </div>
  </div>

  {#if isLoading}
    <div class="loading-state">Loading trash...</div>
  {:else if deletedMessages.length === 0}
    <div class="empty-state">
      <div class="empty-icon">🗑️</div>
      <p>Your trash is empty.</p>
    </div>
  {:else}
    <div class="deleted-grid">
      {#each deletedMessages as msg (msg.id)}
        <div class="deleted-card">
          <div class="card-thumb">
            {#if msg.imageData}
              <img src={msg.imageData} alt="Model Preview" />
            {:else}
              <div class="no-thumb">NO PREVIEW</div>
            {/if}
          </div>
          <div class="card-content">
            <div class="card-header">
              <span class="thread-tag">{msg.threadTitle || 'Unknown Thread'}</span>
              <span class="date-tag">DELETED: {formatDate(msg.deletedAt || msg.timestamp)}</span>
            </div>
            <h3 class="model-title">{msg.output?.title || 'Untitled Model'}</h3>
            <p class="model-version">{msg.output?.versionName || 'Original Version'}</p>
            <div class="model-summary">
              {msg.output?.response || msg.content.slice(0, 100)}...
            </div>
            <div class="card-actions">
              <button class="btn btn-primary" onclick={() => handleRestore(msg.id)}>RECOVER</button>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .deleted-models-page {
    padding: 40px;
    height: 100%;
    overflow-y: auto;
    background: var(--bg);
    color: var(--text);
  }

  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 32px;
    border-bottom: 1px solid var(--bg-300);
    padding-bottom: 20px;
  }

  .title-group h2 {
    font-size: 1.5rem;
    color: var(--secondary);
    letter-spacing: 0.1em;
    margin-bottom: 8px;
  }

  .subtitle {
    font-size: 0.85rem;
    color: var(--text-dim);
  }

  .loading-state, .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 100px;
    color: var(--text-dim);
    font-size: 1rem;
    gap: 16px;
  }

  .empty-icon { font-size: 3rem; }

  .deleted-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 24px;
  }

  .deleted-card {
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transition: transform 0.2s, border-color 0.2s;
  }

  .deleted-card:hover {
    border-color: var(--primary);
    transform: translateY(-4px);
  }

  .card-thumb {
    height: 180px;
    background: #000;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    border-bottom: 1px solid var(--bg-300);
  }

  .card-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.8;
  }

  .no-thumb {
    font-size: 0.7rem;
    color: var(--bg-400);
    letter-spacing: 0.2em;
  }

  .card-content {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    font-size: 0.65rem;
    font-weight: bold;
  }

  .thread-tag {
    color: var(--primary);
    text-transform: uppercase;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .date-tag {
    color: var(--text-dim);
  }

  .model-title {
    font-size: 1.1rem;
    color: var(--text);
    margin: 0;
  }

  .model-version {
    font-size: 0.75rem;
    color: var(--secondary);
    font-weight: bold;
    margin: 0;
  }

  .model-summary {
    font-size: 0.8rem;
    color: var(--text-dim);
    line-height: 1.5;
    height: 3.6em;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
  }

  .card-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }

  .btn {
    padding: 8px 16px;
    font-size: 0.75rem;
    font-weight: bold;
    cursor: pointer;
    border: 1px solid transparent;
  }

  .btn-primary {
    background: var(--primary);
    color: var(--bg-100);
  }

  .btn-primary:hover {
    background: color-mix(in srgb, var(--primary) 80%, white);
  }

</style>
