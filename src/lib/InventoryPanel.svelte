<script lang="ts">
  import { onMount } from 'svelte';
  import { reopenThread, loadInventory } from './stores/history';
  import type { Thread } from './types/domain';

  let threads = $state<Thread[]>([]);
  let isLoading = $state(true);
  let pendingActionId = $state<string | null>(null);

  async function load() {
    isLoading = true;
    try {
      threads = await loadInventory();
    } finally {
      isLoading = false;
    }
  }

  async function handleReopen(id: string) {
    pendingActionId = id;
    try {
      await reopenThread(id);
      threads = threads.filter((t) => t.id !== id);
    } finally {
      pendingActionId = null;
    }
  }

  function formatDate(ts: number) {
    return new Date(ts * 1000).toLocaleString(undefined, {
      month: 'short', day: 'numeric', year: 'numeric',
      hour: '2-digit', minute: '2-digit',
    });
  }

  onMount(() => {
    void load();
  });
</script>

<div class="inventory-page">
  <div class="page-header">
    <div class="title-group">
      <h2>Inventory</h2>
      <p class="subtitle">Finalized design sessions. Completed work, ready for review or re-opening.</p>
    </div>
    <button class="refresh-btn" onclick={load} disabled={isLoading}>
      {isLoading ? 'LOADING...' : 'REFRESH'}
    </button>
  </div>

  {#if isLoading}
    <div class="loading-state">Loading inventory...</div>
  {:else if threads.length === 0}
    <div class="empty-state">
      <div class="empty-icon">📦</div>
      <p>No finalized sessions yet.</p>
      <p class="hint">Finalize a thread from the session list to archive it here.</p>
    </div>
  {:else}
    <div class="inventory-list">
      {#each threads as thread (thread.id)}
        <div class="inventory-card">
          <div class="card-content">
            <div class="card-header">
              <span class="card-title">{thread.title}</span>
              {#if thread.finalizedAt}
                <span class="finalized-tag">FINALIZED {formatDate(thread.finalizedAt)}</span>
              {/if}
            </div>
            {#if thread.summary}
              <p class="card-summary">{thread.summary}</p>
            {/if}
            <div class="card-stats">
              {thread.versionCount ?? 0} {(thread.versionCount ?? 0) === 1 ? 'version' : 'versions'}
              {#if thread.errorCount && thread.errorCount > 0}
                · {thread.errorCount} {thread.errorCount === 1 ? 'error' : 'errors'}
              {/if}
            </div>
            <div class="card-actions">
              <button
                class="btn btn-primary"
                onclick={() => handleReopen(thread.id)}
                disabled={pendingActionId === thread.id}
              >
                {pendingActionId === thread.id ? 'OPENING...' : 'REOPEN'}
              </button>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .inventory-page {
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

  .refresh-btn {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text-dim);
    padding: 8px 16px;
    font-size: 0.7rem;
    font-weight: bold;
    letter-spacing: 0.08em;
    cursor: pointer;
  }

  .refresh-btn:hover:not(:disabled) {
    border-color: var(--primary);
    color: var(--primary);
  }

  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 100px;
    color: var(--text-dim);
    font-size: 1rem;
    gap: 16px;
    text-align: center;
  }

  .empty-icon {
    font-size: 3rem;
  }

  .hint {
    font-size: 0.8rem;
    color: var(--bg-400);
  }

  .inventory-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .inventory-card {
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    transition: border-color 0.2s;
  }

  .inventory-card:hover {
    border-color: var(--secondary);
  }

  .card-content {
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }

  .card-title {
    font-size: 1rem;
    font-weight: bold;
    color: var(--text);
    flex: 1;
  }

  .finalized-tag {
    font-size: 0.65rem;
    font-weight: bold;
    color: var(--secondary);
    letter-spacing: 0.08em;
    white-space: nowrap;
  }

  .card-summary {
    font-size: 0.8rem;
    color: var(--text-dim);
    line-height: 1.5;
    margin: 0;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .card-stats {
    font-size: 0.72rem;
    color: var(--bg-400);
    letter-spacing: 0.04em;
  }

  .card-actions {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
    margin-top: 4px;
  }

  .btn {
    padding: 7px 16px;
    font-size: 0.72rem;
    font-weight: bold;
    cursor: pointer;
    border: 1px solid transparent;
    letter-spacing: 0.06em;
  }

  .btn-primary {
    background: transparent;
    border-color: var(--secondary);
    color: var(--secondary);
  }

  .btn-primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--secondary) 20%, transparent);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: wait;
  }
</style>
