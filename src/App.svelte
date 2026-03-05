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
  let isFreecadRunning = $state(false);
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
  let isLightReasoning = $state(false);
  let isQuestionFlow = $state(false);
  let cookingInterval = $state(null);
  let phraseInterval = $state(null);
  let audioCtx = $state(null);
  let audioNodes = $state([]);
  let masterGain = $state(null);
  let nowSeconds = $state(Math.floor(Date.now() / 1000));
  let isBooting = $state(true);
  let bootStartTime = $state(Date.now());
  let bootElapsed = $state(0);
  let bootInterval = $state(null);
  let dismissedBubbleText = $state('');
  let lastAdvisorBubble = $state('');
  let lastAdvisorQuestion = $state('');
  let lastAssistantMessageId = $state(null);
  let generationInFlight = $state(false);

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
    "Testing the draft against connector and ring constraints.",
    "Folding your prompt into deterministic CAD operations.",
    "Re-centering the model logic for repeatable renders.",
    "Converting design intent into executable macro steps.",
    "Running a precision pass on radii and offsets.",
    "Preparing an STL with cleaner normals and contours.",
    "Applying a no-leak sanity check on mating surfaces.",
    "Synchronizing UI controls with generated geometry state.",
    "Cross-checking dimensions against the active version.",
    "Running a small overthinking cycle for better reliability.",
    "Timer set to ∞. Please wait.",
    "Your CAD is being irradiated with good vibes.",
    "Cooking instructions unclear. Generating anyway.",
    "The mesh will be hot. Use oven mitts when handling normals.",
    "Calibrating final passes...",
    "Keeping features printable while preserving your intent."
  ];

  const LIGHT_REASONING_PHRASES = [
    "Thinking not deep enough. Deciding if this is a question or a geometry change.",
    "Running a quick intent check before heavy generation.",
    "Light pass active: classifying request type.",
    "Checking whether to explain or to modify geometry.",
    "Fast reasoning mode: routing request."
  ];

  function pickPhrase(pool = COOKING_PHRASES) {
    cookingPhrase = pool[Math.floor(Math.random() * pool.length)];
  }

  function startBootPreloader() {
    isBooting = true;
    bootStartTime = Date.now();
    bootElapsed = 0;
    clearInterval(bootInterval);
    bootInterval = setInterval(() => {
      bootElapsed = Math.floor((Date.now() - bootStartTime) / 1000);
    }, 1000);
  }

  function stopBootPreloader() {
    isBooting = false;
    clearInterval(bootInterval);
    bootInterval = null;
  }

  function startLightReasoning() {
    isLightReasoning = true;
    clearInterval(phraseInterval);
    phraseInterval = null;
    pickPhrase(LIGHT_REASONING_PHRASES);
    phraseInterval = setInterval(() => pickPhrase(LIGHT_REASONING_PHRASES), 2600);
  }

  function stopLightReasoning() {
    isLightReasoning = false;
    clearInterval(phraseInterval);
    phraseInterval = null;
  }

  function startCooking() {
    isLightReasoning = false;
    clearInterval(cookingInterval);
    clearInterval(phraseInterval);
    cookingInterval = null;
    phraseInterval = null;
    cookingStartTime = Date.now();
    cookingElapsed = 0;
    pickPhrase(COOKING_PHRASES);

    cookingInterval = setInterval(() => {
      cookingElapsed = Math.floor((Date.now() - cookingStartTime) / 1000);
    }, 1000);

    phraseInterval = setInterval(() => pickPhrase(COOKING_PHRASES), 4000);

    // Web Audio: gentle microwave fan hum (filtered noise + soft sine)
    try {
      audioCtx = new (window.AudioContext || window.webkitAudioContext)();
      masterGain = audioCtx.createGain();
      masterGain.gain.value = 1;
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
  function getPairedQuestionForAssistant(thread, assistantMessage, maxLen = 520) {
    if (!thread?.messages?.length || !assistantMessage?.id) return '';
    const assistantIndex = thread.messages.findIndex(m => m.id === assistantMessage.id);
    if (assistantIndex <= 0) return '';
    const previousMessage = thread.messages[assistantIndex - 1];
    if (previousMessage?.role !== 'user') return '';
    return clampText(previousMessage.content || '', maxLen);
  }

  const latestAssistantMessage = $derived.by(() => {
    if (!activeThread?.messages?.length) return null;
    return [...activeThread.messages].reverse().find(m => m.role === 'assistant') ?? null;
  });
  const latestAssistantQuestion = $derived.by(() => {
    return getPairedQuestionForAssistant(activeThread, latestAssistantMessage, 520);
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
    return clampText(text, 900);
  });

  $effect(() => {
    const msgId = latestAssistantMessage?.id;
    if (!msgId || msgId === lastAssistantMessageId) return;
    lastAssistantMessageId = msgId;
    if (assistantBubble) {
      lastAdvisorBubble = assistantBubble;
      lastAdvisorQuestion = latestAssistantQuestion;
      dismissedBubbleText = '';
    }
  });

  const assistantFresh = $derived.by(() => {
    if (!latestAssistantMessage?.timestamp) return false;
    return nowSeconds - latestAssistantMessage.timestamp <= 45;
  });

  const genieMode = $derived.by(() => {
    if (error) return 'error';
    if (isLightReasoning) return 'light';
    if (isFreecadRunning) return 'rendering';
    if (isGenerating && isQuestionFlow) return 'light';
    if (isGenerating) return 'thinking';
    if (assistantFresh) return 'speaking';
    return 'idle';
  });

  const genieBubbleRaw = $derived.by(() => {
    if (error) return clampText(error, 240);
    if (isLightReasoning) return clampText(cookingPhrase || 'Thinking not deep enough.', 240);
    if (isFreecadRunning) return clampText('FreeCAD is crunching geometry...', 240);
    if (isGenerating && isQuestionFlow) return clampText(cookingPhrase || 'Thinking not deep enough.', 240);
    if (isGenerating) return clampText(cookingPhrase || 'Synthesizing geometry.', 240);
    if (assistantBubble) return assistantBubble;
    if (lastAdvisorBubble) return lastAdvisorBubble;
    return '';
  });

  const genieBubble = $derived.by(() => {
    if (!genieBubbleRaw) return '';
    if (dismissedBubbleText === genieBubbleRaw) return '';
    return genieBubbleRaw;
  });

  const genieQuestion = $derived.by(() => {
    if (!genieBubble) return '';
    if (assistantBubble && genieBubble === assistantBubble) return latestAssistantQuestion;
    if (lastAdvisorBubble && genieBubble === lastAdvisorBubble) return lastAdvisorQuestion;
    return '';
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

    void (async () => {
      startBootPreloader();
      try {
        await loadConfig();
        await loadHistory();
        await restoreLastDesign();
      } finally {
        stopBootPreloader();
      }
    })();

    return () => {
      clearInterval(timer);
      clearInterval(bootInterval);
    };
  });

  async function loadHistory() {
    try {
      const freshHistory = await invoke('get_history');
      history = freshHistory;
      if (activeThreadId && !freshHistory.some(t => t.id === activeThreadId)) {
        activeThreadId = null;
        activeVersionId = null;
      }
    } catch (e) {
      console.error("Failed to load history:", e);
    }
  }

  async function restoreLastDesign() {
    try {
      const last = await invoke('get_last_design');
      if (last) {
        const [design, threadId] = last;
        let restoredFromThread = false;

        if (threadId) {
          const thread = history.find(t => t.id === threadId);
          const lastAssistantMsg = thread
            ? [...thread.messages].reverse().find(m => m.role === 'assistant' && m.output)
            : null;

          if (lastAssistantMsg?.output) {
            activeThreadId = threadId;
            activeVersionId = lastAssistantMsg.id;
            macroCode = lastAssistantMsg.output.macro_code;
            uiSpec = lastAssistantMsg.output.ui_spec;
            parameters = lastAssistantMsg.output.initial_params || {};
            restoredFromThread = true;
          }
        }

        if (!restoredFromThread) {
          macroCode = design.macro_code;
          uiSpec = design.ui_spec;
          parameters = design.initial_params || {};
          activeThreadId = threadId;
          activeVersionId = null;
        }

        status = 'Restored last design session.';
        await handleParamChange(parameters, macroCode);
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
      const loadedConfig = await invoke('get_config');
      let configPatched = false;

      // Ensure there is always a truly selected engine when engines exist.
      if (loadedConfig.engines?.length > 0) {
        const hasSelectedEngine = loadedConfig.engines.some(e => e.id === loadedConfig.selected_engine_id);
        if (!hasSelectedEngine) {
          loadedConfig.selected_engine_id = loadedConfig.engines[0].id;
          configPatched = true;
        }
      }

      config = loadedConfig;

      if (config.selected_engine_id) {
        // Initial fetch
        await fetchModels();
      }

      if (configPatched) {
        await invoke('save_config', { config });
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

      // Auto-fix model selection if empty or stale so what user sees is actually selected.
      if (models.length > 0 && (!selectedEngine.model || !models.includes(selectedEngine.model))) {
        selectedEngine.model = models[0];
        await invoke('save_config', { config });
        status = `Engine active: ${models.length} models available. Model auto-selected: ${selectedEngine.model}.`;
      } else {
        status = `Engine active: ${models.length} models available.`;
      }
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
        const heightFromBottom = sidebarRect.bottom - e.clientY;
        historyHeight = Math.max(100, Math.min(heightFromBottom, sidebarRect.height - 100));
      }
    }
  }

  function stopResizing() {
    isResizingWidth = false;
    isResizingHeight = false;
    isResizingHistory = false;
  }

  function buildLightReasoningContext() {
    const context = [];
    if (activeThread?.title) context.push(`Title: ${activeThread.title}`);
    const currentVersion = activeThread?.messages?.find(m => m.id === activeVersionId);
    const versionName = currentVersion?.output?.version_name;
    if (versionName) context.push(`Version: ${versionName}`);
    if (macroCode) context.push(`Current FreeCAD Macro:\n\`\`\`python\n${macroCode}\n\`\`\``);
    if (uiSpec) context.push(`Current UI Spec:\n\`\`\`json\n${JSON.stringify(uiSpec, null, 2)}\n\`\`\``);
    if (parameters && Object.keys(parameters).length > 0) {
      context.push(`Current Parameters:\n\`\`\`json\n${JSON.stringify(parameters, null, 2)}\n\`\`\``);
    }
    return context.join('\n\n');
  }

  async function handleGenerate(initialPrompt, attachments = []) {
    if (generationInFlight || isGenerating || isLightReasoning || isFreecadRunning) {
      console.warn('Ignoring duplicate generate request while another run is active.');
      return;
    }

    generationInFlight = true;
    error = null;
    isQuestionFlow = false;
    try {
      startLightReasoning();
      const lightContext = buildLightReasoningContext();
      let questionMode = isQuestionIntent(initialPrompt);
      let lightResponse = '';
      try {
        const intent = await invoke('classify_intent', { prompt: initialPrompt, threadId: activeThreadId, context: lightContext });
        const mode = `${intent?.intent_mode ?? ''}`.toLowerCase();
        if (mode === 'question' || mode === 'design') {
          questionMode = mode === 'question';
        }
        if (intent?.response) {
          lightResponse = `${intent.response}`.trim();
          cookingPhrase = lightResponse;
        }
      } catch (intentErr) {
        console.warn('Intent classification failed, using fallback heuristic:', intentErr);
      }

      isQuestionFlow = questionMode;
      if (questionMode) {
        status = 'Answering question...';
        const questionReply = lightResponse || 'Question answered. Geometry unchanged.';
        const result = await invoke('answer_question_light', {
          prompt: initialPrompt,
          response: questionReply,
          threadId: activeThreadId,
          titleHint: activeThread?.title || 'Question Session'
        });
        activeThreadId = result.thread_id;
        await loadHistory();
        status = result.response || questionReply;
        error = null;
        return;
      }

      isGenerating = true;
      startCooking();

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
          const isQuestionOutput = interactionMode === 'question';

          await loadHistory();
          
          const updatedThread = history.find(t => t.id === activeThreadId);
          if (updatedThread) {
            const lastMsg = [...updatedThread.messages].reverse().find(m => m.role === 'assistant' && m.output);
            if (lastMsg) activeVersionId = lastMsg.id;
          }

          if (isQuestionOutput) {
            status = questionResponse || 'Question answered. Geometry unchanged.';
            error = null;
            stopLightReasoning();
            stopCooking(true);
            break;
          }

          status = 'Parsing geometry specification and UI...';
          macroCode = data.macro_code;
          uiSpec = data.ui_spec;
          parameters = data.initial_params || {};

          status = 'Executing FreeCAD engine (BRep/STL)...';
          try {
            isFreecadRunning = true;
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
          } finally {
            isFreecadRunning = false;
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
          stopLightReasoning();
          stopCooking(false);
          break; // LLM failed entirely, don't retry FreeCAD logic
        }
      }
    } catch (e) {
      error = `Generation Failed: ${e}`;
      status = 'LLM API Error.';
      stopLightReasoning();
      stopCooking(false);
    } finally {
      stopLightReasoning();
      isGenerating = false;
      isQuestionFlow = false;
      generationInFlight = false;
    }
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
      isFreecadRunning = true;
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
    } finally {
      isFreecadRunning = false;
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
    const targetThreadId = thread.id;
    activeThreadId = targetThreadId;
    await loadHistory();

    const freshThread = history.find(t => t.id === targetThreadId) || thread;
    const lastAssistantMsg = [...freshThread.messages].reverse().find(m => m.role === 'assistant' && m.output);
    if (lastAssistantMsg) {
      await loadVersion(lastAssistantMsg);
    } else {
      activeVersionId = null;
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
    lastAdvisorBubble = '';
    lastAdvisorQuestion = '';
    lastAssistantMessageId = null;
    status = 'New design session started.';
  }

  function forkDesign() {
    // We keep current macroCode, parameters, and stlUrl
    // but detach from the current thread
    activeThreadId = null;
    activeVersionId = null;
    lastAdvisorBubble = '';
    lastAdvisorQuestion = '';
    lastAssistantMessageId = null;
    status = 'Design forked. Next generation will create a new thread.';
  }

  function getNextManualVersionName() {
    const versionNames = (activeThread?.messages || [])
      .filter(m => m.role === 'assistant' && m.output?.version_name)
      .map(m => `${m.output.version_name}`);

    const nums = versionNames
      .map(name => {
        const match = name.match(/v\s*(\d+)/i);
        return match ? Number.parseInt(match[1], 10) : NaN;
      })
      .filter(Number.isFinite);

    const next = nums.length > 0 ? Math.max(...nums) + 1 : (versionNames.length + 1 || 1);
    return `V${next}`;
  }

  async function commitManualVersion(editedCode) {
    if (!activeThreadId) {
      error = "Cannot commit manual version: No active thread. Please generate first.";
      return;
    }
    
    // Validate by rendering first
    status = "Validating manual edit...";
    try {
      isFreecadRunning = true;
      const absolutePath = await invoke('render_stl', { 
        macroCode: editedCode, 
        parameters 
      });
      stlUrl = convertFileSrc(absolutePath);
      
      // Save to DB
      await invoke('add_manual_version', {
        threadId: activeThreadId,
        title: activeThread?.title || "Manual Edit",
        versionName: getNextManualVersionName(),
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
    } finally {
      isFreecadRunning = false;
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
            <Viewer bind:this={viewerComponent} {stlUrl} isGenerating={isGenerating || isFreecadRunning} />
            <div class="genie-layer">
              <VertexGenie
                mode={genieMode}
                bubble={genieBubble}
                question={genieQuestion}
                onDismiss={dismissGenieBubble}
              />
            </div>

            {#if isGenerating && !isQuestionFlow}
              <div class="microwave-overlay">
                <div class="microwave-glass"></div>
                <div class="microwave-content">
                  <div class="microwave-timer">{formatCookingTime(cookingElapsed)}</div>
                </div>
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
                isGenerating={isGenerating || isLightReasoning}
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

  {#if isBooting}
    <div class="boot-overlay">
      <div class="boot-overlay__glass"></div>
      <div class="boot-overlay__content">
        <div class="boot-overlay__title">DRYDEMACHER</div>
        <div class="boot-overlay__ecky">
          <VertexGenie mode="thinking" bubble="" />
        </div>
        <div class="boot-overlay__status">Restoring config, history, and active workspace.</div>
      </div>
    </div>
  {/if}

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

  .boot-overlay {
    position: absolute;
    inset: 0;
    z-index: 300;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .boot-overlay__glass {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(circle at 50% 50%, rgba(74, 140, 92, 0.16), transparent 42%),
      rgba(8, 12, 20, 0.86);
    backdrop-filter: blur(18px) saturate(0.2);
  }

  .boot-overlay__content {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    min-width: 320px;
    padding: 20px 24px;
    border: 2px solid color-mix(in srgb, var(--primary) 35%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--bg-300) 85%, transparent), var(--shadow);
  }

  .boot-overlay__title {
    color: var(--secondary);
    font-size: 0.82rem;
    font-weight: bold;
    letter-spacing: 0.14em;
  }

  .boot-overlay__ecky {
    width: 150px;
    height: 150px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .boot-overlay__status {
    max-width: 420px;
    color: var(--text-dim);
    font-size: 0.72rem;
    text-align: center;
    letter-spacing: 0.03em;
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
    gap: 0;
    pointer-events: none;
  }

  .microwave-timer {
    font-family: var(--font-mono);
    font-size: 2.2rem;
    font-weight: bold;
    color: var(--primary);
    letter-spacing: 0.15em;
    text-shadow: 0 0 20px color-mix(in srgb, var(--primary) 40%, transparent);
  }
</style>
