<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import {
    formatBackendError,
    libraryPanelIntent,
  } from './tauri/client';
  import type {
    ComponentHeader,
    ComponentPackageHeader,
    ExtractedComponentSearchResult,
    FreecadLibraryItem,
    LibraryPanelProjection,
  } from './tauri/contracts';
  import { config as sharedConfig } from './stores/domainState';

  let {
    onImportFreecadLibraryPart,
    onApplyComponentImport,
  }: {
    onImportFreecadLibraryPart?: (item: FreecadLibraryItem) => Promise<void> | void;
    onApplyComponentImport?: (input: {
      packageId: string;
      version: string;
      componentId: string;
      label: string;
    }) => Promise<void> | void;
  } = $props();

  type LibraryTab = 'components' | 'freecad';

  let activeTab = $state<LibraryTab>('components');
  let searchQuery = $state('');
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  let packageHeaders = $state<ComponentPackageHeader[]>([]);
  let availableComponents = $state<ExtractedComponentSearchResult[]>([]);
  let indexingDiagnostics = $state<string[]>([]);
  let packagesLoaded = $state(false);
  let packageImportBusy = $state(false);
  let importingComponentId = $state<string | null>(null);
  let expandedPackageId = $state<string | null>(null);

  let freecadLibraryRoots = $state<string[]>([]);
  let freecadLibraryResults = $state<FreecadLibraryItem[]>([]);
  let freecadLoaded = $state(false);
  let freecadLibrarySearchBusy = $state(false);
  let freecadLibraryFolderBusy = $state(false);
  let importingFreecadLibraryItemId = $state<string | null>(null);
  let freecadHasMore = $state(false);
  let freecadPage = $state(0);

  async function loadActiveTab() {
    if (
      (activeTab === 'components' && packagesLoaded) ||
      (activeTab === 'freecad' && freecadLoaded)
    ) {
      return;
    }

    loading = true;
    loadError = null;
    try {
      if (activeTab === 'components') {
        projectComponentPackages(await libraryPanelIntent({ kind: 'loadComponents' }));
        packagesLoaded = true;
      } else if (activeTab === 'freecad') {
        projectFreecadLibrary(await libraryPanelIntent({ kind: 'loadFreecad', query: searchQuery, page: 0 }));
        freecadLoaded = true;
      }
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void loadActiveTab();
  });

  const filteredPackages = $derived(
    packageHeaders.filter((pkg) =>
      [
        pkg.displayName,
        pkg.packageId,
        pkg.version,
        ...(pkg.tags ?? []),
      ].join(' ').toLowerCase().includes(searchQuery.toLowerCase()),
    ),
  );

  const filteredComponents = $derived(
    availableComponents.filter((component) =>
      [
        component.name,
        component.oneLiner,
        ...(component.paramKeys ?? []),
        ...(component.tags ?? []),
        ...(component.ports ?? []).flatMap((port) => [port.portId, port.typeId]),
        component.threadId ?? '',
        component.threadTitle ?? '',
      ].join(' ').toLowerCase().includes(searchQuery.toLowerCase()),
    ),
  );

  const filteredFreecadResults = $derived(
    freecadLibraryResults.filter((item) =>
      [
        item.name,
        item.categoryPath,
        ...(item.tags ?? []),
        ...(item.formats ?? []),
      ].join(' ').toLowerCase().includes(searchQuery.toLowerCase()),
    ),
  );

  function packageStats(pkg: ComponentPackageHeader) {
    const count = (value: number, singular: string, plural = `${singular}s`) =>
      `${value} ${value === 1 ? singular : plural}`;
    return [
      count(pkg.components?.length ?? 0, 'component'),
      count(pkg.portTypes?.length ?? 0, 'port type'),
      count(pkg.assemblies?.length ?? 0, 'assembly', 'assemblies'),
    ].join(' / ');
  }

  function projectComponentPackages(projection: LibraryPanelProjection) {
    if (projection.kind !== 'componentPackages') {
      throw new Error(`Library backend returned '${projection.kind}' for component packages.`);
    }
    packageHeaders = projection.packageHeaders;
    availableComponents = projection.components ?? [];
    indexingDiagnostics = projection.indexingDiagnostics ?? [];
  }

  function projectFreecadLibrary(projection: LibraryPanelProjection) {
    if (projection.kind !== 'freecadLibrary') {
      throw new Error(`Library backend returned '${projection.kind}' for FreeCAD library.`);
    }
    sharedConfig.update((config) => ({
      ...config,
      freecadLibraryRoots: projection.freecadLibraryRoots,
    }));
    freecadLibraryRoots = projection.freecadLibraryRoots;
    freecadLibraryResults = projection.items;
    freecadHasMore = projection.hasMore;
    freecadPage = projection.page;
  }

  async function importComponent(pkg: ComponentPackageHeader, component: ComponentHeader) {
    if (importingComponentId || !onApplyComponentImport) return;
    const importId = `${pkg.packageId}@${pkg.version}:${component.componentId}`;
    importingComponentId = importId;
    loadError = null;
    try {
      await onApplyComponentImport({
        packageId: pkg.packageId,
        version: pkg.version,
        componentId: component.componentId,
        label: component.displayName,
      });
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      importingComponentId = null;
    }
  }

  async function handleImportPackageArchive() {
    if (packageImportBusy) return;
    loadError = null;
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Ecky Package', extensions: ['zip'] }],
    });
    if (typeof selected !== 'string' || !selected.trim()) return;

    packageImportBusy = true;
    try {
      projectComponentPackages(await libraryPanelIntent({ kind: 'installPackage', archivePath: selected }));
      packagesLoaded = true;
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      packageImportBusy = false;
    }
  }

  async function handleSetFreecadLibraryFolder() {
    if (freecadLibraryFolderBusy) return;
    loadError = null;
    freecadLibraryFolderBusy = true;
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected !== 'string' || !selected.trim()) return;
      projectFreecadLibrary(await libraryPanelIntent({
        kind: 'setFreecadRoot',
        root: selected,
        query: searchQuery,
      }));
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      freecadLibraryFolderBusy = false;
    }
  }

  async function handleSearchFreecadLibrary() {
    if (freecadLibrarySearchBusy) return;
    loadError = null;
    freecadLibrarySearchBusy = true;
    try {
      projectFreecadLibrary(await libraryPanelIntent({ kind: 'loadFreecad', query: searchQuery, page: 0 }));
    } catch (error) {
      loadError = formatBackendError(error);
      freecadLibraryResults = [];
    } finally {
      freecadLibrarySearchBusy = false;
    }
  }

  async function loadFreecadLibraryPage(nextPage: number) {
    if (freecadLibrarySearchBusy || nextPage < 0 || (nextPage > freecadPage && !freecadHasMore)) return;
    loadError = null;
    freecadLibrarySearchBusy = true;
    try {
      projectFreecadLibrary(await libraryPanelIntent({
        kind: 'loadFreecad',
        query: searchQuery,
        page: nextPage,
      }));
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      freecadLibrarySearchBusy = false;
    }
  }

  async function importFreecadLibraryItem(item: FreecadLibraryItem) {
    if (!onImportFreecadLibraryPart || importingFreecadLibraryItemId) return;
    importingFreecadLibraryItemId = item.id;
    try {
      await onImportFreecadLibraryPart(item);
    } finally {
      importingFreecadLibraryItemId = null;
    }
  }

  function retry() {
    loadError = null;
    if (activeTab === 'components') packagesLoaded = false;
    if (activeTab === 'freecad') freecadLoaded = false;
    void loadActiveTab();
  }
</script>

<div class="library-panel">
  <header class="library-header">
    <nav class="library-tabs" aria-label="Library sections">
      <button class:active={activeTab === 'components'} onclick={() => activeTab = 'components'}>
        COMPONENTS
      </button>
      <button class:active={activeTab === 'freecad'} onclick={() => activeTab = 'freecad'}>
        FREECAD PARTS
      </button>
    </nav>
    <div class="library-tools">
      <input class="search-input" type="search" placeholder="Search library..." bind:value={searchQuery} />
      {#if activeTab === 'components'}
        <button class="secondary-action" onclick={() => packagesLoaded = false} disabled={loading}>
          REFRESH
        </button>
        <button class="primary-action" onclick={handleImportPackageArchive} disabled={packageImportBusy}>
          {packageImportBusy ? 'IMPORTING...' : 'IMPORT PACKAGE'}
        </button>
      {:else if activeTab === 'freecad'}
        <button class="primary-action" onclick={handleSearchFreecadLibrary} disabled={freecadLibrarySearchBusy}>
          {freecadLibrarySearchBusy ? 'SEARCHING...' : 'SEARCH'}
        </button>
      {/if}
    </div>
  </header>

  <main class="library-content">
    {#if loading}
      <div class="state">LOADING LIBRARY...</div>
    {:else if loadError}
      <div class="error-state">
        <strong>LIBRARY ERROR</strong>
        <pre>{loadError}</pre>
        <button onclick={retry}>RETRY</button>
      </div>
    {:else if activeTab === 'components'}
      {#if indexingDiagnostics.length}
        <div class="error-state" role="status" aria-label="Component indexing diagnostics">
          <strong>COMPONENT INDEXING NEEDS ATTENTION</strong>
          {#each indexingDiagnostics as diagnostic}<pre>{diagnostic}</pre>{/each}
        </div>
      {/if}
      {#if filteredComponents.length === 0 && filteredPackages.length === 0}
        <div class="state">{searchQuery.trim() ? 'NO MATCHING COMPONENTS' : 'NO COMPONENTS OR PACKAGES'}</div>
      {:else}
        <div class="library-grid">
          {#each filteredComponents as component (component.componentId ?? `${component.origin}:${component.name}`)}
            <article class="library-card available-component-card">
              <div class="card-title">
                <h3>{component.name}</h3>
                <span>{component.origin}</span>
              </div>
              <p>{component.oneLiner}</p>
              {#if component.paramKeys.length}
                <div class="card-stats">PARAMETERS {component.paramKeys.join(' / ')}</div>
              {/if}
              {#if component.revisionDigest}
                <div class="card-stats">REVISION {component.revisionDigest.slice(0, 19)}</div>
              {/if}
              {#if component.version}
                <div class="card-stats">VERSION {component.version}</div>
              {/if}
              {#if component.threadId}
                <div class="card-stats">SOURCE {component.threadTitle ?? component.threadId} / {component.threadId}</div>
              {/if}
              {#if component.tags.length}
                <div class="tag-list">{#each component.tags as tag}<span>{tag}</span>{/each}</div>
              {/if}
            </article>
          {/each}
          {#each filteredPackages as pkg (pkg.packageId + pkg.version)}
            <article class="library-card">
              <div class="card-title">
                <h3>{pkg.displayName}</h3>
                <span>{pkg.visibility}</span>
              </div>
              <p>{pkg.packageId} / {pkg.version}</p>
              <div class="card-stats">{packageStats(pkg)}</div>
              {#if pkg.components?.length}
                <button
                  class="component-toggle"
                  aria-expanded={expandedPackageId === `${pkg.packageId}@${pkg.version}`}
                  onclick={() => expandedPackageId = expandedPackageId === `${pkg.packageId}@${pkg.version}` ? null : `${pkg.packageId}@${pkg.version}`}
                >
                  SHOW {pkg.displayName} COMPONENTS
                </button>
              {/if}
              {#if expandedPackageId === `${pkg.packageId}@${pkg.version}`}
                <div class="component-list" aria-label={`${pkg.displayName} components`}>
                  {#each pkg.components as component (component.componentId)}
                    {@const importId = `${pkg.packageId}@${pkg.version}:${component.componentId}`}
                    <div class="component-row">
                      <span>{component.displayName}</span>
                      <button
                        aria-label={`${importingComponentId === importId ? 'IMPORTING' : 'IMPORT'} ${component.displayName}`}
                        onclick={() => importComponent(pkg, component)}
                        disabled={Boolean(importingComponentId)}
                      >
                        {importingComponentId === importId ? 'IMPORTING...' : 'IMPORT'}
                      </button>
                    </div>
                  {/each}
                </div>
              {/if}
              {#if pkg.tags?.length}
                <div class="tag-list">
                  {#each pkg.tags as tag}
                    <span>{tag}</span>
                  {/each}
                </div>
              {/if}
            </article>
          {/each}
        </div>
      {/if}
    {:else if activeTab === 'freecad'}
      <section class="source-config">
        <div>
          <strong>LOCAL SOURCE</strong>
          <p>{freecadLibraryRoots[0] || 'NO LOCAL FOLDER CONFIGURED'}</p>
        </div>
        <button onclick={handleSetFreecadLibraryFolder} disabled={freecadLibraryFolderBusy}>
          {freecadLibraryRoots.length ? 'CHANGE FOLDER' : 'SET FOLDER'}
        </button>
      </section>
      {#if filteredFreecadResults.length === 0}
        <div class="state">SEARCH LOCAL FREECAD PARTS</div>
      {:else}
        <div class="result-list">
          {#each filteredFreecadResults as item (item.id)}
            <article class="result-row">
              <div>
                <h3>{item.name}</h3>
                <p>{item.categoryPath}</p>
              </div>
              <button
                aria-label={`${importingFreecadLibraryItemId === item.id ? 'IMPORTING' : 'IMPORT'} ${item.name}`}
                onclick={() => importFreecadLibraryItem(item)}
                disabled={Boolean(importingFreecadLibraryItemId)}
              >
                {importingFreecadLibraryItemId === item.id ? 'IMPORTING...' : 'IMPORT'}
              </button>
            </article>
          {/each}
          {#if freecadPage > 0 || freecadHasMore}
            <nav class="catalog-pagination" aria-label="FreeCAD catalog pages">
              <button
                onclick={() => loadFreecadLibraryPage(freecadPage - 1)}
                disabled={freecadLibrarySearchBusy || freecadPage === 0}
              >PREVIOUS</button>
              <span>PAGE {freecadPage + 1}</span>
              <button
                onclick={() => loadFreecadLibraryPage(freecadPage + 1)}
                disabled={freecadLibrarySearchBusy || !freecadHasMore}
              >{freecadLibrarySearchBusy ? 'LOADING...' : 'NEXT'}</button>
            </nav>
          {/if}
        </div>
      {/if}
    {/if}
  </main>
</div>

<style>
  .library-panel {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--bg-100);
    color: var(--text);
    overflow: hidden;
  }

  .library-header {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-bottom: 1px solid var(--bg-300);
    overflow: hidden;
  }

  .library-tabs,
  .library-tools {
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
    font-size: 0.64rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    padding: 6px 9px;
    cursor: pointer;
  }

  button:hover:not(:disabled),
  button.active {
    border-color: var(--primary);
    color: var(--primary);
  }

  button:disabled {
    opacity: 0.55;
    cursor: wait;
  }

  .search-input {
    flex: 1;
    min-width: 0;
    padding: 7px 9px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text);
  }

  .primary-action {
    flex: 0 0 auto;
    border-color: var(--secondary);
    color: var(--secondary);
  }

  .library-content {
    flex: 1;
    min-width: 0;
    min-height: 0;
    padding: 14px;
    overflow-y: auto;
  }

  .library-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 12px;
    overflow: hidden;
  }

  .library-card,
  .source-config,
  .result-row,
  .error-state {
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    overflow: hidden;
  }

  .library-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
  }

  .component-list {
    display: grid;
    gap: 0.35rem;
    margin-top: 0.65rem;
  }

  .component-toggle {
    margin-top: 0.65rem;
  }

  .component-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    border: 1px solid color-mix(in srgb, var(--primary) 36%, transparent);
    padding: 0.35rem;
    overflow: hidden;
  }

  .component-row span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-title,
  .source-config,
  .result-row {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 10px;
  }

  h3,
  p {
    margin: 0;
  }

  h3 {
    font-size: 0.82rem;
  }

  p,
  .card-stats {
    color: var(--text-dim);
    font-size: 0.68rem;
    line-height: 1.4;
  }

  .card-title span {
    color: var(--secondary);
    font-size: 0.56rem;
  }

  .tag-list {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    overflow: hidden;
  }

  .tag-list span {
    padding: 2px 5px;
    border: 1px solid var(--bg-400);
    color: var(--text-dim);
    font-size: 0.58rem;
  }

  .source-config {
    align-items: center;
    padding: 12px;
    margin-bottom: 12px;
  }

  .result-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: hidden;
  }

  .catalog-pagination {
    align-self: center;
    flex: 0 0 auto;
    margin: 8px;
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--primary);
    font-family: var(--font-mono);
    font-size: 0.64rem;
    font-weight: 700;
    overflow: hidden;
  }

  .result-row {
    align-items: center;
    padding: 10px 12px;
  }

  .state {
    display: grid;
    min-height: 160px;
    place-items: center;
    color: var(--text-dim);
    font-size: 0.72rem;
    overflow: hidden;
  }

  .error-state {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
    padding: 12px;
    color: var(--red);
  }

  .error-state pre {
    width: 100%;
    margin: 0;
    white-space: pre-wrap;
    overflow: hidden;
  }

  @container (max-width: 399px) {
    .library-tabs {
      display: grid;
      grid-template-columns: 1fr;
    }

    .library-tools {
      flex-wrap: wrap;
    }

    .primary-action {
      width: 100%;
    }

    .library-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
