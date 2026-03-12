<script lang="ts">
  import PromptPanel from './lib/PromptPanel.svelte';
  import Viewer from './lib/Viewer.svelte';
  import VertexGenie from './lib/VertexGenie.svelte';
  import DrawingOverlay from './lib/DrawingOverlay.svelte';
  import ParamPanel from './lib/ParamPanel.svelte';
  import ConfigPanel from './lib/ConfigPanel.svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { save } from '@tauri-apps/plugin-dialog';
  import { writeTextFile } from '@tauri-apps/plugin-fs';
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';

  import HistoryPanel from './lib/HistoryPanel.svelte';
  import CodeModal from './lib/CodeModal.svelte';
  import DeletedModels from './lib/DeletedModels.svelte';
  import InventoryPanel from './lib/InventoryPanel.svelte';
  import ImportEnrichmentModal from './lib/ImportEnrichmentModal.svelte';
  import { activeMicrowaveCount, setMuted, stopMicrowaveAudio, setAudibleThread } from './lib/audio/microwave';
  import { onboarding } from './lib/stores/onboarding';
  import { session } from './lib/stores/sessionStore';
  import { handleGenerate, initOrchestrator, isQuestionIntent } from './lib/controllers/requestOrchestrator';
  import { handleParamChange, commitManualVersion, forkManualVersion, stageParamChange } from './lib/controllers/manualController';
  import { applyAgentDraft, loadFromHistory, deleteThread, renameThread, createNewThread, forkDesign, deleteVersion, loadVersion, refreshHistory, finalizeThread } from './lib/stores/history';
  import { workingCopy, isDirty } from './lib/stores/workingCopy';
  import { history, activeThreadId, activeVersionId, config, availableModels, isLoadingModels, freecadAvailable } from './lib/stores/domainState';
  import { sidebarWidth, historyHeight, dialogueHeight, showCodeModal, selectedCode, selectedTitle, currentView } from './lib/stores/viewState';
  import { boot, saveConfig, fetchModels } from './lib/boot/restore';
  import { requestQueue, allRequests, activeRequests, activeRequestCount, currentActiveRequest, activeThreadBusy, activeThreadRequests } from './lib/stores/requestQueue';
  import { nowSeconds } from './lib/stores/timeEngine';
  import { liveApply, paramPanelState } from './lib/stores/paramPanelState';
  import { persistLastSessionSnapshot } from './lib/modelRuntime/sessionSnapshot';
  import {
    buildImportedParams,
    buildImportedPreviewTransforms,
    buildImportedUiSpec,
    type ImportedPreviewTransform,
  } from './lib/modelRuntime/importedRuntime';
  import {
    buildSemanticPatch,
    ensureSemanticManifest,
    materializeControlViews,
    resolveActiveControlViewId,
  } from './lib/modelRuntime/semanticControls';
  import {
    addImportedModelVersion,
    deleteAgentDraft,
    exportFile,
    formatBackendError,
    getAgentDraft,
    getActiveAgentSessions,
    getThreadAgentState,
    getModelManifest,
    importFcstd,
    resolveAgentConfirm,
    resolveAgentPrompt,
    saveModelManifest,
    type ThreadAgentState,
  } from './lib/tauri/client';
  import { listen } from '@tauri-apps/api/event';
  import type {
    AgentDraft,
    AgentSession,
    DesignParams,
    GenieTraits,
    Message,
    ParamValue,
    Request,
    UiField,
    UiSpec,
    ViewerAsset,
  } from './lib/types/domain';
  import type { MaterializedSemanticView } from './lib/modelRuntime/semanticControls';

  type ViewerHandle = {
    captureScreenshot: (overlayCanvas?: HTMLCanvasElement | null) => string | null;
  };

  type DrawingOverlayHandle = {
    hasDrawing: () => boolean;
    getCanvas: () => HTMLCanvasElement | null;
    clear: () => void;
  };

  type ThreadPhase = Request['phase'] | 'idle' | 'booting';
  type ViewerBusyPhase = 'generating' | 'repairing' | 'rendering' | 'committing' | null;

  function formatAgentPhase(phase: string): string {
    return phase.replace(/_/g, ' ').toUpperCase();
  }

  function agentDraftKey(draft: AgentDraft | null | undefined): string | null {
    if (!draft) return null;
    return `${draft.threadId}:${draft.baseMessageId}:${draft.updatedAt}`;
  }

  function mapAgentPhaseToViewerBusy(session: AgentSession | null): ViewerBusyPhase {
    switch (session?.phase) {
      case 'rendering':
      case 'restoring_version':
        return 'rendering';
      case 'saving_version':
        return 'committing';
      case 'patching_params':
      case 'patching_macro':
      case 'reading':
      case 'resolving':
        return 'generating';
      default:
        return null;
    }
  }

  function formatAgentOriginLabel(origin: Message['agentOrigin'] | null | undefined): string | null {
    if (!origin) return null;
    const host = origin.hostLabel?.trim() || origin.agentLabel?.trim() || 'Agent';
    const model = origin.llmModelLabel?.trim() || origin.llmModelId?.trim() || '';
    if (!model || model.toLowerCase() === host.toLowerCase()) {
      return host;
    }
    return `${host} · ${model}`;
  }

  function isActiveRequestPhase(phase: Request['phase']): boolean {
    return !['success', 'error', 'canceled'].includes(phase);
  }

  function isModelBusyRequestPhase(phase: Request['phase']): boolean {
    return ['generating', 'repairing', 'queued_for_render', 'rendering', 'committing'].includes(phase);
  }

  function requestMatchesViewerTarget(
    request: Request,
    threadId: string | null,
    messageId: string | null,
    modelId: string | null,
  ): boolean {
    if (!threadId || request.threadId !== threadId) return false;
    if (modelId) {
      if (request.baseModelId) return request.baseModelId === modelId;
      if (messageId && request.baseMessageId) return request.baseMessageId === messageId;
      return false;
    }
    if (messageId) {
      if (request.baseMessageId) return request.baseMessageId === messageId;
      return false;
    }
    return true;
  }

  function sessionMatchesViewerTarget(
    candidate: AgentSession,
    threadId: string | null,
    messageId: string | null,
    modelId: string | null,
  ): boolean {
    if (!threadId || candidate.threadId !== threadId) return false;
    if (modelId) {
      if (candidate.modelId) return candidate.modelId === modelId;
      if (messageId && candidate.messageId) return candidate.messageId === messageId;
      return false;
    }
    if (messageId) {
      if (candidate.messageId) return candidate.messageId === messageId;
      return false;
    }
    return true;
  }

  // Local reactive aliases for templates
  const phase = $derived($session.phase);
  const status = $derived($session.status);
  const error = $derived($session.error);
  const stlUrl = $derived($session.stlUrl);
  const activeArtifactBundle = $derived($session.artifactBundle);
  const sessionModelManifest = $derived($session.modelManifest);
  const selectedPartId = $derived($session.selectedPartId);

  const isBooting = $derived(phase === 'booting');
  const isQuestionFlow = $derived(phase === 'answering');
  const freecadMissing = $derived(!isBooting && $freecadAvailable === false);
  const isMcpMode = $derived($config.connectionType === 'mcp');

  type DialogueState =
    | { mode: 'generate' }
    | { mode: 'mcp-idle' }
    | { mode: 'agent-reply'; requestId: string; agentLabel: string };

  const dialogueState = $derived.by<DialogueState>(() => {
    if (pendingAgentPrompt) return { mode: 'agent-reply', requestId: pendingAgentPrompt.requestId, agentLabel: pendingAgentPrompt.agentLabel };
    if (isMcpMode && activeAgentSessions.length > 0) return { mode: 'mcp-idle' };
    return { mode: 'generate' };
  });
  const viewerAssets = $derived.by<ViewerAsset[]>(() => {
    const assets = activeArtifactBundle?.viewerAssets || [];
    return assets.map((asset) => ({
      ...asset,
      path: toAssetUrl(asset.path),
    }));
  });
  const effectiveUiSpec = $derived.by<UiSpec>(() => {
    if (($paramPanelState.uiSpec?.fields || []).length > 0) {
      return $paramPanelState.uiSpec;
    }
    return buildImportedUiSpec(sessionModelManifest);
  });
  const effectiveParameters = $derived.by<DesignParams>(() =>
    buildImportedParams(sessionModelManifest, $paramPanelState.params || {}, effectiveUiSpec),
  );
  const activeModelManifest = $derived.by(() =>
    ensureSemanticManifest(sessionModelManifest, effectiveUiSpec, effectiveParameters),
  );
  const importedPreviewTransforms = $derived.by<Record<string, ImportedPreviewTransform>>(() =>
    buildImportedPreviewTransforms(activeModelManifest, effectiveParameters),
  );
  const overlayPreviewOnly = $derived.by(() => {
    if (!(activeModelManifest?.sourceKind === 'importedFcstd' && overlaySelectedPart?.editable)) {
      return false;
    }
    if (!selectedPartId) return false;
    const preview = importedPreviewTransforms[selectedPartId];
    if (!preview) return false;
    return (
      Math.abs(preview.scale.x - 1) > 0.001 ||
      Math.abs(preview.scale.y - 1) > 0.001 ||
      Math.abs(preview.scale.z - 1) > 0.001
    );
  });
  const overlaySelectedPart = $derived.by(() => {
    if (!selectedPartId || !activeModelManifest?.parts?.length) return null;
    return activeModelManifest.parts.find((part) => part.partId === selectedPartId) ?? null;
  });
  const availableControlViews = $derived.by<MaterializedSemanticView[]>(() =>
    materializeControlViews(activeModelManifest, effectiveUiSpec, effectiveParameters),
  );
  let activeControlViewId = $state<string | null>(null);
  $effect(() => {
    activeControlViewId = resolveActiveControlViewId(
      activeModelManifest,
      selectedPartId,
      activeControlViewId,
    );
  });
  const activeControlView = $derived.by(() =>
    availableControlViews.find((view) => view.viewId === activeControlViewId) ??
    availableControlViews[0] ??
    null,
  );
  const overlayControls = $derived.by(() => {
    if (!overlaySelectedPart || !activeControlView) return [];
    const visibleControls = activeControlView.sections
      .filter((section) => !section.collapsed)
      .flatMap((section) => section.controls);
    const partScoped = visibleControls.filter((control) =>
      (control.partIds || []).includes(overlaySelectedPart.partId),
    );
    const globalControls = visibleControls.filter((control) => (control.partIds || []).length === 0);
    const preferred = partScoped.length > 0 ? [...partScoped, ...globalControls] : visibleControls;
    return preferred.slice(0, 4);
  });
  const suppressViewportBusyUi = $derived($showCodeModal);
  let showEnrichmentModal = $state(false);
  const enrichmentManifest = $derived.by(() => {
    if (!showEnrichmentModal) return null;
    const m = sessionModelManifest;
    if (!m || m.sourceKind !== 'importedFcstd') return null;
    if (m.enrichmentState?.status !== 'pending') return null;
    return m;
  });
  const localViewportRequests = $derived.by<Request[]>(() => {
    const threadId = $activeThreadId;
    const messageId = $activeVersionId;
    const modelId = activeArtifactBundle?.modelId ?? null;
    return $activeThreadRequests.filter(
      (request) =>
        isActiveRequestPhase(request.phase) &&
        requestMatchesViewerTarget(request, threadId, messageId, modelId),
    );
  });
  const externalViewerSession = $derived.by<AgentSession | null>(() => {
    const threadId = $activeThreadId;
    const messageId = $activeVersionId;
    const modelId = activeArtifactBundle?.modelId ?? null;
    if (!threadId && !messageId && !modelId) return null;

    return (
      activeAgentSessions.find((candidate) =>
        sessionMatchesViewerTarget(candidate, threadId, messageId, modelId),
      ) ??
      null
    );
  });
  const externalViewerBusyPhase = $derived.by<ViewerBusyPhase>(() =>
    mapAgentPhaseToViewerBusy(externalViewerSession),
  );
  const manualViewerBusyPhase = $derived.by<ViewerBusyPhase>(() => {
    if ($session.isManual && phase === 'rendering') return 'rendering';
    return null;
  });
  const showViewerBusyMask = $derived.by(() => {
    if (suppressViewportBusyUi) return false;
    if (localViewportRequests.some((request) => isModelBusyRequestPhase(request.phase))) return true;
    if (manualViewerBusyPhase === 'rendering') return true;
    return externalViewerBusyPhase === 'rendering' || externalViewerBusyPhase === 'committing';
  });
  const localViewerBusyPhase = $derived.by<ViewerBusyPhase>(() => {
    if (localViewportRequests.some((request) => request.phase === 'committing')) return 'committing';
    if (
      localViewportRequests.some((request) =>
        ['queued_for_render', 'rendering'].includes(request.phase),
      )
    ) {
      return 'rendering';
    }
    if (localViewportRequests.some((request) => request.phase === 'repairing')) return 'repairing';
    if (localViewportRequests.some((request) => request.phase === 'generating')) return 'generating';
    if (manualViewerBusyPhase === 'rendering') return 'rendering';
    return null;
  });
  const viewerBusyPhase = $derived.by<ViewerBusyPhase>(() => {
    return localViewerBusyPhase ?? externalViewerBusyPhase;
  });
  const viewerBusyText = $derived.by<string | null>(() => {
    switch (localViewerBusyPhase) {
      case 'repairing':
        return $session.repairMessage || 'Reweaving the geometry lattice.';
      case 'rendering':
        return 'Stabilizing the geometry into manufacturable solids.';
      case 'committing':
        return 'Finalizing the artifact and sealing it into the thread.';
      case 'generating':
        return $session.cookingPhrase || 'Preparing the next transformation.';
      default:
        if (!externalViewerSession || !externalViewerBusyPhase) return null;
        if (externalViewerSession.statusText.trim()) return externalViewerSession.statusText;
        switch (externalViewerBusyPhase) {
          case 'rendering':
            return `External agent ${externalViewerSession.agentLabel} is updating the model.`;
          case 'committing':
            return `External agent ${externalViewerSession.agentLabel} is saving a version.`;
          case 'generating':
            return `External agent ${externalViewerSession.agentLabel} is preparing an update.`;
          default:
            return null;
        }
    }
  });
  const externalViewerStatusText = $derived.by<string | null>(() => {
    if (!externalViewerSession || !externalViewerBusyPhase) return null;
    if (externalViewerSession.statusText.trim()) return externalViewerSession.statusText;
    switch (externalViewerBusyPhase) {
      case 'rendering':
        return `External agent ${externalViewerSession.agentLabel} is updating the model.`;
      case 'committing':
        return `External agent ${externalViewerSession.agentLabel} is saving a version.`;
      case 'generating':
        return `External agent ${externalViewerSession.agentLabel} is preparing an update.`;
      default:
        return null;
    }
  });

  let viewerComponent = $state<ViewerHandle | null>(null);
  let drawingOverlay = $state<DrawingOverlayHandle | null>(null);
  let drawMode = $state(false);
  let lastAssistantMessageId = $state<string | null>(null);
  let lastAdvisorBubble = $state('');
  let lastAdvisorQuestion = $state('');
  let dismissedBubbleText = $state('');

  let activeAgentSessions = $state<AgentSession[]>([]);
  let lastSeenAgentDraftKey = $state<string | null>(null);
  let genieWakeUpCount = $state(0);
  let prevSessionCount = 0;

  type PendingAgentPrompt = { requestId: string; message: string | null; agentLabel: string; sessionId: string };
  let pendingAgentPrompt = $state<PendingAgentPrompt | null>(null);

  type QueuedMessage = { id: string; text: string; status: 'queued' | 'delivered' };
  let queuedUserMessages = $state<QueuedMessage[]>([]);

  // Auto-deliver the first queued message when agent calls request_user_prompt
  $effect(() => {
    if (!pendingAgentPrompt) return;
    const first = queuedUserMessages.find(m => m.status === 'queued');
    if (!first) return;
    const requestId = pendingAgentPrompt.requestId;
    queuedUserMessages = queuedUserMessages.map(m => m.id === first.id ? { ...m, status: 'delivered' } : m);
    pendingAgentPrompt = null;
    void resolveAgentPrompt(requestId, first.text).catch(() => {});
  });

  $effect(() => {
    const count = activeAgentSessions.length;
    if (count > 0 && prevSessionCount === 0) genieWakeUpCount++;
    prevSessionCount = count;
  });

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
  let idleTimeout: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const hasAudioActivity = $activeRequestCount > 0 || $activeMicrowaveCount > 0;
    if (hasAudioActivity) {
      if (idleTimeout) {
        clearTimeout(idleTimeout);
        idleTimeout = null;
        console.info('[Microwave] idle shutdown canceled', {
          activeRequests: $activeRequestCount,
          activeMicrowaves: $activeMicrowaveCount,
        });
      }
      return;
    }

    if (!idleTimeout) {
      console.info('[Microwave] idle shutdown scheduled');
      idleTimeout = setTimeout(() => {
        const stillActive = get(activeRequestCount) > 0 || get(activeMicrowaveCount) > 0;
        if (stillActive) {
          console.info('[Microwave] idle shutdown skipped due to renewed activity', {
            activeRequests: get(activeRequestCount),
            activeMicrowaves: get(activeMicrowaveCount),
          });
          idleTimeout = null;
          return;
        }
        console.info('[Microwave] idle shutdown closing audio context');
        stopMicrowaveAudio(true);
        idleTimeout = null;
      }, 2000);
    }
  });

  $effect(() => {
    if (!isBooting && !$config.hasSeenOnboarding && !$onboarding.isActive) {
      onboarding.start();
    }
  });

  // Wire thread changes to audio focus
  $effect(() => {
    setAudibleThread($activeThreadId);
  });


  function formatCookingTime(s: number) {
    const m = Math.floor(s / 60);
    const sec = s % 60;
    return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
  }

  // --- Agent confirmation requests ---
  type AgentConfirmItem = { requestId: string; message: string; buttons: string[]; agentLabel: string };
  let pendingConfirms = $state<AgentConfirmItem[]>([]);

  async function answerConfirm(requestId: string, choice: string) {
    pendingConfirms = pendingConfirms.filter(c => c.requestId !== requestId);
    try { await resolveAgentConfirm(requestId, choice); } catch { /* already timed out */ }
  }

  async function answerAgentPrompt(requestId: string, promptText: string) {
    pendingAgentPrompt = null;
    try { await resolveAgentPrompt(requestId, promptText); } catch { /* already timed out */ }
  }

  async function handleDialogueSubmit(prompt: string, attachments: Attachment[]) {
    switch (dialogueState.mode) {
      case 'agent-reply': await answerAgentPrompt(dialogueState.requestId, prompt); break;
      case 'generate':    await handleGenerate(prompt, attachments); break;
      case 'mcp-idle':
        queuedUserMessages = [...queuedUserMessages, { id: crypto.randomUUID(), text: prompt, status: 'queued' }];
        break;
    }
  }

  onMount(() => {
    void boot();
    // Initial fetch of agent sessions (push events only fire on changes, not on load)
    void getActiveAgentSessions().then(sessions => { activeAgentSessions = sessions; }).catch(() => {});

    const unlisten = listen<AgentConfirmItem>('agent-confirm-request', (event) => {
      const item = event.payload;
      if (!pendingConfirms.find(c => c.requestId === item.requestId)) {
        pendingConfirms = [...pendingConfirms, item];
      }
    });
    const unlistenPrompt = listen<PendingAgentPrompt>('agent-prompt-request', (event) => {
      pendingAgentPrompt = event.payload;
    });
    const unlistenHistory = listen('history-updated', () => {
      void refreshHistory();
    });
    const unlistenSessions = listen<AgentSession[]>('agent-sessions-changed', (event) => {
      activeAgentSessions = event.payload;
    });
    return () => {
      void unlisten.then(fn => fn());
      void unlistenPrompt.then(fn => fn());
      void unlistenHistory.then(fn => fn());
      void unlistenSessions.then(fn => fn());
    };
  });

  const activeThread = $derived($history.find(t => t.id === $activeThreadId));
  const activeVersionMessage = $derived.by<Message | null>(() => {
    if (!activeThread) return null;
    return (
      activeThread.messages.find(
        (message) =>
          message.id === $activeVersionId &&
          message.role === 'assistant' &&
          Boolean(message.output || message.artifactBundle),
      ) ?? null
    );
  });
  const activeVersionAgentLabel = $derived(formatAgentOriginLabel(activeVersionMessage?.agentOrigin));
  const eckyTraits = $derived<Partial<GenieTraits>>(activeThread?.genieTraits || {});
  const eckyIntensity = $derived(1.0 + Math.max(0, ($activeRequestCount - 1) * 0.25));
  
  const agentDraft = $derived($session.agentDraft);

  async function loadAgentDraft() {
    if (!agentDraft) return;
    await applyAgentDraft(agentDraft);
  }

  async function discardAgentDraft() {
    if (!agentDraft) return;
    try {
      await deleteAgentDraft(agentDraft.threadId, agentDraft.baseMessageId);
      lastSeenAgentDraftKey = null;
      session.setAgentDraft(null);
    } catch (e) {
      console.error('Failed to discard agent draft:', e);
    }
  }

  function hasTauriIpc(): boolean {
    if (typeof window === 'undefined') return false;
    return typeof (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ === 'object';
  }

  async function refreshExternalAgentState() {
    if (!hasTauriIpc()) return;

    const threadId = $activeThreadId;
    const messageId = $activeVersionId;
    if (!threadId || !messageId) {
      lastSeenAgentDraftKey = null;
      const currentDraft = get(session).agentDraft as AgentDraft | null;
      if (currentDraft) {
        session.setAgentDraft(null);
      }
      return;
    }

    const draft = (await getAgentDraft(threadId, messageId)) as AgentDraft | null;
    const nextDraftKey = agentDraftKey(draft);

    if (!draft) {
      const currentDraft = get(session).agentDraft as AgentDraft | null;
      if (currentDraft?.threadId === threadId && currentDraft?.baseMessageId === messageId) {
        session.setAgentDraft(null);
      }
      lastSeenAgentDraftKey = null;
      return;
    }

    if (nextDraftKey === lastSeenAgentDraftKey) {
      return;
    }

    lastSeenAgentDraftKey = nextDraftKey;
    if (!get(isDirty)) {
      await applyAgentDraft(draft);
      return;
    }

    session.setAgentDraft(draft);
    session.setStatus('External agent draft updated. Load or discard it before saving.');
  }

  $effect(() => {
    const threadId = $activeThreadId;
    const messageId = $activeVersionId;
    lastSeenAgentDraftKey = null;
    if (!threadId || !messageId) return;
    void refreshExternalAgentState();
  });

  const inFlightByThread = $derived.by(() => {
    const counts: Record<string, number> = {};
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

  const activeThreadHighestPhase = $derived.by<ThreadPhase>(() => {
    if (phase === 'booting') return 'booting';

    const activeRequests = $activeThreadRequests.filter(
      (request) => !['success', 'error', 'canceled'].includes(request.phase),
    );
    const activePhases = activeRequests.map((request) => request.phase);
    if (activePhases.some((requestPhase) => ['rendering', 'queued_for_render', 'committing'].includes(requestPhase))) {
      return 'rendering';
    }
    if (activePhases.some((requestPhase) => requestPhase === 'repairing')) return 'repairing';
    if (activePhases.some((requestPhase) => requestPhase === 'generating')) return 'generating';
    if (activePhases.some((requestPhase) => requestPhase === 'answering')) return 'answering';
    if (activePhases.some((requestPhase) => requestPhase === 'classifying')) return 'classifying';

    const threadErrors = $activeThreadRequests.filter((request) => request.phase === 'error' && request.error);
    if (threadErrors.length > 0) return 'error';

    return 'idle';
  });

  const activeConfirm = $derived(pendingConfirms[0] ?? null);

  const genieMode = $derived.by(() => {
    if ($onboarding.isActive) return 'speaking';
    if (activeConfirm) return 'speaking';
    if (pendingAgentPrompt) return 'speaking';
    const atPhase = activeThreadHighestPhase;
    if (atPhase === 'error') return 'error';
    if (atPhase === 'repairing') return 'repairing';
    if (atPhase === 'classifying') return 'light';
    if (atPhase === 'rendering') return 'rendering';
    if (atPhase === 'generating' || atPhase === 'answering') return 'thinking';
    if (assistantFresh && !dismissedBubbleText && lastAdvisorBubble) return 'speaking';
    if (activeAgentSessions.length > 0) return 'light';
    return 'idle';
  });

  const genieBubble = $derived.by(() => {
    if ($onboarding.isActive) return $onboarding.text;
    if (activeConfirm) return activeConfirm.message;
    if (pendingAgentPrompt) return pendingAgentPrompt.message || `${pendingAgentPrompt.agentLabel} is waiting for your input`;
    const atPhase = activeThreadHighestPhase;
    const threadError =
      atPhase === 'error'
        ? [...$activeThreadRequests].reverse().find((request) => request.phase === 'error' && request.error)
            ?.error
        : null;

    const raw = threadError ||
               (atPhase === 'repairing' ? $session.repairMessage : null) ||
               (['classifying', 'generating', 'answering'].includes(atPhase) ? $session.cookingPhrase : null) ||
               lastAdvisorBubble || '';
    return (dismissedBubbleText === raw) ? '' : raw;
  });

  const genieActions = $derived.by(() => {
    if ($onboarding.isActive) {
      return [
        { label: 'NEXT', onclick: () => onboarding.next() },
        { label: 'SKIP', onclick: () => onboarding.skip() }
      ];
    }
    if (!activeConfirm) return null;
    return activeConfirm.buttons.map(btn => ({
      label: btn,
      onclick: () => answerConfirm(activeConfirm.requestId, btn),
    }));
  });

  async function toggleMicrowaveMute() {
    const currentConfig = get(config);
    const newMuted = !(currentConfig?.microwave?.muted);
    const nextConfig = {
      ...currentConfig,
      microwave: {
        ...(currentConfig?.microwave || { humId: null, dingId: null }),
        muted: newMuted,
      },
    };
    config.set(nextConfig);
    setMuted(newMuted, nextConfig);
    await saveConfig();
  }

  function applyCompletedRequest(req: Request) {
    if (!req?.result) return;
    const { design, threadId, messageId, stlUrl: reqStlUrl, artifactBundle, modelManifest } =
      req.result;
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
    if (artifactBundle || modelManifest) {
      session.setModelRuntime(artifactBundle ?? null, modelManifest ?? null);
    }
    void persistLastSessionSnapshot({
      design: design ?? null,
      threadId,
      messageId,
      artifactBundle: artifactBundle ?? null,
      modelManifest: modelManifest ?? null,
    });
    requestQueue.setActive(req.id);
  }

  function dismissRequest(id: string) {
    requestQueue.remove(id);
  }

  function retryRequest(req: Request) {
    void handleGenerate(req.prompt, req.attachments);
    requestQueue.remove(req.id);
  }

  function cancelRequest(id: string) {
    requestQueue.cancel(id);
  }

  function phaseLabel(phase: Request['phase']) {
    const labels: Partial<Record<Request['phase'], string>> = {
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

  function toAssetUrl(path: string | null | undefined): string {
    if (!path) return '';
    try {
      return convertFileSrc(path);
    } catch {
      return path;
    }
  }

  async function exportSTL() {
    if (!stlUrl) return;
    try {
      const path = await save({ filters: [{ name: 'STL 3D Model', extensions: ['stl'] }], defaultPath: 'design.stl' });
      if (typeof path === 'string') {
        let rawPath = decodeURIComponent(stlUrl.split('?')[0].replace('asset://localhost/', '/'));
        if (!rawPath.startsWith('/') && rawPath.match(/^[a-zA-Z]:/)) {} else if (!rawPath.startsWith('/')) { rawPath = '/' + rawPath; }
        await exportFile(rawPath, path);
      }
    } catch (e: unknown) {
      session.setError(`Export Error: ${formatBackendError(e)}`);
    }
  }

  async function exportFCStd() {
    const bundle = $session.artifactBundle;
    if (!bundle?.fcstdPath) return;
    try {
      const path = await save({ filters: [{ name: 'FreeCAD Document', extensions: ['FCStd'] }], defaultPath: 'design.FCStd' });
      if (typeof path === 'string') {
        await exportFile(bundle.fcstdPath, path);
      }
    } catch (e: unknown) {
      session.setError(`Export Error: ${formatBackendError(e)}`);
    }
  }

  function dismissGenie() {
    if (genieBubble) dismissedBubbleText = genieBubble;
  }

  function dismissError() {
    session.setError(null);
  }

  function handlePartSelect(partId: string | null) {
    session.setSelectedPartId(partId);
    void persistLastSessionSnapshot({ selectedPartId: partId });
  }

  function handleSemanticControlChange(primitiveId: string, value: ParamValue) {
    const nextParams = buildSemanticPatch(activeModelManifest, primitiveId, value, effectiveUiSpec);
    if (Object.keys(nextParams).length === 0) return;
    if ($liveApply) {
      void handleParamChange(nextParams);
      return;
    }
    stageParamChange(nextParams);
  }

  function handleSelectControlView(viewId: string | null) {
    activeControlViewId = viewId;
  }

  async function handleImportFcstd(sourcePath: string) {
    try {
      session.setError(null);
      session.setStatus('Importing FCStd...');
      const bundle = await importFcstd(sourcePath);
      const rawManifest = await getModelManifest(bundle.modelId);
      const importedUiSpec = buildImportedUiSpec(rawManifest);
      const importedParams = buildImportedParams(rawManifest, {}, importedUiSpec);
      const manifest = ensureSemanticManifest(rawManifest, importedUiSpec, importedParams) ?? rawManifest;
      const threadId = crypto.randomUUID();
      const importedName = sourcePath.split(/[\\/]/).pop() || 'model.FCStd';
      const title =
        manifest.document.documentLabel ||
        manifest.document.documentName ||
        importedName.replace(/\.fcstd$/i, '');
      const messageId = await addImportedModelVersion({
        threadId,
        title,
        artifactBundle: bundle,
        modelManifest: manifest,
      });
      await saveModelManifest(bundle.modelId, manifest, messageId);
      activeThreadId.set(threadId);
      activeVersionId.set(messageId);
      workingCopy.reset();
      paramPanelState.reset();
      session.setStlUrl(toAssetUrl(bundle.previewStlPath));
      session.setModelRuntime(bundle, manifest);
      await refreshHistory();
      await persistLastSessionSnapshot({
        design: null,
        threadId,
        messageId,
        artifactBundle: bundle,
        modelManifest: manifest,
        selectedPartId: null,
      });
      session.setStatus(`Imported FCStd: ${importedName}`);
      currentView.set('workbench');
      if (manifest.enrichmentState?.status === 'pending') {
        showEnrichmentModal = true;
      }
    } catch (e: unknown) {
      session.setError(`FCStd Import Error: ${formatBackendError(e)}`);
    }
  }

  function startResizingWidth(e: MouseEvent) {
    isResizingWidth = true;
    e.preventDefault();
  }

  function startResizingHeight(e: MouseEvent) {
    isResizingHeight = true;
    e.preventDefault();
  }

  function startResizingHistory(e: MouseEvent) {
    isResizingHistory = true;
    e.preventDefault();
  }

  function handleMouseMove(e: MouseEvent) {
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

  function handleGlobalKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') stopResizing();
  }
</script>

<svelte:window onmousemove={handleMouseMove} onmouseup={stopResizing} onblur={stopResizing} onkeydown={handleGlobalKeydown} />

<div class="app-page" role="application">
  {#if $onboarding.isActive}
    <div class="onboarding-backdrop"></div>
  {/if}
  <div class="app-overlay-actions">
    {#if $currentView === 'workbench'}
      {#if $activeRequestCount > 0}
        <button class="overlay-icon-btn" onclick={toggleMicrowaveMute} title="Toggle Cafeteria Hum">
          {$config?.microwave?.muted ? '🔇' : '🔊'}
        </button>
      {/if}
      <button class="overlay-icon-btn" class:draw-active={drawMode} onclick={() => drawMode = !drawMode} title={drawMode ? 'Exit Draw Mode' : 'Draw Annotations'}>
        ✏️
      </button>
      <button class="settings-overlay-btn" onclick={() => currentView.set('config')} title="Configuration">⚙️</button>
      <button class="settings-overlay-btn" onclick={() => currentView.set('trash')} title="Trash">🗑️</button>
      <button class="settings-overlay-btn" onclick={() => currentView.set('inventory')} title="Inventory">📦</button>
    {:else}
      <button class="settings-overlay-btn" onclick={() => currentView.set('workbench')} title="Close">×</button>
    {/if}
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
    {:else if $currentView === 'inventory'}
      <InventoryPanel />
    {:else}
      <div class="workbench">
        <aside class="sidebar" style="width: {$sidebarWidth}px">
          <div class="sidebar-section flex-1" class:onboarding-highlight={$onboarding.target === 'params'}>
            <div class="pane-header">TUNABLE PARAMETERS</div>
            <div class="sidebar-content scrollable">
              <ParamPanel 
                uiSpec={effectiveUiSpec} 
                modelManifest={activeModelManifest}
                controlViews={availableControlViews}
                activeControlViewId={activeControlViewId}
                onSelectControlView={handleSelectControlView}
                onSemanticChange={handleSemanticControlChange}
                selectedPartId={selectedPartId}
                onSelectPart={handlePartSelect}
                onspecchange={(spec, newParams) => {
                  paramPanelState.setUiSpec(spec);
                  workingCopy.patch({ uiSpec: spec });
                  if (newParams) {
                    paramPanelState.setParams(newParams);
                    workingCopy.patch({ params: newParams });
                  }
                }}
                parameters={effectiveParameters} 
                macroCode={$paramPanelState.macroCode}
                onchange={handleParamChange} 
                activeVersionId={$paramPanelState.versionId}
                messageId={$activeVersionId}
              />
            </div>
          </div>
          <div class="resizer-v" role="slider" aria-label="Resize history" aria-orientation="vertical" aria-valuenow={$historyHeight} tabindex="0" onmousedown={startResizingHistory} onkeydown={(e) => {
            if (e.key === 'ArrowUp') historyHeight.set($historyHeight + 10);
            if (e.key === 'ArrowDown') historyHeight.set($historyHeight - 10);
          }}></div>
          <div class="sidebar-section" style="height: {$historyHeight}px" class:onboarding-highlight={$onboarding.target === 'history'}>
            <div class="pane-header">THREAD HISTORY</div>
            <div class="sidebar-content scrollable">
              <HistoryPanel history={$history} activeThreadId={$activeThreadId}
                inFlightByThread={inFlightByThread}
                activeAgentSessions={activeAgentSessions}
                onSelect={loadFromHistory}
                onDelete={deleteThread}
                onRename={renameThread}
                onNew={createNewThread}
                onImportFcstd={handleImportFcstd}
                onFinalize={finalizeThread}
              />
            </div>
          </div>
        </aside>

        <div class="resizer-w" role="slider" aria-label="Resize sidebar" aria-orientation="horizontal" aria-valuenow={$sidebarWidth} tabindex="0" onmousedown={startResizingWidth} onkeydown={(e) => {
          if (e.key === 'ArrowLeft') sidebarWidth.set($sidebarWidth - 10);
          if (e.key === 'ArrowRight') sidebarWidth.set($sidebarWidth + 10);
        }}></div>

        <div class="main-workbench">
          <main class="viewport-area" role="presentation" class:onboarding-highlight={$onboarding.target === 'viewport'}>
            <Viewer
              bind:this={viewerComponent}
              stlUrl={$activeThreadId ? stlUrl : null}
              viewerAssets={viewerAssets}
              selectedPartId={selectedPartId}
              overlayPartLabel={overlaySelectedPart?.label ?? null}
              overlayPartEditable={overlaySelectedPart?.editable ?? false}
              overlayPreviewOnly={overlayPreviewOnly}
              overlayControls={overlayControls}
              previewTransforms={importedPreviewTransforms}
              onOverlayChange={handleSemanticControlChange}
              onSelectPart={handlePartSelect}
              isGenerating={viewerBusyPhase === 'generating' || viewerBusyPhase === 'repairing'}
              hideModelWhileBusy={showViewerBusyMask}
              busyPhase={viewerBusyPhase}
              busyText={viewerBusyText}
            />
            <DrawingOverlay bind:this={drawingOverlay} active={drawMode} />
            <div class="genie-layer" class:onboarding-active={$onboarding.isActive}>
              <VertexGenie 
                mode={genieMode} 
                bubble={genieBubble} 
                onDismiss={dismissGenie} 
                actions={genieActions} 
                traits={eckyTraits} 
                intensity={eckyIntensity} 
                wakeUp={genieWakeUpCount}
                agentConnected={activeAgentSessions.length > 0}
              />
            </div>

            {#if agentDraft}
              <div class="agent-draft-toast">
                <span class="toast-label">EXTERNAL AGENT DRAFT AVAILABLE FOR THIS VERSION</span>
                <div class="toast-actions">
                  <button class="btn btn-xs btn-primary" onclick={loadAgentDraft}>LOAD DRAFT</button>
                  <button class="btn btn-xs btn-secondary" onclick={discardAgentDraft}>DISCARD</button>
                </div>
              </div>
            {/if}

            {#if error}
              <div
                class="error-banner"
                role="button"
                tabindex="0"
                aria-label="Dismiss error"
                onclick={dismissError}
                onkeydown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    dismissError();
                  }
                }}
              >
                <div class="error-banner__label">ERROR</div>
                <div class="error-banner__body">{error}</div>
                <button
                  class="error-banner__dismiss"
                  onclick={(e) => {
                    e.stopPropagation();
                    dismissError();
                  }}
                  title="Dismiss error"
                >
                  ✕
                </button>
              </div>
            {/if}

            {#if freecadMissing}
              <div class="freecad-missing-banner">
                <div class="freecad-missing-banner__label">FREECAD NOT FOUND</div>
                <div class="freecad-missing-banner__body">FreeCAD is required to generate models. Install it or set the path in <button class="freecad-missing-banner__settings-link" onclick={() => currentView.set('config')}>Settings</button>.</div>
              </div>
            {/if}

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
            

            {#if $activeThreadId && ($workingCopy.macroCode || stlUrl)}
              <div class="viewport-overlay">
                {#if activeVersionAgentLabel}
                  <div class="agent-origin-chip" title={`Current model authored by ${activeVersionAgentLabel}`}>
                    {activeVersionAgentLabel}
                  </div>
                {/if}
                <div class="export-actions">
                  <button class="btn btn-xs btn-secondary" onclick={forkDesign} disabled={showViewerBusyMask} title="Fork this design into a new project">🍴 FORK</button>
                  <button class="btn btn-xs btn-primary" onclick={exportSTL} disabled={!stlUrl || showViewerBusyMask} title="Export as STL">💾 STL</button>
                  <button class="btn btn-xs btn-secondary" onclick={exportFCStd} disabled={!$session.artifactBundle?.fcstdPath || showViewerBusyMask} title="Export as FreeCAD document">💾 FCStd</button>
                </div>
              </div>
            {/if}
          </main>
          
          <div class="resizer-v" role="slider" aria-label="Resize dialogue" aria-orientation="vertical" aria-valuenow={$dialogueHeight} tabindex="0" onmousedown={startResizingHeight} onkeydown={(e) => {
            if (e.key === 'ArrowUp') dialogueHeight.set($dialogueHeight + 10);
            if (e.key === 'ArrowDown') dialogueHeight.set($dialogueHeight - 10);
          }}></div>

          <div class="dialogue-area" style="height: {$dialogueHeight}px" class:onboarding-highlight={$onboarding.target === 'dialogue'}>
            <div class="pane-header dialogue-header">
              DIALOGUE: {activeThread ? activeThread.title : 'New Session'}
            </div>
            <div class="dialogue-content">
              <PromptPanel
                onGenerate={handleDialogueSubmit}
                isGenerating={$activeThreadBusy}
                freecadMissing={freecadMissing}
                dialogueState={dialogueState}
                queuedMessages={queuedUserMessages}
                messages={activeThread?.messages || []}
                activeThreadId={$activeThreadId}
                onShowCode={(m) => { selectedCode.set(m.output.macroCode); selectedTitle.set(m.output.title); showCodeModal.set(true); }}
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
    <CodeModal
      bind:code={$selectedCode}
      title={$selectedTitle}
      onCommit={commitManualVersion}
      onFork={forkManualVersion}
      onclose={() => showCodeModal.set(false)}
    />
  {/if}

  {#if enrichmentManifest}
    <ImportEnrichmentModal
      manifest={enrichmentManifest}
      activeVersionId={$activeVersionId}
      onSelectPart={handlePartSelect}
      onclose={() => showEnrichmentModal = false}
      ondone={(updatedManifest) => {
        session.setModelRuntime($session.artifactBundle, updatedManifest);
        showEnrichmentModal = false;
      }}
    />
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
  .agent-origin-chip {
    padding: 4px 8px;
    border: 1px solid color-mix(in srgb, var(--primary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-200));
    color: var(--primary);
    font-family: var(--font-mono);
    font-size: 0.62rem;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    pointer-events: none;
  }
  .pane-header { padding: 4px 12px; background: var(--bg-200); border-bottom: 1px solid var(--bg-300); color: var(--secondary); font-size: 0.6rem; font-weight: bold; letter-spacing: 0.1em; text-transform: uppercase; }
  .scrollable { overflow-y: auto; }
  .resizer-w { width: 4px; background: var(--bg-300); cursor: col-resize; z-index: 10; flex-shrink: 0; }
  .resizer-v { height: 4px; background: var(--bg-300); cursor: row-resize; z-index: 10; flex-shrink: 0; }
  .app-overlay-actions { position: absolute; top: 10px; right: 10px; z-index: 150; display: flex; gap: 8px; }
  .overlay-icon-btn, .settings-overlay-btn { width: 34px; height: 34px; background: color-mix(in srgb, var(--bg-100) 90%, transparent); border: 1px solid var(--bg-300); color: var(--text); cursor: pointer; display: flex; align-items: center; justify-content: center; box-shadow: var(--shadow); }
  .overlay-icon-btn:hover, .settings-overlay-btn:hover { border-color: var(--primary); color: var(--primary); }
  .overlay-icon-btn.draw-active { border-color: var(--primary); background: color-mix(in srgb, var(--primary) 25%, var(--bg-100)); box-shadow: 0 0 8px var(--primary); }
  .genie-layer { position: absolute; left: 10px; top: 10px; z-index: 120; pointer-events: auto; max-width: min(80vw, 420px); }
  .error-banner {
    position: absolute;
    top: 12px;
    right: 12px;
    z-index: 130;
    max-width: min(46vw, 560px);
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: 10px;
    align-items: start;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--red) 72%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 88%, black 12%);
    box-shadow: var(--shadow);
    overflow: hidden;
    cursor: pointer;
  }
  .error-banner:focus-visible {
    outline: 1px solid var(--red);
    outline-offset: 1px;
  }
  .error-banner__label {
    color: var(--red);
    font-size: 0.62rem;
    font-weight: bold;
    letter-spacing: 0.12em;
  }
  .error-banner__body {
    color: var(--text);
    font-size: 0.78rem;
    line-height: 1.4;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .error-banner__dismiss {
    border: 1px solid var(--bg-400);
    background: var(--bg-200);
    color: var(--text-dim);
    width: 24px;
    height: 24px;
    cursor: pointer;
  }
  .error-banner__dismiss:hover { border-color: var(--red); color: var(--text); }

  .freecad-missing-banner {
    position: absolute;
    bottom: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 135;
    max-width: min(80vw, 520px);
    padding: 8px 14px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--bg-200);
    border: 1px solid var(--accent);
    pointer-events: auto;
  }
  .freecad-missing-banner__label {
    color: var(--accent);
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.12em;
  }
  .freecad-missing-banner__body {
    color: var(--text-dim);
    font-size: 0.78rem;
    line-height: 1.4;
  }
  .freecad-missing-banner__settings-link {
    background: none;
    border: none;
    color: var(--primary);
    cursor: pointer;
    padding: 0;
    font-size: inherit;
    text-decoration: underline;
  }

  .agent-activity-banner {
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 135;
    max-width: min(70vw, 560px);
    padding: 8px 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    border: 1px solid color-mix(in srgb, var(--primary) 68%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 92%, black 8%);
    box-shadow: var(--shadow);
    overflow: hidden;
    pointer-events: none;
  }

  .agent-activity-banner__label {
    color: var(--primary);
    font-size: 0.62rem;
    font-weight: bold;
    letter-spacing: 0.1em;
  }

  .agent-activity-banner__body {
    color: var(--text);
    font-size: 0.76rem;
    line-height: 1.35;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .agent-draft-toast {
    position: absolute;
    top: 72px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 140;
    background: var(--bg-100);
    border: 1px solid var(--secondary);
    padding: 8px 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    box-shadow: var(--shadow);
  }

  .toast-label {
    font-size: 0.65rem;
    font-weight: bold;
    color: var(--secondary);
    letter-spacing: 0.05em;
  }

  .toast-actions {
    display: flex;
    gap: 8px;
  }

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
  .viewport-overlay { position: absolute; bottom: 12px; right: 12px; background: rgba(11, 15, 26, 0.6); backdrop-filter: blur(4px); padding: 8px; border: 1px solid var(--bg-300); z-index: 50; display: flex; flex-direction: column; align-items: flex-end; gap: 8px; }
  .boot-overlay { position: absolute; inset: 0; z-index: 300; display: flex; align-items: center; justify-content: center; background: var(--bg); }
  .boot-overlay__glass { position: absolute; inset: 0; background: radial-gradient(circle, rgba(74, 140, 92, 0.16), transparent), rgba(8, 12, 20, 0.86); backdrop-filter: blur(18px); }
  .boot-overlay__content { position: relative; z-index: 1; display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 20px; }
  .boot-overlay__title { color: var(--secondary); font-weight: bold; letter-spacing: 0.2em; }
  .boot-overlay__status { color: var(--text-dim); font-size: 0.7rem; }
  .flex-1 { flex: 1; }
  .sidebar-section { display: flex; flex-direction: column; min-height: 0; overflow: hidden; }

  /* Onboarding */
  .onboarding-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    z-index: 999;
    pointer-events: all;
  }
  :global(.onboarding-highlight) {
    position: relative !important;
    z-index: 1000 !important;
    box-shadow: 0 0 0 2px var(--primary), 0 0 40px rgba(74, 140, 92, 0.5) !important;
    pointer-events: none;
    background: var(--bg-100);
  }
  :global(.genie-layer.onboarding-active) {
    z-index: 1001 !important;
  }

  /* Agent confirmation stack */
</style>
