<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import {
    formatBackendError,
    libraryPanelIntent,
  } from './tauri/client';
  import type {
    ComponentHeader,
    ComponentIndexStatus,
    ComponentPackageHeader,
    ExtractedComponentSearchResult,
    FreecadLibraryItem,
    LibraryPanelProjection,
    LibraryComponentSelection,
    RegistryReleaseSummary,
  } from './tauri/contracts';
  import { config as sharedConfig } from './stores/domainState';

  let {
    onImportFreecadLibraryPart,
    onApplyComponentImport,
    freecadEnabled = false,
  }: {
    onImportFreecadLibraryPart?: (item: FreecadLibraryItem) => Promise<void> | void;
    onApplyComponentImport?: (input: LibraryComponentSelection & { label: string }) => Promise<void> | void;
    freecadEnabled?: boolean;
  } = $props();

  type LibraryTab = 'components' | 'hub' | 'freecad';

  let activeTab = $state<LibraryTab>('components');
  let searchQuery = $state('');
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  let packageHeaders = $state<ComponentPackageHeader[]>([]);
  let availableComponents = $state<ExtractedComponentSearchResult[]>([]);
  let indexingDiagnostics = $state<string[]>([]);
  let indexStatus = $state.raw<ComponentIndexStatus | null>(null);
  let componentRefreshSerial = 0;
  let packagesLoaded = $state(false);
  let packageImportBusy = $state(false);
  let importingComponentId = $state<string | null>(null);
  let expandedPackageId = $state<string | null>(null);
  let hubReleases = $state<RegistryReleaseSummary[]>([]);
  let hubLoaded = $state(false);
  let hubSearchBusy = $state(false);
  let hubInstallId = $state<string | null>(null);
  let registryUrl = $state('https://hub.ecky-cad.com');


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
      (activeTab === 'hub' && hubLoaded) ||
      (activeTab === 'freecad' && freecadLoaded)
    ) {
      return;
    }

    loading = true;
    loadError = null;
    try {
      if (activeTab === 'components') {
        await refreshComponentIndex();
      } else if (activeTab === 'hub') {
        await searchHub();
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

  async function refreshComponentIndex() {
    const serial = ++componentRefreshSerial;
    try {
      const projection = await libraryPanelIntent({ kind: 'loadComponents' });
      if (serial !== componentRefreshSerial) return;
      projectComponentPackages(projection);
      packagesLoaded = true;
      loadError = null;
    } catch (error) {
      if (serial === componentRefreshSerial) loadError = formatBackendError(error);
    }
  }

  async function rebuildComponentIndex() {
    try {
      projectComponentPackages(await libraryPanelIntent({ kind: 'rebuildComponentIndex' }));
      loadError = null;
    } catch (error) {
      loadError = formatBackendError(error);
    }
  }

  function projectRegistry(projection: LibraryPanelProjection) {
    if (projection.kind !== 'registryPackages') throw new Error('Expected registry package results');
    hubReleases = projection.releases;
    registryUrl = projection.registryUrl;
    hubLoaded = true;
  }

  async function searchHub(connect = false) {
    if (hubSearchBusy) return;
    hubSearchBusy = true;
    loadError = null;
    try {
      projectRegistry(await libraryPanelIntent(connect
        ? { kind: 'setRegistry', registryUrl, query: searchQuery }
        : { kind: 'searchRegistry', query: searchQuery }));
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      hubSearchBusy = false;
    }
  }

  async function installHubPackage(release: RegistryReleaseSummary) {
    if (hubInstallId) return;
    hubInstallId = `${release.header.packageId}@${release.header.version}`;
    loadError = null;
    try {
      projectComponentPackages(await libraryPanelIntent({ kind: 'installRegistryPackage', packageId: release.header.packageId, version: release.header.version }));
      packagesLoaded = true;
      activeTab = 'components';
      searchQuery = '';
    } catch (error) {
      loadError = formatBackendError(error);
    } finally {
      hubInstallId = null;
    }
  }

  onMount(() => {
    const internals = (window as unknown as { __TAURI_INTERNALS__?: { transformCallback?: unknown } }).__TAURI_INTERNALS__;
    if (typeof internals?.transformCallback !== 'function') return;
    const subscriptions = Promise.all([
      listen('component-library-updated', () => void refreshComponentIndex()),
      listen('history-updated', () => void refreshComponentIndex()),
    ]);
    void subscriptions.catch((error) => console.warn('Component library updates unavailable:', error));
    return () => { void subscriptions.then((unlisten) => unlisten.forEach((stop) => stop())).catch(() => {}); };
  });

  $effect(() => {
    if (!freecadEnabled && activeTab === 'freecad') activeTab = 'components';
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

  const componentSearchIndex = $derived(
    availableComponents.map((component) => ({ component, text: [
        component.name,
        component.oneLiner,
        ...(component.paramKeys ?? []),
        ...(component.tags ?? []),
        ...(component.ports ?? []).flatMap((port) => [port.portId, port.typeId]),
        component.threadId ?? '',
        component.threadTitle ?? '',
      ].join(' ').toLowerCase() })),
  );
  const filteredComponents = $derived(componentSearchIndex
    .filter(({ text }) => text.includes(searchQuery.trim().toLowerCase()))
    .map(({ component }) => component));

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
    indexStatus = projection.indexStatus ?? null;
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

  async function insertAvailableComponent(component: ExtractedComponentSearchResult) {
    if (importingComponentId || !onApplyComponentImport) return;
    importingComponentId = component.componentId ?? `${component.origin}:${component.name}`;
    loadError = null;
    try {
      if (component.componentId && !component.revisionDigest) {
        throw new Error('Component has no pinned revision. Refresh the library before inserting.');
      }
      const selection: LibraryComponentSelection = component.componentId && component.revisionDigest
        ? { componentId: component.componentId, revisionDigest: component.revisionDigest }
        : { name: component.name };
      await onApplyComponentImport({ ...selection, label: component.name });
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
    if (activeTab === 'hub') hubLoaded = false;
    void loadActiveTab();
  }
</script>

<div class="library-panel">
  <header class="library-header">
    <nav class="library-tabs" aria-label="Library sections">
      <button class:active={activeTab === 'components'} onclick={() => activeTab = 'components'}>
        COMPONENTS
      </button>
      <button class:active={activeTab === 'hub'} onclick={() => activeTab = 'hub'}>HUB</button>
      {#if freecadEnabled}
        <button class:active={activeTab === 'freecad'} onclick={() => activeTab = 'freecad'}>
          FREECAD PARTS
        </button>
      {/if}
    </nav>
    <div class="library-tools">
      <input class="search-input" type="search" placeholder="Search library..." bind:value={searchQuery} />
      {#if activeTab === 'components'}
        <button class="secondary-action" onclick={() => void refreshComponentIndex()} disabled={loading}>
          REFRESH
        </button>
        <button class="primary-action" onclick={handleImportPackageArchive} disabled={packageImportBusy}>
          {packageImportBusy ? 'IMPORTING...' : 'IMPORT PACKAGE'}
        </button>
      {:else if activeTab === 'hub'}
        <button class="primary-action" onclick={() => void searchHub()} disabled={hubSearchBusy}>{hubSearchBusy ? 'SEARCHING...' : 'SEARCH'}</button>
      {:else if activeTab === 'freecad'}
        <button class="primary-action" onclick={handleSearchFreecadLibrary} disabled={freecadLibrarySearchBusy}>
          {freecadLibrarySearchBusy ? 'SEARCHING...' : 'SEARCH'}
        </button>
      {/if}
    </div>
  </header>

  {#if activeTab === 'components'}
    <section class="component-index-status" aria-label="Component library status">
      <div class="index-message">
        {#if indexStatus?.phase === 'indexing'}
          <div role="status" aria-label="Component index progress">UPDATING LIBRARY · {indexStatus.completed} / {indexStatus.total}</div>
        {:else if indexStatus?.phase === 'failed'}
          <strong>LIBRARY UPDATE INCOMPLETE</strong>
        {:else}
          <span>{availableComponents.length} COMPONENTS AVAILABLE</span>
        {/if}
        {#if indexStatus?.error}<pre>{indexStatus.error}</pre>{/if}
        {#if loadError && packagesLoaded}<div class="error-state"><pre>{loadError}</pre><button class="secondary-action" onclick={() => void refreshComponentIndex()}>RETRY</button></div>{/if}
        {#if indexingDiagnostics.length}
          <details aria-label="Component indexing diagnostics">
            <summary>{indexingDiagnostics.length} source issues · show details</summary>
            {#each indexingDiagnostics as diagnostic, i (i)}<pre>{diagnostic}</pre>{/each}
          </details>
        {/if}
      </div>
      <button class="secondary-action" onclick={rebuildComponentIndex} disabled={indexStatus?.phase === 'indexing'}>REBUILD INDEX</button>
    </section>
  {/if}

  {#if activeTab === 'hub'}
    <section class="component-index-status" aria-label="Hub connection">
      <div class="index-message">
        <span>{registryUrl}</span>
        <div class="registry-controls">
          <input aria-label="Registry URL" bind:value={registryUrl} />
          <button class="secondary-action" onclick={() => void searchHub(true)} disabled={hubSearchBusy}>CONNECT</button>
        </div>
        {#if loadError && hubLoaded}<pre class="hub-error">{loadError}</pre>{/if}
      </div>
    </section>
  {/if}

  <main class="library-content">
    {#if loading}
      <div class="state">LOADING LIBRARY...</div>
    {:else if loadError && !((activeTab === 'components' && packagesLoaded) || (activeTab === 'hub' && hubLoaded))}
      <div class="error-state">
        <strong>LIBRARY ERROR</strong>
        <pre>{loadError}</pre>
        <button onclick={retry}>RETRY</button>
      </div>
    {:else if activeTab === 'hub'}
      {#if hubReleases.length === 0}
        <div class="state">{hubSearchBusy ? 'SEARCHING HUB...' : 'NO MATCHING PACKAGES'}</div>
      {:else}
        <div class="library-grid">
          {#each hubReleases as release (`${release.header.packageId}@${release.header.version}`)}
            <article class="library-card">
              <div class="card-title"><h3>{release.header.displayName}</h3><span>{release.header.version}</span></div>
              <p>{release.header.packageId}</p>
              <p>{release.header.components?.length ?? 0} exports{release.license ? ` · ${release.license}` : ''}</p>
              <div class="tag-list">{#each release.header.tags as tag (tag)}<span>{tag}</span>{/each}</div>
              <button class="primary-action" aria-label={`INSTALL ${release.header.displayName}`} onclick={() => installHubPackage(release)} disabled={Boolean(hubInstallId)}>
                {hubInstallId === `${release.header.packageId}@${release.header.version}` ? 'INSTALLING...' : 'INSTALL'}
              </button>
            </article>
          {/each}
        </div>
      {/if}
    {:else if activeTab === 'components'}
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
              <p class="component-description">
                <strong>DESCRIPTION</strong>
                <span>{component.oneLiner.trim() || 'No description available.'}</span>
              </p>
              {#if component.paramKeys.length}
                <div class="card-stats component-parameters"><strong>PARAMETERS</strong><span>{component.paramKeys.join(' / ')}</span></div>
              {/if}
              {#if component.revisionDigest}
                <div class="card-stats component-revision"><strong>REVISION</strong><code>{component.revisionDigest.slice(0, 19)}</code></div>
              {/if}
              {#if component.version}
                <div class="card-stats component-revision"><strong>VERSION</strong><span>{component.version}</span></div>
              {/if}
              <div class="card-stats component-source">
                <strong>SOURCE THREAD</strong>
                <span>{component.origin === 'builtin' ? 'Built-in library' : component.threadTitle?.trim() || 'SOURCE THREAD UNAVAILABLE'}</span>
                {#if component.origin !== 'builtin' && component.threadId}<small>ID {component.threadId}</small>{/if}
              </div>
              {#if component.tags.length}
                <div class="tag-list">{#each component.tags as tag (tag)}<span>{tag}</span>{/each}</div>
              {/if}
              <button
                class="primary-action"
                aria-label={`${importingComponentId === (component.componentId ?? `${component.origin}:${component.name}`) ? 'INSERTING' : 'INSERT'} ${component.name}`}
                onclick={() => insertAvailableComponent(component)}
                disabled={Boolean(importingComponentId) || !onApplyComponentImport}
              >
                {importingComponentId === (component.componentId ?? `${component.origin}:${component.name}`) ? 'INSERTING...' : 'INSERT INTO CODE'}
              </button>
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
                  {#each pkg.tags as tag (tag)}
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
  .registry-controls { display: flex; gap: 0.5rem; }
  .registry-controls input { min-width: 0; flex: 1; background: var(--bg); color: var(--text); border: 1px solid var(--border); }
  .hub-error { color: var(--danger, #ef6464); }

  .component-index-status {
    height: 4.5rem;
    min-height: 4.5rem;
    flex-shrink: 0;
    overflow: auto;
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    box-sizing: border-box;
    border-bottom: 1px solid var(--border);
    font-size: 0.75rem;
  }
  .index-message { flex: 1; min-width: 0; }
  .component-index-status > button { flex-shrink: 0; position: sticky; top: 0; }
  .component-index-status pre { white-space: pre-wrap; overflow-wrap: anywhere; margin: 0.25rem 0; }
  .component-index-status .error-state { padding: 0; border: 0; }

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
    font-size: 0.68rem;
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

  .component-description,
  .component-parameters,
  .component-revision,
  .component-source {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 4px;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .component-description strong,
  .component-parameters strong,
  .component-revision strong,
  .component-source strong {
    color: var(--primary);
    font-size: 0.68rem;
    letter-spacing: 0.06em;
  }

  .component-description span,
  .component-parameters span,
  .component-revision span,
  .component-source span,
  .component-source small {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .component-revision code,
  .component-source small {
    color: var(--text-dim);
    font-size: 0.64rem;
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
