<script>
  import PromptPanel from './lib/PromptPanel.svelte';
  import Viewer from './lib/Viewer.svelte';
  import VertexGenie from './lib/VertexGenie.svelte';
  import DrawingOverlay from './lib/DrawingOverlay.svelte';
  import ParamPanel from './lib/ParamPanel.svelte';
  import ConfigPanel from './lib/ConfigPanel.svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { save } from '@tauri-apps/plugin-dialog';
  import { writeTextFile } from '@tauri-apps/plugin-fs';
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';

  import HistoryPanel from './lib/HistoryPanel.svelte';
  import CodeModal from './lib/CodeModal.svelte';
  import DeletedModels from './lib/DeletedModels.svelte';
  import { startMicrowaveHum, stopMicrowaveHum, stopMicrowaveAudio, playDing, playErrorBuzz, getActiveMicrowaveCount, setAudibleThread } from './lib/audio/microwave';
  import { session } from './lib/stores/sessionStore';
  import { handleGenerate, initOrchestrator, isQuestionIntent } from './lib/controllers/requestOrchestrator';
  import { handleParamChange, commitManualVersion } from './lib/controllers/manualController';
  import { loadFromHistory, deleteThread, createNewThread, forkDesign, deleteVersion, loadVersion } from './lib/stores/history';
  import { workingCopy, isDirty } from './lib/stores/workingCopy';
  import { history, activeThreadId, activeVersionId, config, availableModels, isLoadingModels } from './lib/stores/domainState';
  import { sidebarWidth, historyHeight, dialogueHeight, showCodeModal, selectedCode, selectedTitle, currentView } from './lib/stores/viewState';
  import { boot, saveConfig, fetchModels } from './lib/boot/restore';
  import { requestQueue, allRequests, activeRequests, activeRequestCount, currentActiveRequest, activeThreadBusy, activeThreadRequests } from './lib/stores/requestQueue';
  import { nowSeconds } from './lib/stores/timeEngine';
  import { paramPanelState } from './lib/stores/paramPanelState';

  // Local reactive aliases for templates
  const phase = $derived($session.phase);
  const status = $derived($session.status);
  const error = $derived($session.error);
  const stlUrl = $derived($session.stlUrl);

  const isBooting = $derived(phase === 'booting');
  const isQuestionFlow = $derived(phase === 'answering');

  let viewerComponent = $state(null);
  let drawingOverlay = $state(null);
  let drawMode = $state(false);
  let lastAssistantMessageId = $state(null);
  let lastAdvisorBubble = $state('');
  let lastAdvisorQuestion = $state('');
  let dismissedBubbleText = $state('');

  let isResizingWidth = $state(false);
  let isResizingHeight = $state(false);
  let isResizingHistory = $state(false);

  // Initialize async design orchestrator
  initOrchestrator({
    get viewerComponent() { return viewerComponent; },
    openCodeModalManual: (data) => {
      selectedCode.set($workingCopy.macroCode);
      selectedTitle.set($workingCopy.title || data.title);
      showCodeModal.set(true);
    },
    getDrawingCanvas: () => drawingOverlay?.hasDrawing() ? drawingOverlay.getCanvas() : null,
    clearDrawing: () => { drawingOverlay?.clear(); drawMode = false; },
  });

  // Shut down audio context when idle for 2s
  let idleTimeout;
  $effect(() => {
    if ($activeRequestCount === 0) {
      idleTimeout = setTimeout(() => stopMicrowaveAudio(true), 2000);
    } else {
      clearTimeout(idleTimeout);
    }
  });

  // Wire thread changes to audio focus
  $effect(() => {
    setAudibleThread($activeThreadId);
  });

  function formatCookingTime(s) {
    const m = Math.floor(s / 60);
    const sec = s % 60;
    return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
  }

  onMount(() => {
    boot();
  });

  const activeThread = $derived($history.find(t => t.id === $activeThreadId));
  const eckyTraits = $derived(activeThread?.genieTraits || {});
  const eckyIntensity = $derived(1.0 + Math.max(0, ($activeRequestCount - 1) * 0.25));
  const inFlightByThread = $derived.by(() => {
    const counts = {};
    for (const req of $allRequests) {
      if (!req?.threadId) continue;
      if (['success', 'error', 'canceled'].includes(req.phase)) continue;
      counts[req.threadId] = (counts[req.threadId] || 0) + 1;
    }
    return counts;
  });

  const latestAssistantMessage = $derived.by(() => {
    if (!activeThread?.messages?.length) return null;
    return [...activeThread.messages].reverse().find(m => m.role === 'assistant' && m.status !== 'pending') ?? null;
  });

  const assistantBubble = $derived.by(() => {
    if (!latestAssistantMessage) return '';
    const out = latestAssistantMessage.output;
    return out?.response || (out?.title ? `Generated: ${out.title}` : latestAssistantMessage.content) || '';
  });

  const assistantFresh = $derived.by(() => {
    if (!latestAssistantMessage?.timestamp) return false;
    return $nowSeconds - latestAssistantMessage.timestamp <= 300;
  });

  $effect(() => {
    const msgId = latestAssistantMessage?.id;
    if (msgId && msgId !== lastAssistantMessageId) {
      lastAssistantMessageId = msgId;
      if (assistantFresh) {
        lastAdvisorBubble = assistantBubble;
        dismissedBubbleText = '';
      } else {
        lastAdvisorBubble = '';
        dismissedBubbleText = '';
      }
    }
  });

  const activeThreadHighestPhase = $derived.by(() => {
    if (phase === 'booting') return 'booting';
    
    // Check if there is an error specifically in this thread
    const threadErrors = $activeThreadRequests.filter(r => r.phase === 'error' && r.error);
    if (threadErrors.length > 0) return 'error';

    const reqPhases = $activeThreadRequests.map(r => r.phase);
    if (reqPhases.some(p => ['rendering', 'queued_for_render', 'committing'].includes(p))) return 'rendering';
    if (reqPhases.some(p => p === 'repairing')) return 'repairing';
    if (reqPhases.some(p => p === 'generating')) return 'generating';
    if (reqPhases.some(p => p === 'answering')) return 'answering';
    if (reqPhases.some(p => p === 'classifying')) return 'classifying';
    
    return 'idle';
  });

  const genieMode = $derived.by(() => {
    const atPhase = activeThreadHighestPhase;
    if (atPhase === 'error') return 'error';
    if (atPhase === 'repairing') return 'repairing';
    if (atPhase === 'classifying') return 'light';
    if (atPhase === 'rendering') return 'rendering';
    if (atPhase === 'generating' || atPhase === 'answering') return 'thinking';
    if (assistantFresh && !dismissedBubbleText && lastAdvisorBubble) return 'speaking';
    return 'idle';
  });

  const genieBubble = $derived.by(() => {
    const atPhase = activeThreadHighestPhase;
    const threadError = $activeThreadRequests.find(r => r.phase === 'error')?.error;
    
    const raw = threadError || 
               (atPhase === 'repairing' ? $session.repairMessage : null) || 
               (['classifying', 'generating', 'answering'].includes(atPhase) ? $session.cookingPhrase : null) || 
               lastAdvisorBubble || '';
    return (dismissedBubbleText === raw) ? '' : raw;
  });

  async function toggleMicrowaveMute() {
    const newMuted = !($config?.microwave?.muted);
    config.update(c => ({ 
      ...c, 
      microwave: { 
        ...(c.microwave || { humId: null, dingId: null }), 
        muted: newMuted 
      } 
    }));
    if (newMuted) stopMicrowaveAudio(true);
    await saveConfig();
  }

  function applyCompletedRequest(req) {
    if (!req?.result) return;
    const { design, threadId, messageId, stlUrl: reqStlUrl } = req.result;
    if (design) {
      workingCopy.loadVersion(design, messageId);
      paramPanelState.hydrateFromVersion(design, messageId);
    }
    if (threadId) {
      activeThreadId.set(threadId);
      activeVersionId.set(messageId);
    }
    if (reqStlUrl) {
      session.setStlUrl(reqStlUrl);
    }
    requestQueue.setActive(req.id);
  }

  function dismissRequest(id) {
    requestQueue.remove(id);
  }

  function retryRequest(req) {
    handleGenerate(req.prompt, req.attachments);
    requestQueue.remove(req.id);
  }

  function cancelRequest(id) {
    requestQueue.cancel(id);
  }

  function phaseLabel(phase) {
    const labels = {
      classifying: 'ROUTING',
      generating: 'LLM',
      queued_for_render: 'QUEUED',
      rendering: 'FREECAD',
      committing: 'SAVING',
      success: 'DONE',
      error: 'ERROR',
      canceled: 'CANCELED',
    };
    return labels[phase] || phase.toUpperCase();
  }

  async function exportSTL() {
    if (!stlUrl) return;
    try {
      const path = await save({ filters: [{ name: 'STL 3D Model', extensions: ['stl'] }], defaultPath: 'design.stl' });
      if (path) {
        let rawPath = decodeURIComponent(stlUrl.split('?')[0].replace('asset://localhost/', '/'));
        if (!rawPath.startsWith('/') && rawPath.match(/^[a-zA-Z]:/)) {} else if (!rawPath.startsWith('/')) { rawPath = '/' + rawPath; }
        await invoke('export_file', { sourcePath: rawPath, targetPath: path });
      }
    } catch (e) {
      session.setError(`Export Error: ${e}`);
    }
  }

  function dismissGenie() {
    if (genieBubble) dismissedBubbleText = genieBubble;
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
      $sidebarWidth = Math.max(250, Math.min(e.clientX, window.innerWidth - 300));
    } else if (isResizingHeight) {
      $dialogueHeight = Math.max(120, Math.min(window.innerHeight - e.clientY, window.innerHeight - 150));
    } else if (isResizingHistory) {
      const sidebarRect = document.querySelector('.sidebar')?.getBoundingClientRect();
      if (sidebarRect) {
        const heightFromBottom = sidebarRect.bottom - e.clientY;
        $historyHeight = Math.max(100, Math.min(heightFromBottom, sidebarRect.height - 100));
      }
    }
  }

  function stopResizing() {
    isResizingWidth = false;
    isResizingHeight = false;
    isResizingHistory = false;
  }

  function handleGlobalKeydown(e) {
    if (e.key === 'Escape') stopResizing();
  }
</script>

<svelte:window onmousemove={handleMouseMove} onmouseup={stopResizing} onblur={stopResizing} onkeydown={handleGlobalKeydown} />

<div class="app-page" role="application">
  <div class="app-overlay-actions">
    {#if $activeRequestCount > 0}
      <button class="overlay-icon-btn" onclick={toggleMicrowaveMute} title="Toggle Cafeteria Hum">
        {$config?.microwave?.muted ? '🔇' : '🔊'}
      </button>
    {/if}
    {#if $currentView !== 'config'}
      <button class="overlay-icon-btn" class:draw-active={drawMode} onclick={() => drawMode = !drawMode} title={drawMode ? 'Exit Draw Mode' : 'Draw Annotations'}>
        ✏️
      </button>
    {/if}
    <button class="settings-overlay-btn" onclick={() => currentView.set($currentView === 'config' ? 'workbench' : 'config')} title="Configuration">
      {$currentView === 'config' ? '⚒️' : '⚙️'}
    </button>
    <button class="settings-overlay-btn" onclick={() => currentView.set($currentView === 'trash' ? 'workbench' : 'trash')} title="Trash">
      {$currentView === 'trash' ? '⚒️' : '🗑️'}
    </button>
  </div>

  <div class="app-container">
    {#if $currentView === 'config'}
      <ConfigPanel
        bind:config={$config}
        availableModels={$availableModels}
        isLoadingModels={$isLoadingModels}
        onfetch={fetchModels}
        onsave={saveConfig}
      />
    {:else if $currentView === 'trash'}
      <DeletedModels />
    {:else}
      <div class="workbench">
        <aside class="sidebar" style="width: {$sidebarWidth}px">
          <div class="sidebar-section flex-1">
            <div class="pane-header">TUNABLE PARAMETERS</div>
            <div class="sidebar-content scrollable">
              <ParamPanel 
                uiSpec={$paramPanelState.uiSpec} 
                onspecchange={(spec, newParams) => {
                  paramPanelState.setUiSpec(spec);
                  workingCopy.patch({ uiSpec: spec });
                  if (newParams) {
                    paramPanelState.setParams(newParams);
                    workingCopy.patch({ params: newParams });
                  }
                }}
                parameters={$paramPanelState.params} 
                macroCode={$paramPanelState.macroCode}
                onchange={handleParamChange} 
                activeVersionId={$paramPanelState.versionId} 
              />
            </div>
          </div>
          <div class="resizer-v" role="slider" aria-label="Resize history" aria-orientation="vertical" aria-valuenow={$historyHeight} tabindex="0" onmousedown={startResizingHistory} onkeydown={(e) => {
            if (e.key === 'ArrowUp') historyHeight.set($historyHeight + 10);
            if (e.key === 'ArrowDown') historyHeight.set($historyHeight - 10);
          }}></div>
          <div class="sidebar-section" style="height: {$historyHeight}px">
            <div class="pane-header">THREAD HISTORY</div>
            <div class="sidebar-content scrollable">
              <HistoryPanel history={$history} activeThreadId={$activeThreadId} 
                inFlightByThread={inFlightByThread}
                onSelect={loadFromHistory} 
                onDelete={deleteThread}
                onNew={createNewThread} 
              />
            </div>
          </div>
        </aside>

        <div class="resizer-w" role="slider" aria-label="Resize sidebar" aria-orientation="horizontal" aria-valuenow={$sidebarWidth} tabindex="0" onmousedown={startResizingWidth} onkeydown={(e) => {
          if (e.key === 'ArrowLeft') sidebarWidth.set($sidebarWidth - 10);
          if (e.key === 'ArrowRight') sidebarWidth.set($sidebarWidth + 10);
        }}></div>

        <div class="main-workbench">
          <main class="viewport-area" role="presentation">
            <Viewer
              bind:this={viewerComponent}
              stlUrl={$activeThreadId || $workingCopy.macroCode ? stlUrl : null}
              isGenerating={$activeThreadBusy}
            />
            <DrawingOverlay bind:this={drawingOverlay} active={drawMode} />
            <div class="genie-layer">
              <VertexGenie mode={genieMode} bubble={genieBubble} onDismiss={dismissGenie} traits={eckyTraits} intensity={eckyIntensity} />
            </div>

            {#if $activeThreadRequests.length > 0}
              <div class="cafeteria-strip">
                {#each $activeThreadRequests as req (req.id)}
                  <div class="microwave-unit" 
                    class:mw-active={!['success','error','canceled'].includes(req.phase)} 
                    class:mw-success={req.phase === 'success' && !req.isQuestion} 
                    class:mw-thinking-result={req.phase === 'success' && req.isQuestion}
                    class:mw-error={req.phase === 'error'} 
                    class:mw-canceled={req.phase === 'canceled'}
                    class:mw-routing={req.phase === 'classifying'}
                    onclick={() => { if (req.phase === 'success') applyCompletedRequest(req); }}
                    role="button"
                    tabindex="0"
                    onkeydown={(e) => { if (req.phase === 'success' && (e.key === 'Enter' || e.key === ' ')) applyCompletedRequest(req); }}
                    >
                    <div class="mw-glass" class:mw-pulse={req.phase === 'generating' || req.phase === 'repairing' || req.phase === 'rendering' || req.phase === 'classifying'}></div>

                    {#if req.screenshot}
                      <img src={req.screenshot} class="mw-screenshot" alt="Snapshot" />
                    {/if}

                    <div class="mw-display">
                      <div class="mw-phase">{req.isQuestion && req.phase === 'success' ? 'ADVICE' : phaseLabel(req.phase)}</div>
                      {#if req.phase === 'classifying'}
                        <div class="mw-routing-indicator">INTENT CHECK...</div>
                      {:else if req.isQuestion && req.phase === 'success'}
                        <div class="mw-advice-ready">READY</div>
                      {:else}
                        <div class="mw-timer">
                          {formatCookingTime(['success', 'error', 'canceled'].includes(req.phase) ? req.cookingElapsed : Math.max(0, $nowSeconds - Math.floor((req.cookingStartTime || Date.now()) / 1000)))}
                        </div>
                      {/if}
                      <div class="mw-prompt" title={req.prompt}>{req.prompt.slice(0, 28)}{req.prompt.length > 28 ? '…' : ''}</div>
                    </div>

                    {#if !['success', 'error', 'canceled'].includes(req.phase)}
                      <div class="mw-actions">
                        <button class="mw-btn mw-btn-cancel" onclick={(e) => { e.stopPropagation(); cancelRequest(req.id); }} title="Cancel">⏹</button>
                      </div>
                    {:else if req.phase === 'success'}
                      <div class="mw-actions">
                        <button class="mw-btn" onclick={(e) => { e.stopPropagation(); dismissRequest(req.id); }} title="Dismiss">✕</button>
                      </div>
                    {:else if req.phase === 'error' || req.phase === 'canceled'}
                      <div class="mw-actions">
                        <button class="mw-btn mw-btn-retry" onclick={(e) => { e.stopPropagation(); retryRequest(req); }} title="Retry">🔄</button>
                        <button class="mw-btn" onclick={(e) => { e.stopPropagation(); dismissRequest(req.id); }} title="Dismiss">✕</button>
                      </div>
                    {/if}
                    </div>                {/each}
              </div>
            {/if}
            
            {#if $workingCopy.macroCode || stlUrl}
              <div class="viewport-overlay">
                <div class="export-actions">
                  <button class="btn btn-xs btn-secondary" onclick={forkDesign} title="Fork this design into a new project">🍴 FORK</button>
                  <button class="btn btn-xs btn-primary" onclick={exportSTL} disabled={!stlUrl}>💾 STL</button>
                </div>
              </div>
            {/if}
          </main>
          
          <div class="resizer-v" role="slider" aria-label="Resize dialogue" aria-orientation="vertical" aria-valuenow={$dialogueHeight} tabindex="0" onmousedown={startResizingHeight} onkeydown={(e) => {
            if (e.key === 'ArrowUp') dialogueHeight.set($dialogueHeight + 10);
            if (e.key === 'ArrowDown') dialogueHeight.set($dialogueHeight - 10);
          }}></div>

          <div class="dialogue-area" style="height: {$dialogueHeight}px">
            <div class="pane-header dialogue-header">
              DIALOGUE: {activeThread ? activeThread.title : 'New Session'}
            </div>
            <div class="dialogue-content">
              <PromptPanel
                onGenerate={handleGenerate}
                isGenerating={$activeThreadBusy}
                messages={activeThread?.messages || []}                onShowCode={(m) => { selectedCode.set(m.output.macroCode); selectedTitle.set(m.output.title); showCodeModal.set(true); }}
                onDeleteVersion={deleteVersion}
                bind:activeVersionId={$activeVersionId}
                onVersionChange={loadVersion}
              />
            </div>
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if isBooting}
    <div class="boot-overlay">
      <div class="boot-overlay__glass"></div>
      <div class="boot-overlay__content">
        <div class="boot-overlay__title">ECKY CAD</div>
        <div class="boot-overlay__ecky">
          <VertexGenie mode="thinking" bubble="" />
        </div>
        <div class="boot-overlay__status">Restoring environment...</div>
      </div>
    </div>
  {/if}

  {#if $showCodeModal}
    <CodeModal bind:code={$selectedCode} title={$selectedTitle} onCommit={commitManualVersion} onclose={() => showCodeModal.set(false)} />
  {/if}
</div>

<style>
  .app-page { position: relative; height: 100vh; display: flex; flex-direction: column; background: var(--bg); color: var(--text); }
  .app-container { flex: 1; display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
  .workbench { display: flex; height: 100%; width: 100%; overflow: hidden; }
  .sidebar { display: flex; flex-direction: column; flex-shrink: 0; background: var(--bg-100); border-right: 1px solid var(--bg-300); }
  .sidebar-content { flex: 1; min-height: 0; }
  .main-workbench { flex: 1; display: flex; flex-direction: column; min-width: 0; overflow: hidden; }
  .viewport-area { flex: 1; min-height: 100px; background: #0b0f1a; position: relative; overflow: hidden; }
  .dialogue-area { flex-shrink: 0; background: var(--bg-100); display: flex; flex-direction: column; border-top: 1px solid var(--bg-300); overflow: hidden; }
  .dialogue-content { flex: 1; min-height: 0; }
  .pane-header { padding: 4px 12px; background: var(--bg-200); border-bottom: 1px solid var(--bg-300); color: var(--secondary); font-size: 0.6rem; font-weight: bold; letter-spacing: 0.1em; text-transform: uppercase; }
  .scrollable { overflow-y: auto; }
  .resizer-w { width: 4px; background: var(--bg-300); cursor: col-resize; z-index: 10; flex-shrink: 0; }
  .resizer-v { height: 4px; background: var(--bg-300); cursor: row-resize; z-index: 10; flex-shrink: 0; }
  .app-overlay-actions { position: absolute; top: 10px; right: 10px; z-index: 150; display: flex; gap: 8px; }
  .overlay-icon-btn, .settings-overlay-btn { width: 34px; height: 34px; background: color-mix(in srgb, var(--bg-100) 90%, transparent); border: 1px solid var(--bg-300); color: var(--text); cursor: pointer; display: flex; align-items: center; justify-content: center; box-shadow: var(--shadow); }
  .overlay-icon-btn:hover, .settings-overlay-btn:hover { border-color: var(--primary); color: var(--primary); }
  .overlay-icon-btn.draw-active { border-color: var(--primary); background: color-mix(in srgb, var(--primary) 25%, var(--bg-100)); box-shadow: 0 0 8px var(--primary); }
  .genie-layer { position: absolute; left: 10px; top: 10px; z-index: 120; pointer-events: auto; max-width: min(80vw, 420px); }

  /* STL Cafeteria — multi-microwave strip */
  .cafeteria-strip { position: absolute; bottom: 48px; left: 12px; right: 12px; z-index: 100; display: flex; gap: 8px; flex-wrap: wrap; pointer-events: auto; }
  .microwave-unit { position: relative; width: 180px; min-height: 72px; background: rgba(10, 14, 24, 0.88); border: 1px solid var(--bg-300); backdrop-filter: blur(8px); display: flex; flex-direction: column; overflow: hidden; transition: all 0.2s ease; }
  .microwave-unit.mw-success, .microwave-unit.mw-thinking-result { cursor: pointer; }
  .microwave-unit.mw-success:hover, .microwave-unit.mw-thinking-result:hover { background: rgba(20, 30, 45, 0.95); box-shadow: 0 0 15px rgba(74, 140, 92, 0.2); transform: translateY(-2px); }
  .microwave-unit.mw-thinking-result:hover { box-shadow: 0 0 15px rgba(139, 231, 255, 0.2); }
  .microwave-unit.mw-active { border-color: var(--primary); }
  .microwave-unit.mw-success { border-color: var(--secondary); }
  .microwave-unit.mw-thinking-result { border-color: #8be7ff; background: rgba(15, 23, 36, 0.95); }
  .microwave-unit.mw-error { border-color: var(--red); }
  .microwave-unit.mw-canceled { border-color: #444; background: rgba(15, 23, 36, 0.6); opacity: 0.75; }
  .microwave-unit.mw-routing { border-color: #4a708b; background: rgba(15, 23, 36, 0.9); }
  .mw-glass { position: absolute; inset: 0; opacity: 0; transition: opacity 0.3s; z-index: 2; pointer-events: none; }
  .mw-glass.mw-pulse { 
    animation: mw-pulse 2.5s infinite; 
    background: linear-gradient(135deg, rgba(74, 140, 92, 0.25), transparent, rgba(200, 166, 32, 0.2)); 
    opacity: 1; 
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
  }
  .mw-routing .mw-glass.mw-pulse, .mw-thinking-result .mw-glass.mw-pulse {
    background: linear-gradient(135deg, rgba(74, 112, 139, 0.35), transparent, rgba(139, 231, 255, 0.3)); 
    animation-duration: 4s;
  }
  @keyframes mw-pulse { 0%, 100% { opacity: 0.8; } 50% { opacity: 0.4; } }
  
  .mw-screenshot {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.3;
    filter: grayscale(0.6) contrast(1.2);
    z-index: 1;
    pointer-events: none;
  }

  .mw-display { position: relative; z-index: 5; padding: 8px; display: flex; flex-direction: column; gap: 2px; flex: 1; }
  .mw-phase { font-size: 0.55rem; font-weight: bold; letter-spacing: 0.1em; color: var(--secondary); }
  .mw-routing .mw-phase, .mw-thinking-result .mw-phase { color: #8be7ff; text-shadow: 0 0 10px rgba(139, 231, 255, 0.4); }
  .mw-routing-indicator { font-size: 0.65rem; color: #8be7ff; font-weight: bold; margin: 4px 0; letter-spacing: 0.05em; animation: mw-routing-blink 1.5s infinite; }
  .mw-advice-ready { font-size: 1.1rem; color: #8be7ff; font-weight: bold; margin: 2px 0; text-shadow: 0 0 10px rgba(139, 231, 255, 0.6); }
  @keyframes mw-routing-blink { 0%, 100% { opacity: 1; } 50% { opacity: 0.5; } }
  .mw-timer { font-family: var(--font-mono); font-size: 1.1rem; font-weight: bold; color: var(--primary); text-shadow: 0 0 12px var(--primary); }
  .mw-error .mw-timer { color: var(--red); text-shadow: 0 0 12px var(--red); }
  .mw-canceled .mw-timer { color: #888; text-shadow: none; }
  .mw-success .mw-timer { color: var(--secondary); text-shadow: 0 0 12px var(--secondary); }
  .mw-prompt { font-size: 0.55rem; color: var(--text-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mw-actions { display: flex; gap: 4px; padding: 0 8px 6px; position: relative; z-index: 1; }
  .mw-btn { background: var(--bg-300); border: 1px solid var(--bg-400); color: var(--text); font-size: 0.55rem; padding: 2px 6px; cursor: pointer; font-weight: bold; }
  .mw-btn:hover { border-color: var(--primary); color: var(--primary); }
  .mw-btn-cancel:hover { background: var(--red); color: white; border-color: var(--red); }
  .viewport-overlay { position: absolute; bottom: 12px; right: 12px; background: rgba(11, 15, 26, 0.6); backdrop-filter: blur(4px); padding: 8px; border: 1px solid var(--bg-300); z-index: 50; }
  .boot-overlay { position: absolute; inset: 0; z-index: 300; display: flex; align-items: center; justify-content: center; background: var(--bg); }
  .boot-overlay__glass { position: absolute; inset: 0; background: radial-gradient(circle, rgba(74, 140, 92, 0.16), transparent), rgba(8, 12, 20, 0.86); backdrop-filter: blur(18px); }
  .boot-overlay__content { position: relative; z-index: 1; display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 20px; }
  .boot-overlay__title { color: var(--secondary); font-weight: bold; letter-spacing: 0.2em; }
  .boot-overlay__status { color: var(--text-dim); font-size: 0.7rem; }
  .flex-1 { flex: 1; }
  .sidebar-section { display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
</style>
