<script>
  import PromptPanel from './lib/PromptPanel.svelte';
  import Viewer from './lib/Viewer.svelte';
  import CodePanel from './lib/CodePanel.svelte';
  import ParamPanel from './lib/ParamPanel.svelte';
  import ConfigPanel from './lib/ConfigPanel.svelte';
  import Dropdown from './lib/Dropdown.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { save } from '@tauri-apps/plugin-dialog';
  import { writeTextFile } from '@tauri-apps/plugin-fs';
  import { onMount } from 'svelte';

  import HistoryPanel from './lib/HistoryPanel.svelte';
  import CodeModal from './lib/CodeModal.svelte';

  let currentView = $state('workbench'); // 'workbench' or 'config'
  let macroCode = $state('');
  let stlUrl = $state(null);
  let isGenerating = $state(false);
  let status = $state('System ready.');
  let error = $state(null);
  
  let uiSpec = $state(null);
  let parameters = $state({});
  let history = $state([]);
  let activeThreadId = $state(null);
  let activeVersionId = $state(null);
  
  // Modals state
  let showCodeModal = $state(false);
  let selectedCode = $state('');
  let selectedTitle = $state('');

  // Centralized Config State
  let config = $state({ engines: [], selected_engine_id: '' });
  let availableModels = $state([]);
  let isLoadingModels = $state(false);

  // Workbench layout state
  let sidebarWidth = $state(320);
  let historyHeight = $state(400);
  let dialogueHeight = $state(250);
  let isResizingWidth = $state(false);
  let isResizingHeight = $state(false);
  let isResizingHistory = $state(false);

  let viewerComponent = $state(null);

  // Derived active thread
  const activeThread = $derived(history.find(t => t.id === activeThreadId));

  onMount(async () => {
    await Promise.all([
      loadConfig(),
      restoreLastDesign(),
      loadHistory()
    ]);
  });

  async function loadHistory() {
    try {
      history = await invoke('get_history');
    } catch (e) {
      console.error("Failed to load history:", e);
    }
  }

  async function restoreLastDesign() {
    try {
      const last = await invoke('get_last_design');
      if (last) {
        macroCode = last.macro_code;
        uiSpec = last.ui_spec;
        parameters = last.initial_params || {};
        status = 'Restored last design session.';
        // Trigger render now that path is fixed
        await handleParamChange(parameters);
      } else {
        await fetchDefaultMacro();
      }
    } catch (e) {
      console.error("Failed to restore last design:", e);
      await fetchDefaultMacro();
    }
  }

  async function fetchDefaultMacro() {
    try {
      const code = await invoke('get_default_macro');
      if (!macroCode) macroCode = code;
    } catch (e) {
      console.error("Failed to load default macro:", e);
    }
  }

  async function loadConfig() {
    try {
      config = await invoke('get_config');
      if (config.selected_engine_id) {
        // Initial fetch
        await fetchModels();
      }
    } catch (e) {
      error = `Config Load Error: ${e}`;
    }
  }

  async function saveConfig() {
    try {
      await invoke('save_config', { config });
      // Don't overwrite error if it's already set to something more useful
      if (!error) status = 'Configuration saved.';
    } catch (e) {
      error = `Config Save Error: ${e}`;
    }
  }

  const selectedEngine = $derived(config.engines.find(e => e.id === config.selected_engine_id));

  // 2. Auto-fetch models when ANY relevant config field changes
  // We use $derived.by to track nested changes effectively in the effect
  const fetchTrigger = $derived.by(() => {
    if (!selectedEngine) return null;
    return {
      id: selectedEngine.id,
      key: selectedEngine.api_key,
      provider: selectedEngine.provider,
      url: selectedEngine.base_url
    };
  });

  let fetchTimeout;
  $effect(() => {
    if (fetchTrigger && (fetchTrigger.key || fetchTrigger.provider === 'ollama')) {
      clearTimeout(fetchTimeout);
      fetchTimeout = setTimeout(() => {
        fetchModels();
      }, 800); // Slightly longer debounce
    }
  });

  async function fetchModels() {
    if (!selectedEngine) return;
    if (!selectedEngine.api_key && selectedEngine.provider !== 'ollama') {
      availableModels = [];
      return;
    }
    
    isLoadingModels = true;
    error = null; // Clear previous errors
    status = `Fetching models for ${selectedEngine.provider}...`;
    
    try {
      const models = await invoke('list_models', {
        provider: selectedEngine.provider,
        apiKey: selectedEngine.api_key,
        baseUrl: selectedEngine.base_url
      });
      availableModels = models;
      status = `Engine active: ${models.length} models available.`;
    } catch (e) {
      console.error("Failed to fetch models:", e);
      availableModels = [];
      // 3. Real raw error reporting
      error = `${e}`; 
      status = 'Engine configuration error.';
    } finally {
      isLoadingModels = false;
    }
  }

  async function handleEngineChange(id) {
    config.selected_engine_id = id;
    await saveConfig();
    // effect will handle fetching
  }

  function startResizingWidth(e) {
    isResizingWidth = true;
    e.preventDefault();
  }

  function startResizingHeight(e) {
    isResizingHeight = true;
    e.preventDefault();
  }

  function startResizingHistory(e) {
    isResizingHistory = true;
    e.preventDefault();
  }

  function handleMouseMove(e) {
    if (isResizingWidth) {
      sidebarWidth = Math.max(250, Math.min(e.clientX, window.innerWidth - 300));
    } else if (isResizingHeight) {
      dialogueHeight = Math.max(120, Math.min(window.innerHeight - e.clientY, window.innerHeight - 150));
    } else if (isResizingHistory) {
      const sidebarRect = document.querySelector('.sidebar')?.getBoundingClientRect();
      if (sidebarRect) {
        historyHeight = Math.max(100, Math.min(e.clientY - sidebarRect.top, sidebarRect.height - 100));
      }
    }
  }

  function stopResizing() {
    isResizingWidth = false;
    isResizingHeight = false;
    isResizingHistory = false;
  }

  async function handleGenerate(initialPrompt) {
    isGenerating = true;
    error = null;
    let currentPrompt = initialPrompt;
    let maxAttempts = 3;
    let attempt = 1;
    
    let currentImageData = null;
    if (viewerComponent && stlUrl) {
      currentImageData = viewerComponent.captureScreenshot();
    }

    while (attempt <= maxAttempts) {
      status = `Consulting LLM (Attempt ${attempt}/${maxAttempts})...`;
      try {
        const data = await invoke('generate_design', { 
          prompt: currentPrompt,
          threadId: activeThreadId,
          parentMacroCode: !activeThreadId ? macroCode : null,
          isRetry: attempt > 1,
          imageData: currentImageData
        });
        
        status = 'Parsing geometry specification and UI...';
        macroCode = data.macro_code;
        uiSpec = data.ui_spec;
        parameters = data.initial_params || {};

        await loadHistory();
        if (!activeThreadId && history.length > 0) {
          activeThreadId = history[0].id;
        }

        const updatedThread = history.find(t => t.id === activeThreadId);
        if (updatedThread) {
          const lastMsg = [...updatedThread.messages].reverse().find(m => m.role === 'assistant' && m.output);
          if (lastMsg) activeVersionId = lastMsg.id;
        }

        status = 'Executing FreeCAD engine (BRep/STL)...';
        try {
          const absolutePath = await invoke('render_stl', { 
            macroCode: macroCode, 
            parameters 
          });
          stlUrl = convertFileSrc(absolutePath);
          status = 'Design synthesized and rendered successfully.';
          error = null;
          break; // Success! Exit loop.
        } catch (renderError) {
          console.error("Render failed on attempt", attempt, renderError);
          error = `Render Error: ${renderError}`;
          
          if (attempt < maxAttempts) {
            // Setup prompt for next retry
            currentPrompt = `The previous code failed during execution in FreeCAD with this error:\n${renderError}\n\nPlease fix the python code and return the updated JSON.`;
            attempt++;
          } else {
            status = 'Failed after maximum attempts.';
            // Open the code modal so user can manually edit
            selectedCode = macroCode;
            selectedTitle = data.title;
            showCodeModal = true;
            break;
          }
        }
      } catch (e) {
        error = `Generation Failed: ${e}`;
        status = 'LLM API Error.';
        break; // LLM failed entirely, don't retry FreeCAD logic
      }
    }
    
    isGenerating = false;
  }

  async function handleParamChange(newParams, forcedCode = null) {
    parameters = { ...parameters, ...newParams };
    const codeToUse = forcedCode || macroCode;
    
    if (!codeToUse) return;
    
    status = 'Executing FreeCAD engine (BRep/STL)...';
    try {
      const absolutePath = await invoke('render_stl', { 
        macroCode: codeToUse, 
        parameters 
      });
      stlUrl = convertFileSrc(absolutePath);
      status = 'Geometry updated.';
      error = null;
    } catch (e) {
      error = `Render Error: ${e}`;
      status = 'Render failed.';
    }
  }

  function openCodeModal(message) {
    if (message.output) {
      selectedCode = message.output.macro_code;
      selectedTitle = message.output.title;
      showCodeModal = true;
    }
  }

  async function loadVersion(msg) {
    if (!msg || !msg.output) return;
    console.log("Loading version:", msg.id);
    activeVersionId = msg.id;
    macroCode = msg.output.macro_code;
    uiSpec = msg.output.ui_spec;
    parameters = msg.output.initial_params || {};
    status = `Loaded Version: ${msg.output.title}`;
    // Pass code explicitly to ensure handleParamChange doesn't use old state
    await handleParamChange(parameters, msg.output.macro_code);
  }

  async function loadFromHistory(thread) {
    activeThreadId = thread.id;
    const lastAssistantMsg = [...thread.messages].reverse().find(m => m.role === 'assistant' && m.output);
    if (lastAssistantMsg) {
      await loadVersion(lastAssistantMsg);
    }
  }

  async function deleteThread(id) {
    try {
      await invoke('delete_thread', { id });
      if (activeThreadId === id) {
        activeThreadId = null;
        activeVersionId = null;
      }
      await loadHistory();
    } catch (e) {
      error = `Delete Error: ${e}`;
    }
  }

  function createNewThread() {
    activeThreadId = null;
    activeVersionId = null;
    macroCode = '';
    uiSpec = null;
    parameters = {};
    stlUrl = null;
    status = 'New design session started.';
  }

  function forkDesign() {
    // We keep current macroCode, parameters, and stlUrl
    // but detach from the current thread
    activeThreadId = null;
    activeVersionId = null;
    status = 'Design forked. Next generation will create a new thread.';
  }

  async function commitManualVersion(editedCode) {
    if (!activeThreadId) {
      error = "Cannot commit manual version: No active thread. Please generate first.";
      return;
    }
    
    // Validate by rendering first
    status = "Validating manual edit...";
    try {
      const absolutePath = await invoke('render_stl', { 
        macroCode: editedCode, 
        parameters 
      });
      stlUrl = convertFileSrc(absolutePath);
      
      // Save to DB
      await invoke('add_manual_version', {
        threadId: activeThreadId,
        title: activeThread?.title || "Manual Edit",
        macroCode: editedCode,
        parameters,
        uiSpec
      });
      
      macroCode = editedCode;
      await loadHistory();
      
      // Select the new version
      const updatedThread = history.find(t => t.id === activeThreadId);
      if (updatedThread) {
        const lastMsg = [...updatedThread.messages].reverse().find(m => m.role === 'assistant' && m.output);
        if (lastMsg) activeVersionId = lastMsg.id;
      }
      
      status = "Manual version committed successfully.";
      showCodeModal = false;
    } catch (e) {
      error = `Manual Commit Failed: ${e}`;
      status = "Validation failed. Check your Python code.";
    }
  }

  function toggleConfig() {
    currentView = currentView === 'workbench' ? 'config' : 'workbench';
  }

  async function exportMacro() {
    if (!macroCode) return;
    try {
      const path = await save({
        filters: [{ name: 'FreeCAD Macro', extensions: ['FCMacro', 'py'] }],
        defaultPath: 'design.FCMacro'
      });
      if (path) {
        await writeTextFile(path, macroCode);
        status = 'Macro exported successfully.';
      }
    } catch (e) {
      error = `Export Error: ${e}`;
    }
  }

  async function exportSTL() {
    if (!stlUrl) return;
    try {
      const path = await save({
        filters: [{ name: 'STL 3D Model', extensions: ['stl'] }],
        defaultPath: 'design.stl'
      });
      if (path) {
        // extract absolute local path from asset:// URL
        // asset://localhost/Users/bogdan/.../file.stl?t=123
        let rawPath = decodeURIComponent(stlUrl.split('?')[0].replace('asset://localhost/', '/'));
        // Tauri's convertFileSrc prepends asset://localhost on macOS/Linux. 
        // We ensure it starts with / on Unix.
        if (!rawPath.startsWith('/') && rawPath.match(/^[a-zA-Z]:/)) {
          // Windows path handling, just in case
        } else if (!rawPath.startsWith('/')) {
           rawPath = '/' + rawPath;
        }

        await invoke('export_file', { sourcePath: rawPath, targetPath: path });
        status = 'STL exported successfully.';
      }
    } catch (e) {
      error = `Export Error: ${e}`;
    }
  }
</script>

<div 
  class="app-page" 
  onmousemove={handleMouseMove} 
  onmouseup={stopResizing}
  onmouseleave={stopResizing}
>
  <div class="system-bar {error ? 'has-error' : ''}">
    <div class="system-bar__left">
      <span class="app-title">DRYDEMACHER</span>
      {#if currentView === 'workbench'}
        {#if config.engines.length > 1}
          <div class="engine-dropdown-wrapper">
            <Dropdown 
              options={config.engines.map(e => ({ id: e.id, name: `${e.provider}: ${e.name}` }))} 
              bind:value={config.selected_engine_id} 
              onchange={handleEngineChange} 
            />
          </div>
        {:else if selectedEngine}
          <span class="engine-info {error ? 'engine-info--error' : ''}">
            {selectedEngine.provider} {isLoadingModels ? '(connecting...)' : (error ? '(offline)' : 'active')}
          </span>
        {/if}
      {/if}
    </div>
    <div class="system-bar__center">
      {#if isGenerating}
        <div class="mini-spinner"></div>
      {/if}
      <span class="status-text">{error || status}</span>
    </div>
    <div class="system-bar__right">
      <button class="icon-btn" onclick={toggleConfig} title="Toggle Configuration">
        {currentView === 'config' ? '⚒️' : '⚙️'}
      </button>
    </div>
  </div>

  <div class="app-container">
    {#if currentView === 'config'}
      <ConfigPanel 
        bind:config 
        {availableModels} 
        {isLoadingModels} 
        onsave={saveConfig} 
      />
    {:else}
      <div class="workbench">
        <aside class="sidebar" style="width: {sidebarWidth}px">
          <div class="sidebar-section flex-1">
            <div class="pane-header">TUNABLE PARAMETERS</div>
            <div class="sidebar-content scrollable">
              <ParamPanel {uiSpec} {parameters} onchange={handleParamChange} />
            </div>
          </div>

          <div class="resizer-v {isResizingHistory ? 'resizing' : ''}" onmousedown={startResizingHistory} role="separator"></div>

          <div class="sidebar-section" style="height: {historyHeight}px">
            <div class="pane-header">THREAD HISTORY</div>
            <div class="sidebar-content scrollable">
              <HistoryPanel 
                {history} 
                {activeThreadId}
                onSelect={loadFromHistory}
                onDelete={deleteThread}
                onNew={createNewThread}
              />
            </div>
          </div>
        </aside>

        <div class="resizer-w {isResizingWidth ? 'resizing' : ''}" onmousedown={startResizingWidth} role="separator"></div>

        <div class="main-workbench">
          <main class="viewport-area">
            <Viewer bind:this={viewerComponent} {stlUrl} />
            
            {#if macroCode || stlUrl}
              <div class="viewport-overlay">
                <div class="export-actions">
                  <button class="btn btn-xs btn-secondary" onclick={forkDesign} title="Fork this design into a new project">🍴 FORK</button>
                  <button class="btn btn-xs btn-ghost" onclick={exportMacro} disabled={!macroCode} title="Export Python Macro">💾 MACRO</button>
                  <button class="btn btn-xs btn-primary" onclick={exportSTL} disabled={!stlUrl} title="Export STL for 3D Printing">💾 STL</button>
                </div>
              </div>
            {/if}
          </main>
          
          <div class="resizer-v {isResizingHeight ? 'resizing' : ''}" onmousedown={startResizingHeight} role="separator"></div>

          <div class="dialogue-area" style="height: {dialogueHeight}px">
            <div class="pane-header">
              DIALOGUE: {activeThread ? activeThread.title : (macroCode ? '[Forked Design]' : '[New Design]')}
            </div>
            <div class="dialogue-content">
              <PromptPanel 
                onGenerate={handleGenerate} 
                {isGenerating} 
                messages={activeThread ? activeThread.messages : []}
                onShowCode={openCodeModal}
                bind:activeVersionId
                onVersionChange={loadVersion}
              />
            </div>
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if showCodeModal}
    <CodeModal 
      bind:code={selectedCode} 
      title={selectedTitle} 
      onCommit={commitManualVersion}
      onclose={() => showCodeModal = false} 
    />
  {/if}
</div>

<style>
  .app-title {
    font-weight: bold;
    color: var(--primary);
    margin-right: 16px;
    font-size: 0.8rem;
    letter-spacing: 0.1em;
  }

  .icon-btn {
    background: none;
    border: none;
    color: var(--text);
    font-size: 1.2rem;
    cursor: pointer;
    padding: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    transition: background 0.2s;
  }

  .icon-btn:hover {
    background: var(--bg-300);
  }

  .flex-1 {
    flex: 1;
  }

  .sidebar-section {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  .app-container {
    padding: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  .workbench {
    display: flex;
    height: 100%;
    width: 100%;
    background: var(--bg);
    overflow: hidden;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    background: var(--bg-100);
    min-width: 200px;
    overflow: hidden;
  }

  .sidebar-content {
    flex: 1;
    min-height: 0;
  }

  .main-workbench {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  .viewport-area {
    flex: 1;
    min-height: 100px;
    background: #0b0f1a;
    position: relative;
    overflow: hidden;
  }

  .dialogue-area {
    flex-shrink: 0;
    background: var(--bg-100);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .dialogue-content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .pane-header {
    padding: 4px 12px;
    background: var(--bg-200);
    border-bottom: 1px solid var(--bg-300);
    color: var(--secondary);
    font-size: 0.6rem;
    font-weight: bold;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    user-select: none;
  }

  .scrollable {
    overflow-y: auto;
  }

  .resizer-w {
    width: 4px;
    background: var(--bg-300);
    cursor: col-resize;
    transition: background 0.2s;
    z-index: 10;
  }

  .resizer-v {
    height: 4px;
    background: var(--bg-300);
    cursor: row-resize;
    transition: background 0.2s;
    z-index: 10;
    flex-shrink: 0;
  }

  .resizer-w:hover, .resizer-w.resizing,
  .resizer-v:hover, .resizer-v.resizing {
    background: var(--primary);
  }

  .system-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    background: var(--bg-100);
    border-bottom: 1px solid var(--bg-300);
    min-height: 32px;
    user-select: none;
  }

  .system-bar.has-error {
    background: color-mix(in srgb, var(--red) 15%, var(--bg-100));
  }

  .system-bar.has-error .status-text {
    color: var(--red);
  }

  .system-bar__center {
    flex: 1;
    text-align: center;
    padding: 0 20px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .engine-dropdown-wrapper {
    width: 250px;
  }

  .mini-spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--primary);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .status-text {
    font-size: 0.7rem;
    color: var(--text-dim);
    text-transform: uppercase;
    font-family: var(--font-mono);
    letter-spacing: 0.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    display: block;
  }

  .engine-info {
    font-size: 0.65rem;
    color: var(--text-dim);
    text-transform: uppercase;
    font-family: var(--font-mono);
  }

  .engine-info--error {
    color: var(--red);
  }

  .export-actions {
    display: flex;
    gap: 8px;
  }

  .viewport-overlay {
    position: absolute;
    bottom: 12px;
    right: 12px;
    background: rgba(11, 15, 26, 0.6);
    backdrop-filter: blur(4px);
    padding: 8px;
    border: 1px solid var(--bg-300);
    z-index: 50;
  }
</style>
