<script>
  import PromptPanel from './lib/PromptPanel.svelte';
  import Viewer from './lib/Viewer.svelte';
  import VertexGenie from './lib/VertexGenie.svelte';
  import ParamPanel from './lib/ParamPanel.svelte';
  import ConfigPanel from './lib/ConfigPanel.svelte';
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

  // Microwave cooking state
  let cookingStartTime = $state(null);
  let cookingElapsed = $state(0);
  let cookingPhrase = $state('');
  let phraseKey = $state(0);
  let cookingInterval = $state(null);
  let phraseInterval = $state(null);
  let audioCtx = $state(null);
  let audioNodes = $state([]);
  let isMuted = $state(false);
  let masterGain = $state(null);
  let nowSeconds = $state(Math.floor(Date.now() / 1000));
  let dismissedBubbleText = $state('');

  const COOKING_PHRASES = [
    "Heating up the tensor cores...",
    "Defrosting the latent space...",
    "Rotating the probability distribution...",
    "Microwaving your geometry at 2.45 GHz...",
    "BRep nuclei reaching critical temperature...",
    "Agitating water molecules in the weight matrix...",
    "Turntable spinning at 6 RPM (Revolutions Per Manifold)...",
    "Reticulating splines in a convection field...",
    "CAUTION: Contents may be topologically hot...",
    "Nuking the mesh from orbit...",
    "Electromagnetic radiation applied to your prompt...",
    "Standing wave pattern detected in the hidden layers...",
    "The magnetron hums its ancient song...",
    "Popcorn mode: ON (kernel expansion imminent)...",
    "Do NOT open the door. The geometry is still raw inside...",
    "Detected sparks. Someone put foil in the embeddings...",
    "Thawing frozen parameters from last session...",
    "Power level: MAXIMUM OVERTHINKING...",
    "Timer set to ∞. Please wait.",
    "Your CAD is being irradiated with good vibes...",
    "Cooking instructions unclear. Generating anyway...",
    "The mesh will be hot. Use oven mitts when handling normals.",
  ];

  function pickPhrase() {
    cookingPhrase = COOKING_PHRASES[Math.floor(Math.random() * COOKING_PHRASES.length)];
    phraseKey++;
  }

  function toggleMute() {
    isMuted = !isMuted;
    if (masterGain) {
      masterGain.gain.value = isMuted ? 0 : 1;
    }
  }

  function startCooking() {
    cookingStartTime = Date.now();
    cookingElapsed = 0;
    pickPhrase();

    cookingInterval = setInterval(() => {
      cookingElapsed = Math.floor((Date.now() - cookingStartTime) / 1000);
    }, 1000);

    phraseInterval = setInterval(pickPhrase, 4000);

    // Web Audio: gentle microwave fan hum (filtered noise + soft sine)
    try {
      audioCtx = new (window.AudioContext || window.webkitAudioContext)();
      masterGain = audioCtx.createGain();
      masterGain.gain.value = isMuted ? 0 : 1;
      masterGain.connect(audioCtx.destination);

      const humAssetId = config.microwave?.hum_id;
      const humAsset = config.assets?.find(a => a.id === humAssetId);

      if (humAsset) {
        // Use custom hum file
        const audio = new Audio(convertFileSrc(humAsset.path));
        audio.loop = true;
        const source = audioCtx.createMediaElementSource(audio);
        source.connect(masterGain);
        audio.play();
        audioNodes = [audio];
      } else {
        // Fallback to generated hum
        const bufferSize = audioCtx.sampleRate * 2;
        const noiseBuffer = audioCtx.createBuffer(1, bufferSize, audioCtx.sampleRate);
        const data = noiseBuffer.getChannelData(0);
        let brown = 0;
        for (let i = 0; i < bufferSize; i++) {
          const white = Math.random() * 2 - 1;
          brown = (brown + (0.02 * white)) / 1.02;
          data[i] = (brown * 0.7 + white * 0.3) * 3.5;
        }
        const noise = audioCtx.createBufferSource();
        noise.buffer = noiseBuffer;
        noise.loop = true;

        const noiseFilter = audioCtx.createBiquadFilter();
        noiseFilter.type = 'lowpass';
        noiseFilter.frequency.value = 400;
        noiseFilter.Q.value = 0.5;

        const noiseGain = audioCtx.createGain();
        noiseGain.gain.value = 0.08;

        noise.connect(noiseFilter);
        noiseFilter.connect(noiseGain);
        noiseGain.connect(masterGain);
        noise.start();

        const hum = audioCtx.createOscillator();
        hum.type = 'sine';
        hum.frequency.value = 60;
        const humGain = audioCtx.createGain();
        humGain.gain.value = 0.02;
        hum.connect(humGain);
        humGain.connect(masterGain);
        hum.start();

        audioNodes = [noise, hum];
      }
    } catch (e) {
      console.warn('Audio not available:', e);
    }
  }

  function stopCooking(success) {
    clearInterval(cookingInterval);
    clearInterval(phraseInterval);
    cookingInterval = null;
    phraseInterval = null;

    // Stop hum
    for (const node of audioNodes) {
      try { 
        if (node instanceof HTMLMediaElement) {
          node.pause();
          node.currentTime = 0;
        } else {
          node.stop(); 
        }
      } catch(e) {}
    }
    audioNodes = [];

    // Ding!
    if (success && audioCtx) {
      try {
        const dingAssetId = config.microwave?.ding_id;
        const dingAsset = config.assets?.find(a => a.id === dingAssetId);

        if (dingAsset) {
          const ding = new Audio(convertFileSrc(dingAsset.path));
          const source = audioCtx.createMediaElementSource(ding);
          source.connect(masterGain);
          ding.play();
        } else {
          // Fallback ding
          const now = audioCtx.currentTime;
          const g = audioCtx.createGain();
          g.gain.setValueAtTime(0, now);
          g.gain.linearRampToValueAtTime(0.2, now + 0.02);
          g.gain.exponentialRampToValueAtTime(0.001, now + 0.8);
          g.connect(masterGain);

          const o = audioCtx.createOscillator();
          o.type = 'sine';
          o.frequency.setValueAtTime(1200, now);
          o.frequency.exponentialRampToValueAtTime(1180, now + 0.8);
          o.connect(g);
          o.start(now);
          o.stop(now + 0.8);
        }
      } catch(e) {}
    }

    // Clean up audio context after ding fades
    setTimeout(() => {
      if (audioCtx) { try { audioCtx.close(); } catch(e) {} audioCtx = null; }
      masterGain = null;
    }, 2000);
  }

  function formatCookingTime(s) {
    const m = Math.floor(s / 60);
    const sec = s % 60;
    return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
  }

  // Derived active thread
  const activeThread = $derived(history.find(t => t.id === activeThreadId));
  const latestAssistantMessage = $derived.by(() => {
    if (!activeThread?.messages?.length) return null;
    return [...activeThread.messages].reverse().find(m => m.role === 'assistant') ?? null;
  });

  function clampText(text, max = 120) {
    const compact = `${text ?? ''}`.replace(/\s+/g, ' ').trim();
    if (!compact) return '';
    return compact.length > max ? `${compact.slice(0, max - 1)}…` : compact;
  }

  const assistantBubble = $derived.by(() => {
    if (!latestAssistantMessage) return '';
    const outputResponse = latestAssistantMessage.output?.response;
    const outputTitle = latestAssistantMessage.output?.title;
    const content = latestAssistantMessage.content;
    const text = outputResponse || (outputTitle ? `Generated: ${outputTitle}` : content);
    return clampText(text, 240);
  });

  const assistantFresh = $derived.by(() => {
    if (!latestAssistantMessage?.timestamp) return false;
    return nowSeconds - latestAssistantMessage.timestamp <= 45;
  });

  const genieMode = $derived.by(() => {
    if (error) return 'error';
    if (isGenerating) return 'thinking';
    if (assistantFresh) return 'speaking';
    return 'idle';
  });

  const genieBubbleRaw = $derived.by(() => {
    if (error) return clampText(error, 240);
    if (isGenerating) return clampText('Synthesizing geometry...', 240);
    if (assistantFresh && assistantBubble) return assistantBubble;
    return clampText(status, 140);
  });

  const genieBubble = $derived.by(() => {
    if (!genieBubbleRaw) return '';
    if (dismissedBubbleText === genieBubbleRaw) return '';
    return genieBubbleRaw;
  });

  function dismissGenieBubble() {
    if (genieBubbleRaw) {
      dismissedBubbleText = genieBubbleRaw;
    }
  }

  function isQuestionIntent(promptText) {
    const prompt = `${promptText ?? ''}`.trim().toLowerCase();
    if (!prompt) return false;
    if (prompt.startsWith('/ask ')) return true;

    const hasQuestionSignal =
      prompt.includes('?') ||
      /\b(explain|why|how|what|which|walk me through|help me understand|what does|can you explain|could you explain)\b/.test(prompt);
    const hasDesignAction =
      /\b(generate|create|make|add|remove|change|update|increase|decrease|set|resize|extrude|fillet|chamfer|connector|diameter|length|height|width)\b/.test(prompt);

    return hasQuestionSignal && !hasDesignAction;
  }

  onMount(() => {
    const timer = setInterval(() => {
      nowSeconds = Math.floor(Date.now() / 1000);
    }, 1000);

    void Promise.all([
      loadConfig(),
      restoreLastDesign(),
      loadHistory()
    ]);

    return () => {
      clearInterval(timer);
    };
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
        const [design, threadId] = last;
        macroCode = design.macro_code;
        uiSpec = design.ui_spec;
        parameters = design.initial_params || {};
        activeThreadId = threadId;
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

  async function handleGenerate(initialPrompt, attachments = []) {
    isGenerating = true;
    error = null;
    startCooking();
    let currentPrompt = initialPrompt;
    const questionMode = isQuestionIntent(initialPrompt);
    let maxAttempts = questionMode ? 1 : 3;
    let attempt = 1;
    
    let currentImageData = null;
    if (viewerComponent && stlUrl) {
      currentImageData = viewerComponent.captureScreenshot();
    }

    while (attempt <= maxAttempts) {
      status = `Consulting LLM (Attempt ${attempt}/${maxAttempts})...`;
      try {
        const result = await invoke('generate_design', { 
          prompt: currentPrompt,
          threadId: activeThreadId,
          parentMacroCode: !activeThreadId ? macroCode : null,
          isRetry: attempt > 1,
          imageData: currentImageData,
          attachments: attachments,
          questionMode
        });
        
        const data = result.design;
        activeThreadId = result.thread_id;
        const questionResponse = `${data.response ?? ''}`.trim();
        const interactionMode = `${data.interaction_mode ?? ''}`.toLowerCase();
        const isQuestionOutput = questionMode || interactionMode === 'question';

        await loadHistory();
        
        const updatedThread = history.find(t => t.id === activeThreadId);
        if (updatedThread) {
          const lastMsg = [...updatedThread.messages].reverse().find(m => m.role === 'assistant' && m.output);
          if (lastMsg) activeVersionId = lastMsg.id;
        }

        if (isQuestionOutput) {
          status = questionResponse || 'Question answered. Geometry unchanged.';
          error = null;
          stopCooking(true);
          break;
        }

        status = 'Parsing geometry specification and UI...';
        macroCode = data.macro_code;
        uiSpec = data.ui_spec;
        parameters = data.initial_params || {};

        status = 'Executing FreeCAD engine (BRep/STL)...';
        try {
          const absolutePath = await invoke('render_stl', { 
            macroCode: macroCode, 
            parameters 
          });
          stlUrl = convertFileSrc(absolutePath);
          status = 'Design synthesized and rendered successfully.';
          error = null;
          stopCooking(true);
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
            stopCooking(false);
            // Open the code modal so user can manually edit
            selectedCode = macroCode;
            selectedTitle = data.title;
            showCodeModal = true;
            break;
          }
        }
      } catch (e) {
        // Handle context-preserving error
        if (typeof e === 'string' && e.startsWith("ERR_ID:")) {
          const [idPart, errorMsg] = e.split('|');
          activeThreadId = idPart.replace("ERR_ID:", "");
          error = `Generation Failed: ${errorMsg}`;
          await loadHistory();
        } else {
          error = `Generation Failed: ${e}`;
        }
        status = 'LLM API Error.';
        stopCooking(false);
        break; // LLM failed entirely, don't retry FreeCAD logic
      }
    }
    
    isGenerating = false;
  }

  async function handleParamChange(newParams, forcedCode = null) {
    parameters = { ...parameters, ...newParams };
    const codeToUse = forcedCode || macroCode;
    
    if (!codeToUse) return;

    if (activeVersionId) {
      try {
        await invoke('update_parameters', { messageId: activeVersionId, parameters });
      } catch (e) {
        console.error('Failed to persist parameters:', e);
      }
    }
    
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
  <button class="settings-overlay-btn" onclick={toggleConfig} title="Toggle Configuration">
    {currentView === 'config' ? '⚒️' : '⚙️'}
  </button>

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
              <ParamPanel bind:uiSpec {parameters} onchange={handleParamChange} {activeVersionId} />
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
            <Viewer bind:this={viewerComponent} {stlUrl} {isGenerating} />
            <div class="genie-layer">
              <VertexGenie mode={genieMode} bubble={genieBubble} label="ECKBERT" onDismiss={dismissGenieBubble} />
            </div>

            {#if isGenerating}
              <div class="microwave-overlay">
                <div class="microwave-glass"></div>
                <div class="microwave-content">
                  <div class="microwave-turntable">
                    <div class="turntable-plate"></div>
                    <div class="turntable-object"></div>
                  </div>
                  <div class="microwave-timer">{formatCookingTime(cookingElapsed)}</div>
                  {#key phraseKey}
                    <div class="microwave-phrase">{cookingPhrase}</div>
                  {/key}
                  <div class="microwave-dots">
                    <span class="dot"></span><span class="dot"></span><span class="dot"></span>
                  </div>
                </div>
                <button class="mute-btn" onclick={toggleMute} title={isMuted ? 'Unmute' : 'Mute'}>
                  {isMuted ? '🔇' : '🔊'}
                </button>
              </div>
            {/if}
            
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
  .app-page {
    position: relative;
  }

  .settings-overlay-btn {
    position: absolute;
    top: 10px;
    right: 10px;
    z-index: 150;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 1.05rem;
    line-height: 1;
    width: 34px;
    height: 34px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-shadow: var(--shadow);
  }

  .settings-overlay-btn:hover {
    border-color: var(--primary);
    color: var(--primary);
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

  .genie-layer {
    position: absolute;
    left: 10px;
    top: 10px;
    z-index: 120;
    pointer-events: auto;
    max-width: min(80vw, 420px);
  }

  /* Microwave cooking overlay */
  .microwave-overlay {
    position: absolute;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .microwave-glass {
    position: absolute;
    inset: 0;
    background: rgba(10, 14, 24, 0.78);
    backdrop-filter: blur(16px) saturate(0.08);
    animation: microwave-pulse 2.5s ease-in-out infinite;
  }

  @keyframes microwave-pulse {
    0%, 100% { background: rgba(10, 14, 24, 0.70); }
    50% { background: rgba(10, 14, 24, 0.60); }
  }

  .microwave-content {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 20px;
    pointer-events: none;
  }

  /* Turntable */
  .microwave-turntable {
    position: relative;
    width: 120px;
    height: 120px;
    animation: turntable-spin 4s linear infinite;
  }

  .turntable-plate {
    position: absolute;
    inset: 0;
    border: 2px solid var(--bg-400);
    border-radius: 50%;
    opacity: 0.5;
  }

  .turntable-object {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 60px;
    height: 22px;
    background: var(--primary);
    opacity: 0.6;
    border-radius: 11px;
    transform: translate(-50%, -50%);
    box-shadow: 0 0 16px color-mix(in srgb, var(--primary) 30%, transparent);
    animation: object-throb 1.5s ease-in-out infinite;
  }

  @keyframes turntable-spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  @keyframes object-throb {
    0%, 100% { opacity: 0.5; transform: translate(-50%, -50%) scale(1); }
    50% { opacity: 0.8; transform: translate(-50%, -50%) scale(1.08); }
  }

  .microwave-timer {
    font-family: var(--font-mono);
    font-size: 2.2rem;
    font-weight: bold;
    color: var(--primary);
    letter-spacing: 0.15em;
    text-shadow: 0 0 20px color-mix(in srgb, var(--primary) 40%, transparent);
  }

  .microwave-phrase {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--text-dim);
    text-align: center;
    max-width: 400px;
    letter-spacing: 0.03em;
    min-height: 1.5em;
    animation: phrase-fade 4s ease-in-out forwards;
  }

  @keyframes phrase-fade {
    0% { opacity: 0; transform: translateY(6px); }
    12% { opacity: 1; transform: translateY(0); }
    88% { opacity: 1; transform: translateY(0); }
    100% { opacity: 0; transform: translateY(-6px); }
  }

  .microwave-dots {
    display: flex;
    gap: 6px;
  }

  .microwave-dots .dot {
    width: 6px;
    height: 6px;
    background: var(--secondary);
    border-radius: 50%;
    animation: dot-bounce 1.4s ease-in-out infinite;
  }

  .microwave-dots .dot:nth-child(2) { animation-delay: 0.2s; }
  .microwave-dots .dot:nth-child(3) { animation-delay: 0.4s; }

  @keyframes dot-bounce {
    0%, 80%, 100% { transform: scale(0.6); opacity: 0.3; }
    40% { transform: scale(1); opacity: 1; }
  }

  .mute-btn {
    position: absolute;
    bottom: 12px;
    right: 12px;
    z-index: 2;
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    font-size: 1rem;
    padding: 4px 8px;
    cursor: pointer;
    pointer-events: all;
  }

  .mute-btn:hover {
    border-color: var(--primary);
  }
</style>
