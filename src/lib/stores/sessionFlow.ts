import { writable, derived, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { workingCopy } from './workingCopy';
import { history, activeThreadId, activeVersionId } from './domainState';
import { refreshHistory } from './history';
import { showCodeModal } from './viewState';
import { requestQueue, activeRequests, type QueuedRequest } from './requestQueue';
import { setAudibleThread, startMicrowaveHum, stopMicrowaveHum, stopMicrowaveAudio, ensureContext } from '../audio/microwave';

// ---------------------------------------------------------------------------
// Session store — keeps backward-compatible shape for App.svelte
// ---------------------------------------------------------------------------

function createSessionStore() {
  const { subscribe, set, update } = writable({
    phase: 'booting' as string,
    status: 'System ready.',
    error: null as string | null,
    stlUrl: null as string | null,
    isManual: false as boolean,
  });

  return {
    subscribe,
    set,
    update,
    setPhase: (p: string) => update(s => ({ ...s, phase: p })),
    setStatus: (msg: string) => update(s => ({ ...s, status: msg })),
    setError: (err: string | null) => update(s => ({ ...s, error: err })),
    setStlUrl: (url: string | null) => update(s => ({ ...s, stlUrl: url })),
    setIsManual: (m: boolean) => update(s => ({ ...s, isManual: m })),
  };
}

export const session = createSessionStore();

// Convenience accessors (backward compat)
export const phase = { subscribe: (fn: any) => session.subscribe(s => fn(s.phase)), set: session.setPhase };
export const status = { subscribe: (fn: any) => session.subscribe(s => fn(s.status)), set: session.setStatus };
export const error = { subscribe: (fn: any) => session.subscribe(s => fn(s.error)), set: session.setError };
export const stlUrl = { subscribe: (fn: any) => session.subscribe(s => fn(s.stlUrl)), set: session.setStlUrl };
export const isManual = { subscribe: (fn: any) => session.subscribe(s => fn(s.isManual)) };

let manualRenderActive = false;

// ---------------------------------------------------------------------------
// Derive session.phase from aggregate request queue state
// ---------------------------------------------------------------------------

function syncSessionPhaseFromQueue() {
  const q = get(requestQueue);
  const requests = Object.values(q.byId);
  const phases = requests.map(r => r.phase);

  let newPhase = 'idle';
  const hasActiveLLM = phases.some(p => ['classifying', 'answering', 'generating', 'repairing', 'rendering', 'queued_for_render', 'committing'].includes(p));
  
  // We prefer showing LLM phases if they exist. 
  // If manualRenderActive is true, it only drives 'rendering' if no LLM is doing it.
  if (phases.some(p => p === 'rendering' || p === 'queued_for_render' || p === 'committing')) {
    newPhase = 'rendering';
  } else if (phases.some(p => p === 'repairing')) {
    newPhase = 'repairing';
  } else if (phases.some(p => p === 'generating')) {
    newPhase = 'generating';
  } else if (phases.some(p => p === 'answering')) {
    newPhase = 'answering';
  } else if (phases.some(p => p === 'classifying')) {
    newPhase = 'classifying';
  } else if (manualRenderActive) {
    newPhase = 'rendering';
  } else {
    const s = get(session);
    if (s.phase === 'booting') {
      newPhase = 'booting';
    } else {
      newPhase = 'idle';
    }
  }

  session.update(s => ({ 
    ...s, 
    phase: newPhase, 
    isManual: manualRenderActive && !hasActiveLLM 
  }));
}

// ---------------------------------------------------------------------------
// Callbacks (microwave, phrases, etc.)
// ---------------------------------------------------------------------------

let appState: any = null;

export function initSessionFlow(state: any) {
  appState = state;
  
  // Hook into thread changes to update audible microwave
  activeThreadId.subscribe(tid => {
    setAudibleThread(tid);
  });
}

const REPAIR_PHRASES = [
  "FreeCAD blinked first. Asking the LLM for a cleaner retry.",
  "Repair cycle engaged. Convincing the macro to respect causality.",
  "Patching the geometry after a Boolean tantrum.",
  "Render failed. Rewriting the macro before the solver notices.",
  "Running emergency emotional support for a wounded BRep.",
  "The mesh has unionized. Negotiating a repair attempt.",
  "Reconstructing dignity after a FreeCAD traceback.",
  "The model broke character. Sending it back with notes.",
  "Second pass active: less chaos, more solids.",
  "Repairing the macro with the confidence of a forged permit."
];

function pickRetryMessage(nextAttempt: number, maxAttempts: number): string {
  const phrase = REPAIR_PHRASES[Math.floor(Math.random() * REPAIR_PHRASES.length)];
  return `${phrase} Retry ${nextAttempt} of ${maxAttempts}.`;
}

export function startLightReasoning() {
  if (appState?.startLightReasoning) appState.startLightReasoning();
}

export function stopLightReasoning() {
  if (appState?.stopLightReasoning) appState.stopLightReasoning();
}

export function startCooking() {
  if (appState?.startCooking) appState.startCooking();
}

export function stopCooking(success: boolean) {
  if (appState?.stopCooking) appState.stopCooking(success);
}

// ---------------------------------------------------------------------------
// Per-request cooking timer
// ---------------------------------------------------------------------------

let cookingTickInterval: ReturnType<typeof setInterval> | null = null;

function ensureCookingTick() {
  if (cookingTickInterval) return;
  cookingTickInterval = setInterval(() => {
    const q = get(requestQueue);
    const now = Date.now();
    for (const id of q.order) {
      const req = q.byId[id];
      if (req?.cookingStartTime && !['success', 'error', 'canceled'].includes(req.phase)) {
        requestQueue.patch(id, { cookingElapsed: Math.floor((now - req.cookingStartTime) / 1000) });
      }
    }
    const anyActive = q.order.some(id => {
      const r = q.byId[id];
      return r && !['success', 'error', 'canceled'].includes(r.phase);
    });
    if (!anyActive && cookingTickInterval) {
      clearInterval(cookingTickInterval);
      cookingTickInterval = null;
    }
  }, 1000);
}

// ---------------------------------------------------------------------------
// Helper: build context snapshots
// ---------------------------------------------------------------------------

function buildLightReasoningContext(): string {
  const context: string[] = [];
  const wc = get(workingCopy);
  if (wc.title) context.push(`Title: ${wc.title}`);
  if (wc.versionName) context.push(`Version: ${wc.versionName}`);
  if (wc.macroCode) context.push(`Current FreeCAD Macro:\n\`\`\`python\n${wc.macroCode}\n\`\`\``);
  if (wc.uiSpec) context.push(`Current UI Spec:\n\`\`\`json\n${JSON.stringify(wc.uiSpec, null, 2)}\n\`\`\``);
  if (wc.params && Object.keys(wc.params).length > 0) {
    context.push(`Current Parameters:\n\`\`\`json\n${JSON.stringify(wc.params, null, 2)}\n\`\`\``);
  }
  return context.join('\n\n');
}

function buildWorkingDesignSnapshot(): any {
  const wc = get(workingCopy);
  if (!wc.macroCode) return null;
  return {
    title: wc.title || 'Untitled Design',
    version_name: wc.versionName || 'Working Copy',
    response: '',
    interaction_mode: 'design',
    macro_code: wc.macroCode,
    ui_spec: wc.uiSpec || { fields: [] },
    initial_params: wc.params || {}
  };
}

// ---------------------------------------------------------------------------
// handleGenerate — now re-entrant, fires concurrent pipelines
// ---------------------------------------------------------------------------

export async function handleGenerate(initialPrompt: string, attachments: any[] = []): Promise<string> {
  session.setError(null);
  const currentThreadId = get(activeThreadId);
  const requestId = requestQueue.submit(initialPrompt, attachments, currentThreadId);

  // Kick audio context in user gesture
  ensureContext();

  runRequestPipeline(requestId).catch(err => {
    requestQueue.patch(requestId, { phase: 'error', error: String(err) });
    syncSessionPhaseFromQueue();
  });

  return requestId;
}

// ---------------------------------------------------------------------------
// runRequestPipeline — the per-request async pipeline
// ---------------------------------------------------------------------------

async function runRequestPipeline(requestId: string) {
  const q = get(requestQueue);
  const req = q.byId[requestId];
  if (!req) return;

  const {
    isQuestionIntent,
    viewerComponent
  } = appState;

  // STEP 1 FIX: Capture stable snapshot for this request pipeline
  const snapshotThreadId = req.threadId;
  const snapshotParentMacroCode = get(workingCopy).macroCode || null;
  const snapshotWorkingDesign = buildWorkingDesignSnapshot();

  try {
    requestQueue.patch(requestId, { phase: 'classifying' });
    syncSessionPhaseFromQueue();
    startLightReasoning();

    let isQuestion = isQuestionIntent(req.prompt);
    let lightResponse = '';

    try {
      const intent = await invoke('classify_intent', {
        prompt: req.prompt,
        threadId: snapshotThreadId,
        context: buildLightReasoningContext()
      });
      if (intent?.intent_mode === 'question' || intent?.intent_mode === 'design') {
        isQuestion = intent.intent_mode === 'question';
      }
      if (intent?.response) {
        lightResponse = intent.response;
        appState.setCookingPhrase(lightResponse);
      }
      requestQueue.patch(requestId, { isQuestion, lightResponse });
    } catch (e) {
      console.warn(`[RequestQueue:${requestId}] Intent classification fallback:`, e);
    }

    let currentScreenshot: string | null = null;
    if (viewerComponent && get(stlUrl)) {
      currentScreenshot = viewerComponent.captureScreenshot();
      requestQueue.patch(requestId, { screenshot: currentScreenshot });
    }

    if (isQuestion) {
      session.setStatus('Answering question...');
      requestQueue.patch(requestId, { phase: 'answering' });
      syncSessionPhaseFromQueue();
      const questionReplyText = lightResponse || 'Question answered. Geometry unchanged.';

      const result = await invoke('answer_question_light', {
        prompt: req.prompt,
        response: questionReplyText,
        threadId: snapshotThreadId,
        titleHint: snapshotThreadId ? undefined : 'Question Session',
        imageData: currentScreenshot,
        attachments: req.attachments
      });

      // Only update active thread if the user hasn't navigated away
      if (get(activeThreadId) === snapshotThreadId) {
        activeThreadId.set(result.thread_id);
      }
      requestQueue.patch(requestId, { threadId: result.thread_id });
      await refreshHistory();

      session.setStatus(result.response || questionReplyText);
      requestQueue.patch(requestId, { phase: 'success', result: { design: null, threadId: result.thread_id, messageId: '', stlUrl: '' } });
      stopLightReasoning();
      syncSessionPhaseFromQueue();
      return;
    }

    stopLightReasoning();
    startCooking();
    requestQueue.patch(requestId, { cookingStartTime: Date.now() });
    ensureCookingTick();

    const config = get(appState.configStore);
    startMicrowaveHum(requestId, config, req.threadId);

    function stopRequestMicrowave(success: boolean) {
      stopMicrowaveHum(requestId);
      stopCooking(success);
    }

    let attempt = 1;
    let currentPrompt = req.prompt;

    while (attempt <= req.maxAttempts) {
      if (attempt === 1) {
        session.setStlUrl(null);
      }
      requestQueue.patch(requestId, { phase: attempt > 1 ? 'repairing' : 'generating', attempt });
      syncSessionPhaseFromQueue();
      session.setStatus(`Consulting LLM (Attempt ${attempt}/${req.maxAttempts})...`);

      try {
        const result = await invoke('generate_design', {
          prompt: currentPrompt,
          threadId: snapshotThreadId,
          parentMacroCode: snapshotParentMacroCode,
          workingDesign: snapshotWorkingDesign,
          isRetry: attempt > 1,
          imageData: currentScreenshot,
          attachments: req.attachments,
          questionMode: false
        });

        const data = result.design;
        const interactionMode = `${data.interaction_mode ?? ''}`.toLowerCase();

        if (interactionMode === 'question') {
          const qResult = await invoke('answer_question_light', {
            prompt: currentPrompt,
            response: data.response || 'Question answered.',
            threadId: result.thread_id,
            imageData: currentScreenshot,
            attachments: req.attachments
          });
          if (get(activeThreadId) === snapshotThreadId) {
            activeThreadId.set(qResult.thread_id);
          }
          await refreshHistory();
          session.setStatus(data.response || 'Question answered.');
          requestQueue.patch(requestId, { phase: 'success', result: { design: data, threadId: qResult.thread_id, messageId: '', stlUrl: '' } });
          stopRequestMicrowave(true);
          syncSessionPhaseFromQueue();
          break;
        }

        requestQueue.patch(requestId, { phase: 'rendering' });
        syncSessionPhaseFromQueue();
        session.setStatus('Executing FreeCAD engine...');

        let absolutePath: string | null = null;
        try {
          absolutePath = await invoke('render_stl', {
            macroCode: data.macro_code,
            parameters: data.initial_params || {}
          });
          const stlUrlValue = convertFileSrc(absolutePath);

          requestQueue.patch(requestId, { phase: 'committing' });
          syncSessionPhaseFromQueue();

          try {
            const commitResult = await invoke('commit_generated_version', {
              threadId: result.thread_id,
              prompt: req.prompt, // Original prompt
              design: data,
              imageData: currentScreenshot,
              attachments: req.attachments
            });

            if (get(activeThreadId) === snapshotThreadId) {
              activeThreadId.set(commitResult.thread_id);
              activeVersionId.set(commitResult.message_id);
              const currentQ = get(requestQueue);
              if (currentQ.activeId === requestId) {
                workingCopy.loadVersion(data, commitResult.message_id);
                session.setStlUrl(stlUrlValue);
              }
            }
            requestQueue.patch(requestId, { threadId: commitResult.thread_id });

            await refreshHistory();
            requestQueue.patch(requestId, {
              phase: 'success',
              result: { design: data, threadId: commitResult.thread_id, messageId: commitResult.message_id, stlUrl: stlUrlValue }
            });
            session.setStatus('Design synthesized successfully.');
            stopRequestMicrowave(true);
            syncSessionPhaseFromQueue();
            break;

          } catch (commitError) {
            session.setError(`Database Error: ${commitError}`);
            requestQueue.patch(requestId, { phase: 'error', error: `Database Error: ${commitError}` });
            stopRequestMicrowave(false);
            syncSessionPhaseFromQueue();
            break;
          }
        } catch (renderError) {
          if (attempt < req.maxAttempts) {
            const repairMsg = pickRetryMessage(attempt + 1, req.maxAttempts);
            appState.setRepairMessage(repairMsg);
            currentPrompt = `The previous code failed in FreeCAD with this error:\n${renderError}\n\nPlease fix it.`;
            attempt++;
            continue;
          } else {
            session.setError(`Render Error: ${renderError}`);
            
            // Show the mess STL
            try {
              const messPath = await invoke('get_mess_stl_path');
              const messUrl = convertFileSrc(messPath);
              session.setStlUrl(messUrl);
            } catch (e) {
              console.warn("Failed to load mess.stl:", e);
              session.setStlUrl(null);
            }

            appState.openCodeModalManual(data);
            requestQueue.patch(requestId, { phase: 'error', error: `Render Error: ${renderError}` });
            stopRequestMicrowave(false);
            syncSessionPhaseFromQueue();
            break;
          }
        }
      } catch (e) {
        session.setError(`Generation Failed: ${e}`);
        
        // Show the mess STL for generation failures too
        try {
          const messPath = await invoke('get_mess_stl_path');
          const messUrl = convertFileSrc(messPath);
          session.setStlUrl(messUrl);
        } catch (err) {
          session.setStlUrl(null);
        }

        requestQueue.patch(requestId, { phase: 'error', error: `Generation Failed: ${e}` });
        stopRequestMicrowave(false);
        syncSessionPhaseFromQueue();
        break;
      }
    }
  } catch (err) {
    session.setError(`Pipeline Error: ${err}`);
    
    // Show the mess STL for pipeline errors too
    try {
      const messPath = await invoke('get_mess_stl_path');
      const messUrl = convertFileSrc(messPath);
      session.setStlUrl(messUrl);
    } catch (e) {
      session.setStlUrl(null);
    }

    requestQueue.patch(requestId, { phase: 'error', error: `Pipeline Error: ${err}` });
    stopRequestMicrowave(false);
    syncSessionPhaseFromQueue();
  } finally {
    stopLightReasoning();
  }
}

// ---------------------------------------------------------------------------
// handleParamChange — now thread-aware audio
// ---------------------------------------------------------------------------

export async function handleParamChange(newParams: any, forcedCode: string | null = null, persist: boolean = true) {
  const wc = get(workingCopy);
  const snapshotThreadId = get(activeThreadId);
  const currentParams = { ...wc.params, ...newParams };
  workingCopy.updateParams(newParams);

  const codeToUse = forcedCode || wc.macroCode;
  if (!codeToUse) return;

  // Kick audio context in user gesture
  ensureContext();

  session.setStatus('Executing FreeCAD engine...');
  try {
    manualRenderActive = true;
    syncSessionPhaseFromQueue();
    
    const config = get(appState.configStore);
    startMicrowaveHum('__manual__', config, snapshotThreadId);

    const absolutePath = await invoke('render_stl', {
      macroCode: codeToUse,
      parameters: currentParams
    });

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(convertFileSrc(absolutePath));
    }

    if (persist && wc.sourceVersionId) {
      try {
        await invoke('update_parameters', { messageId: wc.sourceVersionId, parameters: currentParams });
        if (get(activeThreadId) === snapshotThreadId) {
          await refreshHistory();
        }
      } catch (e) {
        console.error('[SessionFlow] Failed to persist parameters:', e);
      }
    }
    
    stopMicrowaveHum('__manual__');
    manualRenderActive = false;
    syncSessionPhaseFromQueue();
  } catch (e) {
    if (get(activeThreadId) === snapshotThreadId) {
      session.setError(`Render Error: ${e}`);
    }
    stopMicrowaveHum('__manual__');
    manualRenderActive = false;
    syncSessionPhaseFromQueue();
  }
}

// ---------------------------------------------------------------------------
// commitManualVersion — now thread-aware audio
// ---------------------------------------------------------------------------

export async function commitManualVersion(editedCode: string) {
  const wc = get(workingCopy);
  const snapshotThreadId = get(activeThreadId);

  if (!snapshotThreadId) {
    session.setError("Cannot commit manual version: No active thread. Please generate first.");
    return;
  }

  session.setStatus("Validating manual edit...");
  try {
    manualRenderActive = true;
    syncSessionPhaseFromQueue();
    
    const config = get(appState.configStore);
    startMicrowaveHum('__manual__', config, snapshotThreadId);

    const absolutePath = await invoke('render_stl', {
      macroCode: editedCode,
      parameters: wc.params
    });

    const newMsgId = await invoke('add_manual_version', {
      threadId: snapshotThreadId,
      title: wc.title || "Manual Edit",
      versionName: "V-manual",
      macroCode: editedCode,
      parameters: wc.params,
      uiSpec: wc.uiSpec
    });

    if (get(activeThreadId) === snapshotThreadId) {
      session.setStlUrl(convertFileSrc(absolutePath));
      workingCopy.loadVersion({ ...wc, macro_code: editedCode }, newMsgId);
      activeVersionId.set(newMsgId);
      showCodeModal.set(false);
      session.setStatus("Manual version committed successfully.");
      await refreshHistory();
    }
    
    stopMicrowaveHum('__manual__');
    manualRenderActive = false;
    syncSessionPhaseFromQueue();
  } catch (e) {
    if (get(activeThreadId) === snapshotThreadId) {
      session.setError(`Manual Commit Failed: ${e}`);
    }
    stopMicrowaveHum('__manual__');
    manualRenderActive = false;
    syncSessionPhaseFromQueue();
  }
}
