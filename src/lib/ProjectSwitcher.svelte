<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import {
    activeThreadLoadingId,
    createNewThread,
    deleteThread,
    finalizeThread,
    loadFromHistory,
    loadInventory,
    openInventoryThread,
    refreshHistory,
    renameThread,
    reopenThread,
  } from './stores/history';
  import {
    activeThreadIdStore as activeThreadId,
    historyStore as history,
  } from './stores/domainState';
  import {
    formatBackendError,
    getDeletedThreadPreview,
    getDeletedThreadsPage,
    getThreadPreview,
    restoreDeletedThread,
  } from './tauri/client';
  import type { DeletedThreadSummary } from './tauri/contracts';
  import type { Thread } from './types/domain';
  import PreviewFrame from './PreviewFrame.svelte';
  import { toPreviewSrc } from './previewSource';
  import ManualImportModal from './ManualImportModal.svelte';
  import Modal from './Modal.svelte';
  import AsyncActionButton from './components/AsyncActionButton.svelte';
  import type { CampaignRun } from './projects/campaignRunClient';
  import { campaignRunPreviewSrc } from './projects/campaignCardPreview';
  import { campaignRunProjectDriver } from './projects/projectDriverRegistry';

  let {
    onImportFcstd,
    onOpenNewProjectChooser,
    freecadUnavailableReason = null,
    campaignRuns = [],
    activeCampaignRunId = null,
    onStartCampaign,
    onOpenCampaignRun,
    onDeleteCampaignRun,
  }: {
    onImportFcstd?: (sourcePath: string) => void;
    onOpenNewProjectChooser?: () => void;
    freecadUnavailableReason?: string | null;
    campaignRuns?: CampaignRun[];
    activeCampaignRunId?: string | null;
    onStartCampaign?: () => void;
    onOpenCampaignRun?: (run: CampaignRun) => void;
    onDeleteCampaignRun?: (run: CampaignRun) => Promise<void> | void;
  } = $props();

  type ProjectTypeTab = 'designs' | 'campaigns';
  type Tab = 'active' | 'completed' | 'trash';

  const TRASH_PAGE_SIZE = 24;
  const CAMPAIGN_PREVIEW_SRC = '/docs/assets/corner-bracket.png';

  let projectTypeTab = $state<ProjectTypeTab>('designs');
  let activeTab = $state<Tab>('active');
  let searchQuery = $state('');
  let isLoading = $state(false);
  let loadError = $state<string | null>(null);

  let completedProjects = $state<Thread[]>([]);
  let completedLoaded = $state(false);
  let deletedProjects = $state<DeletedThreadSummary[]>([]);
  let deletedProjectPreviews = $state<Record<string, string | null>>({});
  let trashLoaded = $state(false);
  let trashNextBefore = $state<string | null>(null);
  let trashHasMore = $state(false);
  let trashLoadMoreBusy = $state(false);

  let previewImages = $state<Record<string, string | null>>({});

  let showNewChooser = $state(false);
  let showImport = $state(false);
  let projectToDelete = $state<Thread | null>(null);
  let campaignRunToDelete = $state<CampaignRun | null>(null);
  let editingProjectId = $state<string | null>(null);
  let editingTitle = $state('');
  let renameBusy = $state(false);
  let pendingActionId = $state<string | null>(null);

  onMount(() => {
    void refreshHistory();

    const onPreviewUpdated = (event: Event) => {
      const detail = (event as CustomEvent<{
        threadId?: string;
        messageId?: string;
        imageData?: string;
      }>).detail;
      if (!detail?.threadId || !detail.imageData) return;

      previewImages = {
        ...previewImages,
        [detail.threadId]: detail.imageData,
      };
    };

    window.addEventListener('ecky:version-preview-updated', onPreviewUpdated);
    return () => window.removeEventListener('ecky:version-preview-updated', onPreviewUpdated);
  });

  function threadPreviewImage(thread: Thread): string | null {
    return toPreviewSrc(previewImages[thread.id]);
  }

  function projectPreviewCard(node: HTMLElement, project: Thread) {
    let currentProject = project;

    const fetch = async () => {
      const threadId = currentProject.id;
      if (previewImages[threadId] !== undefined) return;

      previewImages = { ...previewImages, [threadId]: null };
      try {
        const imageData = await getThreadPreview(threadId);
        if (previewImages[threadId]) return;
        previewImages = { ...previewImages, [threadId]: imageData };
      } catch (error) {
        console.error(`Failed to fetch project preview for ${threadId}:`, formatBackendError(error));
      }
    };

    if (typeof IntersectionObserver === 'undefined') {
      void fetch();
      return {
        update(nextProject: Thread) {
          currentProject = nextProject;
          void fetch();
        },
      };
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) void fetch();
      },
      { root: node.closest('.scrollable'), rootMargin: '240px 0px' },
    );
    observer.observe(node);

    return {
      update(nextProject: Thread) {
        currentProject = nextProject;
      },
      destroy() {
        observer.disconnect();
      },
    };
  }

  async function loadActiveTab() {
    if (
      activeTab === 'active' ||
      (activeTab === 'completed' && completedLoaded) ||
      (activeTab === 'trash' && trashLoaded)
    ) {
      return;
    }

    isLoading = true;
    loadError = null;
    try {
      if (activeTab === 'completed') {
        completedProjects = await loadInventory();
        completedLoaded = true;
      } else {
        const page = await getDeletedThreadsPage(null, TRASH_PAGE_SIZE);
        deletedProjects = page.items;
        trashNextBefore = page.nextBefore;
        trashHasMore = page.hasMore;
        trashLoaded = true;
      }
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      isLoading = false;
    }
  }

  $effect(() => {
    void loadActiveTab();
  });

  const activeProjects = $derived(
    $history.filter(
      (project: Thread) =>
        project.status !== 'finalized' &&
        !project.isBlank &&
        (
          project.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
          Boolean(project.summary?.toLowerCase().includes(searchQuery.toLowerCase()))
        ),
    ),
  );
  const visibleCampaignRuns = $derived(
    campaignRuns.filter((run) => run.title.toLowerCase().includes(searchQuery.toLowerCase())),
  );
  const visibleCompletedProjects = $derived(
    completedProjects.filter(
      (project) =>
        project.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
        Boolean(project.summary?.toLowerCase().includes(searchQuery.toLowerCase())),
    ),
  );
  const visibleDeletedProjects = $derived(
    deletedProjects.filter(
      (project) =>
        project.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
        Boolean(project.summary?.toLowerCase().includes(searchQuery.toLowerCase())),
    ),
  );

  function trashPreviewCard(node: HTMLElement, project: DeletedThreadSummary) {
    let currentProject = project;
    const fetch = async () => {
      if (deletedProjectPreviews[currentProject.id] !== undefined) return;
      deletedProjectPreviews = {
        ...deletedProjectPreviews,
        [currentProject.id]: null,
      };
      try {
        const imageData = await getDeletedThreadPreview(currentProject.id);
        deletedProjectPreviews = {
          ...deletedProjectPreviews,
          [currentProject.id]: imageData,
        };
      } catch (error) {
        console.error(
          `Failed to fetch deleted project preview for ${currentProject.id}:`,
          formatBackendError(error),
        );
      }
    };

    if (typeof IntersectionObserver === 'undefined') {
      void fetch();
      return {
        update(nextProject: DeletedThreadSummary) {
          currentProject = nextProject;
          void fetch();
        },
      };
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) void fetch();
      },
      { root: node.closest('.scrollable'), rootMargin: '240px 0px' },
    );
    observer.observe(node);

    return {
      update(nextProject: DeletedThreadSummary) {
        currentProject = nextProject;
      },
      destroy() {
        observer.disconnect();
      },
    };
  }

  function formatDate(timestamp: number) {
    return new Date(timestamp * 1000).toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  function attentionText(project: Thread): string | null {
    const queued = Number(project.queuedCount || 0);
    const confirm = project.pendingConfirm?.trim();
    if (confirm && queued > 0) return `ACTION REQUIRED · ${queued} queued`;
    if (confirm) return 'ACTION REQUIRED';
    if (queued > 0) return `${queued} queued ${queued === 1 ? 'message' : 'messages'}`;
    return null;
  }

  function selectProject(project: Thread) {
    if (editingProjectId === project.id || $activeThreadLoadingId === project.id) return;
    if (activeTab === 'completed') {
      void openInventoryThread(project.id);
    } else {
      void loadFromHistory(project);
    }
  }

  async function completeProject(id: string) {
    pendingActionId = id;
    try {
      await finalizeThread(id);
      completedProjects = await loadInventory();
      completedLoaded = true;
    } finally {
      pendingActionId = null;
    }
  }

  async function reopenProject(id: string) {
    pendingActionId = id;
    try {
      await reopenThread(id);
      completedProjects = completedProjects.filter((project) => project.id !== id);
    } finally {
      pendingActionId = null;
    }
  }

  async function recoverProject(id: string) {
    pendingActionId = id;
    try {
      await restoreDeletedThread(id);
      deletedProjects = deletedProjects.filter((project) => project.id !== id);
      await refreshHistory();
    } finally {
      pendingActionId = null;
    }
  }

  async function loadMoreTrash() {
    if (!trashHasMore || !trashNextBefore || trashLoadMoreBusy) return;
    trashLoadMoreBusy = true;
    try {
      const page = await getDeletedThreadsPage(trashNextBefore, TRASH_PAGE_SIZE);
      const seen = new Set(deletedProjects.map((project) => project.id));
      deletedProjects = [
        ...deletedProjects,
        ...page.items.filter((project) => !seen.has(project.id)),
      ];
      trashNextBefore = page.nextBefore;
      trashHasMore = page.hasMore;
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      trashLoadMoreBusy = false;
    }
  }

  async function confirmDeleteProject() {
    if (!projectToDelete) return;
    const id = projectToDelete.id;
    pendingActionId = id;
    try {
      await deleteThread(id);
      trashLoaded = false;
      projectToDelete = null;
    } catch (error) {
      loadError = formatBackendError(error);
      throw error;
    } finally {
      pendingActionId = null;
    }
  }

  async function confirmDeleteCampaignRun() {
    if (!campaignRunToDelete || !onDeleteCampaignRun) return;
    const run = campaignRunToDelete;
    pendingActionId = run.id;
    try {
      await onDeleteCampaignRun(run);
      campaignRunToDelete = null;
    } catch (error) {
      loadError = formatBackendError(error);
      throw error;
    } finally {
      pendingActionId = null;
    }
  }

  function startRename(project: Thread) {
    editingProjectId = project.id;
    editingTitle = project.title;
  }

  function cancelRename() {
    editingProjectId = null;
    editingTitle = '';
  }

  async function commitRename(project: Thread) {
    if (renameBusy) return;
    const title = editingTitle.trim();
    if (!title || title === project.title) {
      cancelRename();
      return;
    }

    renameBusy = true;
    try {
      await renameThread(project.id, title);
      cancelRename();
    } finally {
      renameBusy = false;
    }
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

  function retryActiveTab() {
    loadError = null;
    if (activeTab === 'completed') completedLoaded = false;
    if (activeTab === 'trash') trashLoaded = false;
    void loadActiveTab();
  }
</script>

<div class="project-switcher">
  <header class="switcher-header">
    <nav class="tabs" aria-label="Project type">
      <button class:active={projectTypeTab === 'designs'} onclick={() => projectTypeTab = 'designs'}>DESIGNS</button>
      <button class:active={projectTypeTab === 'campaigns'} onclick={() => projectTypeTab = 'campaigns'}>CAMPAIGNS</button>
    </nav>
    {#if projectTypeTab === 'designs'}
      <nav class="tabs tabs--lifecycle" aria-label="Design lifecycle">
        <button class:active={activeTab === 'active'} onclick={() => activeTab = 'active'}>ACTIVE</button>
        <button class:active={activeTab === 'completed'} onclick={() => activeTab = 'completed'}>COMPLETED</button>
        <button class:active={activeTab === 'trash'} onclick={() => activeTab = 'trash'}>TRASH</button>
      </nav>
    {/if}
    <div class="header-actions">
      <input class="search-input" type="search" placeholder="Search..." bind:value={searchQuery} />
      {#if projectTypeTab === 'designs'}
        <button
          class="new-btn"
          onclick={() => onOpenNewProjectChooser ? onOpenNewProjectChooser() : showNewChooser = true}
        >
          + NEW
        </button>
      {/if}
    </div>
  </header>

  <main class="switcher-content scrollable">
    {#if projectTypeTab === 'campaigns'}
      <div class="project-grid">
        <article class="project-card campaign-definition-card" data-campaign-definition="ecky-ir">
          <PreviewFrame src={CAMPAIGN_PREVIEW_SRC} alt="Ecky IR campaign preview" />
          <div class="card-body">
            <div class="card-header"><h3>Ecky IR</h3></div>
            <p class="summary">Six linked build missions. Start at the bracket; finish with the film scanner.</p>
            <div class="card-footer">
              <span>6 missions</span>
              <button class="card-open-action" onclick={onStartCampaign}>START</button>
            </div>
          </div>
        </article>
        {#each visibleCampaignRuns as run (run.id)}
          {@const card = campaignRunProjectDriver.card(run)}
          {@const campaignPreview = campaignRunPreviewSrc(run.currentStepId)}
          <article
            class="project-card"
            class:active={activeCampaignRunId === run.id}
            data-project-kind="campaign"
            data-project-id={run.id}
          >
            <PreviewFrame
              src={campaignPreview}
              state={campaignPreview ? 'ready' : 'error'}
              alt={`${run.title} preview`}
            />
            <div class="card-body">
              <div class="card-header">
                <h3>{run.title}</h3>
                <span>{formatDate(run.updatedAt)}</span>
              </div>
              <p class="summary">{card.progress}</p>
              <div class="card-footer">
                <span>{run.currentStepId ? 'In progress' : 'Not started'}</span>
                <div class="actions">
                  <button class="card-open-action" onclick={() => onOpenCampaignRun?.(run)}>RESUME</button>
                </div>
              </div>
              {#if onDeleteCampaignRun}
                <div class="card-management-actions">
                  <button class="danger" onclick={() => campaignRunToDelete = run}>DELETE</button>
                </div>
              {/if}
            </div>
          </article>
        {/each}
      </div>
    {:else if isLoading}
      <div class="loading-state">
        {activeTab === 'completed' ? 'LOADING COMPLETED...' : 'LOADING TRASH...'}
      </div>
    {:else if loadError}
      <div class="error-state">
        <strong>
          {activeTab === 'completed' ? 'COMPLETED LOAD ERROR' : 'TRASH LOAD ERROR'}
        </strong>
        <pre>{loadError}</pre>
        <button onclick={retryActiveTab}>RETRY</button>
      </div>
    {:else}
      <div class="project-grid">
        {#if activeTab === 'active'}
          {#if activeProjects.length === 0}
            <div class="empty-state">
              {searchQuery ? 'NO MATCHING ACTIVE PROJECTS' : 'NO ACTIVE PROJECTS'}
            </div>
          {/if}
          {#each activeProjects as project (project.id)}
            {@const attention = attentionText(project)}
            <article
              class="project-card"
              class:active={$activeThreadId === project.id}
              data-project-id={project.id}
              use:projectPreviewCard={project}
            >
              <PreviewFrame src={threadPreviewImage(project)} alt={`${project.title} preview`} />
              <div class="card-body">
                <div class="card-header">
                  {#if editingProjectId === project.id}
                    <input
                      class="rename-input"
                      aria-label="Project name"
                      bind:value={editingTitle}
                      onkeydown={(event) => event.key === 'Enter' && commitRename(project)}
                    />
                  {:else}
                    <h3>{project.title}</h3>
                  {/if}
                  <span>{formatDate(project.updatedAt)}</span>
                </div>
                {#if project.summary}
                  <p class="summary">{project.summary}</p>
                {/if}
                {#if attention}
                  <p class="attention">{attention}</p>
                {/if}
                <div class="card-footer">
                  <span>{project.versionCount || 0} versions</span>
                  <div class="actions">
                    <button class="card-open-action" onclick={() => selectProject(project)}>OPEN</button>
                  </div>
                </div>
                <div class="card-management-actions">
                  <button onclick={() => completeProject(project.id)}>COMPLETE</button>
                  <button onclick={() => startRename(project)}>RENAME</button>
                  <button class="danger" onclick={() => projectToDelete = project}>DELETE</button>
                </div>
              </div>
            </article>
          {/each}
        {:else if activeTab === 'completed'}
          {#if visibleCompletedProjects.length === 0}
            <div class="empty-state">
              {searchQuery ? 'NO MATCHING COMPLETED PROJECTS' : 'NO COMPLETED PROJECTS'}
            </div>
          {/if}
          {#each visibleCompletedProjects as project (project.id)}
            <article
              class="project-card"
              data-project-id={project.id}
              use:projectPreviewCard={project}
            >
              <PreviewFrame src={threadPreviewImage(project)} alt={`${project.title} preview`} />
              <div class="card-body">
                <div class="card-header">
                  <h3>{project.title}</h3>
                  <span>{formatDate(project.finalizedAt ?? project.updatedAt)}</span>
                </div>
                {#if project.summary}
                  <p class="summary">{project.summary}</p>
                {/if}
                <div class="card-footer">
                  <span>{project.versionCount || 0} versions</span>
                  <div class="actions">
                    <button class="card-open-action" onclick={() => selectProject(project)}>VIEW</button>
                    <button onclick={() => reopenProject(project.id)}>REOPEN</button>
                  </div>
                </div>
              </div>
            </article>
          {/each}
        {:else}
          {#if visibleDeletedProjects.length === 0}
            <div class="empty-state">
              {searchQuery ? 'NO MATCHING DELETED PROJECTS' : 'TRASH IS EMPTY'}
            </div>
          {/if}
          {#each visibleDeletedProjects as project (project.id)}
            <article
              class="project-card"
              data-project-id={project.id}
              use:trashPreviewCard={project}
            >
              <PreviewFrame src={toPreviewSrc(deletedProjectPreviews[project.id])} alt={`${project.title} preview`} />
              <div class="card-body">
                <div class="card-header">
                  <h3>{project.title}</h3>
                  <span>{formatDate(project.deletedAt)}</span>
                </div>
                {#if project.summary}
                  <p class="summary">{project.summary}</p>
                {/if}
                <div class="card-footer">
                  <span>{project.versionCount} versions</span>
                  <button
                    onclick={() => recoverProject(project.id)}
                    disabled={pendingActionId === project.id}
                  >
                    {pendingActionId === project.id ? 'RECOVERING...' : 'RECOVER'}
                  </button>
                </div>
              </div>
            </article>
          {/each}
          {#if trashHasMore}
            <div class="trash-pagination">
              <button onclick={loadMoreTrash} disabled={trashLoadMoreBusy}>
                {trashLoadMoreBusy ? 'LOADING...' : 'LOAD MORE'}
              </button>
            </div>
          {/if}
        {/if}
      </div>
    {/if}
  </main>

  {#if !onOpenNewProjectChooser && showNewChooser}
    <Modal title="Start New Project" onclose={() => showNewChooser = false}>
      <div class="new-chooser">
        <button onclick={() => { void createNewThread({ mode: 'blank' }); showNewChooser = false; }}>
          Blank Project
        </button>
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

  {#if !onOpenNewProjectChooser && showImport}
    <ManualImportModal
      bind:show={showImport}
      onImport={(data) => {
        void createNewThread({ mode: 'macro', ...data });
        showImport = false;
      }}
    />
  {/if}

  {#if projectToDelete}
    <Modal title="Trash Project" onclose={() => projectToDelete = null}>
      <div class="confirm-delete">
        <p>Move <strong>{projectToDelete.title}</strong> to trash?</p>
        <p>You can recover the complete project and its versions from <strong>TRASH</strong>.</p>
        <div class="confirm-actions">
          <button onclick={() => projectToDelete = null}>CANCEL</button>
          <AsyncActionButton
            className="danger"
            label="MOVE TO TRASH"
            pendingLabel="MOVING…"
            disabled={pendingActionId === projectToDelete.id}
            action={confirmDeleteProject}
          />
        </div>
      </div>
    </Modal>
  {/if}

  {#if campaignRunToDelete}
    <Modal title="Delete Campaign Run" onclose={() => campaignRunToDelete = null}>
      <div class="confirm-delete">
        <p>Delete <strong>{campaignRunToDelete.title}</strong> and its saved progress?</p>
        <div class="confirm-actions">
          <button onclick={() => campaignRunToDelete = null}>CANCEL</button>
          <AsyncActionButton
            className="danger"
            label="DELETE"
            pendingLabel="DELETING…"
            disabled={pendingActionId === campaignRunToDelete.id}
            action={confirmDeleteCampaignRun}
          />
        </div>
      </div>
    </Modal>
  {/if}
</div>

<style>
  .project-switcher {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--bg-100);
    color: var(--text);
    overflow: hidden;
  }

  .switcher-header {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-bottom: 1px solid var(--bg-300);
    overflow: hidden;
  }

  .tabs,
  .header-actions,
  .actions,
  .confirm-actions {
    display: flex;
    gap: 6px;
    min-width: 0;
    overflow: hidden;
  }

  button {
    border: 1px solid var(--bg-400);
    background: var(--bg-300);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: var(--ui-font-caption);
    font-weight: 700;
    letter-spacing: 0.06em;
    padding: 5px 8px;
    cursor: pointer;
  }

  button:hover:not(:disabled),
  .tabs button.active {
    border-color: var(--primary);
    color: var(--primary);
  }

  button:disabled {
    opacity: 0.55;
    cursor: wait;
  }

  button.danger {
    border-color: var(--red);
    color: var(--red);
  }

  .tabs button {
    background: transparent;
    border-color: transparent;
  }

  .search-input,
  .rename-input {
    min-width: 0;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text);
  }

  .search-input {
    flex: 1;
    padding: 7px 9px;
  }

  .new-btn {
    flex: 0 0 auto;
    border-color: var(--primary);
    background: var(--primary);
    color: var(--bg-100);
  }

  .switcher-content {
    flex: 1;
    min-width: 0;
    min-height: 0;
    padding: 14px;
    overflow-y: auto;
  }

  .project-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(240px, 100%), 1fr));
    gap: 14px;
    min-width: 0;
    overflow: hidden;
  }

  .project-card {
    display: flex;
    flex-direction: column;
    min-width: 0;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    overflow: hidden;
  }

  .project-card.active {
    border-color: var(--primary);
    box-shadow: inset 0 0 0 1px var(--primary);
  }

  .card-body {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    padding: 11px;
    overflow: hidden;
  }

  .card-header,
  .card-footer {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
    overflow: hidden;
  }

  .card-header h3,
  .rename-input {
    font-family: var(--font-mono);
    font-size: var(--ui-font-title);
    font-weight: 700;
    line-height: 1.4;
    letter-spacing: normal;
  }

  .card-header h3 {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    color: var(--text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-header span,
  .card-footer > span {
    flex: 0 0 auto;
    color: var(--text-dim);
    font-size: var(--ui-font-caption);
  }

  .summary,
  .attention {
    margin: 0;
    overflow: hidden;
    font-size: var(--ui-font-body);
    line-height: 1.35;
  }

  .summary {
    display: -webkit-box;
    color: var(--text-dim);
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  .attention {
    color: var(--secondary);
  }

  .card-footer {
    align-items: center;
    margin-top: auto;
  }

  .card-management-actions {
    display: flex;
    justify-content: flex-end;
    gap: 5px;
    padding-top: 8px;
    border-top: 1px solid var(--bg-300);
    overflow: hidden;
  }

  .card-management-actions button {
    padding: 4px 7px;
    background: transparent;
    color: var(--text-dim);
    font-size: var(--ui-font-caption);
  }

  .card-open-action {
    min-width: 74px;
    padding: 7px 14px;
    border-color: var(--primary);
    background: var(--primary);
    color: var(--bg-100);
  }

  .card-open-action:hover:not(:disabled) {
    border-color: var(--secondary);
    background: var(--secondary);
    color: var(--bg-100);
  }

  .rename-input {
    flex: 1;
    width: 100%;
    padding: 3px 5px;
  }

  .trash-pagination {
    grid-column: 1 / -1;
    display: flex;
    justify-content: center;
    padding: 8px;
    overflow: hidden;
  }

  .loading-state,
  .empty-state,
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
    padding: 24px;
    color: var(--text-dim);
    overflow: hidden;
  }

  .error-state {
    border: 1px solid var(--red);
    color: var(--red);
  }

  .error-state pre {
    width: 100%;
    margin: 0;
    white-space: pre-wrap;
    overflow: hidden;
  }

  .new-chooser,
  .confirm-delete {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px;
    overflow: hidden;
  }

  .new-chooser button {
    padding: 11px;
    text-align: left;
  }

  .confirm-delete p {
    margin: 0;
    color: var(--text);
    font-size: 0.78rem;
    line-height: 1.45;
  }

  .confirm-actions {
    justify-content: flex-end;
  }

  @container (max-width: 359px) {
    .switcher-header {
      padding: 9px;
    }

    .tabs {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }

    .tabs button {
      padding-inline: 3px;
    }

    .switcher-content {
      padding: 9px;
    }

    .project-grid {
      grid-template-columns: 1fr;
      gap: 9px;
    }

    .project-card {
      display: grid;
      grid-template-columns: 82px minmax(0, 1fr);
    }

    :global(.preview-frame) {
      min-height: 104px;
      border-right: 1px solid var(--bg-300);
      border-bottom: 0;
    }

    .card-body {
      padding: 9px;
    }

    .summary {
      -webkit-line-clamp: 1;
      line-clamp: 1;
    }
  }
</style>
