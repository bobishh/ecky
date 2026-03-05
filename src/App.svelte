<script>
  import PromptPanel from './lib/PromptPanel.svelte';
  import Viewer from './lib/Viewer.svelte';
  import VertexGenie from './lib/VertexGenie.svelte';
  import ParamPanel from './lib/ParamPanel.svelte';
  import ConfigPanel from './lib/ConfigPanel.svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { save } from '@tauri-apps/plugin-dialog';
  import { writeTextFile } from '@tauri-apps/plugin-fs';
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';

  import HistoryPanel from './lib/HistoryPanel.svelte';
  import CodeModal from './lib/CodeModal.svelte';
  import { startMicrowaveHum, stopMicrowaveHum, stopMicrowaveAudio, playDing, playErrorBuzz, getActiveMicrowaveCount } from './lib/audio/microwave';
  import { session, handleGenerate, handleParamChange, commitManualVersion, initSessionFlow } from './lib/stores/sessionFlow';
  import { loadFromHistory, deleteThread, createNewThread, forkDesign, deleteVersion, loadVersion } from './lib/stores/history';
  import { workingCopy, isDirty } from './lib/stores/workingCopy';
  import { history, activeThreadId, activeVersionId, config, availableModels, isLoadingModels } from './lib/stores/domainState';
  import { sidebarWidth, historyHeight, dialogueHeight, showCodeModal, selectedCode, selectedTitle, currentView } from './lib/stores/viewState';
  import { boot, saveConfig, fetchModels } from './lib/stores/boot';
  import { requestQueue, allRequests, activeRequests, activeRequestCount, currentActiveRequest } from './lib/stores/requestQueue';

  // Local reactive aliases for templates
  let phase = $state('booting');
  let status = $state('System ready.');
  let error = $state(null);
  let stlUrl = $state(null);

  // Sync with session store
  $effect(() => {
    const s = $session;
    phase = s.phase;
    status = s.status;
    error = s.error;
    stlUrl = s.stlUrl;
  });

  const isGenerating = $derived(phase === 'generating' || phase === 'repairing');
  const isFreecadRunning = $derived(phase === 'rendering');
  const isLightReasoning = $derived(phase === 'classifying' || phase === 'answering');
  const isRepairingRender = $derived(phase === 'repairing');
  const isBooting = $derived(phase === 'booting');
  const isQuestionFlow = $derived(phase === 'answering');

  let viewerComponent = $state(null);
  let cookingPhrase = $state('');
  let repairMessage = $state('');
  let cookingElapsed = $state(0);
  let cookingStartTime = $state(null);
  let cookingInterval = $state(null);
  let phraseInterval = $state(null);
  let nowSeconds = $state(Math.floor(Date.now() / 1000));
  let lastAssistantMessageId = $state(null);
  let lastAdvisorBubble = $state('');
  let lastAdvisorQuestion = $state('');
  let dismissedBubbleText = $state('');

  let isResizingWidth = $state(false);
  let isResizingHeight = $state(false);
  let isResizingHistory = $state(false);

  initSessionFlow({
    setRepairMessage: (m) => repairMessage = m,
    setCookingPhrase: (p) => cookingPhrase = p,
    startLightReasoning,
    stopLightReasoning,
    startCooking,
    stopCooking,
    isQuestionIntent,
    configStore: config,
    get viewerComponent() { return viewerComponent; },
    openCodeModalManual: (data) => {
      selectedCode.set($workingCopy.macroCode);
      selectedTitle.set($workingCopy.title || data.title);
      showCodeModal.set(true);
    },
    // Per-request microwave hooks
    startMicrowaveForRequest: (requestId) => startMicrowaveHum(requestId, $config),
    stopMicrowaveForRequest: (requestId, success) => {
      const slot = $activeRequests.findIndex(r => r.id === requestId);
      stopMicrowaveHum(requestId);
      if (success && !($config?.microwave?.muted)) playDing($config, Math.max(0, slot));
      else if (!success && !($config?.microwave?.muted)) playErrorBuzz($config);
    },
  });

  const COOKING_PHRASES = [
    "Packing constraints and dimensions into a fresh build plan.",
    "Tracing connector paths and locking wall thickness.",
    "Balancing tolerances so parts print clean and snap right.",
    "Checking manifold integrity and shell continuity.",
    "Projecting cuts and bores onto stable reference axes.",
    "Compiling a safer BRep sequence for FreeCAD execution.",
    "Revalidating clearances to avoid accidental intersections.",
    "Aligning param ranges with current geometry intent.",
    "Running edge cleanup before final mesh output.",
    "Rebuilding topology around your latest parameter edits.",
    "Testing the draft against connector and ring constraints."
  ];

  const LIGHT_REASONING_PHRASES = [
    "Thinking not deep enough. Deciding if this is a question or a geometry change.",
    "Running a quick intent check before heavy generation.",
    "Light pass active: classifying request type.",
    "Checking whether to explain or to modify geometry.",
    "Fast reasoning mode: routing request.",
    "Consulting the goblin responsible for causality."
  ];

  function pickPhrase(pool) {
    cookingPhrase = pool[Math.floor(Math.random() * pool.length)];
  }

  function startLightReasoning() {
    clearInterval(phraseInterval);
    pickPhrase(LIGHT_REASONING_PHRASES);
    phraseInterval = setInterval(() => pickPhrase(LIGHT_REASONING_PHRASES), 2600);
  }

  function stopLightReasoning() {
    clearInterval(phraseInterval);
  }

  function startCooking() {
    clearInterval(phraseInterval);
    cookingStartTime = Date.now();
    cookingElapsed = 0;
    pickPhrase(COOKING_PHRASES);
    cookingInterval = setInterval(() => {
      cookingElapsed = Math.floor((Date.now() - cookingStartTime) / 1000);
    }, 1000);
    phraseInterval = setInterval(() => pickPhrase(COOKING_PHRASES), 4000);
  }

  function stopCooking(success) {
    clearInterval(cookingInterval);
    clearInterval(phraseInterval);
    // Clean up all microwaves if nothing is active anymore
    if ($activeRequestCount === 0) {
      setTimeout(() => stopMicrowaveAudio(true), success ? 2000 : 0);
    }
  }

  function formatCookingTime(s) {
    const m = Math.floor(s / 60);
    const sec = s % 60;
    return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
  }

  function isQuestionIntent(promptText) {
    const prompt = `${promptText ?? ''}`.trim().toLowerCase();
    if (!prompt) return false;
    if (prompt.startsWith('/ask ')) return true;
    const hasQuestionSignal = prompt.includes('?') || /\b(explain|why|how|what|which)\b/.test(prompt);
    const hasDesignAction = /\b(generate|create|make|add|remove|change|update|resize)\b/.test(prompt);
    return hasQuestionSignal && !hasDesignAction;
  }

  onMount(() => {
    const timer = setInterval(() => nowSeconds = Math.floor(Date.now() / 1000), 1000);
    boot({
      applyWorkingDesign: (design, messageId) => workingCopy.loadVersion(design, messageId)
    });
    return () => {
      clearInterval(timer);
      clearInterval(cookingInterval);
      clearInterval(phraseInterval);
    };
  });

  const activeThread = $derived($history.find(t => t.id === $activeThreadId));
  const eckyTraits = $derived(activeThread?.genie_traits || {});
  const eckyIntensity = $derived(1.0 + Math.max(0, ($activeRequestCount - 1) * 0.25));

  const latestAssistantMessage = $derived.by(() => {
    if (!activeThread?.messages?.length) return null;
    return [...activeThread.messages].reverse().find(m => m.role === 'assistant') ?? null;
  });

  const assistantBubble = $derived.by(() => {
    if (!latestAssistantMessage) return '';
    const out = latestAssistantMessage.output;
    return out?.response || (out?.title ? `Generated: ${out.title}` : latestAssistantMessage.content) || '';
  });

  const assistantFresh = $derived.by(() => {
    if (!latestAssistantMessage?.timestamp) return false;
    return nowSeconds - latestAssistantMessage.timestamp <= 300;
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

  const genieMode = $derived.by(() => {
    if (error) return 'error';
    if (isRepairingRender) return 'repairing';
    if (phase === 'classifying') return 'light';
    if (isFreecadRunning) return 'rendering';
    if (isGenerating) return 'thinking';
    if (assistantFresh && !dismissedBubbleText && lastAdvisorBubble) return 'speaking';
    return 'idle';
  });

  const genieBubble = $derived.by(() => {
    const raw = error || (isRepairingRender ? repairMessage : null) || (isLightReasoning || isGenerating ? cookingPhrase : null) || lastAdvisorBubble || '';
    return (dismissedBubbleText === raw) ? '' : raw;
  });

  async function toggleMicrowaveMute() {
    const newMuted = !($config?.microwave?.muted);
    config.update(c => ({ 
      ...c, 
      microwave: { 
        ...(c.microwave || { hum_id: null, ding_id: null }), 
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
    } catch (e) { error = `Export Error: ${e}`; }
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
</script>

<div class="app-page" role="application" onmousemove={handleMouseMove} onmouseup={stopResizing} onmouseleave={stopResizing}>
  <div class="app-overlay-actions">
    {#if $activeRequestCount > 0}
      <button class="overlay-icon-btn" onclick={toggleMicrowaveMute} title="Toggle Cafeteria Hum">
        {$config?.microwave?.muted ? '🔇' : '🔊'}
      </button>
    {/if}
    <button class="settings-overlay-btn" onclick={() => currentView.set($currentView === 'config' ? 'workbench' : 'config')} title="Configuration">
      {$currentView === 'config' ? '⚒️' : '⚙️'}
    </button>
  </div>

  <div class="app-container">
    {#if $currentView === 'config'}
      <ConfigPanel bind:config={$config} availableModels={$availableModels} isLoadingModels={$isLoadingModels} onsave={saveConfig} />
    {:else}
      <div class="workbench">
        <aside class="sidebar" style="width: {$sidebarWidth}px">
          <div class="sidebar-section flex-1">
            <div class="pane-header">TUNABLE PARAMETERS</div>
            <div class="sidebar-content scrollable">
              <ParamPanel 
                uiSpec={$workingCopy.uiSpec} 
                onspecchange={(spec) => workingCopy.patch({ uiSpec: spec })}
                parameters={$workingCopy.params} 
                macroCode={$workingCopy.macroCode}
                onchange={handleParamChange} 
                activeVersionId={$activeVersionId} 
              />
            </div>
          </div>
          <div class="resizer-v" role="separator" tabindex="0" onmousedown={startResizingHistory}></div>
          <div class="sidebar-section" style="height: {$historyHeight}px">
            <div class="pane-header">THREAD HISTORY</div>
            <div class="sidebar-content scrollable">
              <HistoryPanel history={$history} activeThreadId={$activeThreadId} 
                onSelect={loadFromHistory} 
                onDelete={deleteThread}
                onNew={createNewThread} 
              />
            </div>
          </div>
        </aside>

        <div class="resizer-w" role="separator" tabindex="0" onmousedown={startResizingWidth}></div>

        <div class="main-workbench">
          <main class="viewport-area" role="presentation">
            <Viewer
              bind:this={viewerComponent}
              stlUrl={$activeThreadId || $workingCopy.macroCode ? stlUrl : null}
              isGenerating={isGenerating || (isFreecadRunning && !$session.isManual)}
            />            <div class="genie-layer">
              <VertexGenie mode={genieMode} bubble={genieBubble} onDismiss={dismissGenie} traits={eckyTraits} intensity={eckyIntensity} />
            </div>

            {#if $allRequests.length > 0}
              <div class="cafeteria-strip">
                {#each $allRequests as req (req.id)}
                  <div class="microwave-unit" 
                    class:mw-active={!['success','error','canceled'].includes(req.phase)} 
                    class:mw-success={req.phase === 'success' && !req.isQuestion} 
                    class:mw-thinking-result={req.phase === 'success' && req.isQuestion}
                    class:mw-error={req.phase === 'error'} 
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
                          {formatCookingTime(['success', 'error', 'canceled'].includes(req.phase) ? req.cookingElapsed : Math.max(0, nowSeconds - Math.floor((req.cookingStartTime || Date.now()) / 1000)))}
                        </div>
                      {/if}
                      <div class="mw-prompt" title={req.prompt}>{req.prompt.slice(0, 28)}{req.prompt.length > 28 ? '…' : ''}</div>
                    </div>
                    {#if req.phase === 'success'}
                      <div class="mw-actions">
                        <button class="mw-btn" onclick={(e) => { e.stopPropagation(); dismissRequest(req.id); }} title="Dismiss">✕</button>
                      </div>
                    {:else if req.phase === 'error'}
                      <div class="mw-actions">
                        <button class="mw-btn" onclick={(e) => { e.stopPropagation(); dismissRequest(req.id); }} title="Dismiss">✕</button>
                      </div>
                    {/if}
                  </div>
                {/each}
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
          
          <div class="resizer-v" role="separator" tabindex="0" onmousedown={startResizingHeight}></div>

          <div class="dialogue-area" style="height: {$dialogueHeight}px">
            <div class="pane-header">
              DIALOGUE: {activeThread ? activeThread.title : 'New Session'}
            </div>
            <div class="dialogue-content">
              <PromptPanel 
                onGenerate={handleGenerate} 
                isGenerating={isGenerating || isLightReasoning}
                messages={activeThread?.messages || []}
                onShowCode={(m) => { selectedCode.set(m.output.macro_code); selectedTitle.set(m.output.title); showCodeModal.set(true); }}
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
        <div class="boot-overlay__title">DRYDEMACHER</div>
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
  .dialogue-area { flex-shrink: 0; background: var(--bg-100); display: flex; flex-direction: column; border-top: 1px solid var(--bg-300); }
  .dialogue-content { flex: 1; min-height: 0; }
  .pane-header { padding: 4px 12px; background: var(--bg-200); border-bottom: 1px solid var(--bg-300); color: var(--secondary); font-size: 0.6rem; font-weight: bold; letter-spacing: 0.1em; text-transform: uppercase; }
  .scrollable { overflow-y: auto; }
  .resizer-w { width: 4px; background: var(--bg-300); cursor: col-resize; z-index: 10; }
  .resizer-v { height: 4px; background: var(--bg-300); cursor: row-resize; z-index: 10; flex-shrink: 0; }
  .app-overlay-actions { position: absolute; top: 10px; right: 10px; z-index: 150; display: flex; gap: 8px; }
  .overlay-icon-btn, .settings-overlay-btn { width: 34px; height: 34px; background: color-mix(in srgb, var(--bg-100) 90%, transparent); border: 1px solid var(--bg-300); color: var(--text); cursor: pointer; display: flex; align-items: center; justify-content: center; box-shadow: var(--shadow); }
  .overlay-icon-btn:hover, .settings-overlay-btn:hover { border-color: var(--primary); color: var(--primary); }
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
  .mw-success .mw-timer { color: var(--secondary); text-shadow: 0 0 12px var(--secondary); }
  .mw-prompt { font-size: 0.55rem; color: var(--text-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mw-actions { display: flex; gap: 4px; padding: 0 8px 6px; position: relative; z-index: 1; }
  .mw-btn { background: var(--bg-300); border: 1px solid var(--bg-400); color: var(--text); font-size: 0.55rem; padding: 2px 6px; cursor: pointer; font-weight: bold; }
  .mw-btn:hover { border-color: var(--primary); color: var(--primary); }
  .viewport-overlay { position: absolute; bottom: 12px; right: 12px; background: rgba(11, 15, 26, 0.6); backdrop-filter: blur(4px); padding: 8px; border: 1px solid var(--bg-300); z-index: 50; }
  .boot-overlay { position: absolute; inset: 0; z-index: 300; display: flex; align-items: center; justify-content: center; background: var(--bg); }
  .boot-overlay__glass { position: absolute; inset: 0; background: radial-gradient(circle, rgba(74, 140, 92, 0.16), transparent), rgba(8, 12, 20, 0.86); backdrop-filter: blur(18px); }
  .boot-overlay__content { position: relative; z-index: 1; display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 20px; }
  .boot-overlay__title { color: var(--secondary); font-weight: bold; letter-spacing: 0.2em; }
  .boot-overlay__status { color: var(--text-dim); font-size: 0.7rem; }
  .flex-1 { flex: 1; }
  .sidebar-section { display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
</style>
