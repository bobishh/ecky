<script>
  let { history, activeThreadId, onSelect, onDelete, onNew } = $props();

  let searchQuery = $state('');
  let currentPage = $state(1);
  const itemsPerPage = 10;

  const filteredHistory = $derived(
    history.filter(thread => 
      thread.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
      thread.messages.some(m => m.content.toLowerCase().includes(searchQuery.toLowerCase()))
    )
  );

  const totalPages = $derived(Math.max(1, Math.ceil(filteredHistory.length / itemsPerPage)));
  const paginatedHistory = $derived(
    filteredHistory.slice((currentPage - 1) * itemsPerPage, currentPage * itemsPerPage)
  );

  $effect(() => {
    if (currentPage > totalPages) {
      currentPage = totalPages;
    }
  });

  function formatDate(timestamp) {
    return new Date(timestamp * 1000).toLocaleString(undefined, {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit'
    });
  }

  function handleSearch(e) {
    searchQuery = e.target.value;
    currentPage = 1;
  }
</script>

<div class="history-panel">
  <div class="history-search">
    <input 
      type="text" 
      placeholder="Search threads..." 
      value={searchQuery}
      oninput={handleSearch}
      class="search-input"
    />
    <button 
      class="new-thread-btn" 
      onclick={onNew} 
      disabled={activeThreadId === null}
      title="Create New Thread"
    >
      ➕
    </button>
  </div>

  <div class="history-list">
    {#each paginatedHistory as thread (thread.id)}
      <div 
        class="history-card {activeThreadId === thread.id ? 'active' : ''}" 
        onclick={() => onSelect(thread)}
      >
        <div class="card-header">
          <span class="card-title">{thread.title}</span>
          <span class="card-date">{formatDate(thread.updated_at)}</span>
        </div>
        <div class="card-stats">
          {thread.messages.filter(m => m.role === 'assistant' && m.output).length} versions
        </div>
        <div class="card-actions">
          <button 
            class="card-btn delete" 
            onclick={(e) => { e.stopPropagation(); onDelete(thread.id); }}
            title="Delete Thread"
          >
            🗑️
          </button>
        </div>
      </div>
    {:else}
      <div class="empty-state">No threads found.</div>
    {/each}
  </div>

  {#if totalPages > 1}
    <div class="pagination">
      <button 
        disabled={currentPage === 1} 
        onclick={() => currentPage--}
      >
        &lt;
      </button>
      <span class="page-info">{currentPage} / {totalPages}</span>
      <button 
        disabled={currentPage === totalPages} 
        onclick={() => currentPage++}
      >
        &gt;
      </button>
    </div>
  {/if}
</div>

<style>
  .history-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-100);
  }

  .history-search {
    padding: 8px;
    border-bottom: 1px solid var(--bg-300);
    display: flex;
    gap: 8px;
  }

  .search-input {
    flex: 1;
    width: 100%;
    padding: 6px 10px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.75rem;
    outline: none;
  }

  .search-input:focus {
    border-color: var(--primary);
  }

  .new-thread-btn {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.9rem;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 8px;
    transition: all 0.2s;
  }

  .new-thread-btn:hover:not(:disabled) {
    background: var(--bg-300);
    border-color: var(--primary);
    color: var(--primary);
  }

  .new-thread-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .history-list {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .history-card {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    padding: 8px;
    cursor: pointer;
    transition: all 0.2s;
    user-select: none;
    position: relative;
  }

  .history-card:hover {
    border-color: var(--primary);
    background: var(--bg-300);
  }

  .history-card.active {
    border-color: var(--primary);
    background: var(--bg-300);
    box-shadow: inset 3px 0 0 var(--primary);
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 4px;
    gap: 8px;
  }

  .card-title {
    font-weight: bold;
    font-size: 0.75rem;
    color: var(--primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-date {
    font-size: 0.6rem;
    color: var(--text-dim);
    white-space: nowrap;
  }

  .card-stats {
    font-size: 0.65rem;
    color: var(--text-dim);
    margin-bottom: 8px;
  }

  .card-actions {
    display: flex;
    justify-content: flex-end;
    gap: 4px;
  }

  .card-btn {
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    font-size: 0.6rem;
    padding: 2px 6px;
    cursor: pointer;
    text-transform: uppercase;
    font-weight: bold;
  }

  .card-btn:hover {
    background: var(--bg-400);
    color: var(--primary);
  }

  .card-btn.delete {
    color: var(--red);
  }

  .card-btn.delete:hover {
    background: var(--red);
    color: white;
  }

  .empty-state {
    padding: 20px;
    text-align: center;
    color: var(--text-dim);
    font-size: 0.75rem;
  }

  .pagination {
    padding: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    border-top: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .pagination button {
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    padding: 2px 8px;
    cursor: pointer;
  }

  .pagination button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .page-info {
    font-size: 0.7rem;
    color: var(--text-dim);
  }
</style>
