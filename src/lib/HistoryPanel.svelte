<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import ManualImportModal from './ManualImportModal.svelte';
  import Modal from './Modal.svelte';
  import type { Thread, AgentSession } from './types/domain';

  type NewThreadPayload =
    | { mode: 'blank' }
    | { mode: 'macro'; code: string; title: string };

  type ThreadState = {
    label: string;
    className: string;
    title: string;
  };

  let {
    history,
    activeThreadId,
    inFlightByThread = {},
    activeAgentSessions = [],
    onSelect,
    onDelete,
    onRename,
    onNew,
    onImportFcstd,
    onFinalize,
  }: {
    history: Thread[];
    activeThreadId: string | null;
    inFlightByThread?: Record<string, number>;
    activeAgentSessions?: AgentSession[];
    onSelect: (thread: Thread) => void;
    onDelete: (id: string) => void;
    onRename: (id: string, title: string) => Promise<void> | void;
    onNew: (payload: NewThreadPayload) => void;
    onImportFcstd: (sourcePath: string) => void;
    onFinalize?: (id: string) => void;
  } = $props();

  let searchQuery = $state('');
  let currentPage = $state(1);
  let showImport = $state(false);
  let showNewChooser = $state(false);
  let threadToDelete = $state<Thread | null>(null);
  let editingThreadId = $state<string | null>(null);
  let editingTitle = $state('');
  let renameBusy = $state(false);
  const itemsPerPage = 10;

  const filteredHistory = $derived(
    history.filter(thread => 
      thread.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
      (thread.summary && thread.summary.toLowerCase().includes(searchQuery.toLowerCase()))
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

  function formatDate(timestamp: number) {
    return new Date(timestamp * 1000).toLocaleString(undefined, {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit'
    });
  }

  function handleSearch(e: Event) {
    searchQuery = (e.currentTarget as HTMLInputElement).value;
    currentPage = 1;
  }

  function handleImport(data: { code: string; title: string }) {
    showImport = false;
    onNew({ mode: 'macro', ...data });
  }

  async function handleFcstdImport() {
    showNewChooser = false;
    const selected = await open({
      multiple: false,
      filters: [{ name: 'FreeCAD Document', extensions: ['fcstd'] }],
    });

    if (typeof selected === 'string' && selected.trim()) {
      onImportFcstd(selected);
    }
  }

  function startBlankThread() {
    showNewChooser = false;
    onNew({ mode: 'blank' });
  }

  function startMacroImport() {
    showNewChooser = false;
    showImport = true;
  }

  function confirmDelete(id: string) {
    const thread = history.find(t => t.id === id);
    if (thread) {
      threadToDelete = thread;
    }
  }

  function executeDelete() {
    if (threadToDelete) {
      onDelete(threadToDelete.id);
      threadToDelete = null;
    }
  }

  function hasTextSelection() {
    const selection = window.getSelection();
    return !!selection && !selection.isCollapsed && selection.toString().trim().length > 0;
  }

  function selectThread(thread: Thread) {
    if (editingThreadId === thread.id) return;
    if (hasTextSelection()) return;
    onSelect(thread);
  }

  function startRename(thread: Thread, event?: Event) {
    event?.stopPropagation();
    editingThreadId = thread.id;
    editingTitle = thread.title;
  }

  function cancelRename(event?: Event) {
    event?.stopPropagation();
    editingThreadId = null;
    editingTitle = '';
    renameBusy = false;
  }

  async function commitRename(thread: Thread, event?: Event) {
    event?.stopPropagation();
    if (renameBusy) return;
    const trimmed = editingTitle.trim();
    if (!trimmed) {
      cancelRename();
      return;
    }
    if (trimmed === thread.title.trim()) {
      cancelRename();
      return;
    }
    renameBusy = true;
    try {
      await onRename(thread.id, trimmed);
      editingThreadId = null;
      editingTitle = '';
    } finally {
      renameBusy = false;
    }
  }

  function pluralize(count: number, noun: string) {
    return `${count} ${noun}${count === 1 ? '' : 's'}`;
  }

  function isLiveAgentPhase(phase: string) {
    return phase !== 'idle' && phase !== 'error';
  }

  function hasImportPendingSetup(thread: Thread): boolean {
    return thread.messages.some(
      (m) =>
        m.modelManifest?.sourceKind === 'importedFcstd' &&
        m.modelManifest?.enrichmentState?.status === 'pending',
    );
  }

  function getThreadState(thread: Thread): ThreadState {
    const inFlightCount = Number(inFlightByThread?.[thread?.id] || 0);
    const agentSession = (activeAgentSessions || []).find(
      (s) => s.threadId === thread.id && isLiveAgentPhase(s.phase),
    );
    const pendingCount = Number(thread?.pendingCount || 0);
    const errorCount = Number(thread?.errorCount || 0);
    const versionCount = Number(thread?.versionCount || 0);

    if (inFlightCount > 0) {
      return {
        label: 'RUNNING',
        className: 'running',
        title: `${pluralize(inFlightCount, 'request')} currently in progress`
      };
    }

    if (agentSession) {
      const sessionLabel =
        agentSession.llmModelLabel || agentSession.agentLabel || agentSession.hostLabel;
      return {
        label: sessionLabel.toUpperCase(),
        className: 'agent-active',
        title: `Active agent session: ${agentSession.hostLabel}${agentSession.llmModelLabel ? ` / ${agentSession.llmModelLabel}` : ''} (${agentSession.phase})`
      };
    }

    if (errorCount > 0 && versionCount === 0) {
      return {
        label: 'FAILED',
        className: 'failed',
        title: `${pluralize(errorCount, 'failed attempt')} and no successful versions`
      };
    }

    if (errorCount > 0) {
      return {
        label: 'ISSUES',
        className: 'issues',
        title: `${pluralize(errorCount, 'failed attempt')} across ${pluralize(versionCount, 'version')}`
      };
    }

    if (pendingCount > 0) {
      return {
        label: 'PENDING',
        className: 'pending',
        title: `${pluralize(pendingCount, 'pending attempt')} saved in history`
      };
    }

    if (versionCount > 0) {
      return {
        label: 'DONE',
        className: 'done',
        title: `${pluralize(versionCount, 'successful version')}`
      };
    }

    return {
      label: 'EMPTY',
      className: 'empty',
      title: 'Thread initialized but no generated versions yet'
    };
  }
</script>

<div class="history-panel">
  {#if showImport}
    <ManualImportModal bind:show={showImport} onImport={handleImport} />
  {/if}

  {#if threadToDelete}
    <Modal title="Confirm Deletion" onclose={() => threadToDelete = null}>
      <div class="confirm-delete-body">
        <p>Are you sure you want to purge <strong>{threadToDelete.title}</strong>?</p>
        <p class="warning">
          This will hide {threadToDelete.versionCount} {threadToDelete.versionCount === 1 ? 'model' : 'models'} from your history.
        </p>
        <p class="minor">You can technically recover this from the database if you're desperate enough, but it won't be fun.</p>
        
        <div class="confirm-actions">
          <button class="btn btn-secondary" onclick={() => threadToDelete = null}>CANCEL</button>
          <button class="btn btn-danger" onclick={executeDelete}>DELETE FOREVER*</button>
        </div>
      </div>
    </Modal>
  {/if}

  {#if showNewChooser}
    <Modal title="Start New Thread" onclose={() => showNewChooser = false}>
      <div class="new-thread-chooser">
        <button class="chooser-action" onclick={startBlankThread}>
          <span class="chooser-icon" aria-hidden="true">➕</span>
          <span class="chooser-copy">
            <span class="chooser-title">Blank Thread</span>
            <span class="chooser-subtitle">Start from a fresh prompt.</span>
          </span>
        </button>
        <button class="chooser-action" onclick={handleFcstdImport}>
          <span class="chooser-icon" aria-hidden="true">📦</span>
          <span class="chooser-copy">
            <span class="chooser-title">Import FCStd</span>
            <span class="chooser-subtitle">Open an existing FreeCAD document.</span>
          </span>
        </button>
        <button class="chooser-action" onclick={startMacroImport}>
          <span class="chooser-icon" aria-hidden="true">📜</span>
          <span class="chooser-copy">
            <span class="chooser-title">Import Macro</span>
            <span class="chooser-subtitle">Paste or load Python/FCMacro code.</span>
          </span>
        </button>
      </div>
    </Modal>
  {/if}

  <div class="history-search">
    <input 
      type="text" 
      placeholder="Search threads..." 
      value={searchQuery}
      oninput={handleSearch}
      class="search-input"
    />
    <div class="header-actions">
      <button 
        class="header-btn" 
        onclick={() => showNewChooser = true} 
        title="Start a new thread"
      >
        <span class="header-btn-icon" aria-hidden="true">➕</span>
        <span class="header-btn-label">NEW</span>
      </button>
    </div>
  </div>

  <div class="history-list">
    {#each paginatedHistory as thread (thread.id)}
      {@const threadState = getThreadState(thread)}
      <div 
        class="history-card status-{threadState.className} {activeThreadId === thread.id ? 'active' : ''}" 
        role="button"
        tabindex="0"
        onclick={() => selectThread(thread)}
        onkeydown={(e) => { if (editingThreadId !== thread.id && (e.key === 'Enter' || e.key === ' ')) onSelect(thread); }}
      >
        <div class="card-header">
          {#if editingThreadId === thread.id}
            <input
              class="card-title-input"
              bind:value={editingTitle}
              onclick={(e) => e.stopPropagation()}
              onkeydown={(e) => {
                if (e.key === 'Enter') void commitRename(thread, e);
                if (e.key === 'Escape') cancelRename(e);
              }}
            />
          {:else}
            <button
              class="card-title-button"
              onclick={(e) => startRename(thread, e)}
              title="Rename thread"
            >
              <span class="card-title">{thread.title}</span>
              <span class="card-title-pencil" aria-hidden="true">✎</span>
            </button>
          {/if}
          <span class="status-badge {threadState.className}" title={threadState.title}>{threadState.label}</span>
          <span class="card-date">{formatDate(thread.updatedAt)}</span>
          {#if hasImportPendingSetup(thread)}
            <span class="status-badge needs-setup" title="Imported model has pending parameter binding proposals">NEEDS SETUP</span>
          {/if}
        </div>
        {#if thread.summary}
          <div class="card-summary">{thread.summary}</div>
        {/if}
        <div class="card-stats">
          {#if thread.versionCount > 0}
            {thread.versionCount} {thread.versionCount === 1 ? 'version' : 'versions'}
          {:else}
            No successful versions
          {/if}
        </div>
        <div class="card-actions">
          {#if editingThreadId === thread.id}
            <button
              class="card-btn rename"
              onclick={(e) => void commitRename(thread, e)}
              disabled={renameBusy}
              title="Save title"
            >
              ✓
            </button>
            <button
              class="card-btn rename"
              onclick={cancelRename}
              disabled={renameBusy}
              title="Cancel rename"
            >
              ✕
            </button>
          {:else}
            <button
              class="card-btn rename"
              onclick={(e) => startRename(thread, e)}
              title="Rename Thread"
            >
              ✎
            </button>
            {#if onFinalize && thread.versionCount > 0}
              <button
                class="card-btn finalize"
                onclick={(e) => { e.stopPropagation(); onFinalize(thread.id); }}
                title="Finalize — move to inventory"
              >
                ✓
              </button>
            {/if}
            <button
              class="card-btn delete"
              onclick={(e) => { e.stopPropagation(); confirmDelete(thread.id); }}
              title="Delete Thread"
            >
              🗑️
            </button>
          {/if}
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

  .header-actions {
    display: flex;
    gap: 4px;
  }

  .header-btn {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-width: 74px;
    padding: 0 10px;
    transition: all 0.2s;
  }

  .header-btn:hover {
    background: var(--bg-300);
    border-color: var(--primary);
    color: var(--primary);
  }

  .header-btn-icon {
    font-size: 0.85rem;
    line-height: 1;
  }

  .header-btn-label {
    font-size: 0.6rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    line-height: 1;
  }

  .new-thread-chooser {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow: hidden;
  }

  .chooser-action {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px;
    text-align: left;
    cursor: pointer;
    transition: all 0.2s;
    overflow: hidden;
  }

  .chooser-action:hover {
    border-color: var(--primary);
    background: var(--bg-300);
  }

  .chooser-icon {
    width: 30px;
    flex: 0 0 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 1rem;
    line-height: 1;
  }

  .chooser-copy {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .chooser-title {
    font-size: 0.72rem;
    font-weight: 700;
    color: var(--text);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .chooser-subtitle {
    font-size: 0.65rem;
    color: var(--text-dim);
    line-height: 1.35;
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
    border-left: 3px solid var(--bg-400);
    padding: 8px;
    cursor: pointer;
    transition: all 0.2s;
    user-select: none;
    position: relative;
  }

  .history-card.status-running { border-left-color: var(--primary); }
  .history-card.status-agent-active { border-left-color: var(--secondary); }
  .history-card.status-pending { border-left-color: color-mix(in srgb, var(--secondary) 45%, var(--bg-300)); }
  .history-card.status-failed { border-left-color: var(--red); }
  .history-card.status-issues { border-left-color: var(--secondary); }
  .history-card.status-done { border-left-color: color-mix(in srgb, var(--secondary) 60%, var(--bg-300)); }
  .history-card.status-empty { border-left-color: var(--bg-400); }

  .history-card:hover {
    border-color: var(--primary);
    background: var(--bg-300);
  }

  .history-card.active {
    border-color: var(--primary);
    background: var(--bg-300);
    box-shadow: inset 1px 0 0 var(--primary);
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
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .card-title-button {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    cursor: text;
    text-align: left;
    overflow: hidden;
  }

  .card-title-button:hover .card-title,
  .card-title-button:hover .card-title-pencil {
    color: var(--primary);
  }

  .card-title-button:focus-visible {
    outline: 1px solid var(--primary);
    outline-offset: 2px;
  }

  .card-title-pencil {
    flex: 0 0 auto;
    font-size: 0.65rem;
    color: var(--text-dim);
    opacity: 0.75;
  }

  .card-title-input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--primary);
    background: var(--bg-100);
    color: var(--text);
    font-size: 0.75rem;
    font-weight: bold;
    outline: none;
  }

  .history-card.active .card-title {
    color: var(--primary);
  }

  .status-badge {
    font-size: 0.5rem;
    padding: 1px 5px;
    border: 1px solid var(--bg-400);
    background: var(--bg-100);
    color: var(--text-dim);
    font-weight: bold;
    letter-spacing: 0.05em;
    white-space: nowrap;
  }

  .status-badge.running {
    border-color: var(--primary);
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 12%, var(--bg-100));
    animation: status-pulse 2s infinite;
  }

  .status-badge.agent-active {
    border-color: var(--secondary);
    color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 12%, var(--bg-100));
    animation: status-pulse 4s infinite;
  }

  .status-badge.pending {
    border-color: color-mix(in srgb, var(--secondary) 50%, var(--bg-300));
    color: color-mix(in srgb, var(--secondary) 85%, var(--text));
    background: color-mix(in srgb, var(--secondary) 8%, var(--bg-100));
  }

  .status-badge.failed {
    border-color: color-mix(in srgb, var(--red) 80%, #000 20%);
    background: var(--red);
    color: white;
  }

  .status-badge.issues {
    border-color: var(--secondary);
    color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-100));
  }

  .status-badge.done {
    border-color: color-mix(in srgb, var(--secondary) 60%, var(--bg-300));
    color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 10%, var(--bg-100));
  }

  .status-badge.empty {
    border-color: var(--bg-400);
    color: var(--text-dim);
    background: color-mix(in srgb, var(--bg-300) 35%, var(--bg-100));
  }

  .status-badge.needs-setup {
    border-color: color-mix(in srgb, var(--primary) 65%, var(--bg-300));
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 15%, var(--bg-100));
    animation: status-pulse 3s infinite;
  }

  @keyframes status-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  .card-date {
    font-size: 0.6rem;
    color: var(--text-dim);
    white-space: nowrap;
  }

  .card-summary {
    font-size: 0.65rem;
    color: var(--text);
    margin-bottom: 8px;
    line-clamp: 2;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    line-height: 1.3;
    cursor: text;
    -webkit-user-select: text;
    user-select: text;
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

  .card-btn.rename {
    border-color: color-mix(in srgb, var(--primary) 45%, var(--bg-300));
  }

  .card-btn.rename:hover {
    border-color: var(--primary);
    color: var(--primary);
  }

  .card-btn.delete:hover {
    background: var(--red);
    color: white;
  }

  .card-btn.finalize {
    color: var(--secondary);
    border-color: color-mix(in srgb, var(--secondary) 45%, var(--bg-300));
  }

  .card-btn.finalize:hover {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 20%, var(--bg-300));
    color: var(--secondary);
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

  .confirm-delete-body .minor {
    font-size: 0.7rem;
    color: var(--text-dim);
    font-style: italic;
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
