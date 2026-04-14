<script lang="ts">
  import { onMount } from 'svelte';
  import {
    loadFromHistory,
    deleteThread,
    renameThread,
    createNewThread,
    finalizeThread,
    reopenThread,
    loadInventory,
    restoreVersion,
    activeThreadLoadingId,
  } from './stores/history';
  import { historyStore as history, activeThreadIdStore as activeThreadId } from './stores/domainState';
  import {
    getDeletedMessages,
    hideDeletedMessage,
    getThreadLatestVersion,
    getThreadMessagesPage,
    formatBackendError,
  } from './tauri/client';
  import type { Thread, DeletedMessage, Message } from './types/domain';
  import Modal from './Modal.svelte';
  import ManualImportModal from './ManualImportModal.svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';

  let {
    onImportFcstd,
    freecadUnavailableReason = null,
  }: {
    onImportFcstd?: (sourcePath: string) => void;
    freecadUnavailableReason?: string | null;
  } = $props();

  type Tab = 'in-work' | 'archived' | 'trash';
  let activeTab = $state<Tab>('in-work');
  let searchQuery = $state('');
  let isLoading = $state(false);
  let archivedThreads = $state<Thread[]>([]);
  let deletedMessages = $state<DeletedMessage[]>([]);
  let latestVersions = $state<Record<string, Message | null>>({});
  let previewImages = $state<Record<string, string | null>>({});
  let previewLoading = $state<Record<string, boolean>>({});
  let archivedLoaded = $state(false);
  let trashLoaded = $state(false);

  onMount(() => {
    const onPreviewUpdated = (event: Event) => {
      const detail = (event as CustomEvent<{
        threadId?: string;
        messageId?: string;
        imageData?: string;
      }>).detail;
      if (!detail?.threadId || !detail.imageData) return;
      previewImages = { ...previewImages, [detail.threadId]: detail.imageData };
      const latest = latestVersions[detail.threadId];
      if (latest && latest.id === detail.messageId) {
        latestVersions = {
          ...latestVersions,
          [detail.threadId]: { ...latest, imageData: detail.imageData },
        };
      }
    };
    window.addEventListener('ecky:version-preview-updated', onPreviewUpdated);
    return () => window.removeEventListener('ecky:version-preview-updated', onPreviewUpdated);
  });

  function previewSrc(raw: string | null | undefined): string | null {
    const value = raw?.trim();
    if (!value) return null;
    if (/^(data:image\/|blob:|https?:|asset:|tauri:)/i.test(value)) return value;
    try {
      return convertFileSrc(value);
    } catch {
      return value;
    }
  }

  function threadPreviewImage(thread: Thread): string | null {
    const latest = latestVersions[thread.id];
    const latestPreview = previewSrc(latest?.imageData);
    if (latestPreview) return latestPreview;
    if (previewImages[thread.id] !== undefined) return previewSrc(previewImages[thread.id]);
    const fallback = [...(thread.messages || [])].reverse().find((message) => message.imageData);
    return previewSrc(fallback?.imageData);
  }

  async function loadData() {
    if (
      activeTab === 'in-work' ||
      (activeTab === 'archived' && archivedLoaded) ||
      (activeTab === 'trash' && trashLoaded)
    ) {
      return;
    }
    isLoading = true;
    try {
      if (activeTab === 'archived') {
        archivedThreads = await loadInventory();
        archivedLoaded = true;
      } else if (activeTab === 'trash') {
        deletedMessages = await getDeletedMessages();
        trashLoaded = true;
      }
    } catch (e) {
      console.error('Failed to load projects:', formatBackendError(e));
    } finally {
      isLoading = false;
    }
  }

  $effect(() => {
    void loadData();
  });

  async function fetchLatestVersion(threadId: string) {
    if (latestVersions[threadId] !== undefined) return;
    try {
      const version = await getThreadLatestVersion(threadId);
      latestVersions = { ...latestVersions, [threadId]: version };
      if (version?.imageData) {
        previewImages = { ...previewImages, [threadId]: version.imageData };
        return;
      }
    } catch (e) {
      console.error(`Failed to fetch latest version for ${threadId}:`, e);
      latestVersions = { ...latestVersions, [threadId]: null };
    }
    void fetchPreviewImage(threadId);
  }

  async function fetchPreviewImage(threadId: string) {
    if (previewImages[threadId] !== undefined || previewLoading[threadId]) return;
    previewLoading = { ...previewLoading, [threadId]: true };
    try {
      const page = await getThreadMessagesPage(threadId, null, 100, true);
      const previewMessage =
        [...page.messages].reverse().find((message) => message.imageData) ??
        [...page.messages].reverse().find((message) => message.attachmentImages?.length);
      previewImages = {
        ...previewImages,
        [threadId]: previewMessage?.imageData ?? previewMessage?.attachmentImages?.[0] ?? null,
      };
    } catch (e) {
      console.error(`Failed to fetch preview image for ${threadId}:`, e);
      previewImages = { ...previewImages, [threadId]: null };
    } finally {
      previewLoading = { ...previewLoading, [threadId]: false };
    }
  }

  // Pre-fetch latest versions for visible cards in current tab
  $effect(() => {
    const previewLimit = 24;
    if (activeTab === 'in-work') {
      filteredInWork.slice(0, previewLimit).forEach(t => fetchLatestVersion(t.id));
    } else if (activeTab === 'archived') {
      filteredArchived.slice(0, previewLimit).forEach(t => fetchLatestVersion(t.id));
    }
  });

  const filteredInWork = $derived(
    $history.filter((t: Thread) =>
      t.status !== 'finalized' &&
      (
        t.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
        Boolean(t.summary && t.summary.toLowerCase().includes(searchQuery.toLowerCase()))
      )
    )
  );

  const filteredArchived = $derived(
    archivedThreads.filter((t: Thread) => 
      t.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
      (t.summary && t.summary.toLowerCase().includes(searchQuery.toLowerCase()))
    )
  );

  const filteredTrash = $derived(
    deletedMessages.filter((m: DeletedMessage) => 
      (m.threadTitle && m.threadTitle.toLowerCase().includes(searchQuery.toLowerCase())) ||
      (m.output?.title && m.output.title.toLowerCase().includes(searchQuery.toLowerCase())) ||
      (m.output?.versionName && m.output.versionName.toLowerCase().includes(searchQuery.toLowerCase()))
    )
  );

  let showNewChooser = $state(false);
  let showImport = $state(false);
  let threadToDelete = $state<Thread | null>(null);
  let editingThreadId = $state<string | null>(null);
  let editingTitle = $state('');
  let renameBusy = $state(false);
  let pendingActionId = $state<string | null>(null);

  // Actions
  function handleSelect(thread: Thread) {
    if (editingThreadId === thread.id) return;
    loadFromHistory(thread);
  }

  async function handleArchive(id: string) {
    pendingActionId = id;
    try {
      await finalizeThread(id);
      archivedThreads = await loadInventory();
      archivedLoaded = true;
    } finally {
      pendingActionId = null;
    }
  }

  async function handleReopen(id: string) {
    pendingActionId = id;
    try {
      await reopenThread(id);
      archivedThreads = archivedThreads.filter(t => t.id !== id);
    } finally {
      pendingActionId = null;
    }
  }

  async function handleRestoreTrash(id: string) {
    pendingActionId = id;
    try {
      await restoreVersion(id);
      deletedMessages = await getDeletedMessages();
      trashLoaded = true;
    } finally {
      pendingActionId = null;
    }
  }

  async function handleHideTrash(id: string) {
    pendingActionId = id;
    try {
      await hideDeletedMessage(id);
      deletedMessages = deletedMessages.filter(m => m.id !== id);
    } finally {
      pendingActionId = null;
    }
  }

  function startRename(thread: Thread) {
    editingThreadId = thread.id;
    editingTitle = thread.title;
  }

  function cancelRename() {
    editingThreadId = null;
    editingTitle = '';
  }

  async function commitRename(thread: Thread) {
    if (renameBusy) return;
    const trimmed = editingTitle.trim();
    if (!trimmed || trimmed === thread.title) {
      cancelRename();
      return;
    }
    renameBusy = true;
    try {
      await renameThread(thread.id, trimmed);
      cancelRename();
    } finally {
      renameBusy = false;
    }
  }

  function formatDate(ts: number) {
    return new Date(ts * 1000).toLocaleString(undefined, {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit'
    });
  }

  async function handleImportFcstd() {
    if (freecadUnavailableReason) return;
    showNewChooser = false;
    const selected = await open({
      multiple: false,
      filters: [{ name: 'FreeCAD Document', extensions: ['fcstd'] }],
    });
    if (typeof selected === 'string' && selected.trim()) {
      onImportFcstd?.(selected);
    }
  }
</script>

<div class="project-switcher">
  <div class="switcher-header">
    <div class="tabs">
      <button class="tab-btn" class:active={activeTab === 'in-work'} onclick={() => activeTab = 'in-work'}>IN WORK</button>
      <button class="tab-btn" class:active={activeTab === 'archived'} onclick={() => activeTab = 'archived'}>ARCHIVED</button>
      <button class="tab-btn" class:active={activeTab === 'trash'} onclick={() => activeTab = 'trash'}>TRASH</button>
    </div>
    <div class="header-actions">
      <input type="text" placeholder="Search..." bind:value={searchQuery} class="search-input" />
      <button class="new-btn" onclick={() => showNewChooser = true}>+ NEW</button>
    </div>
  </div>

  <div class="switcher-content scrollable">
    {#if isLoading}
      <div class="loading-state">Loading...</div>
    {:else}
      <div class="project-grid">
        {#if activeTab === 'in-work'}
          {#each filteredInWork as thread (thread.id)}
            <div class="project-card" class:active={$activeThreadId === thread.id}>
              <div class="card-thumb">
                {#if threadPreviewImage(thread)}
                  <img src={threadPreviewImage(thread) ?? ''} alt="Preview" />
                {:else}
                  <div class="no-thumb">NO PREVIEW</div>
                {/if}
              </div>
              <div class="card-body">
                <div class="card-header">
                  {#if editingThreadId === thread.id}
                    <input class="rename-input" bind:value={editingTitle} onkeydown={(e) => e.key === 'Enter' && commitRename(thread)} onblur={() => cancelRename()} />
                  {:else}
                    <h3 ondblclick={() => startRename(thread)}>{thread.title}</h3>
                  {/if}
                  <span class="date">{formatDate(thread.updatedAt)}</span>
                </div>
                {#if thread.summary}
                  <p class="summary">{thread.summary}</p>
                {/if}
                <div class="card-footer">
                  <div class="stats">{thread.versionCount || 0} versions</div>
                  <div class="actions">
                    <button class="btn-text" onclick={() => handleSelect(thread)} title="Open">OPEN</button>
                    <button class="btn-text" onclick={() => handleArchive(thread.id)} title="Archive">ARCHIVE</button>
                    <button class="btn-text delete" onclick={() => { threadToDelete = thread }} title="Delete">DELETE</button>
                  </div>
                </div>
              </div>
            </div>
          {/each}
        {:else if activeTab === 'archived'}
          {#each filteredArchived as thread (thread.id)}
            <div class="project-card">
              <div class="card-thumb">
                {#if threadPreviewImage(thread)}
                  <img src={threadPreviewImage(thread) ?? ''} alt="Preview" />
                {:else}
                  <div class="no-thumb">NO PREVIEW</div>
                {/if}
              </div>
              <div class="card-body">
                <div class="card-header">
                  <h3>{thread.title}</h3>
                  <span class="date">{formatDate(thread.updatedAt)}</span>
                </div>
                {#if thread.summary}
                  <p class="summary">{thread.summary}</p>
                {/if}
                <div class="card-footer">
                  <div class="stats">{thread.versionCount || 0} versions</div>
                  <div class="actions">
                    <button class="btn-text" onclick={() => handleSelect(thread)} title="Open">OPEN</button>
                    <button class="btn-text" onclick={() => handleReopen(thread.id)}>REOPEN</button>
                  </div>
                </div>
              </div>
            </div>
          {/each}
        {:else if activeTab === 'trash'}
          {#each filteredTrash as msg (msg.id)}
            <div class="project-card">
              <div class="card-thumb">
                {#if previewSrc(msg.imageData)}
                  <img src={previewSrc(msg.imageData) ?? ''} alt="Preview" />
                {:else}
                  <div class="no-thumb">NO PREVIEW</div>
                {/if}
              </div>
              <div class="card-body">
                <div class="card-header">
                  <h3>{msg.output?.title || 'Untitled Model'}</h3>
                  <span class="date">{formatDate(msg.deletedAt || msg.timestamp)}</span>
                </div>
                <p class="summary">{msg.threadTitle || 'Unknown Thread'} / {msg.output?.versionName || 'Original'}</p>
                <div class="card-footer">
                  <div class="actions">
                    <button class="btn-text" onclick={() => handleHideTrash(msg.id)}>HIDE</button>
                    <button class="btn-text primary" onclick={() => handleRestoreTrash(msg.id)}>RECOVER</button>
                  </div>
                </div>
              </div>
            </div>
          {/each}
        {/if}
      </div>
    {/if}
  </div>

  {#if showNewChooser}
    <Modal title="Start New Project" onclose={() => showNewChooser = false}>
      <div class="new-chooser">
        <button onclick={() => { createNewThread({ mode: 'blank' }); showNewChooser = false; }}>Blank Project</button>
        <button
          onclick={handleImportFcstd}
          disabled={Boolean(freecadUnavailableReason)}
          title={freecadUnavailableReason ?? undefined}
        >
          Import FreeCAD
        </button>
        <button onclick={() => { showImport = true; showNewChooser = false; }}>Import Macro</button>
      </div>
    </Modal>
  {/if}

  {#if showImport}
    <ManualImportModal bind:show={showImport} onImport={(data) => { createNewThread({ mode: 'macro', ...data }); showImport = false; }} />
  {/if}

  {#if threadToDelete}
    <Modal title="Purge Project" onclose={() => threadToDelete = null}>
      <div class="confirm-delete">
        <p>Purge <strong>{threadToDelete.title}</strong>?</p>
        <div class="actions">
          <button onclick={() => threadToDelete = null}>CANCEL</button>
          <button class="danger" onclick={() => { deleteThread(threadToDelete!.id); threadToDelete = null; }}>DELETE FOREVER</button>
        </div>
      </div>
    </Modal>
  {/if}
</div>

<style>
  .project-switcher {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-100);
    color: var(--text);
  }

  .switcher-header {
    padding: 12px;
    border-bottom: 1px solid var(--bg-300);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
  }

  .tabs {
    display: flex;
    gap: 4px;
  }

  .tab-btn {
    padding: 6px 12px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.7rem;
    font-weight: bold;
    cursor: pointer;
  }

  .tab-btn.active {
    border-color: var(--primary);
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
  }

  .header-actions {
    display: flex;
    gap: 8px;
    flex: 1;
    justify-content: flex-end;
  }

  .search-input {
    max-width: 200px;
    padding: 6px 10px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.75rem;
  }

  .new-btn {
    padding: 6px 12px;
    background: var(--primary);
    color: var(--bg-100);
    border: none;
    font-weight: bold;
    font-size: 0.7rem;
    cursor: pointer;
  }

  .switcher-content {
    flex: 1;
    padding: 16px;
  }

  .project-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 16px;
  }

  .project-card {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transition: transform 0.2s, border-color 0.2s;
  }

  .project-card:hover {
    border-color: var(--primary);
  }

  .project-card.active {
    border-color: var(--primary);
    box-shadow: 0 0 0 1px var(--primary);
  }

  .card-thumb {
    height: 120px;
    background: #000;
    display: flex;
    align-items: center;
    justify-content: center;
    border-bottom: 1px solid var(--bg-300);
    overflow: hidden;
  }

  .card-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.7;
  }

  .no-thumb {
    font-size: 0.6rem;
    color: var(--bg-400);
    letter-spacing: 0.1em;
  }

  .card-body {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 8px;
  }

  .card-header h3 {
    margin: 0;
    font-size: 0.85rem;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
  }

  .date {
    font-size: 0.6rem;
    color: var(--text-dim);
  }

  .summary {
    font-size: 0.75rem;
    color: var(--text-dim);
    margin: 0;
    line-height: 1.3;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .card-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: auto;
  }

  .stats {
    font-size: 0.65rem;
    color: var(--bg-400);
  }

  .actions {
    display: flex;
    gap: 4px;
  }

  .btn-text {
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    font-size: 0.6rem;
    font-weight: bold;
    padding: 2px 6px;
    cursor: pointer;
  }

  .btn-text.primary {
    border-color: var(--primary);
    color: var(--primary);
  }

  .loading-state {
    padding: 40px;
    text-align: center;
    color: var(--text-dim);
  }

  .scrollable {
    overflow-y: auto;
  }

  .rename-input {
    flex: 1;
    background: var(--bg-100);
    border: 1px solid var(--primary);
    color: var(--text);
    font-size: 0.85rem;
    padding: 2px 4px;
    width: 100%;
  }

  .confirm-delete {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .confirm-delete .actions {
    justify-content: flex-end;
  }

  .danger {
    background: var(--red);
    color: white;
    border: none;
    padding: 6px 12px;
    font-weight: bold;
    cursor: pointer;
  }

  .new-chooser {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .new-chooser button {
    padding: 12px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }

  .new-chooser button:hover {
    border-color: var(--primary);
    background: var(--bg-300);
  }
</style>
