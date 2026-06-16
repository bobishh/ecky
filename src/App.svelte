<script lang="ts">
  import PromptPanel from './lib/PromptPanel.svelte';
  import Viewer from './lib/Viewer.svelte';
  import VertexGenie from './lib/VertexGenie.svelte';
  import DrawingOverlay from './lib/DrawingOverlay.svelte';
  import ParamPanel from './lib/ParamPanel.svelte';
  import ConfigPanel from './lib/ConfigPanel.svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { writeTextFile } from '@tauri-apps/plugin-fs';
  import { onDestroy, onMount, tick } from 'svelte';
  import { get } from 'svelte/store';
  import { buildGenieTraitsFromSeed, buildModelGenieTraits } from './lib/genie/traits';

  import CodeModal from './lib/CodeModal.svelte';
  import SessionActivityWindow from './lib/SessionActivityWindow.svelte';
  import ImportEnrichmentModal from './lib/ImportEnrichmentModal.svelte';
  import ManualImportModal from './lib/ManualImportModal.svelte';
  import AgentTerminalSurface from './lib/AgentTerminalSurface.svelte';
  import SketchWorkspace from './lib/SketchWorkspace.svelte';
  import DocsSite from './lib/DocsSite.svelte';
  import Modal from './lib/Modal.svelte';
  import Window from './lib/Window.svelte';
  import ProjectSwitcher from './lib/ProjectSwitcher.svelte';
  import {
    windowStore,
    windowLayoutRemembered,
    loadLayoutForThread,
    showWindow,
    toggleWindow,
    closeWindow as closeWindowStore,
    hardFlush as hardFlushWindowLayout,
    teardown as teardownWindowStore,
    setThreadWindowLayoutRemembered,
    type WindowId,
  } from './lib/stores/windowStore';
  import { triggerMacroNodeFocus } from './lib/stores/uiHighlightStore';
  import {
    activeMicrowaveCount,
    setMuted,
    setAudibleThread,
    startMicrowaveHum,
    stopMicrowaveAudio,
    stopMicrowaveHum,
  } from './lib/audio/microwave';
  import { setSpeechMuted, speakEckyText, stopEckySpeech } from './lib/audio/tts';
  import { resolveGenieSpeechCue } from './lib/genie/speechPolicy';
  import { onboarding } from './lib/stores/onboarding';
  import { session } from './lib/stores/sessionStore';
  import { startCookingPhraseLoop, stopPhraseLoop } from './lib/stores/phraseEngine';
  import { handleGenerate, isQuestionIntent } from './lib/controllers/requestOrchestrator';
  import { handleParamChange, commitManualVersion, stageParamChange, applyManualCodeDraft } from './lib/controllers/manualController';
  import { openProjectInEditor } from './lib/tauri/client';
  import {
    loadFromHistory,
    createNewThread,
    forkDesign,
    deleteVersion,
    restoreVersion,
    loadVersion,
    refreshHistory,
    loadOlderThreadMessages,
    activeThreadMessagesLoading,
    threadMessagePageState,
  } from './lib/stores/history';
  import { workingCopy, isDirty } from './lib/stores/workingCopy';
  import {
    historyStore as history,
    activeThreadIdStore as activeThreadId,
    activeVersionId,
    config,
    availableModels,
    isLoadingModels,
    runtimeCapabilities,
  } from './lib/stores/domainState';
  import {
    createSketchPreviewDraftScopeId,
    normalizeSketchPreviewDraftScopeId,
  } from './lib/sketchPreviewDraftStore';
  import { selectedCode, selectedTitle, currentView } from './lib/stores/viewState';
  import { boot, saveConfig, fetchModels } from './lib/boot/restore';
  import { requestQueue, allRequests, activeRequests, activeRequestCount, currentActiveRequest, activeThreadBusy, activeThreadRequests } from './lib/stores/requestQueue';
  import { nowSeconds } from './lib/stores/timeEngine';
  import { liveApply, paramPanelState } from './lib/stores/paramPanelState';
  import { resolveEngineCapabilitySummary } from './lib/modelRuntime/modelCapabilities';
  import { persistLastSessionSnapshot } from './lib/modelRuntime/sessionSnapshot';
  import { getRenderableRuntimeBundle, inspectRuntimeBundle } from './lib/modelRuntime/runtimeBundle';
  import { sameArtifactVersion, shouldPersistVersionPreview } from './lib/versionPreviewPersistence';
  import { resolveDraftPreviewDesign } from './lib/agents/draftPreviewParams';
  import {
    deriveThreadAttentionIds,
    deriveMascotStateForThreadAgent,
    derivePrimaryAgentId,
    hasLiveAgentSession,
    resolveActivePendingPrompt,
    shouldAutoFocusAgentWorkingVersion,
    usesAgentDialogueMode,
    usesMcpConnection,
    usesActiveMcpMode,
  } from './lib/agents/state';
  import { resolveRelayPresence } from './lib/agents/relayPresence';
  import { deriveDialogueState, type DialogueState } from './lib/composables/dialogueState';
  import {
    buildOptimisticQueuedDialogueMessage,
    deriveOptimisticDialogueMessages,
    hasLiveApiEngineConnection,
    mergeOptimisticQueuedDialogueMessages,
    type OptimisticQueuedDialogueMessage,
  } from './lib/composables/apiDialogue';
  import {
    deriveViewerBusyState,
    mapThreadAgentStateToViewerBusy,
    type ViewerBusyPhase,
  } from './lib/composables/viewerBusyState';
  import {
    agentTerminalSessionKey,
    buildAgentTerminalKeyInput,
    buildAgentTerminalLineInput,
  } from './lib/agents/terminal';
  import {
    composeAgentDraftFeedbackBubbleText,
    isVisibleAgentDraftFeedback,
    type AgentAuthoringLint,
    type AgentDraftFeedback,
  } from './lib/agents/draftFeedback';
  import {
    isWorkspaceCaptureEnabled,
    readWorkspaceCapturePrefs,
    setWorkspaceCaptureEnabled,
    writeWorkspaceCapturePrefs,
  } from './lib/agents/workspaceCapture';
  import { codeInspectorTitle } from './lib/modelEngineLabel';
  import { buildFailedDraftSeed } from './lib/manualDraftSeed';
  import { loadSketchPreviewDraft } from './lib/tauri/client';
  import type { TopologyMode } from './lib/viewerDisplayMode';
  import {
    agentTerminalAttentionStore,
    enqueueAgentTerminalSnapshot,
    replaceAgentTerminalSnapshots,
    resetAgentTerminalStore,
    setAgentTerminalSelection,
    visibleAgentTerminalStore,
  } from './lib/stores/agentTerminalStore';
  import {
    isThreadAgentBusy,
    resolveActiveMcpBubble,
    resolveGenieBubblePresentation,
    resolveTerminalActivityMeta,
  } from './lib/agents/activity';
  import {
    chooseViewportCaptureMode,
    rememberTargetCameraState,
    rememberTargetScreenshot,
    resolveFallbackScreenshotSource,
    viewportCameraKey,
    viewportTargetKey,
    type ViewportScreenshotCapture,
  } from './lib/agents/screenshot';
  import {
    buildImportedParams,
    buildImportedPreviewTransforms,
    buildImportedUiSpec,
    type ImportedPreviewTransform,
  } from './lib/modelRuntime/importedRuntime';
  import {
    buildPreviewViewTransforms,
    mergePreviewTransforms,
    resolveActivePreviewView,
  } from './lib/modelRuntime/previewViews';
  import {
    buildSemanticPatch,
    ensureSemanticManifest,
    materializeControlViews,
  } from './lib/modelRuntime/semanticControls';
  import {
    buildContextSelectionTargets,
    createGlobalContextTarget,
    deriveSelectedPartId,
    pickContextAdvisories,
    pickContextControls,
    resolveMeasurementCallout,
    resolveActiveContextViewId,
    resolveContextSelectionTarget,
    type MeasurementControlFocus,
    type ContextSelectionTarget,
  } from './lib/modelRuntime/contextualEditing';
  import {
    getStepExportPath,
    type ExportMode,
  } from './lib/exportOptions';
  import { deriveContextState } from './lib/composables/contextState';
  import { deriveViewportState } from './lib/composables/viewportState';
  import { deriveAgentOpsState, type PendingViewportScreenshotChoice } from './lib/composables/agentOps';
  import { deriveExportState } from './lib/composables/exportOps';
  import {
    composeBubbleEvent,
    composeSessionActivity,
    type SessionEvent,
  } from './lib/sessionActivity';
  import {
    recordSessionActivityEvent,
    sessionActivityEvents as sessionActivityEventStore,
  } from './lib/stores/sessionActivityStore';
  import {
    capabilityForAuthoringContext,
    resolveActiveAuthoringContext,
  } from './lib/runtimeCapabilities';
  import { isRenderableVersionTimelineMessage } from './lib/threadTimeline';
  import {
    clearSketchPreviewDraft,
    saveSketchPreviewDraft,
    addImportedModelVersion,
    exportFile,
    exportMultipart3mf,
    exportMultipartStlZip,
    formatBackendError,
    getActiveAgentSessions,
    getAgentTerminalSnapshots,
    getMessageAttachments,
    getThread,
    getThreadAgentState,
    getModelManifest,
    importFreecadLibraryPart,
    importFcstd,
    preparePromptAttachments,
    preparePromptWorkspaceCapture,
    rejectAgentViewportScreenshot,
    renderModel,
    restartPrimaryAutoAgent,
    resizeAgentTerminal,
    queueAgentPrompt,
    resolveAgentConfirm,
    resolveAgentPrompt,
    resolveAgentViewportScreenshot,
    saveConfig as persistBackendConfig,
    sendAgentTerminalInput,
    stopPrimaryAutoAgent,
    updateVersionRuntime,
    updateVersionPreview,
    wakePrimaryAutoAgent,
    saveModelManifest,
    type PostProcessingSpec,
    type ThreadAgentState,
  } from './lib/tauri/client';
  import { listen } from '@tauri-apps/api/event';
  import type { FreecadLibraryItem, SketchDraftSource } from './lib/tauri/contracts';
  import type {
    AgentSession,
    AgentTerminalInput,
    AgentTerminalSnapshot,
    Attachment,
    ArtifactBundle,
    DesignOutput,
    DesignParams,
    GenieTraits,
    Message,
    ModelManifest,
    ParamValue,
    Request,
    RuntimeBackendCapability,
    SourceLanguage,
    UiField,
    UiSpec,
    ViewerAsset,
    ViewportCameraState,
    GeometryBackend,
  } from './lib/types/domain';
  import type { MaterializedSemanticView } from './lib/modelRuntime/semanticControls';

  type ViewerHandle = {
    captureScreenshot: (overlayCanvas?: HTMLCanvasElement | null) => string | null;
    captureMultiAngleScreenshots: () => string[];
    captureScreenshotDetails: (overlayCanvas?: HTMLCanvasElement | null) => {
      dataUrl: string;
      width: number;
      height: number;
      camera: ViewportCameraState;
    } | null;
    getCameraState: () => ViewportCameraState | null;
    setCameraState: (camera: ViewportCameraState | null) => void;
  };

  const GENIE_SEED_OVERRIDES_KEY = 'ecky.genie.seedOverrides.v1';

  function readGenieSeedOverrides(): Record<string, number> {
    if (typeof localStorage === 'undefined') return {};
    try {
      const parsed = JSON.parse(localStorage.getItem(GENIE_SEED_OVERRIDES_KEY) ?? '{}');
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
      return Object.fromEntries(
        Object.entries(parsed as Record<string, unknown>).filter((entry): entry is [string, number] => (
          typeof entry[1] === 'number' && Number.isFinite(entry[1]) && entry[1] > 0
        )),
      );
    } catch {
      return {};
    }
  }

  function writeGenieSeedOverrides(overrides: Record<string, number>) {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(GENIE_SEED_OVERRIDES_KEY, JSON.stringify(overrides));
    } catch {
      // Ignore storage failures in private or restricted contexts.
    }
  }

  function randomGenieSeed(): number {
    if (typeof crypto !== 'undefined' && crypto.getRandomValues) {
      const buffer = new Uint32Array(1);
      crypto.getRandomValues(buffer);
      return buffer[0] || 1;
    }
    return (Date.now() >>> 0) || 1;
  }

  type AgentDraftPreviewUpdatedEvent = {
    sessionId: string;
    threadId: string;
    previewId: string;
    baseMessageId?: string | null;
    modelId?: string | null;
    design: DesignOutput;
    artifactBundle: ArtifactBundle;
    modelManifest: ModelManifest;
    feedback?: {
      status: 'checking' | 'passed' | 'failed' | 'warning';
      summary: string;
      items: Array<string | { code: string; message: string }>;
      source: 'structuralVerification' | 'renderError' | 'toolError' | 'visualRepair';
      authoringLints?: Array<{
        kind?: string | null;
        partKey?: string | null;
        paramKey?: string | null;
        suggestedParamKey?: string | null;
        occurrenceCount?: number | null;
        message: string;
      }>;
    } | null;
  };
  type ThreadAgentStateWithAuthoringLints = ThreadAgentState & {
    authoringLints?: AgentAuthoringLint[];
  };

  type DrawingOverlayHandle = {
    hasDrawing: () => boolean;
    getCanvas: () => HTMLCanvasElement | null;
    clear: () => void;
  };

  type ThreadPhase = Request['phase'] | 'idle' | 'booting';
  type ViewerCaptureDetails = {
    dataUrl: string;
    width: number;
    height: number;
    camera: ViewportCameraState;
  };
  type AgentViewportScreenshotEvent = {
    requestId: string;
    threadId: string;
    messageId: string;
    modelId?: string | null;
    previewStlPath: string;
    viewerAssets: ViewerAsset[];
    includeOverlays: boolean;
    camera?: ViewportCameraState | null;
  };
  type AgentWorkingVersionCreatedEvent = {
    sessionId: string;
    threadId: string;
    messageId: string;
    modelId: string | null;
  };
  type HiddenViewerSpec = {
    requestId: string;
    targetKey: string;
    stlUrl: string;
    viewerAssets: ViewerAsset[];
  };
  type SketchPreviewState = {
    draft: SketchDraftSource;
    artifactBundle: ArtifactBundle;
  };
  type SketchPreviewDraftState = {
    scopeId: string | null;
    savedAt: number | null;
  };
  type SketchViewportStatus = {
    title: string;
    verdict: string;
    detail: string;
    backend: string;
    artifactName: string;
  };

  function formatAgentPhase(phase: string): string {
    return phase.replace(/_/g, ' ').toUpperCase();
  }

  function addOptimisticQueuedAgentMessage(
    threadId: string,
    prompt: string,
    attachments: Attachment[],
    id = `optimistic-queued-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
  ): string {
    optimisticQueuedAgentMessages = {
      ...optimisticQueuedAgentMessages,
      [id]: {
        threadId,
        message: buildOptimisticQueuedDialogueMessage({
          id,
          prompt,
          attachments,
        }),
      },
    };
    return id;
  }

  function confirmOptimisticQueuedAgentMessage(
    optimisticId: string | null,
    threadId: string,
    messageId: string,
  ) {
    if (!optimisticId) return;
    const optimistic = optimisticQueuedAgentMessages[optimisticId];
    if (!optimistic) return;
    const next = { ...optimisticQueuedAgentMessages };
    delete next[optimisticId];
    next[messageId] = {
      threadId,
      message: {
        ...optimistic.message,
        id: messageId,
      },
    };
    optimisticQueuedAgentMessages = next;
  }

  function removeOptimisticQueuedAgentMessage(optimisticId: string | null) {
    if (!optimisticId || !optimisticQueuedAgentMessages[optimisticId]) return;
    const next = { ...optimisticQueuedAgentMessages };
    delete next[optimisticId];
    optimisticQueuedAgentMessages = next;
  }

  function shouldSuppressOnboardingForAutomation(): boolean {
    if (typeof navigator === 'undefined') return false;
    return Boolean(navigator.webdriver);
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

  function toAssetUrl(path: string | null | undefined): string {
    if (!path) return '';
    try {
      return convertFileSrc(path);
    } catch {
      return path;
    }
  }

  function fileBasename(path: string | null | undefined): string {
    if (!path) return '';
    return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
  }

  async function openVersionCodeModal(seed?: {
    code?: string;
    title?: string;
    sourceLanguage?: SourceLanguage | null;
    geometryBackend?: GeometryBackend | null;
  }) {
    const shouldReopenDocs = $windowStore.docs.visible;
    if (!$activeThreadId) {
      createNewThread({ mode: 'blank' });
      await tick();
      if (shouldReopenDocs) {
        showWindow('docs');
        await tick();
      }
    }

    const current = get(workingCopy);
    const hasSeedCode = seed ? Object.prototype.hasOwnProperty.call(seed, 'code') : false;
    const nextCode = hasSeedCode ? seed?.code ?? '' : current.macroCode;
    const nextTitle = seed?.title ?? (current.title || 'Manual Edit');
    const nextSourceLanguage = seed?.sourceLanguage ?? current.sourceLanguage ?? 'legacyPython';
    const nextGeometryBackend = seed?.geometryBackend ?? current.geometryBackend ?? 'freecad';

    if (
      nextCode !== current.macroCode ||
      nextTitle !== current.title ||
      nextSourceLanguage !== current.sourceLanguage ||
      nextGeometryBackend !== current.geometryBackend
    ) {
      workingCopy.patch({
        title: nextTitle,
        macroCode: nextCode,
        sourceLanguage: nextSourceLanguage,
        geometryBackend: nextGeometryBackend,
        dirty: false,
      });
      paramPanelState.hydrate({
        versionId: current.sourceVersionId,
        uiSpec: current.uiSpec,
        params: current.params,
      });
    }

    codeModalMode = 'version';
    codeModalSourceLanguage = nextSourceLanguage;
    selectedCode.set(nextCode);
    selectedTitle.set(codeInspectorTitle(nextTitle, nextSourceLanguage, nextGeometryBackend));
    showWindow('code');
  }

  function closeCodeModal() {
    closeWindowStore('code');
  }

  function openDocsSnippetInCode(snippet: string, title: string) {
    void openVersionCodeModal({
      code: snippet,
      title,
      sourceLanguage: 'ecky',
      geometryBackend: 'build123d',
    });
  }

  function handleDockCodeToggle() {
    if ($windowStore.code.visible) {
      closeCodeModal();
      return;
    }
    void openVersionCodeModal();
  }

  // Local reactive aliases for templates
  const phase = $derived($session.phase);
  const status = $derived($session.status);
  const error = $derived($session.error);
  const stlUrl = $derived($session.stlUrl);
  const runtimeRevision = $derived($session.runtimeRevision);
  const activeArtifactBundle = $derived($session.artifactBundle);
  const sessionModelManifest = $derived($session.modelManifest);
  let selectedContextTargetId = $state<string | null>(null);
  let sharedContextSearchQuery = $state('');
  let focusedMeasurementControl = $state<MeasurementControlFocus | null>(null);
  let lastViewportContextKey = $state<string | null>(null);
  let viewerOutlineEnabled = $state(true);
  let viewerTopologyMode = $state<TopologyMode>('mesh');
  let viewerMode = $state<'orbit' | 'select' | 'measure'>('orbit');
  let showNewProjectChooser = $state(false);
  let showNewProjectImport = $state(false);
  let sketchPreview = $state<SketchPreviewState | null>(null);
  let sketchPreviewDraft = $state<SketchPreviewDraftState | null>(null);
  let codeModalMode = $state<'version' | 'sketch-preview' | 'docs-snippet'>('version');
  let codeModalSourceLanguage = $state<SourceLanguage | null>(null);
  const enableViewportContextOverlay = false;
  let activeDraftFeedback = $state<AgentDraftFeedback | null>(null);
  const LIVE_APPLY_DEBOUNCE_MS = 250;
  let liveApplyTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingLiveApplyParams: DesignParams = {};
  let pendingLiveApplySourceKey: string | null = null;

  const isBooting = $derived(phase === 'booting');
  const isQuestionFlow = $derived(phase === 'answering');
  const isMcpConnection = $derived(usesMcpConnection($config.connectionType));
  const isActiveMcpMode = $derived(usesActiveMcpMode($config.connectionType, $config.mcp.mode));
  const usesQueuedAgentDialogue = $derived.by<boolean>(() =>
    usesAgentDialogueMode($config.connectionType, threadAgentState),
  );
  let activeAgentSessions = $state<AgentSession[]>([]);
  let threadAgentState = $state<ThreadAgentState | null>(null);
  const primaryAgentId = $derived.by<string | null>(() =>
    derivePrimaryAgentId($config.mcp.autoAgents ?? [], $config.mcp.primaryAgentId ?? null),
  );
  const primaryAgentLabel = $derived.by<string | null>(() =>
    $config.mcp.autoAgents.find((agent) => agent.id === primaryAgentId)?.label ?? null,
  );
  const visibleAgentTerminal = $derived($visibleAgentTerminalStore);
  const activeAgentTerminalAttention = $derived($agentTerminalAttentionStore);
  const activeThread = $derived($history.find((t) => t.id === $activeThreadId));
  let optimisticQueuedAgentMessages = $state<Record<string, OptimisticQueuedDialogueMessage>>({});
  const activeThreadDialogueMessages = $derived.by(() =>
    mergeOptimisticQueuedDialogueMessages(
      deriveOptimisticDialogueMessages(activeThread?.messages ?? [], $activeThreadRequests),
      Object.values(optimisticQueuedAgentMessages),
      $activeThreadId ?? null,
    ),
  );

  $effect(() => {
    const threadId = $activeThreadId;
    const messages = activeThread?.messages ?? [];
    if (!threadId || !messages.length) return;
    const persistedIds = new Set(messages.map((message) => message.id));
    const next = { ...optimisticQueuedAgentMessages };
    let changed = false;
    for (const [key, optimistic] of Object.entries(optimisticQueuedAgentMessages)) {
      if (optimistic.threadId === threadId && persistedIds.has(optimistic.message.id)) {
        delete next[key];
        changed = true;
      }
    }
    if (changed) optimisticQueuedAgentMessages = next;
  });
  const hasLiveApiConnection = $derived.by(() =>
    hasLiveApiEngineConnection($config.connectionType, selectedEngine),
  );
  const activeVersionMessage = $derived.by<Message | null>(() => {
    if (!activeThread) return null;
    return (
      activeThread.messages.find(
        (message) =>
          message.id === $activeVersionId &&
          isRenderableVersionTimelineMessage(message),
      ) ?? null
    );
  });
  let cameraStateByTarget = $state<Record<string, ViewportCameraState>>({});
  type PendingAgentPrompt = {
    requestId: string;
    message: string | null;
    agentLabel: string;
    sessionId: string;
    threadId?: string | null;
    messageId?: string | null;
    modelId?: string | null;
  };
  type ClosedAgentPrompt = {
    requestId: string;
    sessionId: string;
    threadId?: string | null;
    reason: string;
  };
  let pendingAgentPrompts = $state<PendingAgentPrompt[]>([]);
  // Plain Set (non-reactive) — mutations must not re-trigger the drain effect.
  const autoDrainingPromptRequestIds = new Set<string>();
  let pendingViewportScreenshotChoices = $state<PendingViewportScreenshotChoice[]>([]);

  let activeControlViewId = $state<string | null>(null);
  let activePreviewViewId = $state<string | null>(null);
  const contextState = $derived.by(() =>
    deriveContextState({
      activeArtifactBundle,
      activeControlViewId,
      focusedMeasurementControl,
      paramUiSpec: $paramPanelState.uiSpec || null,
      paramValues: $paramPanelState.params || {},
      selectedContextTargetId,
      selectedPartId: $session.selectedPartId ?? null,
      sessionModelManifest,
    }),
  );
  const effectiveUiSpec = $derived.by<UiSpec>(() => contextState.effectiveUiSpec);
  const effectiveParameters = $derived.by<DesignParams>(() => contextState.effectiveParameters);
  const activeModelManifest = $derived.by(() => contextState.activeModelManifest);
  const contextSelectionTargets = $derived.by<ContextSelectionTarget[]>(
    () => contextState.contextSelectionTargets,
  );
  const selectedTarget = $derived.by<ContextSelectionTarget | null>(
    () => contextState.selectedTarget,
  );
  const selectedPartId = $derived.by(() => contextState.selectedPartId);
  const importedPreviewTransforms = $derived.by<Record<string, ImportedPreviewTransform>>(
    () => contextState.importedPreviewTransforms,
  );
  const availablePreviewViews = $derived.by(() => activeModelManifest?.previewViews ?? []);
  const activePreviewView = $derived.by(
    () => resolveActivePreviewView(activeModelManifest, activePreviewViewId),
  );
  const authoredPreviewTransforms = $derived.by<Record<string, ImportedPreviewTransform>>(
    () => buildPreviewViewTransforms(activeModelManifest, activePreviewViewId),
  );
  const effectivePreviewTransforms = $derived.by<Record<string, ImportedPreviewTransform>>(
    () => mergePreviewTransforms(importedPreviewTransforms, authoredPreviewTransforms),
  );
  const overlaySelectedPart = $derived.by(() => contextState.overlaySelectedPart);
  const overlayPreviewOnly = $derived.by(() => contextState.overlayPreviewOnly);
  const availableControlViews = $derived.by<MaterializedSemanticView[]>(
    () => contextState.availableControlViews,
  );
  const activeControlView = $derived.by(() => contextState.activeControlView);
  const overlayControls = $derived.by(() => contextState.overlayControls);
  const overlayAdvisories = $derived.by(() => contextState.overlayAdvisories);
  const activeMeasurementCallout = $derived.by(() => contextState.activeMeasurementCallout);
  $effect(() => {
    activeControlViewId = contextState.resolvedActiveControlViewId;
  });
  $effect(() => {
    activePreviewViewId = activePreviewView?.viewId ?? null;
  });
  const suppressViewportBusyUi = $derived(isBooting);
  let showEnrichmentModal = $state(false);
  let showExportChooser = $state(false);
  const enrichmentManifest = $derived.by(() => {
    if (!showEnrichmentModal) return null;
    const m = sessionModelManifest;
    if (!m || m.sourceKind !== 'importedFcstd') return null;
    if (m.enrichmentState?.status !== 'pending') return null;
    return m;
  });
  const viewerBusyState = $derived.by(() =>
    deriveViewerBusyState({
      activeThreadId: $activeThreadId ?? null,
      activeVersionId: $activeVersionId ?? null,
      activeModelId: activeArtifactBundle?.modelId ?? null,
      activeThreadRequests: $activeThreadRequests,
      activeAgentSessions,
      threadAgentState,
      phase,
      isManual: $session.isManual,
      manualThreadId: $session.manualThreadId ?? null,
      manualMessageId: $session.manualMessageId ?? null,
      repairMessage: $session.repairMessage ?? null,
      cookingPhrase: $session.cookingPhrase ?? null,
      hasRenderableModel,
      suppressViewportBusyUi,
    }),
  );
  const showViewerBusyMask = $derived(viewerBusyState.showViewerBusyMask);
  const viewerBusyPhase = $derived<ViewerBusyPhase>(viewerBusyState.viewerBusyPhase);
  const viewerBusyText = $derived<string | null>(viewerBusyState.viewerBusyText);

  const viewportState = $derived.by(() =>
    deriveViewportState({
      activeArtifactBundle,
      activeThreadId: $activeThreadId ?? null,
      activeThreadMessages: activeThreadDialogueMessages,
      activeVersionId: $activeVersionId ?? null,
      activeVersionMessage,
      cameraStateByTarget,
      runtimeRevision,
      stlUrl,
      toAssetUrl,
    }),
  );
  const viewerAssets = $derived.by<ViewerAsset[]>(() => viewportState.viewerAssets);
  const hasSketchPreview = $derived(Boolean(sketchPreview?.artifactBundle));
  const sketchPreviewStlUrl = $derived.by<string | null>(() =>
    sketchPreview?.artifactBundle ? toAssetUrl(sketchPreview.artifactBundle.previewStlPath) : null,
  );
  const sketchPreviewViewerAssets = $derived.by<ViewerAsset[]>(() =>
    sketchPreview?.artifactBundle
      ? viewerAssetsToUrls(sketchPreview.artifactBundle.viewerAssets ?? [])
      : [],
  );
  const sketchPreviewStatus = $derived.by<SketchViewportStatus | null>(() => {
    if (!sketchPreview?.artifactBundle) return null;

    const warnings = sketchPreview.draft.warnings ?? [];
    const warningText = warnings.join(' ').toLowerCase();
    const isPreviewHull =
      warningText.includes('preview hull') ||
      sketchPreview.artifactBundle.modelId.toLowerCase().includes('preview-hull');

    return {
      title: isPreviewHull ? 'PREVIEW HULL' : 'SKETCH PREVIEW',
      verdict: 'NOT ACCEPTED CAD',
      detail: 'Diagnostic mesh from sketch evidence. Accepted CAD needs exact BRep/STEP validation.',
      backend: (sketchPreview.artifactBundle.geometryBackend ?? 'unknown').toUpperCase(),
      artifactName: fileBasename(sketchPreview.artifactBundle.previewStlPath) || 'preview.stl',
    };
  });
  const sketchPreviewDraftLabel = $derived.by<string | null>(() => {
    if (!sketchPreviewDraft) return null;
    return sketchPreviewDraft.savedAt ? 'DRAFT SAVED' : 'DRAFT ACTIVE';
  });
  const effectiveViewerStlUrl = $derived.by<string | null>(() =>
    sketchPreview?.artifactBundle ? sketchPreviewStlUrl : ($activeThreadId ? stlUrl : null),
  );
  const effectiveViewerAssets = $derived.by<ViewerAsset[]>(() =>
    sketchPreview?.artifactBundle ? sketchPreviewViewerAssets : viewerAssets,
  );
  const hasRenderableModel = $derived.by(() => viewportState.hasRenderableModel);
  const currentViewportTargetKey = $derived.by<string | null>(
    () => viewportState.currentViewportTargetKey,
  );
  const currentViewerModelKey = $derived.by<string | null>(
    () => viewportState.currentViewerModelKey,
  );
  const effectiveViewerModelKey = $derived.by<string | null>(() =>
    sketchPreview?.artifactBundle
      ? [
          'sketch-preview',
          sketchPreview.artifactBundle.modelId,
          sketchPreview.artifactBundle.artifactVersion ?? '',
          sketchPreview.artifactBundle.contentHash ?? '',
        ].join(':')
      : currentViewerModelKey,
  );
  const persistedViewportCameraState = $derived.by<ViewportCameraState | null>(
    () => viewportState.persistedViewportCameraState,
  );
  const activeVersionAgentLabel = $derived(viewportState.activeVersionAgentLabel);

  const agentOpsState = $derived.by(() =>
    deriveAgentOpsState({
      activeAgentSessions,
      activeThreadId: $activeThreadId ?? null,
      activeThreadRequests: $activeThreadRequests,
      activeVersionId: $activeVersionId ?? null,
      autoAgents: $config.mcp.autoAgents ?? [],
      connectionType: $config.connectionType,
      cookingPhrase: $session.cookingPhrase ?? null,
      hasRenderableModel,
      mcpMode: $config.mcp.mode,
      nowSecs: $nowSeconds,
      pendingAgentPrompts,
      pendingViewportScreenshotChoices,
      primaryAgentId: $config.mcp.primaryAgentId ?? null,
      primaryAgentLabel,
      suppressViewportBusyUi,
      threadAgentState,
      visibleAgentTerminal,
    }),
  );
  const activePendingAgentPrompt = $derived.by(() => agentOpsState.activePendingAgentPrompt);
  const threadAttentionIds = $derived.by(() => agentOpsState.threadAttentionIds);
  const activeViewportScreenshotChoice = $derived.by(() => agentOpsState.activeViewportScreenshotChoice);
  const activeMcpBusy = $derived.by(() => agentOpsState.activeMcpBusy);
  const activeMcpRenderBusy = $derived.by(() => agentOpsState.activeMcpRenderBusy);
  const activeMcpBubbleSummary = $derived.by(() => agentOpsState.activeMcpBubbleSummary);
  const activeAgentTerminalMetaSummary = $derived.by(() => agentOpsState.activeAgentTerminalMetaSummary);
  const activeDraftFeedbackSummary = $derived.by(() => {
    const visibleFeedback = isVisibleAgentDraftFeedback(
      activeDraftFeedback,
      $activeThreadId,
      $activeVersionId,
    )
      ? activeDraftFeedback
      : null;
    return composeAgentDraftFeedbackBubbleText({
      feedback: visibleFeedback,
      fallbackAuthoringLints:
        (threadAgentState as ThreadAgentStateWithAuthoringLints | null)?.authoringLints ?? [],
    });
  });
  const hasLiveMcpSession = $derived.by(() => agentOpsState.hasLiveMcpSession);
  const isAudioMuted = $derived(Boolean($config?.microwave?.muted));
  const dialogueState = $derived.by<DialogueState>(() => {
    return deriveDialogueState(activePendingAgentPrompt, usesQueuedAgentDialogue);
  });

  const exportState = $derived.by(() =>
    deriveExportState({
      activeArtifactBundle,
      activeThreadTitle: activeThread?.title ?? null,
      activeVersionMessage,
      runtimeCapabilities: $runtimeCapabilities,
    }),
  );
  const exportModelTitle = $derived.by(() => exportState.exportModelTitle);
  const exportDefaultNames = $derived.by(() => exportState.exportDefaultNames);
  const exportOptions = $derived.by(() => exportState.exportOptions);
  const hasMultipartExportModel = $derived.by(() => exportState.hasMultipartExportModel);
  const multipartExportParts = $derived.by(() => exportState.multipartExportParts);
  const canExportModel = $derived.by(() => exportState.canExportModel);
  const viewportCodeWorkingCopyAligned = $derived.by(
    () =>
      Boolean(
        $workingCopy.macroCode &&
          (!activeVersionMessage || $workingCopy.sourceVersionId === activeVersionMessage.id),
      ),
  );
  let viewerComponent = $state<ViewerHandle | null>(null);
  let hiddenViewerComponent = $state<ViewerHandle | null>(null);
  let drawingOverlay = $state<DrawingOverlayHandle | null>(null);
  let overlayActionsEl = $state<HTMLElement | null>(null);
  let genieSafeRightInset = $state(360);
  let drawingOverlayDirty = $state(false);
  let viewportAreaEl = $state<HTMLElement | null>(null);
  let hiddenViewerSpec = $state<HiddenViewerSpec | null>(null);
  let visibleViewerLoadNonce = $state(0);
  let hiddenViewerLoadNonce = $state(0);
  let visibleViewerRecoveryKey = $state<string | null>(null);
  let versionPreviewCaptureSeq = 0;
  let lastLiveScreenshotByTarget = $state<Record<string, ViewportScreenshotCapture>>({});
  let drawMode = $state(false);
  let workspaceCapturePrefs = $state<Record<string, boolean>>(readWorkspaceCapturePrefs());
  let lastAssistantMessageId = $state<string | null>(null);
  let lastSpokenAssistantKey = $state('');
  let lastAdvisorBubble = $state('');
  let lastAdvisorQuestion = $state('');
  let dismissedBubbleText = $state('');
  let agentControlBusy = $state(false);

  $effect(() => {
    if (!overlayActionsEl || typeof ResizeObserver === 'undefined' || typeof window === 'undefined') {
      genieSafeRightInset = 360;
      return;
    }

    const measure = () => {
      const width = overlayActionsEl?.getBoundingClientRect().width ?? 0;
      genieSafeRightInset = Math.max(220, Math.ceil(width) + 28);
    };

    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(overlayActionsEl);
    window.addEventListener('resize', measure);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', measure);
    };
  });

  let genieWakeUpCount = $state(0);
  let genieSeedOverrides = $state<Record<string, number>>(readGenieSeedOverrides());
  let lastAgentPresenceConnected = false;
  let threadAgentPollInterval: ReturnType<typeof setInterval> | null = null;
  const terminalWindowState = $derived($windowStore.terminal);
  const codeWindowState = $derived($windowStore.code);
  const sketchWindowState = $derived($windowStore.sketch);
  const projectsWindowState = $derived($windowStore.projects);
  const paramsWindowState = $derived($windowStore.params);
  const dialogueWindowState = $derived($windowStore.dialogue);
  const docsWindowState = $derived($windowStore.docs);
  const settingsWindowState = $derived($windowStore.settings);
  const activityWindowState = $derived($windowStore.activity);
  let mountedWindows = $state<Record<WindowId, boolean>>({
    code: false,
    projects: false,
    params: false,
    dialogue: false,
    docs: false,
    settings: false,
    terminal: false,
    sketch: false,
    activity: false,
  });
  $effect(() => {
    const s = $windowStore;
    for (const id of ['code', 'projects', 'params', 'dialogue', 'docs', 'settings', 'terminal', 'activity', 'sketch'] as WindowId[]) {
      if (s[id].visible) {
        mountedWindows[id] = true;
      }
    }
  });

  $effect(() => {
    if (mountedWindows.params || isBooting || $currentView !== 'workbench') return;
    if (typeof window === 'undefined') return;
    const mountParams = () => {
      mountedWindows.params = true;
    };
    if ('requestIdleCallback' in window) {
      const idleId = window.requestIdleCallback(mountParams, { timeout: 1500 });
      return () => window.cancelIdleCallback(idleId);
    }
    const timerId = setTimeout(mountParams, 600);
    return () => clearTimeout(timerId);
  });

  let agentTerminalInput = $state('');
  let agentTerminalSurface = $state<{ focusTerminal: () => void } | null>(null);
  let lastAgentTerminalFocusKey = $state('');
  let lastFocusedAgentWorkingVersionKey = $state('');
  let activeMcpMicrowaveKey = $state('');
  let ownsMcpPhraseLoop = $state(false);

  async function collectQueuedThreadBatch(threadId: string): Promise<{
    messageIds: string[];
    promptText: string;
    attachments: Attachment[];
  } | null> {
    const thread = get(history).find((candidate) => candidate.id === threadId);
    if (!thread) return null;
    const queuedMessages = thread.messages
      .filter((message) => message.role === 'user' && message.status === 'pending')
      .map((message, index) => ({ message, index }))
      .sort((left, right) => {
        if (left.message.timestamp !== right.message.timestamp) {
          return left.message.timestamp - right.message.timestamp;
        }
        return left.index - right.index;
      })
      .map(({ message }) => message);
    if (!queuedMessages.length) return null;

    const attachmentGroups = await Promise.all(
      queuedMessages.map((message) => getMessageAttachments(message.id).catch(() => [])),
    );
    const attachmentMap = new Map<string, Attachment>();
    for (const attachments of attachmentGroups) {
      for (const attachment of attachments) {
        const key = `${attachment.path}::${attachment.dataUrl ?? ''}::${attachment.name}`;
        if (!attachmentMap.has(key)) {
          attachmentMap.set(key, attachment);
        }
      }
    }

    return {
      messageIds: queuedMessages.map((message) => message.id),
      promptText: queuedMessages.map((message) => message.content).join('\n\n'),
      attachments: [...attachmentMap.values()],
    };
  }

  // Auto-deliver the full queued batch whenever an agent opens a live prompt for that thread.
  // Also re-runs when $history changes so it can retry if messages arrive after the prompt opened.
  $effect(() => {
    void $history; // reactive dep — retriggers when messages arrive
    // For each threadId, only drain the newest prompt (last in array) to avoid draining stale ones.
    const newestPerThread = new Map<string, PendingAgentPrompt>();
    for (const prompt of pendingAgentPrompts) {
      if (prompt.threadId) {
        newestPerThread.set(prompt.threadId, prompt);
      }
    }
    const deliverablePrompts = [...newestPerThread.values()].filter(
      (prompt) => !autoDrainingPromptRequestIds.has(prompt.requestId),
    );
    for (const prompt of deliverablePrompts) {
      autoDrainingPromptRequestIds.add(prompt.requestId);
      void (async () => {
        try {
          const batch = await collectQueuedThreadBatch(prompt.threadId ?? '');
          if (!batch) return;
          pendingAgentPrompts = pendingAgentPrompts.filter(
            (candidate) => candidate.requestId !== prompt.requestId,
          );
          await resolveAgentPrompt({
            requestId: prompt.requestId,
            promptText: batch.promptText,
            messageIds: batch.messageIds,
            messageId: batch.messageIds[0] ?? null,
            attachments: batch.attachments,
          });
        } catch (error) {
          session.setError(`Agent Prompt Error: ${formatBackendError(error)}`);
        } finally {
          autoDrainingPromptRequestIds.delete(prompt.requestId);
        }
      })();
    }
  });

  $effect(() => {
    const nextConnectionState = threadAgentState?.connectionState ?? 'none';
    const nextPresenceConnected =
      hasLiveAgentSession(activeAgentSessions) ||
      ['waking', 'waiting', 'active'].includes(nextConnectionState);
    if (nextPresenceConnected && !lastAgentPresenceConnected) {
      genieWakeUpCount++;
    }
    lastAgentPresenceConnected = nextPresenceConnected;
  });

  function shortSessionId(sessionId: string | null | undefined): string {
    if (!sessionId) return 'NO SESSION';
    return sessionId.slice(0, 8);
  }

  async function sendAgentTerminalPayload(
    payload: AgentTerminalInput | null,
    options?: { clearComposer?: boolean; refocusTerminal?: boolean },
  ) {
    if (!payload) return;
    try {
      await sendAgentTerminalInput(payload);
      if (options?.clearComposer) {
        agentTerminalInput = '';
      }
      if (options?.refocusTerminal) {
        await tick();
        agentTerminalSurface?.focusTerminal();
      }
    } catch (error) {
      session.setError(`Agent Terminal Error: ${formatBackendError(error)}`);
    }
  }

  async function submitAgentTerminalInput(forceEnter = false) {
    if (!visibleAgentTerminal) return;
    const payload = forceEnter
      ? buildAgentTerminalKeyInput(visibleAgentTerminal.agentId, {
          key: 'Enter',
          ctrlKey: false,
          altKey: false,
          shiftKey: false,
          metaKey: false,
        })
      : buildAgentTerminalLineInput(visibleAgentTerminal.agentId, agentTerminalInput, true);
    await sendAgentTerminalPayload(payload, {
      clearComposer: !forceEnter,
      refocusTerminal: true,
    });
  }

  async function handleAgentTerminalRawInput(data: string) {
    if (!visibleAgentTerminal?.active || !data.length) return;
    await sendAgentTerminalPayload(
      {
        agentId: visibleAgentTerminal.agentId,
        text: data,
        key: null,
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
        submit: false,
      },
      { refocusTerminal: false },
    );
  }

  async function handleAgentTerminalResize(agentId: string, cols: number, rows: number) {
    try {
      await resizeAgentTerminal(agentId, cols, rows);
    } catch (error) {
      session.setError(`Agent Terminal Error: ${formatBackendError(error)}`);
    }
  }

  async function handleNudgeAgentPromptRearm() {
    if (!visibleAgentTerminal?.active) return;
    await sendAgentTerminalPayload(
      buildAgentTerminalLineInput(
        visibleAgentTerminal.agentId,
        'Call `request_user_prompt` now so Ecky can queue the next user message.',
        true,
      ),
      { refocusTerminal: true },
    );
  }

  // Wake animation fires when the selected active agent asks for a prompt.
  // No startup pre-waking — genie should be idle until the primary agent actually greets.

  const requestOrchestratorUiDeps = {
    get viewerComponent() { return viewerComponent; },
    openCodeModalManual: (data: DesignOutput) => {
      const seededDraft = buildFailedDraftSeed(data, $workingCopy);
      workingCopy.loadVersion(seededDraft, null);
      paramPanelState.hydrateFromVersion(seededDraft, null);
      codeModalMode = 'version';
      codeModalSourceLanguage = seededDraft.sourceLanguage;
      selectedCode.set(seededDraft.macroCode);
      selectedTitle.set(
        codeInspectorTitle(
          seededDraft.title,
          seededDraft.sourceLanguage,
          seededDraft.geometryBackend,
        ),
      );
      showWindow('code');
    },
    getDrawingCanvas: () => drawingOverlay?.hasDrawing() ? drawingOverlay.getCanvas() : null,
    clearDrawing: () => { drawingOverlay?.clear(); drawMode = false; },
  };

  type ViewerLoadWaiter = {
    targetNonce: number;
    resolve: () => void;
    reject: (error: Error) => void;
    timer: ReturnType<typeof setTimeout>;
  };
  let visibleViewerWaiters: ViewerLoadWaiter[] = [];
  let hiddenViewerWaiters: ViewerLoadWaiter[] = [];

  function settleViewerLoadWaiters(
    waiters: ViewerLoadWaiter[],
    currentNonce: number,
  ): ViewerLoadWaiter[] {
    const pending: ViewerLoadWaiter[] = [];
    for (const waiter of waiters) {
      if (currentNonce >= waiter.targetNonce) {
        clearTimeout(waiter.timer);
        waiter.resolve();
      } else {
        pending.push(waiter);
      }
    }
    return pending;
  }

  function rejectViewerLoadWaiters(
    waiters: ViewerLoadWaiter[],
    error: Error,
  ): ViewerLoadWaiter[] {
    for (const waiter of waiters) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    return [];
  }

  function waitForViewerLoad(
    kind: 'visible' | 'hidden',
    previousNonce: number,
    timeoutMs = 12000,
  ): Promise<void> {
    const currentNonce = kind === 'visible' ? visibleViewerLoadNonce : hiddenViewerLoadNonce;
    if (currentNonce > previousNonce) {
      return Promise.resolve();
    }
    return new Promise<void>((resolve, reject) => {
      const waiter: ViewerLoadWaiter = {
        targetNonce: previousNonce + 1,
        resolve,
        reject,
        timer: setTimeout(() => {
          if (kind === 'visible') {
            visibleViewerWaiters = visibleViewerWaiters.filter((candidate) => candidate !== waiter);
          } else {
            hiddenViewerWaiters = hiddenViewerWaiters.filter((candidate) => candidate !== waiter);
          }
          reject(new Error(`Timed out waiting for the ${kind} viewer to load.`));
        }, timeoutMs),
      };
      if (kind === 'visible') {
        visibleViewerWaiters = [...visibleViewerWaiters, waiter];
      } else {
        hiddenViewerWaiters = [...hiddenViewerWaiters, waiter];
      }
    });
  }

  function handleVisibleViewerLoaded() {
    visibleViewerLoadNonce += 1;
    visibleViewerWaiters = settleViewerLoadWaiters(visibleViewerWaiters, visibleViewerLoadNonce);
    if (
      !hasSketchPreview &&
      shouldPersistVersionPreview(activeVersionMessage, get(session).artifactBundle, get(session).stlUrl)
    ) {
      void persistVisibleVersionPreview(visibleViewerLoadNonce);
    }
  }

  function handleHiddenViewerLoaded() {
    hiddenViewerLoadNonce += 1;
    hiddenViewerWaiters = settleViewerLoadWaiters(hiddenViewerWaiters, hiddenViewerLoadNonce);
  }

  function handleVisibleViewerLoadError(message: string) {
    void recoverVisibleViewerRuntime(message);
  }

  function handleHiddenViewerLoadError(message: string) {
    hiddenViewerWaiters = rejectViewerLoadWaiters(
      hiddenViewerWaiters,
      new Error(`Hidden viewer failed to load model. ${message}`),
    );
  }

  function isMissingViewerArtifactError(message: string): boolean {
    const normalized = message.toLowerCase();
    return (
      normalized.includes('responded with 404') ||
      normalized.includes('not found') ||
      normalized.includes('status 404')
    );
  }

  async function recoverVisibleViewerRuntime(message: string) {
    visibleViewerWaiters = rejectViewerLoadWaiters(
      visibleViewerWaiters,
      new Error(`Visible viewer failed to load model. ${message}`),
    );

    const threadId = get(activeThreadId);
    const messageId = get(activeVersionId);
    const currentSession = get(session);
    const panel = get(paramPanelState);
    const wc = get(workingCopy);
    const bundle = currentSession.artifactBundle;
    const recoveryKey =
      threadId && messageId && bundle
        ? `${threadId}:${messageId}:${bundle.modelId}:${bundle.previewStlPath}`
        : null;

    if (
      recoveryKey &&
      isMissingViewerArtifactError(message) &&
      visibleViewerRecoveryKey !== recoveryKey
    ) {
      visibleViewerRecoveryKey = recoveryKey;
      session.setError(null);
      session.setStatus('Runtime artifact missing. Re-rendering cached model...');
      try {
        const recoverySource =
          activeVersionMessage?.id === messageId
            ? activeVersionMessage.output?.macroCode
            : wc.sourceVersionId === messageId
              ? wc.macroCode
              : '';
        const recoveryParams =
          activeVersionMessage?.id === messageId
            ? activeVersionMessage.output?.initialParams || {}
            : panel.params;
        await handleParamChange(recoveryParams, recoverySource || null, false);
        const repairedSession = get(session);
        const repairedBundle = repairedSession.artifactBundle;
        const repairedManifest = repairedSession.modelManifest;
        if (
          get(activeThreadId) === threadId &&
          get(activeVersionId) === messageId &&
          repairedBundle &&
          repairedManifest &&
          repairedBundle.modelId === repairedManifest.modelId
        ) {
          await updateVersionRuntime(messageId!, repairedBundle, repairedManifest);
          session.setStatus('Missing runtime rebuilt.');
          await refreshHistory();
          return;
        }
      } catch (error) {
        session.setError(`Runtime Rebuild Error: ${formatBackendError(error)}`);
        return;
      } finally {
        if (visibleViewerRecoveryKey === recoveryKey) {
          visibleViewerRecoveryKey = null;
        }
      }
    }

    session.setError(`Viewer Load Error: ${message}`);
  }

  function patchThreadMessagePreview(threadId: string, messageId: string, imageData: string) {
    history.update((items) =>
      items.map((thread) => {
        if (thread.id !== threadId) return thread;
        return {
          ...thread,
          messages: thread.messages.map((message) =>
            message.id === messageId ? { ...message, imageData } : message,
          ),
        };
      }),
    );
  }

  async function persistVisibleVersionPreview(loadNonce: number) {
    const threadId = get(activeThreadId);
    const messageId = get(activeVersionId);
    const bundle = get(session).artifactBundle;
    const stlUrlValue = get(session).stlUrl;
    if (!threadId || !messageId || !bundle || !stlUrlValue || !viewerComponent) return;
    const versionMessage = activeVersionMessage;
    if (!shouldPersistVersionPreview(versionMessage, bundle, stlUrlValue)) return;

    const captureSeq = ++versionPreviewCaptureSeq;
    await tick();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    if (
      captureSeq !== versionPreviewCaptureSeq ||
      loadNonce !== visibleViewerLoadNonce ||
      get(activeThreadId) !== threadId ||
      get(activeVersionId) !== messageId ||
      get(session).stlUrl !== stlUrlValue ||
      !sameArtifactVersion(versionMessage?.artifactBundle, get(session).artifactBundle)
    ) {
      return;
    }

    const imageData = viewerComponent?.captureScreenshot();
    if (!imageData?.startsWith('data:image/')) return;

    try {
      await updateVersionPreview(messageId, imageData, bundle);
      patchThreadMessagePreview(threadId, messageId, imageData);
      recordSessionActivityEvent({
        threadId,
        versionId: messageId,
        kind: 'preview_updated',
        title: 'Preview updated',
        summary: 'Viewport preview thumbnail persisted.',
        severity: 'success',
        artifacts: [
          {
            kind: 'preview_image',
            label: 'Viewport preview',
            value: imageData,
            mimeType: 'image/png',
          },
        ],
        raw: {
          modelId: bundle.modelId,
          artifactVersion: bundle.artifactVersion,
        },
      });
      window.dispatchEvent(
        new CustomEvent('ecky:version-preview-updated', {
          detail: { threadId, messageId, imageData },
        }),
      );
    } catch (error) {
      console.warn('Failed to persist version preview:', formatBackendError(error));
    }
  }

  function handleVisibleViewerCameraChange(nextCamera: ViewportCameraState) {
    if (hasSketchPreview) return;
    if (!currentViewportTargetKey) return;
    cameraStateByTarget = rememberTargetCameraState(
      cameraStateByTarget,
      currentViewportTargetKey,
      nextCamera,
      true,
    );
  }

  function liveOverlayCanvas(includeOverlays: boolean): HTMLCanvasElement | null {
    if (!includeOverlays) return null;
    return drawingOverlay?.hasDrawing() ? drawingOverlay.getCanvas() : null;
  }

  function currentVisibleTargetRef() {
    if (hasSketchPreview) return null;
    const threadId = get(activeThreadId);
    const messageId = get(activeVersionId);
    const modelId = get(session).artifactBundle?.modelId ?? null;
    if (!threadId || !messageId) return null;
    return { threadId, messageId, modelId };
  }

  const sendWorkspaceCaptureForActiveThread = $derived.by<boolean>(() =>
    isWorkspaceCaptureEnabled(workspaceCapturePrefs, $activeThreadId),
  );
  const workspaceCaptureHint = $derived.by<string | null>(() => {
    if (drawingOverlayDirty) {
      return 'Enabled automatically because the current viewport has annotated content.';
    }
    if (dialogueState.mode === 'generate') return null;
    if (sendWorkspaceCaptureForActiveThread) {
      return 'The current visible workspace will be attached as a reference image for this thread.';
    }
    return null;
  });

  function setWorkspaceCaptureForActiveThread(enabled: boolean) {
    const next = setWorkspaceCaptureEnabled(workspaceCapturePrefs, $activeThreadId, enabled);
    workspaceCapturePrefs = next;
    writeWorkspaceCapturePrefs(next);
  }

  function adoptWorkspaceCapturePreference(threadId: string) {
    if ($activeThreadId || !sendWorkspaceCaptureForActiveThread) return;
    const next = setWorkspaceCaptureEnabled(workspaceCapturePrefs, threadId, true);
    workspaceCapturePrefs = next;
    writeWorkspaceCapturePrefs(next);
  }

  function clearPromptDrawingOverlay() {
    drawingOverlay?.clear();
    drawingOverlayDirty = false;
    drawMode = false;
  }

  async function capturePromptWorkspaceImageData(): Promise<string | null> {
    if (!viewerComponent) return null;
    return viewerComponent.captureScreenshot(liveOverlayCanvas(true));
  }

  async function prepareMcpPromptAttachments(
    attachments: Attachment[],
    targetThreadId: string | null,
  ): Promise<{ attachments: Attachment[]; clearDrawingAfterSend: boolean }> {
    const hadDrawing = drawingOverlay?.hasDrawing() ?? drawingOverlayDirty;

    let nextAttachments = attachments;
    if (sendWorkspaceCaptureForActiveThread || hadDrawing) {
      const dataUrl = await capturePromptWorkspaceImageData();
      if (dataUrl) {
        const workspaceAttachment = await preparePromptWorkspaceCapture({
          dataUrl,
          threadId: targetThreadId,
          name: hadDrawing ? 'workspace-annotated.png' : 'workspace-view.png',
          explanation: hadDrawing
            ? 'Current workspace view with annotated content.'
            : 'Current workspace view.',
        });
        nextAttachments = [...attachments, workspaceAttachment];
      }
    }

    return {
      attachments: await preparePromptAttachments(nextAttachments),
      clearDrawingAfterSend: hadDrawing,
    };
  }

  function viewerAssetsToUrls(assets: ViewerAsset[]): ViewerAsset[] {
    return assets.map((asset) => ({
      ...asset,
      path: toAssetUrl(asset.path),
    }));
  }

  function handleSketchPreviewChange(preview: SketchPreviewState | null) {
    if (preview) {
      sketchPreview = preview;
      const scopeId = sketchPreviewDraft?.scopeId ?? null;
      if (!sketchPreviewDraft || sketchPreviewDraft.scopeId !== scopeId) {
        sketchPreviewDraft = { scopeId, savedAt: null };
      }
      void persistSketchPreviewDraft(scopeId, preview);
      return;
    }

    sketchPreview = preview;
  }

  function handleSketchManualPreviewResult(preview: SketchPreviewState | null) {
    return preview;
  }

  async function persistSketchPreviewDraft(scopeId: string | null, preview: SketchPreviewState) {
    try {
      await saveSketchPreviewDraft({
        draftScopeId: scopeId,
        draftSource: preview.draft,
        artifactBundle: preview.artifactBundle,
      });
    } catch (error) {
      console.warn('[Sketch] Failed to persist preview draft:', error);
    }
  }

  async function saveSketchPreviewDraftAsCurrentScope() {
    if (!sketchPreview) return;

    const scopeId = normalizeSketchPreviewDraftScopeId(sketchPreviewDraft?.scopeId ?? null);
    sketchPreviewDraft = { scopeId, savedAt: Date.now() };
    await persistSketchPreviewDraft(scopeId, sketchPreview);
    session.setStatus('Sketch draft saved.');
  }

  async function saveSketchPreviewDraftAsNewScope() {
    if (!sketchPreview) return;
    const scopeId = createSketchPreviewDraftScopeId();
    sketchPreviewDraft = { scopeId, savedAt: Date.now() };
    await persistSketchPreviewDraft(scopeId, sketchPreview);
    session.setStatus('Sketch draft saved.');
  }

  async function handleSketchSaveDraft(input: { newScope: boolean }) {
    if (input.newScope) {
      await saveSketchPreviewDraftAsNewScope();
      return;
    }
    await saveSketchPreviewDraftAsCurrentScope();
  }

  async function discardSketchPreviewDraft() {
    const scopeId = normalizeSketchPreviewDraftScopeId(sketchPreviewDraft?.scopeId ?? null);
    sketchPreview = null;
    sketchPreviewDraft = null;
    try {
      await clearSketchPreviewDraft({ draftScopeId: scopeId });
    } catch (error) {
      console.warn('[Sketch] Failed to clear preview draft:', error);
    }
    session.setStatus('Sketch draft discarded.');
  }

  function rememberVisibleViewportCapture(capture: ViewportScreenshotCapture) {
    if (!capture.threadId || !capture.messageId) return;
    const screenshotKey = viewportTargetKey(capture.threadId, capture.messageId);
    lastLiveScreenshotByTarget = rememberTargetScreenshot(
      lastLiveScreenshotByTarget,
      screenshotKey,
      capture,
    );
    const runtimeBundle =
      capture.threadId === get(activeThreadId) && capture.messageId === get(activeVersionId)
        ? get(session).artifactBundle
        : null;
    const cameraKey =
      capture.threadId === get(activeThreadId) &&
      capture.messageId === get(activeVersionId) &&
      currentViewportTargetKey
        ? currentViewportTargetKey
        : viewportCameraKey(
            capture.threadId,
            capture.messageId,
            runtimeBundle?.modelId ?? capture.modelId ?? null,
            runtimeBundle?.artifactVersion ?? null,
            runtimeBundle?.contentHash ?? null,
          );
    cameraStateByTarget = rememberTargetCameraState(
      cameraStateByTarget,
      cameraKey,
      capture.camera,
      true,
    );
  }

  async function sendViewportScreenshotReply(
    requestId: string,
    capture: ViewportScreenshotCapture,
  ) {
    await resolveAgentViewportScreenshot({
      requestId,
      dataUrl: capture.dataUrl,
      width: capture.width,
      height: capture.height,
      camera: capture.camera,
      source: capture.source ?? 'visible-live',
      threadId: capture.threadId ?? '',
      messageId: capture.messageId ?? '',
      modelId: capture.modelId ?? null,
      includeOverlays: capture.includeOverlays ?? false,
    });
  }

  async function rejectViewportScreenshotReply(requestId: string, error: unknown) {
    const message = typeof error === 'string' ? error : formatBackendError(error);
    try {
      await rejectAgentViewportScreenshot(requestId, message);
    } catch {
      // Ignore races with timeout cleanup on the backend side.
    }
  }

  function captureVisibleViewport(
    request: AgentViewportScreenshotEvent,
    source: string,
  ): ViewportScreenshotCapture | null {
    const visibleTarget = currentVisibleTargetRef();
    if (!viewerComponent || !visibleTarget) return null;
    const details = viewerComponent.captureScreenshotDetails(
      liveOverlayCanvas(request.includeOverlays),
    );
    if (!details) return null;
    const capture: ViewportScreenshotCapture = {
      dataUrl: details.dataUrl,
      width: details.width,
      height: details.height,
      camera: details.camera,
      capturedAt: Date.now(),
      source,
      threadId: visibleTarget.threadId,
      messageId: visibleTarget.messageId,
      modelId: visibleTarget.modelId,
      includeOverlays: request.includeOverlays,
    };
    rememberVisibleViewportCapture(capture);
    return capture;
  }

  async function captureHiddenTarget(
    request: AgentViewportScreenshotEvent,
    source: string,
  ): Promise<ViewportScreenshotCapture> {
    const targetKey = viewportTargetKey(request.threadId, request.messageId);
    const previousNonce = hiddenViewerLoadNonce;
    hiddenViewerSpec = null;
    await tick();
    hiddenViewerSpec = {
      requestId: request.requestId,
      targetKey,
      stlUrl: toAssetUrl(request.previewStlPath),
      viewerAssets: [],
    };
    await waitForViewerLoad('hidden', previousNonce, 60000);
    if (!hiddenViewerComponent) {
      throw new Error('Hidden viewer is unavailable.');
    }
    hiddenViewerComponent.setCameraState(request.camera ?? null);
    const details = hiddenViewerComponent.captureScreenshotDetails();
    if (!details) {
      throw new Error('Failed to capture the hidden target preview.');
    }
    return {
      dataUrl: details.dataUrl,
      width: details.width,
      height: details.height,
      camera: details.camera,
      capturedAt: Date.now(),
      source,
      threadId: request.threadId,
      messageId: request.messageId,
      modelId: request.modelId ?? null,
      includeOverlays: false,
    };
  }

  async function switchToViewportTarget(request: AgentViewportScreenshotEvent) {
    const previousNonce = visibleViewerLoadNonce;
    const thread = await getThread(request.threadId);
    upsertThreadInHistory(thread);
    const targetMessage =
      thread.messages.find(
        (message) =>
          message.id === request.messageId &&
          isRenderableVersionTimelineMessage(message),
      ) ?? null;
    if (!targetMessage) {
      throw new Error(`Target version ${request.messageId} is unavailable for screenshot capture.`);
    }
    activeThreadId.set(thread.id);
    currentView.set('workbench');
    await loadVersion(targetMessage);
    await waitForViewerLoad('visible', previousNonce);
  }

  function upsertThreadInHistory(thread: Awaited<ReturnType<typeof getThread>>) {
    history.update((items) => {
      const nextThread = { ...thread, messages: thread.messages };
      return items.some((candidate) => candidate.id === thread.id)
        ? items.map((candidate) => (candidate.id === thread.id ? nextThread : candidate))
        : [nextThread, ...items];
    });
  }

  async function focusAgentWorkingVersion(event: AgentWorkingVersionCreatedEvent) {
    const focusKey = `${event.sessionId}:${event.messageId}`;
    if (lastFocusedAgentWorkingVersionKey === focusKey) return;

    const thread = await getThread(event.threadId);
    upsertThreadInHistory(thread);
    const targetMessage =
      thread.messages.find(
        (message) =>
          message.id === event.messageId &&
          isRenderableVersionTimelineMessage(message),
      ) ?? null;
    if (!targetMessage) return;

    if (
      !shouldAutoFocusAgentWorkingVersion({
        currentView: get(currentView),
        activeThreadId: get(activeThreadId),
        eventThreadId: event.threadId,
      })
    ) {
      return;
    }

    lastFocusedAgentWorkingVersionKey = focusKey;
    activeThreadId.set(thread.id);
    currentView.set('workbench');
    await loadVersion(targetMessage);
    void refreshThreadAgentState();
  }

  async function processViewportScreenshotChoice(
    request: AgentViewportScreenshotEvent,
    choice: string,
  ) {
    const normalizedChoice = choice.trim().toLowerCase();
    if (normalizedChoice === 'cancel') {
      await rejectViewportScreenshotReply(request.requestId, 'Viewport screenshot cancelled by the user.');
      return;
    }

    if (normalizedChoice === 'current view') {
      const capture = captureVisibleViewport(request, 'current-view-mismatch');
      if (!capture) {
        await rejectViewportScreenshotReply(
          request.requestId,
          'Current view capture is unavailable because the workbench viewport is not visible.',
        );
        return;
      }
      await sendViewportScreenshotReply(request.requestId, capture);
      return;
    }

    if (normalizedChoice === 'switch & capture') {
      await switchToViewportTarget(request);
      const capture = captureVisibleViewport(request, 'switched-visible');
      if (!capture) {
        throw new Error('Switched to the target but failed to capture the visible viewport.');
      }
      await sendViewportScreenshotReply(request.requestId, capture);
      return;
    }

    if (normalizedChoice === 'fallback preview') {
      const targetKey = viewportTargetKey(request.threadId, request.messageId);
      const fallback = resolveFallbackScreenshotSource(lastLiveScreenshotByTarget, targetKey);
      if (fallback.kind === 'cached-live') {
        await sendViewportScreenshotReply(request.requestId, {
          ...fallback.capture,
          source: fallback.capture.source ?? 'cached-live',
          threadId: fallback.capture.threadId ?? request.threadId,
          messageId: fallback.capture.messageId ?? request.messageId,
          modelId: fallback.capture.modelId ?? request.modelId ?? null,
          includeOverlays: fallback.capture.includeOverlays ?? true,
        });
        return;
      }
      const capture = await captureHiddenTarget(request, request.camera ? 'hidden-target' : 'hidden-preview');
      await sendViewportScreenshotReply(request.requestId, capture);
      return;
    }

    await rejectViewportScreenshotReply(
      request.requestId,
      `Unsupported viewport screenshot choice: ${choice}`,
    );
  }

  async function handleViewportScreenshotEvent(request: AgentViewportScreenshotEvent) {
    try {
      const mode = chooseViewportCaptureMode({
        currentView: get(currentView),
        currentThreadId: get(activeThreadId),
        currentMessageId: get(activeVersionId),
        requestedThreadId: request.threadId,
        requestedMessageId: request.messageId,
        cameraOverride: request.camera ?? null,
        hasVisibleViewer: Boolean(
          viewerComponent &&
            get(currentView) === 'workbench' &&
            get(activeThreadId) &&
            get(activeVersionId),
        ),
      });

      if (mode === 'visible-live') {
        const capture = captureVisibleViewport(request, 'visible-live');
        if (!capture) {
          throw new Error('Visible viewport capture is unavailable.');
        }
        await sendViewportScreenshotReply(request.requestId, capture);
        return;
      }

      if (mode === 'hidden-target') {
        const capture = await captureHiddenTarget(request, 'hidden-target');
        await sendViewportScreenshotReply(request.requestId, capture);
        return;
      }

      const requestedLabel = `${request.threadId} / ${request.messageId}`;
      const message =
        'Agent wants a visual check, but the requested target is not the current live viewport. ' +
        `Choose what to send back for ${requestedLabel}.`;
      const nextChoice: PendingViewportScreenshotChoice = {
        ...request,
        message,
        buttons: ['Current View', 'Switch & Capture', 'Fallback Preview', 'Cancel'],
      };
      if (!pendingViewportScreenshotChoices.find((item) => item.requestId === request.requestId)) {
        pendingViewportScreenshotChoices = [...pendingViewportScreenshotChoices, nextChoice];
      }
    } catch (error) {
      await rejectViewportScreenshotReply(request.requestId, error);
    }
  }

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
    if (
      !isBooting &&
      !$config.hasSeenOnboarding &&
      !$onboarding.isActive &&
      !shouldSuppressOnboardingForAutomation()
    ) {
      onboarding.start();
    }
  });

  $effect(() => {
    if (!$onboarding.isActive || !$onboarding.windowIdToOpen) return;
    showWindow($onboarding.windowIdToOpen);
  });

  // Wire thread changes to audio focus
  $effect(() => {
    setAudibleThread($activeThreadId);
  });

  // Load window layout when thread changes
  $effect(() => {
    const threadId = $activeThreadId;
    if (threadId) {
      void loadLayoutForThread(threadId);
    }
  });

  $effect(() => {
    const nextMicrowaveKey =
      activeMcpRenderBusy && threadAgentState?.sessionId
        ? `__mcp__:${threadAgentState.sessionId}`
        : '';
    if (activeMcpMicrowaveKey && activeMcpMicrowaveKey !== nextMicrowaveKey) {
      stopMicrowaveHum(activeMcpMicrowaveKey);
    }
    if (nextMicrowaveKey && nextMicrowaveKey !== activeMcpMicrowaveKey) {
      startMicrowaveHum(nextMicrowaveKey, $config, $activeThreadId ?? null);
    }
    activeMcpMicrowaveKey = nextMicrowaveKey;
  });

  $effect(() => {
    const localThinkingActive = ['classifying', 'generating', 'answering'].includes(activeThreadHighestPhase);
    const shouldOwnPhraseLoop = isMcpConnection && activeMcpBusy && !localThinkingActive;
    if (shouldOwnPhraseLoop && !ownsMcpPhraseLoop) {
      startCookingPhraseLoop();
      ownsMcpPhraseLoop = true;
      return;
    }
    if (!shouldOwnPhraseLoop && ownsMcpPhraseLoop) {
      if (!localThinkingActive) {
        stopPhraseLoop();
      }
      ownsMcpPhraseLoop = false;
    }
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

  async function answerViewportScreenshotChoice(requestId: string, choice: string) {
    const request =
      pendingViewportScreenshotChoices.find((item) => item.requestId === requestId) ?? null;
    pendingViewportScreenshotChoices = pendingViewportScreenshotChoices.filter(
      (item) => item.requestId !== requestId,
    );
    if (!request) return;
    try {
      await processViewportScreenshotChoice(request, choice);
    } catch (error) {
      await rejectViewportScreenshotReply(requestId, error);
    }
  }

  async function answerAgentPrompt(requestId: string, promptText: string, attachments: Attachment[]) {
    const pendingPrompt =
      pendingAgentPrompts.find((prompt) => prompt.requestId === requestId) ?? null;
    pendingAgentPrompts = pendingAgentPrompts.filter((prompt) => prompt.requestId !== requestId);
    const promptThreadId = pendingPrompt?.threadId ?? null;
    if (!promptThreadId) {
      session.setError('Agent Prompt Error: pending prompt is not bound to a thread.');
      return;
    }
    let preparedAttachments: Attachment[] = attachments;
    let clearDrawingAfterSend = false;
    try {
      const prepared = await prepareMcpPromptAttachments(
        attachments,
        promptThreadId,
      );
      preparedAttachments = prepared.attachments;
      clearDrawingAfterSend = prepared.clearDrawingAfterSend;
      await resolveAgentPrompt({
        requestId,
        promptText,
        messageIds: [],
        messageId: null,
        attachments: preparedAttachments,
      });
      if (clearDrawingAfterSend) {
        clearPromptDrawingOverlay();
      }
    } catch (e) {
      const errorText = formatBackendError(e);
      if (
        errorText.includes('No pending prompt request') ||
        errorText.includes('timed out after')
      ) {
        let optimisticId: string | null = null;
        if (promptThreadId) {
          optimisticId = addOptimisticQueuedAgentMessage(
            promptThreadId,
            promptText,
            preparedAttachments,
          );
        }
        try {
          const queuedMessage = await queueAgentPrompt({
            threadId: promptThreadId,
            promptText,
            attachments: preparedAttachments,
          });
          if (!optimisticId) {
            optimisticId = addOptimisticQueuedAgentMessage(
              queuedMessage.threadId,
              promptText,
              preparedAttachments,
              queuedMessage.messageId,
            );
          } else {
            confirmOptimisticQueuedAgentMessage(
              optimisticId,
              queuedMessage.threadId,
              queuedMessage.messageId,
            );
          }
          adoptWorkspaceCapturePreference(queuedMessage.threadId);
          if (clearDrawingAfterSend) {
            clearPromptDrawingOverlay();
          }
          if ($activeThreadId !== queuedMessage.threadId) {
            activeThreadId.set(queuedMessage.threadId);
            activeVersionId.set(null);
          }
          await refreshHistory();
          session.setStatus(
            'No pending prompt request. Message queued in the thread for any agent to pick up.',
          );
        } catch (queueError) {
          removeOptimisticQueuedAgentMessage(optimisticId);
          session.setError(`Agent Queue Error: ${formatBackendError(queueError)}`);
        }
      } else {
        session.setError(`Agent Prompt Error: ${errorText}`);
      }
    }
    void refreshThreadAgentState();
  }

  async function handleDialogueSubmit(prompt: string, attachments: Attachment[]) {
    switch (dialogueState.mode) {
      case 'agent-reply': await answerAgentPrompt(dialogueState.requestId, prompt, attachments); break;
      case 'generate':    await handleGenerate(prompt, attachments, { uiDeps: requestOrchestratorUiDeps }); break;
      case 'mcp-idle': {
        let preparedAttachments: Attachment[] = attachments;
        let clearDrawingAfterSend = false;
        try {
          const prepared = await prepareMcpPromptAttachments(
            attachments,
            $activeThreadId ?? null,
          );
          preparedAttachments = prepared.attachments;
          clearDrawingAfterSend = prepared.clearDrawingAfterSend;
        } catch (e) {
          session.setError(`Attachment Import Error: ${formatBackendError(e)}`);
          break;
        }
        let queuedMessage: { threadId: string; messageId: string };
        let optimisticId: string | null = null;
        const optimisticThreadId = $activeThreadId ?? null;
        if (optimisticThreadId) {
          optimisticId = addOptimisticQueuedAgentMessage(
            optimisticThreadId,
            prompt,
            preparedAttachments,
          );
        }
        try {
          queuedMessage = await queueAgentPrompt({
            threadId: $activeThreadId ?? null,
            promptText: prompt,
            attachments: preparedAttachments,
          });
          if (!optimisticId) {
            optimisticId = addOptimisticQueuedAgentMessage(
              queuedMessage.threadId,
              prompt,
              preparedAttachments,
              queuedMessage.messageId,
            );
          } else {
            confirmOptimisticQueuedAgentMessage(
              optimisticId,
              queuedMessage.threadId,
              queuedMessage.messageId,
            );
          }
          adoptWorkspaceCapturePreference(queuedMessage.threadId);
          if (clearDrawingAfterSend) {
            clearPromptDrawingOverlay();
          }
        } catch (e) {
          removeOptimisticQueuedAgentMessage(optimisticId);
          session.setError(`Agent Queue Error: ${formatBackendError(e)}`);
          break;
        }
        if ($activeThreadId !== queuedMessage.threadId) {
          activeThreadId.set(queuedMessage.threadId);
          activeVersionId.set(null);
        }
        await refreshHistory();
        session.setStatus('Message queued for the agent.');
        if (isActiveMcpMode) {
          try {
            await persistBackendConfig(get(config));
          } catch (e) {
            session.setError(`Config Save Error: ${formatBackendError(e)}`);
            break;
          }
          try {
            const target = currentVisibleTargetRef();
            await wakePrimaryAutoAgent(
              queuedMessage.threadId,
              target?.threadId === queuedMessage.threadId ? target.messageId ?? null : null,
              target?.threadId === queuedMessage.threadId ? target.modelId ?? null : null,
            );
            await refreshThreadAgentState();
          } catch (e) {
            console.error('[MCP] Failed to wake the primary agent for a queued message:', e);
            session.setStatus(`Message queued. Wake attempt failed: ${formatBackendError(e)}`);
          }
        } else {
          void refreshThreadAgentState();
        }
        break;
      }
    }
  }

  async function handlePromptPanelSubmit(prompt: string, attachments: Attachment[]) {
    if (dialogueState.mode !== 'generate') {
      await handleDialogueSubmit(prompt, attachments);
      return;
    }
    if (generationUnavailableReason) {
      session.setError(`Render Error: ${generationUnavailableReason}`);
      return;
    }
    await handleGenerate(prompt, attachments, { uiDeps: requestOrchestratorUiDeps });
  }

  async function handlePromptPanelAuthoredVerifyFocus(message: Message, stableNodeId: string) {
    const requestedNodeId = stableNodeId.trim();
    if (!requestedNodeId || !isRenderableVersionTimelineMessage(message) || !$activeThreadId) return;
    await loadVersion(message, $activeThreadId);
    await tick();
    triggerMacroNodeFocus(requestedNodeId);
  }

  function startBlankProject() {
    showNewProjectChooser = false;
    createNewThread({ mode: 'blank' });
  }

  async function handleTopImportFcstd() {
    showNewProjectChooser = false;
    if (freecadUnavailableReason) {
      session.setError(`FCStd Import Error: ${freecadUnavailableReason}`);
      return;
    }
    const selected = await open({
      multiple: false,
      filters: [{ name: 'FreeCAD Document', extensions: ['fcstd'] }],
    });
    if (typeof selected === 'string' && selected.trim()) {
      handleImportFcstd(selected);
    }
  }

  function startMacroImport() {
    showNewProjectChooser = false;
    showNewProjectImport = true;
  }

  function handleTopMacroImport(data: { code: string; title: string }) {
    createNewThread({ mode: 'macro', ...data });
    showNewProjectImport = false;
  }

  onMount(() => {
    void (async () => {
      await boot();
      const snapshot = await loadSketchPreviewDraft({});
      if (snapshot) {
        sketchPreviewDraft = {
          scopeId: normalizeSketchPreviewDraftScopeId(snapshot.scopeId ?? null),
          savedAt: snapshot.updatedAt,
        };
        sketchPreview = {
          draft: snapshot.draftSource,
          artifactBundle: snapshot.artifactBundle,
        };
      }
    })();
    // Initial fetch of agent sessions (push events only fire on changes, not on load)
    void getActiveAgentSessions().then(sessions => { activeAgentSessions = sessions; }).catch(() => {});
    void getAgentTerminalSnapshots()
      .then((snapshots) => {
        replaceAgentTerminalSnapshots(snapshots);
      })
      .catch(() => {});
    threadAgentPollInterval = setInterval(() => void refreshThreadAgentState(), 1000);

    const noopUnlisten = Promise.resolve(() => {});
    const canListenToTauri = hasTauriIpc();

    const unlisten = canListenToTauri ? listen<AgentConfirmItem>('agent-confirm-request', (event) => {
      const item = event.payload;
      if (!pendingConfirms.find(c => c.requestId === item.requestId)) {
        pendingConfirms = [...pendingConfirms, item];
      }
    }) : noopUnlisten;
    const unlistenPrompt = canListenToTauri ? listen<PendingAgentPrompt>('agent-prompt-request', (event) => {
      // Replace any existing prompt for this session (supersede semantics), then append the new one.
      pendingAgentPrompts = [
        ...pendingAgentPrompts.filter((prompt) => prompt.sessionId !== event.payload.sessionId),
        event.payload,
      ];
      void refreshThreadAgentState();
    }) : noopUnlisten;
    const unlistenPromptClosed = canListenToTauri ? listen<ClosedAgentPrompt>('agent-prompt-closed', (event) => {
      const { requestId, sessionId, reason } = event.payload;
      if (reason === 'session_disconnected' || reason === 'superseded' || reason === 'agent_stopped') {
        // Broad cleanup: remove all prompts for this session.
        pendingAgentPrompts = pendingAgentPrompts.filter((prompt) => prompt.sessionId !== sessionId);
      } else {
        pendingAgentPrompts = pendingAgentPrompts.filter((prompt) => prompt.requestId !== requestId);
      }
      if (reason === 'timed_out') {
        session.setStatus(
          'No pending prompt request. The last request_user_prompt timed out; queued thread messages can still be picked up later.',
        );
      }
      void refreshThreadAgentState();
    }) : noopUnlisten;
    const unlistenViewportScreenshot = canListenToTauri ? listen<AgentViewportScreenshotEvent>(
      'agent-viewport-screenshot-request',
      (event) => {
        void handleViewportScreenshotEvent(event.payload);
      },
    ) : noopUnlisten;
    const unlistenHistory = canListenToTauri ? listen('history-updated', async () => {
      await refreshHistory();
      const currentThreadId = get(activeThreadId);
      if (currentThreadId) {
        const thread = await getThread(currentThreadId);
        upsertThreadInHistory(thread);
      }
      void refreshThreadAgentState();
    }) : noopUnlisten;
    const unlistenDraftPreview = canListenToTauri ? listen<AgentDraftPreviewUpdatedEvent>(
      'agent-draft-preview-updated',
      (event) => {
        const preview = event.payload;
        const previousThreadId = get(activeThreadId);
        const previewDesign = resolveDraftPreviewDesign({
          design: preview.design,
          previewThreadId: preview.threadId,
          activeThreadId: previousThreadId,
          currentParams: get(paramPanelState).params,
        });
        activeThreadId.set(preview.threadId);
        activeVersionId.set(preview.previewId);
        activeDraftFeedback = preview.feedback
          ? {
              ...preview.feedback,
              items: preview.feedback.items.map((item, index) =>
                typeof item === 'string'
                  ? { code: `feedback-${index + 1}`, message: item }
                  : item,
              ),
              authoringLints: preview.feedback.authoringLints ?? [],
              threadId: preview.threadId,
              previewId: preview.previewId,
              sessionId: preview.sessionId,
            }
          : null;
        workingCopy.loadVersion(previewDesign, preview.previewId);
        paramPanelState.hydrateFromVersion(previewDesign, preview.previewId);
        session.setStlUrl(toAssetUrl(preview.artifactBundle.previewStlPath));
        session.setModelRuntime(preview.artifactBundle, preview.modelManifest);
        recordSessionActivityEvent({
          threadId: preview.threadId,
          versionId: preview.previewId,
          sessionId: preview.sessionId ?? 'local-session',
          actor: {
            kind: 'agent',
            id: preview.sessionId ?? 'agent',
            label: threadAgentState?.agentLabel ?? 'Agent',
          },
          kind: preview.feedback ? 'validation_reported' : 'preview_updated',
          title: preview.feedback ? 'Preview validation reported' : 'Draft preview updated',
          summary: preview.feedback?.summary || 'Draft preview rendered.',
          severity: preview.feedback?.status === 'failed' ? 'error' : preview.feedback ? 'warning' : 'success',
          artifacts: [
            {
              kind: 'preview_file',
              label: 'Draft preview STL',
              value: preview.artifactBundle.previewStlPath ?? preview.artifactBundle.modelId,
              raw: {
                modelId: preview.artifactBundle.modelId,
                artifactVersion: preview.artifactBundle.artifactVersion,
              },
            },
          ],
          raw: preview.feedback ?? null,
        });
        session.setStatus(preview.feedback?.summary || 'Preview rendered.');
        void persistLastSessionSnapshot({
          design: previewDesign,
          threadId: preview.threadId,
          messageId: preview.previewId,
          artifactBundle: preview.artifactBundle,
          modelManifest: preview.modelManifest,
          selectedPartId: null,
        });
      },
    ) : noopUnlisten;
    const unlistenSessions = canListenToTauri ? listen<AgentSession[]>('agent-sessions-changed', (event) => {
      activeAgentSessions = event.payload;
      void refreshThreadAgentState();
    }) : noopUnlisten;
    const unlistenTerminal = canListenToTauri ? listen<AgentTerminalSnapshot>('agent-terminal-updated', (event) => {
      enqueueAgentTerminalSnapshot(event.payload);
    }) : noopUnlisten;
    const unlistenWorkingVersion = canListenToTauri ? listen<AgentWorkingVersionCreatedEvent>(
      'agent-working-version-created',
      (event) => {
        void focusAgentWorkingVersion(event.payload).catch((error) => {
          console.warn('[Agent] Failed to focus working version:', error);
        });
      },
    ) : noopUnlisten;
    return () => {
      teardownWindowStore();
      if (threadAgentPollInterval) clearInterval(threadAgentPollInterval);
      resetAgentTerminalStore();
      void unlisten.then(fn => fn());
      void unlistenPrompt.then(fn => fn());
      void unlistenPromptClosed.then(fn => fn());
      void unlistenViewportScreenshot.then(fn => fn());
      void unlistenHistory.then(fn => fn());
      void unlistenDraftPreview.then(fn => fn());
      void unlistenSessions.then(fn => fn());
      void unlistenTerminal.then(fn => fn());
      void unlistenWorkingVersion.then(fn => fn());
    };
  });

  const activeAuthoringContext = $derived.by(() =>
    resolveActiveAuthoringContext({
      config: $config,
      activeVersionMessage,
      sessionArtifactBundle: activeArtifactBundle,
      sessionModelManifest,
    }),
  );
  const activeAuthoringCapability = $derived.by<RuntimeBackendCapability | null>(() =>
    capabilityForAuthoringContext(
      $runtimeCapabilities,
      activeAuthoringContext.sourceLanguage,
      activeAuthoringContext.geometryBackend,
    ),
  );
  const selectedEngine = $derived.by(() =>
    $config.engines.find((engine) => engine.id === $config.selectedEngineId) ?? null,
  );
  const selectedModelCapabilities = $derived.by(() =>
    resolveEngineCapabilitySummary(selectedEngine),
  );
  const imageInputUnavailableReason = $derived.by<string | null>(() =>
    selectedModelCapabilities.supportsVision ? null : selectedModelCapabilities.reason,
  );
  const generationUnavailableReason = $derived.by<string | null>(() => {
    if (isBooting) return null;
    if (!activeAuthoringCapability) return null;
    return activeAuthoringCapability.available ? null : activeAuthoringCapability.detail;
  });
  const freecadUnavailableReason = $derived.by<string | null>(() => {
    if (isBooting || !$runtimeCapabilities) return null;
    return $runtimeCapabilities.freecad.available ? null : $runtimeCapabilities.freecad.detail;
  });
  const eckySeedIdentity = $derived.by(() => {
    const bundle = activeArtifactBundle;
    const manifest = sessionModelManifest;
    const authoring = activeAuthoringContext;
    return [
      'model',
      bundle?.modelId ?? manifest?.modelId ?? '',
      bundle?.contentHash ?? '',
      `${bundle?.artifactVersion ?? ''}`,
      activeVersionMessage?.id ?? activeVersionMessage?.output?.versionName ?? '',
      authoring?.engineKind ?? bundle?.engineKind ?? manifest?.engineKind ?? '',
      authoring?.sourceLanguage ?? bundle?.sourceLanguage ?? manifest?.sourceLanguage ?? '',
      authoring?.geometryBackend ?? bundle?.geometryBackend ?? manifest?.geometryBackend ?? '',
    ]
      .map((part) => `${part}`.trim().toLowerCase())
      .filter(Boolean)
      .join('|') || 'model|ecky|boot';
  });
  const baseEckyTraits = $derived<GenieTraits>(
    buildModelGenieTraits({
      artifactBundle: activeArtifactBundle,
      modelManifest: sessionModelManifest,
      messageId: activeVersionMessage?.id ?? null,
      versionName: activeVersionMessage?.output?.versionName ?? null,
      authoringContext: activeAuthoringContext,
    }),
  );
  const eckyTraits = $derived<Partial<GenieTraits>>(
    genieSeedOverrides[eckySeedIdentity] ? buildGenieTraitsFromSeed(genieSeedOverrides[eckySeedIdentity]) : baseEckyTraits,
  );
  const eckyIntensity = $derived(1.0 + Math.max(0, ($activeRequestCount - 1) * 0.25));

  function rerollEckySeed() {
    const nextOverrides = {
      ...genieSeedOverrides,
      [eckySeedIdentity]: randomGenieSeed(),
    };
    genieSeedOverrides = nextOverrides;
    writeGenieSeedOverrides(nextOverrides);
    genieWakeUpCount++;
  }

  function hasTauriIpc(): boolean {
    if (typeof window === 'undefined') return false;
    return typeof (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ === 'object';
  }

  async function refreshThreadAgentState() {
    if (!hasTauriIpc() || !$activeThreadId) {
      threadAgentState = null;
      return;
    }
    try {
      const nextState = await getThreadAgentState($activeThreadId);
      threadAgentState = nextState;
    } catch {
      threadAgentState = null;
    }
  }

  $effect(() => {
    if (!$activeThreadId) {
      threadAgentState = null;
      return;
    }
    void refreshThreadAgentState();
  });

  $effect(() => {
    setAgentTerminalSelection(primaryAgentId, threadAgentState?.sessionId ?? null);
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
    if (!activeThreadDialogueMessages.length) return null;
    return [...activeThreadDialogueMessages].reverse().find(m => m.role === 'assistant' && m.status !== 'pending') ?? null;
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
  const hasPreviewArtifact = $derived.by(() =>
    Boolean(sketchPreview?.artifactBundle?.previewStlPath || activeArtifactBundle?.previewStlPath),
  );
  const previewArtifactName = $derived.by<string | null>(() =>
    sketchPreviewStatus?.artifactName ||
    fileBasename(sketchPreview?.artifactBundle?.previewStlPath ?? activeArtifactBundle?.previewStlPath) ||
    null,
  );

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

  $effect(() => {
    setSpeechMuted(isAudioMuted);
  });

  $effect(() => {
    setMuted(isAudioMuted, $config);
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
  const activeThreadLatestErrorRequest = $derived.by(() =>
    [...$activeThreadRequests].reverse().find((request) => request.phase === 'error' && request.error) ?? null,
  );
  const activeThreadErrorText = $derived(activeThreadLatestErrorRequest?.error ?? '');

  const activeConfirm = $derived(pendingConfirms[0] ?? null);
  const threadAgentMascot = $derived.by(() => deriveMascotStateForThreadAgent(threadAgentState));

  const genieMode = $derived.by(() => {
    if ($onboarding.isActive) return 'speaking';
    if (activeViewportScreenshotChoice) return 'speaking';
    if (activeConfirm) return 'speaking';
    if (isActiveMcpMode && activeAgentTerminalAttention) return 'speaking';
    if (activePendingAgentPrompt) return 'speaking';
    if (hasQueuedAgentMessageWithoutPrompt) return 'light';
    if (threadAgentState?.connectionState === 'waking') return 'waking';
    if (threadAgentState?.connectionState === 'waiting') return 'light';
    if (threadAgentState?.connectionState === 'active') return threadAgentMascot.mode;
    if (threadAgentState?.connectionState === 'error') return 'error';
    if (threadAgentState?.connectionState === 'sleeping') return 'sleeping';
    if (activeMcpRenderBusy) return 'rendering';
    if (activeMcpBusy && activeMcpBubbleSummary) return 'speaking';
    if (activeMcpBusy) return 'thinking';
    if (hasLiveMcpSession) return 'light';
    const atPhase = activeThreadHighestPhase;
    if (atPhase === 'error') return 'error';
    if (atPhase === 'repairing') return 'repairing';
    if (atPhase === 'classifying') return 'light';
    if (atPhase === 'rendering') return 'rendering';
    if (atPhase === 'generating' || atPhase === 'answering') return 'thinking';
    if (assistantFresh && !dismissedBubbleText && lastAdvisorBubble) return 'speaking';
    if (threadAgentState?.connectionState === 'disconnected') return 'idle';
    return 'idle';
  });

  const genieBubbleState = $derived.by(() =>
    resolveGenieBubblePresentation({
      sessionError: error,
      onboardingText: $onboarding.isActive ? $onboarding.text : null,
      viewportScreenshotMessage: activeViewportScreenshotChoice?.message ?? null,
      confirmMessage: activeConfirm?.message ?? null,
      terminalAttentionSummary:
        isActiveMcpMode && activeAgentTerminalAttention
          ? (
              activeAgentTerminalAttention.summary ||
              `${activeAgentTerminalAttention.agentLabel} needs terminal input.`
            )
          : null,
      pendingAgentPrompt: activePendingAgentPrompt
        ? {
            message: activePendingAgentPrompt.message ?? null,
            agentLabel: activePendingAgentPrompt.agentLabel,
          }
        : null,
      draftFeedbackSummary: activeDraftFeedbackSummary,
      hasQueuedAgentMessageWithoutPrompt,
      threadAgentState,
      activeMcpBubbleSummary,
      threadAgentMascotBubble: threadAgentMascot.bubble,
      threadError: activeThreadHighestPhase === 'error' ? activeThreadErrorText : null,
      repairMessage: activeThreadHighestPhase === 'repairing' ? $session.repairMessage : null,
      cookingPhrase: ['classifying', 'generating', 'answering'].includes(activeThreadHighestPhase)
        ? $session.cookingPhrase
        : null,
      assistantBubble: lastAdvisorBubble,
      dismissedBubbleText,
      hasPreviewArtifact,
      previewArtifactName,
    }),
  );
  const genieBubble = $derived(genieBubbleState.text);
  const genieRelay = $derived.by(() =>
    resolveRelayPresence({
      source: genieBubbleState.source,
      connectionType: $config.connectionType,
      autoAgents: $config.mcp.autoAgents ?? [],
      primaryAgentId,
      senderLabel: threadAgentState?.agentLabel ?? null,
    }),
  );
  let selectedSessionActivityEventId = $state<string | null>(null);
  let lastBubbleActivityKey = $state('');
  let bubbleActivityTimestamp = $state(0);
  const bubbleActivityKey = $derived(
    `${$activeThreadId ?? 'threadless'}:${$activeVersionId ?? 'versionless'}:${genieBubbleState.badge ?? ''}:${genieBubble}`,
  );

  $effect(() => {
    if (!genieBubble || bubbleActivityKey === lastBubbleActivityKey) return;
    lastBubbleActivityKey = bubbleActivityKey;
    bubbleActivityTimestamp = Date.now();
  });

  const bubbleSessionEvent = $derived.by<SessionEvent | null>(() => {
    if (!genieBubble) return null;
    const severity: SessionEvent['severity'] =
      activeThreadHighestPhase === 'error'
        ? 'error'
        : activeDraftFeedbackSummary || genieBubbleState.badge === 'PREVIEW CHECK'
          ? 'warning'
          : 'info';
    const kind: SessionEvent['kind'] =
      activeThreadHighestPhase === 'error'
        ? 'render_failed'
        : activeDraftFeedbackSummary
          ? 'validation_reported'
          : 'agent_action_finished';
    const actor: SessionEvent['actor'] = threadAgentState?.agentLabel
      ? {
          kind: 'agent',
          id: threadAgentState.sessionId ?? 'agent',
          label: threadAgentState.agentLabel,
        }
      : { kind: 'system', id: 'ecky' };

    return {
      id: `bubble:${bubbleActivityKey}`,
      sessionId: threadAgentState?.sessionId ?? 'local-session',
      threadId: $activeThreadId ?? null,
      versionId: $activeVersionId ?? null,
      actor,
      kind,
      title: genieBubbleState.badge || genieBubbleState.contextLabel || 'Ecky update',
      summary: genieBubble,
      timestamp: bubbleActivityTimestamp || Date.now(),
      severity,
      artifacts: hasPreviewArtifact && previewArtifactName
        ? [
            {
              kind: 'preview_file',
              label: 'Preview artifact',
              value: previewArtifactName,
            },
          ]
        : undefined,
      raw: {
        contextLabel: genieBubbleState.contextLabel,
        threadPhase: activeThreadHighestPhase,
        error: activeThreadErrorText || null,
      },
    };
  });
  const sessionActivityEvents = $derived.by<SessionEvent[]>(() => {
    const recordedEvents = $sessionActivityEventStore;
    if (!bubbleSessionEvent) return recordedEvents;
    if (recordedEvents.some((event) => event.id === bubbleSessionEvent.id)) return recordedEvents;
    return [...recordedEvents, bubbleSessionEvent];
  });
  const sessionActivity = $derived.by(() =>
    composeSessionActivity(sessionActivityEvents, $activeThreadId ?? null, $activeVersionId ?? null),
  );
  const sessionBubbleEvent = $derived.by(() => composeBubbleEvent(sessionActivity));

  function openSessionActivityFromBubble() {
    const event = sessionBubbleEvent.event ?? bubbleSessionEvent;
    if (event) selectedSessionActivityEventId = event.id;
    mountedWindows.activity = true;
    showWindow('activity');
  }

  function selectSessionActivityEvent(id: string) {
    selectedSessionActivityEventId = id;
  }

  $effect(() => {
    const speechCue = resolveGenieSpeechCue({
      latestAssistantMessage,
      assistantFresh,
      visibleBubble: genieBubble,
      activeErrorId: activeThreadLatestErrorRequest?.id ?? null,
      activeErrorText: activeThreadErrorText,
    });
    if (!speechCue || isAudioMuted || dismissedBubbleText === speechCue.text) return;
    if (speechCue.key === lastSpokenAssistantKey) return;
    lastSpokenAssistantKey = speechCue.key;
    speakEckyText(speechCue.text, { muted: isAudioMuted });
  });

  // Reset dismiss state and waking message when a new agent prompt arrives
  $effect(() => {
    if (activePendingAgentPrompt?.requestId) {
      dismissedBubbleText = '';
    }
  });

  const hasQueuedAgentMessageWithoutPrompt = $derived.by<boolean>(() => {
    if (!usesQueuedAgentDialogue) return false;
    if (activePendingAgentPrompt) return false;
    return (
      activeThread?.messages?.some(
        (message) => message.role === 'user' && message.status === 'pending',
      ) ?? false
    );
  });

  const genieActions = $derived.by(() => {
    if ($onboarding.isActive) {
      return [
        { label: 'NEXT', onclick: () => onboarding.next() },
        { label: 'SKIP', onclick: () => onboarding.skip() }
      ];
    }
    if (activeViewportScreenshotChoice) {
      return activeViewportScreenshotChoice.buttons.map((button) => ({
        label: button,
        onclick: () => answerViewportScreenshotChoice(activeViewportScreenshotChoice.requestId, button),
      }));
    }
    if (activeConfirm) {
      return activeConfirm.buttons.map(btn => ({
        label: btn,
        onclick: () => answerConfirm(activeConfirm.requestId, btn),
      }));
    }
    if (isActiveMcpMode && activeAgentTerminalAttention) {
      return [
        {
          label: 'OPEN TERMINAL',
          onclick: () => {
            if (!terminalWindowState.visible) toggleWindow('terminal');
          },
        },
      ];
    }
    if (isActiveMcpMode && $activeThreadId && threadAgentState?.connectionState !== 'none') {
      const connectionState = threadAgentState?.connectionState;
      if (!connectionState) return null;
      const actions: Array<{ label: string; onclick: () => void }> = [];
      if (visibleAgentTerminal) {
        actions.push({
          label: 'OPEN TERMINAL',
          onclick: () => {
            if (!terminalWindowState.visible) toggleWindow('terminal');
          },
        });
      }
      if (connectionState === 'sleeping') {
        actions.push({
          label: 'WAKE AGENT',
          onclick: () => {
            void handleWakePrimaryAgent();
          },
        });
      } else {
        if (
          visibleAgentTerminal?.active &&
          hasQueuedAgentMessageWithoutPrompt &&
          threadAgentState?.connectionState === 'active'
        ) {
          actions.push({
            label: 'NUDGE AGENT',
            onclick: () => {
              void handleNudgeAgentPromptRearm();
            },
          });
        }
        actions.push({
          label: 'RESTART AGENT',
          onclick: () => {
            void handleRestartPrimaryAgent();
          },
        });
        actions.push({
          label: 'STOP AGENT',
          onclick: () => {
            void handleStopPrimaryAgent();
          },
        });
      }
      return actions;
    }
    return null;
  });

  async function handleWakePrimaryAgent() {
    if (!$activeThreadId || agentControlBusy) return;
    agentControlBusy = true;
    try {
      const target = currentVisibleTargetRef();
      await wakePrimaryAutoAgent(
        target?.threadId ?? $activeThreadId,
        target?.messageId ?? null,
        target?.modelId ?? null,
      );
      await refreshThreadAgentState();
    } catch (e: unknown) {
      session.setError(`Agent Wake Error: ${formatBackendError(e)}`);
    } finally {
      agentControlBusy = false;
    }
  }

  async function handleStopPrimaryAgent() {
    if (!$activeThreadId || agentControlBusy) return;
    agentControlBusy = true;
    try {
      const target = currentVisibleTargetRef();
      await stopPrimaryAutoAgent(
        target?.threadId ?? $activeThreadId,
        target?.messageId ?? null,
        target?.modelId ?? null,
      );
      await refreshThreadAgentState();
    } catch (e: unknown) {
      session.setError(`Agent Stop Error: ${formatBackendError(e)}`);
    } finally {
      agentControlBusy = false;
    }
  }

  async function handleRestartPrimaryAgent() {
    if (!$activeThreadId || agentControlBusy) return;
    agentControlBusy = true;
    try {
      const target = currentVisibleTargetRef();
      await restartPrimaryAutoAgent(
        target?.threadId ?? $activeThreadId,
        target?.messageId ?? null,
        target?.modelId ?? null,
      );
      await refreshThreadAgentState();
    } catch (e: unknown) {
      session.setError(`Agent Restart Error: ${formatBackendError(e)}`);
    } finally {
      agentControlBusy = false;
    }
  }

  async function applyCompletedRequest(req: Request) {
    if (!req?.result) return;
    const { design, threadId, messageId, stlUrl: reqStlUrl, artifactBundle, modelManifest } =
      req.result;
    const runtime = await inspectRuntimeBundle(
      artifactBundle ?? null,
      undefined,
      undefined,
      design?.postProcessing ?? null,
      design?.initialParams ?? {},
    );
    const renderableBundle =
      runtime.bundle ??
      getRenderableRuntimeBundle(
        artifactBundle ?? null,
        design?.postProcessing ?? null,
        design?.initialParams ?? {},
      );
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
    if (renderableBundle || modelManifest) {
      session.setModelRuntime(renderableBundle ?? null, modelManifest ?? null);
    }
    if (runtime.skippedOversizedPreview) {
      session.setStatus(
        'Loaded completed request. Lithophane preview was skipped in the viewer; base part meshes are shown instead.',
      );
    }
    void persistLastSessionSnapshot({
      design: design ?? null,
      threadId,
      messageId,
      artifactBundle: renderableBundle ?? null,
      modelManifest: modelManifest ?? null,
    });
    requestQueue.setActive(req.id);
  }

  function dismissRequest(id: string) {
    requestQueue.remove(id);
  }

  function retryRequest(req: Request) {
    void handleGenerate(req.prompt, req.attachments, { uiDeps: requestOrchestratorUiDeps });
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

  async function handleExport(mode: ExportMode) {
    const bundle = activeArtifactBundle;
    if (!bundle) return;

    showExportChooser = false;
    try {
      if (mode === '3mf') {
        if (!hasMultipartExportModel) return;
        const path = await save({
          filters: [{ name: '3MF Package', extensions: ['3mf'] }],
          defaultPath: exportDefaultNames.threeMf,
        });
        if (typeof path === 'string') {
          await exportMultipart3mf(multipartExportParts, path, exportModelTitle);
          session.setStatus('Exported multipart 3MF.');
        }
        return;
      }

      if (mode === 'multipartStlZip') {
        if (!hasMultipartExportModel) return;
        const path = await save({
          filters: [{ name: 'Multipart STL Archive', extensions: ['zip'] }],
          defaultPath: exportDefaultNames.multipartStlZip,
        });
        if (typeof path === 'string') {
          await exportMultipartStlZip(multipartExportParts, path, exportModelTitle);
          session.setStatus('Exported multipart STL archive.');
        }
        return;
      }

      if (mode === 'stl') {
        if (!bundle.previewStlPath) return;
        const path = await save({
          filters: [{ name: 'STL 3D Model', extensions: ['stl'] }],
          defaultPath: exportDefaultNames.stl,
        });
        if (typeof path === 'string') {
          await exportFile(bundle.previewStlPath, path);
          session.setStatus(
            hasMultipartExportModel
              ? 'Exported flattened STL. Use 3MF or Multipart STL to preserve separate bodies.'
              : 'Exported STL.',
          );
        }
        return;
      }

      if (mode === 'step') {
        const sourcePath = getStepExportPath(bundle);
        if (!sourcePath) return;
        const path = await save({
          filters: [{ name: 'STEP CAD Model', extensions: ['step', 'stp'] }],
          defaultPath: exportDefaultNames.step,
        });
        if (typeof path === 'string') {
          await exportFile(sourcePath, path);
          session.setStatus('Exported STEP.');
        }
        return;
      }

      if (!bundle.fcstdPath) return;
      const path = await save({
        filters: [{ name: 'FreeCAD Document', extensions: ['FCStd'] }],
        defaultPath: exportDefaultNames.fcstd,
      });
      if (typeof path === 'string') {
        await exportFile(bundle.fcstdPath, path);
        session.setStatus('Exported FCStd.');
      }
    } catch (e: unknown) {
      session.setError(`Export Error: ${formatBackendError(e)}`);
    }
  }

  function dismissGenie() {
    if (genieBubble) dismissedBubbleText = genieBubble;
    stopEckySpeech();
  }

  onDestroy(() => {
    if (liveApplyTimer) clearTimeout(liveApplyTimer);
  });

  $effect(() => {
    if (terminalWindowState.visible && !visibleAgentTerminal) {
      closeWindowStore('terminal');
    }
  });

  $effect(() => {
    const nextKey = `${$activeThreadId ?? ''}:${$activeVersionId ?? ''}:${activeArtifactBundle?.modelId ?? ''}`;
    if (nextKey === lastViewportContextKey) return;
    lastViewportContextKey = nextKey;
    selectedContextTargetId = null;
    sharedContextSearchQuery = '';
    focusedMeasurementControl = null;
  });

  $effect(() => {
    if (activeModelManifest) return;
    selectedContextTargetId = null;
    sharedContextSearchQuery = '';
    focusedMeasurementControl = null;
  });

  $effect(() => {
    const snapshot = visibleAgentTerminal;
    const focusKey = terminalWindowState.visible && snapshot?.active
      ? `${agentTerminalSessionKey(snapshot)}:${snapshot.active}`
      : '';
    if (!focusKey) {
      lastAgentTerminalFocusKey = '';
      return;
    }
    if (focusKey === lastAgentTerminalFocusKey) return;
    lastAgentTerminalFocusKey = focusKey;
    void tick().then(() => {
      agentTerminalSurface?.focusTerminal();
    });
  });

  function handleTargetSelect(
    target: ContextSelectionTarget | null,
    options?: { allowMissReset?: boolean },
  ) {
    if (!target && viewerMode === 'select' && !options?.allowMissReset) {
      return;
    }
    const nextTarget = target ?? createGlobalContextTarget(activeModelManifest);
    const partId = deriveSelectedPartId(nextTarget);
    selectedContextTargetId = nextTarget?.targetId ?? null;
    focusedMeasurementControl = null;
    session.setSelectedPartId(partId);
    void persistLastSessionSnapshot({ selectedPartId: partId });
  }

  function handlePartSelect(partId: string | null) {
    if (!partId) {
      handleTargetSelect(null, { allowMissReset: true });
      return;
    }
    const nextTarget =
      contextSelectionTargets.find((target) => target.kind === 'part' && target.partId === partId) ??
      resolveContextSelectionTarget(activeModelManifest, contextSelectionTargets, null, partId);
    handleTargetSelect(nextTarget);
  }

  function liveParamSourceKey(): string {
    const wc = get(workingCopy);
    return [
      get(activeThreadId) ?? '',
      wc.sourceVersionId ?? get(activeVersionId) ?? '',
      wc.macroCode,
    ].join('\u0000');
  }

  function scheduleLiveParamChange(nextParams: DesignParams) {
    const sourceKey = liveParamSourceKey();
    if (pendingLiveApplySourceKey && pendingLiveApplySourceKey !== sourceKey) {
      pendingLiveApplyParams = {};
    }
    pendingLiveApplySourceKey = sourceKey;
    pendingLiveApplyParams = { ...pendingLiveApplyParams, ...nextParams };
    if (liveApplyTimer) clearTimeout(liveApplyTimer);
    liveApplyTimer = setTimeout(() => {
      const params = pendingLiveApplyParams;
      const expectedSourceKey = pendingLiveApplySourceKey;
      pendingLiveApplyParams = {};
      pendingLiveApplySourceKey = null;
      liveApplyTimer = null;
      const currentSourceKey = liveParamSourceKey();
      if (expectedSourceKey !== currentSourceKey) {
        console.warn('[App] Dropping stale live parameter apply', {
          expectedSourceKey,
          currentSourceKey,
        });
        return;
      }
      void handleParamChange(params, null, false);
    }, LIVE_APPLY_DEBOUNCE_MS);
  }

  function handleParamPanelChange(nextParams: DesignParams) {
    if ($liveApply) {
      scheduleLiveParamChange(nextParams);
      return;
    }
    return handleParamChange(nextParams, null, false);
  }

  function handleParamPanelCommit(nextParams: DesignParams) {
    return handleParamChange(nextParams, null, true);
  }

  function handleSemanticControlChange(primitiveId: string, value: ParamValue) {
    const nextParams = buildSemanticPatch(activeModelManifest, primitiveId, value, effectiveUiSpec);
    if (Object.keys(nextParams).length === 0) return;
    if ($liveApply) {
      scheduleLiveParamChange(nextParams);
      return;
    }
    stageParamChange(nextParams);
  }

  function handleSelectControlView(viewId: string | null) {
    activeControlViewId = viewId;
  }

  async function handleImportFcstd(sourcePath: string) {
    try {
      if (freecadUnavailableReason) {
        session.setError(`FCStd Import Error: ${freecadUnavailableReason}`);
        return;
      }
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

  async function handleImportFreecadLibraryPart(item: FreecadLibraryItem) {
    try {
      const isMeshLibraryItem = ['stl', 'obj', '3mf'].includes((item.preferredFormat || '').toLowerCase());
      if (!isMeshLibraryItem && freecadUnavailableReason) {
        session.setError(`FreeCAD Library Import Error: ${freecadUnavailableReason}`);
        return;
      }
      session.setError(null);
      session.setStatus(`Importing FreeCAD library part: ${item.name}...`);
      const bundle = await importFreecadLibraryPart({ item });
      const rawManifest = await getModelManifest(bundle.modelId);
      const importedUiSpec = buildImportedUiSpec(rawManifest);
      const importedParams = buildImportedParams(rawManifest, {}, importedUiSpec);
      const manifest = ensureSemanticManifest(rawManifest, importedUiSpec, importedParams) ?? rawManifest;
      const threadId = crypto.randomUUID();
      const title =
        manifest.document.documentLabel ||
        manifest.document.documentName ||
        item.name ||
        'FreeCAD Library Part';
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
      session.setStatus(`Imported FreeCAD library part: ${item.name}`);
      currentView.set('workbench');
      if (manifest.enrichmentState?.status === 'pending') {
        showEnrichmentModal = true;
      }
    } catch (e: unknown) {
      session.setError(`FreeCAD Library Import Error: ${formatBackendError(e)}`);
      throw e;
    }
  }

</script>

<svelte:window onbeforeunload={hardFlushWindowLayout} />

<div class="app-page" role="application">
  {#if $onboarding.isActive}
    <div class="onboarding-backdrop"></div>
  {/if}
  <div
    class="app-overlay-actions"
    class:app-overlay-actions--dock={$currentView === 'workbench'}
    data-testid="workbench-bottom-dock"
    bind:this={overlayActionsEl}
  >
    {#if $currentView === 'workbench'}
      <div class="dock-group dock-group--primary">
        <button
          class="dock-btn"
          class:dock-btn--active={$windowStore.params.visible}
          class:onboarding-highlight={$onboarding.highlightTarget === 'params'}
          data-onboarding-target="params"
          data-dock-label="PARAMS"
          onclick={() => toggleWindow('params')}
          aria-label="PARAMS"
          title="Parameters"
        >
          <svg class="dock-svg dock-svg--params" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M4 7h16" />
            <path d="M4 12h16" />
            <path d="M4 17h16" />
            <path d="M8 5v4" />
            <path d="M15 10v4" />
            <path d="M11 15v4" />
          </svg>
        </button>
        <button
          class="dock-btn"
          class:dock-btn--active={$windowStore.dialogue.visible}
          class:onboarding-highlight={$onboarding.highlightTarget === 'dialogue'}
          data-onboarding-target="dialogue"
          data-dock-label="DIALOGUE"
          onclick={() => toggleWindow('dialogue')}
          aria-label="DIALOGUE"
          title="Dialogue"
        >
          <svg class="dock-svg dock-svg--dialogue" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M5 6h14v10H10l-5 4V6Z" />
            <path d="M8 10h8" />
            <path d="M8 13h5" />
          </svg>
        </button>
        <button
          class="dock-btn"
          class:draw-active={drawMode}
          class:dock-btn--disabled={!selectedModelCapabilities.supportsVision}
          data-dock-label="DRAW"
          disabled={!selectedModelCapabilities.supportsVision}
          onclick={() => drawMode = !drawMode}
          aria-label={drawMode ? 'Exit Draw Mode' : 'Draw Annotations'}
          title={selectedModelCapabilities.supportsVision
            ? (drawMode ? 'Exit Draw Mode' : 'Draw Annotations')
            : (selectedModelCapabilities.reason ?? 'Drawing unavailable for this model')}
        >
          <svg class="dock-svg" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="m6 17 1-4 9-9 4 4-9 9-4 1Z" />
            <path d="m14 6 4 4" />
            <path d="M5 21h14" />
          </svg>
        </button>
        <button
          class="dock-btn"
          class:dock-btn--active={$windowStore.code.visible}
          data-dock-label="CODE"
          onclick={handleDockCodeToggle}
          aria-label="CODE"
          title="Code inspector"
        >
          <svg class="dock-svg dock-svg--code" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="m9 7-5 5 5 5" />
            <path d="m15 7 5 5-5 5" />
            <path d="m13 4-2 16" />
          </svg>
        </button>
      </div>
      <div class="dock-group dock-group--utility">
        <button
          class="dock-btn"
          class:dock-btn--active={$windowStore.projects.visible}
          class:onboarding-highlight={$onboarding.highlightTarget === 'projects'}
          data-onboarding-target="projects"
          data-dock-label="PROJECTS"
          onclick={() => toggleWindow('projects')}
          aria-label="PROJECTS"
          title="Projects"
        >
          <svg class="dock-svg dock-svg--projects" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M4 7h6l2 2h8v10H4V7Z" />
            <path d="M4 11h16" />
            <path d="M7 15h10" />
          </svg>
        </button>
        <button
          class="dock-btn"
          class:dock-btn--active={$windowStore.docs.visible}
          data-dock-label="DOCS"
          onclick={() => toggleWindow('docs')}
          aria-label="DOCS"
          title="Ecky IR docs"
        >
          <svg class="dock-svg dock-svg--docs" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M7 4h8l4 4v12H7V4Z" />
            <path d="M15 4v5h5" />
            <path d="M10 13h7" />
            <path d="M10 17h5" />
          </svg>
        </button>
        {#if visibleAgentTerminal}
          <button
            class="dock-btn dock-btn--utility terminal-overlay-btn"
            class:terminal-overlay-btn-attention={visibleAgentTerminal.attentionRequired}
            data-dock-label="TERMINAL"
            onclick={() => {
              if (!terminalWindowState.visible) toggleWindow('terminal');
            }}
            aria-label={
              visibleAgentTerminal.attentionRequired
                ? `${visibleAgentTerminal.agentLabel} needs terminal input`
                : `Open ${visibleAgentTerminal.agentLabel} terminal`
            }
            title={
              visibleAgentTerminal.attentionRequired
                ? `${visibleAgentTerminal.agentLabel} needs terminal input`
                : `Open ${visibleAgentTerminal.agentLabel} terminal`
            }
            >
            >_
          </button>
        {/if}
        <button
          class="dock-btn"
          class:dock-btn--active={sketchWindowState.visible}
          data-dock-label="SKETCH"
          onclick={() => toggleWindow('sketch')}
          aria-label="SKETCH"
          title="Sketch Workspace"
        >
          <svg class="dock-svg dock-svg--sketch" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M5 17 10 8l5 5 4-7" />
            <path d="M4 17h2v2H4Z" />
            <path d="M9 7h2v2H9Z" />
            <path d="M14 12h2v2h-2Z" />
            <path d="M18 5h2v2h-2Z" />
          </svg>
        </button>
        <button
          class="dock-btn dock-btn--utility dock-btn--settings"
          data-dock-label="SETTINGS"
          onclick={() => toggleWindow('settings')}
          aria-label="Settings"
          title="Settings"
        >
          <svg class="dock-svg" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M12 4v3" />
            <path d="M12 17v3" />
            <path d="M4 12h3" />
            <path d="M17 12h3" />
            <path d="m6.3 6.3 2.1 2.1" />
            <path d="m15.6 15.6 2.1 2.1" />
            <path d="m17.7 6.3-2.1 2.1" />
            <path d="m8.4 15.6-2.1 2.1" />
            <path d="M9 12a3 3 0 1 0 6 0 3 3 0 0 0-6 0Z" />
          </svg>
        </button>
      </div>
    {:else}
      <button
        class="settings-overlay-btn"
        onclick={() => currentView.set('workbench')}
        title="Close"
      >
        ×
      </button>
    {/if}
  </div>

  <div class="app-container">
    {#if $currentView === 'workbench' || $currentView === 'inventory-model'}
      <div class="workbench">
        <div class="main-workbench">
          <main
            class="viewport-area"
            role="presentation"
            bind:this={viewportAreaEl}
            class:onboarding-highlight={$onboarding.highlightTarget === 'viewport'}
            data-onboarding-target="viewport"
          >
            <div
              class="viewer-shell"
              data-model-key={effectiveViewerModelKey ?? ''}
              data-stl-url={effectiveViewerStlUrl ?? ''}
            >
              <Viewer
                bind:this={viewerComponent}
                modelKey={effectiveViewerModelKey}
                stlUrl={effectiveViewerStlUrl}
                viewerAssets={effectiveViewerAssets}
                manifestParts={hasSketchPreview ? [] : activeModelManifest?.parts ?? []}
                edgeTargets={sketchPreview?.artifactBundle?.edgeTargets ?? activeArtifactBundle?.edgeTargets ?? []}
                faceTargets={sketchPreview?.artifactBundle?.faceTargets ?? activeArtifactBundle?.faceTargets ?? []}
                selectionTargets={hasSketchPreview ? [] : contextSelectionTargets}
                selectedTarget={hasSketchPreview ? null : selectedTarget}
                searchQuery={hasSketchPreview ? '' : sharedContextSearchQuery}
                outlineEnabled={viewerOutlineEnabled}
                persistedCameraState={hasSketchPreview ? null : persistedViewportCameraState}
                selectedPartId={hasSketchPreview ? null : selectedPartId}
                overlayPartLabel={hasSketchPreview ? null : selectedTarget?.label ?? overlaySelectedPart?.label ?? null}
                overlayPartEditable={hasSketchPreview ? false : selectedTarget?.editable ?? overlaySelectedPart?.editable ?? false}
                overlayPreviewOnly={hasSketchPreview ? false : overlayPreviewOnly}
                showContextOverlay={enableViewportContextOverlay}
                overlayControls={hasSketchPreview ? [] : overlayControls}
                overlayAdvisories={hasSketchPreview ? [] : overlayAdvisories}
                activeMeasurementCallout={hasSketchPreview ? null : activeMeasurementCallout}
                previewTransforms={hasSketchPreview ? {} : effectivePreviewTransforms}
                viewerMode={!hasSketchPreview && paramsWindowState.visible ? viewerMode : 'orbit'}
                onOverlayChange={handleSemanticControlChange}
                onControlFocusChange={(focus) => focusedMeasurementControl = focus}
                onSearchQueryChange={(query) => sharedContextSearchQuery = query}
                onSelectTarget={handleTargetSelect}
                onCameraStateChange={handleVisibleViewerCameraChange}
                onModelLoaded={handleVisibleViewerLoaded}
                onModelLoadError={handleVisibleViewerLoadError}
                isGenerating={viewerBusyPhase === 'generating' || viewerBusyPhase === 'repairing'}
                hideModelWhileBusy={showViewerBusyMask}
                busyPhase={viewerBusyPhase}
                busyText={viewerBusyText}
                topologyMode={viewerTopologyMode}
              />
              {#if !hasSketchPreview && availablePreviewViews.length > 0}
                <div class="preview-view-switcher" aria-label="Preview view switcher">
                  {#each availablePreviewViews as previewView (previewView.viewId)}
                    <button
                      class="preview-view-switcher__button"
                      class:preview-view-switcher__button--active={previewView.viewId === activePreviewViewId}
                      onclick={() => activePreviewViewId = previewView.viewId}
                      type="button"
                    >
                      {previewView.label}
                    </button>
                  {/each}
                </div>
              {/if}
              {#if sketchPreviewStatus}
                <section class="sketch-preview-status" aria-label="Sketch preview status">
                  <div class="sketch-preview-status__head">
                    <span>{sketchPreviewStatus.title}</span>
                    <strong>{sketchPreviewStatus.verdict}</strong>
                  </div>
                  <div class="sketch-preview-status__detail">{sketchPreviewStatus.detail}</div>
                  <div class="sketch-preview-status__meta">
                    {#if sketchPreviewDraftLabel}
                      <span>{sketchPreviewDraftLabel}</span>
                    {/if}
                    <span>{sketchPreviewStatus.backend}</span>
                    <span>EXPORT LOCKED</span>
                    <span>{sketchPreviewStatus.artifactName}</span>
                  </div>
                </section>
              {/if}
            </div>
            <DrawingOverlay
              bind:this={drawingOverlay}
              active={drawMode}
              onDirtyChange={(dirty) => {
                drawingOverlayDirty = dirty;
              }}
            />
            <div class="hidden-viewer-host" aria-hidden="true">
              <Viewer
                bind:this={hiddenViewerComponent}
                modelKey={hiddenViewerSpec?.targetKey ?? null}
                stlUrl={hiddenViewerSpec?.stlUrl ?? null}
                viewerAssets={hiddenViewerSpec?.viewerAssets ?? []}
                edgeTargets={[]}
                faceTargets={[]}
                selectionTargets={[]}
                selectedTarget={null}
                searchQuery=""
                selectedPartId={null}
                overlayPartLabel={null}
                overlayPartEditable={false}
                overlayPreviewOnly={false}
                showContextOverlay={false}
                overlayControls={[]}
                overlayAdvisories={[]}
                activeMeasurementCallout={null}
                previewTransforms={{}}
                viewerMode="orbit"
                onControlFocusChange={() => { focusedMeasurementControl = null; }}
                onSearchQueryChange={() => {}}
                onSelectTarget={() => {}}
                onCameraStateChange={() => {}}
                onModelLoaded={handleHiddenViewerLoaded}
                onModelLoadError={handleHiddenViewerLoadError}
                isGenerating={false}
                hideModelWhileBusy={false}
                busyPhase={null}
                busyText={null}
              />
            </div>
            <div class="genie-layer" class:onboarding-active={$onboarding.isActive}>
              <VertexGenie 
                mode={genieMode} 
                bubble={genieBubble}
                compact={genieBubbleState.compact}
                badge={genieBubbleState.badge}
                contextLabel={genieBubbleState.contextLabel}
                safeRightInset={genieSafeRightInset}
                onBubbleClick={openSessionActivityFromBubble}
                bubbleTestId="genie-session-bubble"
                onDismiss={dismissGenie}
                actions={genieActions}
                relay={genieRelay}
                traits={eckyTraits} 
                intensity={eckyIntensity} 
                wakeUp={genieWakeUpCount}
                agentConnected={
                  threadAgentMascot.connected ||
                  hasLiveMcpSession ||
                  !!visibleAgentTerminal?.active ||
                  hasLiveApiConnection
                }
              />
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
            

            {#if $activeThreadId && ($workingCopy.macroCode || stlUrl)}
              <div class="viewport-overlay">
                <div class="export-actions">
                  <button class="btn btn-xs btn-secondary" onclick={forkDesign} disabled={showViewerBusyMask} title="Fork this design into a new project">🍴 FORK</button>
                  {#if activeArtifactBundle}
                    <button
                      class="btn btn-xs btn-primary"
                      onclick={() => showExportChooser = true}
                      disabled={!canExportModel || showViewerBusyMask || hasSketchPreview}
                      title={hasSketchPreview ? 'Sketch preview is diagnostic only. Accepted CAD export needs exact BRep/STEP validation.' : 'Open export options'}
                    >
                      💾 EXPORT
                    </button>
                  {/if}
                </div>
              </div>
            {/if}
          </main>
        </div>
      </div>
    {/if}
  </div>

  {#if showNewProjectChooser}
    <Modal title="Start New Project" onclose={() => showNewProjectChooser = false}>
      <div class="new-project-chooser">
        <button class="new-project-chooser__btn" onclick={startBlankProject}>Blank Project</button>
        <button
          class="new-project-chooser__btn"
          onclick={handleTopImportFcstd}
          disabled={Boolean(freecadUnavailableReason)}
          title={freecadUnavailableReason ?? undefined}
        >
          Import FreeCAD
        </button>
        <button class="new-project-chooser__btn" onclick={startMacroImport}>Import Macro</button>
      </div>
    </Modal>
  {/if}

  {#if showNewProjectImport}
    <ManualImportModal bind:show={showNewProjectImport} onImport={handleTopMacroImport} />
  {/if}

  {#if isBooting}
    <div class="boot-overlay">
      <div class="boot-overlay__glass"></div>
      <div class="boot-overlay__content">
        <div class="boot-overlay__title">ECKY CAD</div>
        <div class="boot-overlay__ecky">
          <VertexGenie mode="thinking" bubble="" fitToCanvas={true} />
        </div>
        <div class="boot-overlay__status">{status || 'Restoring environment...'}</div>
      </div>
    </div>
  {/if}

  {#if projectsWindowState.visible}
    <Window
      windowId="projects"
      x={projectsWindowState.x}
      y={projectsWindowState.y}
      width={projectsWindowState.width}
      height={projectsWindowState.height}
      z={projectsWindowState.z}
      minWidth={320}
      minHeight={300}
      title="Projects"
      focused={projectsWindowState.active}
      hidden={!projectsWindowState.visible}
      highlighted={$onboarding.highlightTarget === 'projects'}
      onclose={() => closeWindowStore('projects')}
    >
      <ProjectSwitcher
        onImportFcstd={handleImportFcstd}
        onImportFreecadLibraryPart={handleImportFreecadLibraryPart}
        onOpenNewProjectChooser={() => showNewProjectChooser = true}
        freecadUnavailableReason={freecadUnavailableReason}
      />
    </Window>
  {/if}

  {#if mountedWindows.params}
    <Window
      windowId="params"
      x={paramsWindowState.x}
      y={paramsWindowState.y}
      width={paramsWindowState.width}
      height={paramsWindowState.height}
      z={paramsWindowState.z}
      minWidth={280}
      minHeight={250}
      title="Parameters"
      focused={paramsWindowState.active}
      hidden={!paramsWindowState.visible}
      highlighted={$onboarding.highlightTarget === 'params'}
      onclose={() => closeWindowStore('params')}
      >
      <div class="window-scroll-container">
        <ParamPanel
          uiSpec={effectiveUiSpec}
          parameters={effectiveParameters}
          modelManifest={activeModelManifest}
          postProcessing={$workingCopy.postProcessing ?? null}
          artifactBundle={activeArtifactBundle}
          controlViews={availableControlViews}
          activeControlViewId={activeControlViewId}
          selectedTarget={selectedTarget}
          selectedPartId={selectedPartId}
          bind:searchQuery={sharedContextSearchQuery}
          onControlFocusChange={(focus) => focusedMeasurementControl = focus}
          onSelectControlView={handleSelectControlView}
          onSelectPart={handlePartSelect}
          onpostprocessingchange={(nextPostProcessing) => {
            workingCopy.patch({ postProcessing: nextPostProcessing });
          }}
          onSemanticChange={handleSemanticControlChange}
          onApplyMacroCode={(code) => applyManualCodeDraft(code)}
          onchange={handleParamPanelChange}
          oncommit={handleParamPanelCommit}
          onspecchange={(spec, params) => {
            paramPanelState.setUiSpec(spec);
            workingCopy.patch({ uiSpec: spec });
            if (params) {
              paramPanelState.setParams(params);
              workingCopy.patch({ params });
            }
          }}
          activeVersionId={$paramPanelState.versionId}
          messageId={$activeVersionId}
          macroCode={viewportCodeWorkingCopyAligned
            ? $workingCopy.macroCode
            : activeVersionMessage?.output?.macroCode ?? ''}
          outlineEnabled={viewerOutlineEnabled}
          topologyMode={viewerTopologyMode}
          selectionMode={viewerMode}
          onViewerDisplayChange={(display) => {
            viewerOutlineEnabled = display.outlineEnabled;
            viewerTopologyMode = display.topologyMode;
          }}
          onViewerSelectionModeChange={(mode) => {
            viewerMode = mode;
          }}
          onOpenInEditor={() => {
            void openProjectInEditor($activeThreadId ?? null, $activeVersionId ?? null).catch((error) => {
              console.error('open in editor failed:', error);
            });
          }}
          onShowCode={() => {
            void openVersionCodeModal({
              code: $workingCopy.macroCode,
              title: $workingCopy.title,
              sourceLanguage: $workingCopy.sourceLanguage,
              geometryBackend: $workingCopy.geometryBackend,
            });
          }}
        />

      </div>
      </Window>

  {/if}

  {#if settingsWindowState.visible}
    <Window
      windowId="settings"
      x={settingsWindowState.x}
      y={settingsWindowState.y}
      width={settingsWindowState.width}
      height={settingsWindowState.height}
      z={settingsWindowState.z}
      minWidth={400}
      minHeight={350}
      title="Settings"
      focused={settingsWindowState.active}
      hidden={!settingsWindowState.visible}
      highlighted={false}
      onclose={() => closeWindowStore('settings')}
    >
      <div class="window-scroll-container">
        <ConfigPanel
          bind:config={$config}
          availableModels={$availableModels}
          isLoadingModels={$isLoadingModels}
          runtimeCapabilities={$runtimeCapabilities}
          eckyTraits={eckyTraits}
          onRerollEcky={rerollEckySeed}
          onfetch={fetchModels}
          onsave={saveConfig}
        />
      </div>
    </Window>
  {/if}

  {#if mountedWindows.activity}
    <Window
      windowId="activity"
      x={activityWindowState.x}
      y={activityWindowState.y}
      width={activityWindowState.width}
      height={activityWindowState.height}
      z={activityWindowState.z}
      minWidth={440}
      minHeight={320}
      title="Session Activity"
      focused={activityWindowState.active}
      hidden={!activityWindowState.visible}
      highlighted={false}
      onclose={() => closeWindowStore('activity')}
    >
      <SessionActivityWindow
        events={sessionActivity.visibleEvents}
        selectedEventId={selectedSessionActivityEventId}
        onSelectEvent={selectSessionActivityEvent}
      />
    </Window>
  {/if}

  {#if mountedWindows.sketch}
    <Window
      windowId="sketch"
      x={sketchWindowState.x}
      y={sketchWindowState.y}
      width={sketchWindowState.width}
      height={sketchWindowState.height}
      z={sketchWindowState.z}
      minWidth={520}
      minHeight={360}
      title="Sketch Workspace"
      focused={sketchWindowState.active}
      hidden={!sketchWindowState.visible}
      highlighted={false}
      onclose={() => closeWindowStore('sketch')}
    >
      <div class="sketch-window-shell">
        <SketchWorkspace
          restoredPreview={sketchPreview}
          onPreviewResult={handleSketchPreviewChange}
          onManualPreviewResult={handleSketchManualPreviewResult}
          onSaveDraft={handleSketchSaveDraft}
          onDiscardDraft={discardSketchPreviewDraft}
        />
      </div>
    </Window>
  {/if}

  {#if mountedWindows.dialogue}
    <Window
      windowId="dialogue"
      x={dialogueWindowState.x}
      y={dialogueWindowState.y}
      width={dialogueWindowState.width}
      height={dialogueWindowState.height}
      z={dialogueWindowState.z}
      minWidth={350}
      minHeight={260}
      title="Dialogue"
      focused={dialogueWindowState.active}
      hidden={!dialogueWindowState.visible}
      highlighted={$onboarding.highlightTarget === 'dialogue'}
      onclose={() => closeWindowStore('dialogue')}
    >
      <div class="dialogue-content">
        <div class="dialogue-toolbar">
          <label class="dialogue-toolbar__remember">
            <input
              type="checkbox"
              checked={$windowLayoutRemembered}
              onchange={(event) => void setThreadWindowLayoutRemembered((event.currentTarget as HTMLInputElement).checked)}
            />
            <span>Remember layout</span>
          </label>
        </div>
        {#key $activeThreadId ?? 'new-thread'}
          <PromptPanel
            onGenerate={handlePromptPanelSubmit}
            isGenerating={$activeThreadBusy}
            generationUnavailableReason={generationUnavailableReason}
            imageAttachmentUnavailableReason={imageInputUnavailableReason}
            dialogueState={dialogueState}
            messages={activeThreadDialogueMessages}
            messagesLoading={$activeThreadMessagesLoading}
            messagesHasMore={activeThread ? ($threadMessagePageState[activeThread.id]?.hasMore ?? false) : false}
            messagesPageLoading={activeThread ? ($threadMessagePageState[activeThread.id]?.isLoading ?? false) : false}
            requests={$activeThreadRequests}
            onLoadOlderMessages={() => activeThread && loadOlderThreadMessages(activeThread.id)}
            activeThreadId={$activeThreadId}
            sendWorkspaceCapture={sendWorkspaceCaptureForActiveThread}
            workspaceCaptureHint={workspaceCaptureHint}
            sttLanguageCode={$config.voice?.sttLanguageCode ?? 'en-US'}
            onToggleWorkspaceCapture={setWorkspaceCaptureForActiveThread}
            onShowCode={(m) => {
              void openVersionCodeModal({
                code: m.output.macroCode,
                title: m.output.title,
                sourceLanguage:
                  m.artifactBundle?.sourceLanguage ?? m.modelManifest?.sourceLanguage ?? m.output.sourceLanguage ?? null,
                geometryBackend:
                  m.artifactBundle?.geometryBackend ?? m.modelManifest?.geometryBackend ?? m.output.geometryBackend ?? null,
              });
            }}
            onDeleteVersion={deleteVersion}
            onRestoreVersion={restoreVersion}
            onAuthoredVerifyFocus={handlePromptPanelAuthoredVerifyFocus}
            bind:activeVersionId={$activeVersionId}
            onVersionChange={loadVersion}
          />
        {/key}
      </div>
    </Window>
  {/if}

  {#if mountedWindows.docs}
    <Window
      windowId="docs"
      x={docsWindowState.x}
      y={docsWindowState.y}
      width={docsWindowState.width}
      height={docsWindowState.height}
      z={docsWindowState.z}
      minWidth={760}
      minHeight={480}
      title="Ecky IR Docs"
      focused={docsWindowState.active}
      hidden={!docsWindowState.visible}
      highlighted={false}
      onclose={() => closeWindowStore('docs')}
    >
      <DocsSite showHead={false} onOpenSnippet={openDocsSnippetInCode} />
    </Window>
  {/if}

  {#if mountedWindows.terminal && visibleAgentTerminal}
    <Window
      windowId="terminal"
      x={terminalWindowState.x}
      y={terminalWindowState.y}
      width={terminalWindowState.width}
      height={terminalWindowState.height}
      z={terminalWindowState.z}
      minWidth={400}
      minHeight={300}
      title={`${visibleAgentTerminal.agentLabel} Terminal`}
      focused={terminalWindowState.active}
      hidden={!terminalWindowState.visible}
      highlighted={false}
      onclose={() => {
        closeWindowStore('terminal');
      }}
    >
      <div class="agent-terminal-window">
        <div class="agent-terminal-window__meta">
          <div class="agent-terminal-window__status">
            {#if visibleAgentTerminal.active}
              LIVE PTY
            {:else}
              LAST SESSION
            {/if}
          </div>
          {#if activeAgentTerminalMetaSummary}
            <div class="agent-terminal-window__summary">{activeAgentTerminalMetaSummary}</div>
          {/if}
        </div>
        {#if threadAgentState?.sessionId}
          <div class="agent-terminal-window__trace-meta">
            <span>SESSION {shortSessionId(threadAgentState.sessionId)}</span>
            <span>THREAD {activeThread?.title ?? 'UNKNOWN'}</span>
            {#if threadAgentState.providerKind}
              <span>PROVIDER {threadAgentState.providerKind.toUpperCase()}</span>
            {/if}
            {#if threadAgentState.waitingOnPrompt}
              <span>WAITING ON PROMPT</span>
            {:else if activeMcpBusy}
              <span>TURN ACTIVE</span>
            {:else if threadAgentState.phase}
              <span>{formatAgentPhase(threadAgentState.phase)}</span>
            {/if}
          </div>
        {/if}
        <div class="agent-terminal-window__hint">
          {#if visibleAgentTerminal.active}
            CLICK TERMINAL TO TYPE DIRECTLY. ARROWS, TAB, ESC, CTRL+C AND PASTE GO STRAIGHT TO THE PTY.
          {:else}
            LAST CAPTURED TERMINAL OUTPUT
          {/if}
        </div>
        <div
          class="agent-terminal-window__screen"
          class:agent-terminal-window__screen--live={visibleAgentTerminal.active}
          aria-label={`${visibleAgentTerminal.agentLabel} terminal`}
        >
          <AgentTerminalSurface
            bind:this={agentTerminalSurface}
            snapshot={visibleAgentTerminal}
            visible={terminalWindowState.visible}
            onRawInput={(data) => void handleAgentTerminalRawInput(data)}
            onResize={({ cols, rows }) =>
              void handleAgentTerminalResize(visibleAgentTerminal.agentId, cols, rows)}
          />
        </div>
        <div class="agent-terminal-window__composer">
          <input
            class="input-mono agent-terminal-window__input"
            bind:value={agentTerminalInput}
            placeholder={`Paste or send a full line to ${visibleAgentTerminal.agentLabel}...`}
            disabled={!visibleAgentTerminal.active}
            onkeydown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                void submitAgentTerminalInput();
              }
            }}
          />
          <button
            class="btn btn-xs btn-secondary"
            onclick={() => void submitAgentTerminalInput(true)}
            disabled={!visibleAgentTerminal.active}
            title="Send Enter"
          >
            ENTER
          </button>
          <button
            class="btn btn-xs btn-primary"
            onclick={() => void submitAgentTerminalInput()}
            disabled={!visibleAgentTerminal.active || !agentTerminalInput.length}
          >
            SEND
          </button>
        </div>
      </div>
    </Window>
  {/if}

  {#if showExportChooser}
    <Modal title="Export Model" onclose={() => showExportChooser = false}>
      <div class="export-chooser">
        {#if hasMultipartExportModel}
          <div class="export-chooser__note">
            Plain STL flattens the assembly. Use 3MF or Multipart STL to keep separate bodies for Bambu Studio or Orca.
          </div>
        {/if}
        {#each exportOptions as option (option.id)}
          <button
            class="export-chooser__action"
            disabled={option.disabled}
            onclick={() => void handleExport(option.id)}
          >
            <span class="export-chooser__copy">
              <span class="export-chooser__title">{option.title}</span>
              <span class="export-chooser__subtitle">
                {option.disabled && option.disabledReason ? option.disabledReason : option.subtitle}
              </span>
            </span>
          </button>
        {/each}
      </div>
    </Modal>
  {/if}

  {#if mountedWindows.code}
    <CodeModal
      bind:code={$selectedCode}
      mode={codeModalMode}
      sourceLanguage={codeModalSourceLanguage}
      title={$selectedTitle}
      defaultTitle={$workingCopy.title}
      defaultVersionName={$workingCopy.versionName || 'V-manual'}
      onApply={codeModalMode === 'version' ? applyManualCodeDraft : undefined}
      onCommit={codeModalMode === 'version' ? commitManualVersion : undefined}
      z={codeWindowState.z}
      hidden={!codeWindowState.visible}
      focused={codeWindowState.active}
      onclose={closeCodeModal}
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
  .sketch-window-shell {
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  .workbench { display: flex; height: 100%; width: 100%; overflow: hidden; }
  .main-workbench { flex: 1; display: flex; flex-direction: column; min-width: 0; overflow: hidden; }
  .viewport-area { flex: 1; min-height: 100px; background: #0b0f1a; position: relative; overflow: hidden; }
  .viewer-shell {
    position: absolute;
    inset: 0;
    z-index: 5;
    transition: opacity 180ms ease, filter 180ms ease;
    overflow: hidden;
  }
  .preview-view-switcher {
    position: absolute;
    left: 12px;
    top: 12px;
    z-index: 36;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    max-width: min(420px, calc(100% - 24px));
    overflow: hidden;
  }
  .preview-view-switcher__button {
    border: 1px solid color-mix(in srgb, var(--secondary) 36%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.58rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 6px 10px;
    cursor: pointer;
    overflow: hidden;
  }
  .preview-view-switcher__button--active {
    color: var(--primary);
    border-color: color-mix(in srgb, var(--primary) 52%, var(--bg-300));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--primary) 24%, transparent);
  }
  .sketch-preview-status {
    position: absolute;
    left: 12px;
    bottom: 12px;
    z-index: 35;
    width: min(360px, calc(100% - 24px));
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--primary) 44%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    box-shadow: var(--shadow);
    font-family: var(--font-mono);
    text-transform: uppercase;
    overflow: hidden;
    pointer-events: none;
  }
  .sketch-preview-status__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-width: 0;
    color: var(--primary);
    font-size: 0.66rem;
    font-weight: 700;
    letter-spacing: 0.1em;
  }
  .sketch-preview-status__head strong {
    color: var(--red);
    font-size: 0.6rem;
    white-space: nowrap;
  }
  .sketch-preview-status__detail {
    margin-top: 6px;
    color: var(--text-dim);
    font-size: 0.62rem;
    line-height: 1.45;
    text-transform: none;
  }
  .sketch-preview-status__meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
    color: var(--secondary);
    font-size: 0.58rem;
    letter-spacing: 0.08em;
  }
  .sketch-preview-status__meta span {
    min-width: 0;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dialogue-content { flex: 1; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
  .dialogue-toolbar {
    flex: 0 0 auto;
    padding: 6px 10px;
    border-bottom: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-200) 90%, transparent);
  }
  .dialogue-toolbar__remember {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 0.65rem;
    color: var(--text-dim);
    font-family: var(--font-mono);
  }
  .app-overlay-actions {
    position: absolute;
    top: 10px;
    right: 10px;
    z-index: 5001;
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .app-overlay-actions--dock {
    top: auto;
    right: auto;
    left: 50%;
    bottom: 16px;
    transform: translateX(-50%);
    max-width: calc(100vw - 24px);
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 6px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 94%, transparent);
    box-shadow: 0 10px 22px color-mix(in srgb, #000 42%, transparent);
    backdrop-filter: blur(10px);
    overflow: visible;
  }
  .dock-group { display: flex; gap: 4px; min-width: 0; }
  .dock-group--primary,
  .dock-group--utility {
    overflow: visible;
  }
  .dock-group--utility {
    position: relative;
    padding-left: 10px;
  }
  .dock-group--utility::before {
    content: '';
    position: absolute;
    left: 2px;
    top: 5px;
    bottom: 5px;
    width: 1px;
    background: linear-gradient(180deg, transparent, var(--secondary), transparent);
  }
  .dock-btn {
    position: relative;
    width: 44px;
    height: 44px;
    padding: 0;
    background: color-mix(in srgb, var(--bg-200) 86%, transparent);
    border: 1px solid var(--bg-300);
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    font-weight: bold;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    backdrop-filter: blur(6px);
    box-shadow: none;
    overflow: visible;
  }
  .dock-btn:hover,
  .dock-btn:focus-visible {
    border-color: var(--primary);
    color: var(--primary);
    box-shadow: inset 0 -2px 0 color-mix(in srgb, var(--primary) 72%, transparent);
  }
  .dock-btn--active {
    border-color: var(--primary);
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 16%, var(--bg-100));
  }
  .dock-btn:disabled,
  .dock-btn--disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .dock-btn:disabled:hover,
  .dock-btn--disabled:hover,
  .dock-btn:disabled:focus-visible,
  .dock-btn--disabled:focus-visible {
    border-color: var(--bg-300);
    color: var(--text-dim);
    box-shadow: none;
  }
  .dock-btn[data-dock-label]::after {
    content: attr(data-dock-label);
    position: absolute;
    left: 50%;
    bottom: calc(100% + 10px);
    transform: translateX(-50%) translateY(4px);
    min-width: max-content;
    padding: 4px 8px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 96%, transparent);
    color: var(--text-dim);
    font-size: 0.58rem;
    letter-spacing: 0.08em;
    opacity: 0;
    pointer-events: none;
    transition: opacity 120ms ease, transform 120ms ease;
  }
  .dock-btn:hover::after,
  .dock-btn:focus-visible::after {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
  .dock-svg {
    width: 25px;
    height: 25px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.9;
    stroke-linecap: square;
    stroke-linejoin: miter;
  }
  .dock-btn--utility {
    width: 38px;
    height: 38px;
    color: color-mix(in srgb, var(--text) 78%, var(--primary) 22%);
  }
  .dock-btn--settings {
    width: 44px;
    height: 44px;
  }
  .settings-overlay-btn { width: 34px; height: 34px; background: color-mix(in srgb, var(--bg-100) 90%, transparent); border: 1px solid var(--bg-300); color: var(--text); cursor: pointer; display: flex; align-items: center; justify-content: center; box-shadow: var(--shadow); }
  .settings-overlay-btn:hover { border-color: var(--primary); color: var(--primary); }
  .dock-btn.draw-active { border-color: var(--primary); background: color-mix(in srgb, var(--primary) 25%, var(--bg-100)); box-shadow: 0 0 8px var(--primary); }
  .new-project-chooser {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    min-width: 320px;
  }
  .new-project-chooser__btn {
    min-height: 36px;
    padding: 8px 12px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    text-align: left;
    cursor: pointer;
  }
  .new-project-chooser__btn:hover {
    border-color: var(--primary);
    color: var(--primary);
  }
  .terminal-overlay-btn { font-family: var(--font-mono); font-size: 0.72rem; letter-spacing: 0.04em; }
  .terminal-overlay-btn-attention { border-color: var(--secondary); color: var(--secondary); box-shadow: 0 0 10px color-mix(in srgb, var(--secondary) 50%, transparent); }
  .genie-layer { position: absolute; left: 10px; top: 10px; z-index: 120; pointer-events: auto; max-width: min(56vw, 380px); }

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
  .hidden-viewer-host {
    position: fixed;
    left: -200vw;
    top: 0;
    width: 1024px;
    height: 768px;
    pointer-events: none;
    visibility: hidden;
    overflow: hidden;
  }
  .viewport-overlay { position: absolute; bottom: 12px; right: 12px; max-width: min(420px, calc(100vw - 24px)); background: rgba(11, 15, 26, 0.6); backdrop-filter: blur(4px); padding: 8px; border: 1px solid var(--bg-300); z-index: 50; display: flex; flex-direction: column; align-items: flex-end; gap: 8px; overflow: hidden; }
  .export-actions { display: flex; gap: 4px; }
  .export-chooser {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: min(520px, 78vw);
    padding: 12px;
    overflow: hidden;
  }
  .export-chooser__note {
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    color: var(--text-dim);
    font-size: 0.72rem;
    line-height: 1.45;
    padding: 10px 12px;
  }
  .export-chooser__action {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    width: 100%;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 94%, transparent);
    color: var(--text);
    cursor: pointer;
    text-align: left;
    padding: 14px 16px;
    transition: border-color 120ms ease, background 120ms ease, transform 120ms ease;
  }
  .export-chooser__action:hover:not(:disabled) {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--primary) 12%, var(--bg-100));
    transform: translateY(-1px);
  }
  .export-chooser__action:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .export-chooser__copy {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .export-chooser__title {
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.84rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .export-chooser__subtitle {
    color: var(--text-dim);
    font-size: 0.72rem;
    line-height: 1.45;
  }
  @media (max-width: 960px) {
    .viewport-overlay {
      left: 12px;
      right: 12px;
      bottom: 12px;
      align-items: stretch;
    }
    .export-chooser {
      min-width: min(92vw, 520px);
    }
  }
  .boot-overlay { position: absolute; inset: 0; z-index: 300; display: flex; align-items: center; justify-content: center; background: var(--bg); }
  .boot-overlay__glass { position: absolute; inset: 0; background: radial-gradient(circle, rgba(74, 140, 92, 0.16), transparent), rgba(8, 12, 20, 0.86); backdrop-filter: blur(18px); }
  .boot-overlay__content { position: relative; z-index: 1; display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 20px; }
  .boot-overlay__title { color: var(--secondary); font-weight: bold; letter-spacing: 0.2em; }
  .boot-overlay__status { color: var(--text-dim); font-size: 0.7rem; }
  .agent-terminal-window { display: flex; flex-direction: column; height: 100%; background: linear-gradient(180deg, color-mix(in srgb, var(--bg-100) 92%, #071019 8%), var(--bg)); overflow: hidden; }
  .agent-terminal-window__meta { display: flex; flex-direction: column; gap: 6px; padding: 10px 12px; border-bottom: 1px solid var(--bg-300); background: color-mix(in srgb, var(--bg-200) 88%, transparent); }
  .agent-terminal-window__status { font-family: var(--font-mono); font-size: 0.65rem; letter-spacing: 0.14em; color: var(--secondary); text-transform: uppercase; }
  .agent-terminal-window__summary { font-size: 0.76rem; color: var(--text-dim); }
  .agent-terminal-window__trace-meta { display: flex; flex-wrap: wrap; gap: 8px 14px; padding: 8px 12px; border-bottom: 1px solid var(--bg-300); background: color-mix(in srgb, var(--bg-200) 80%, transparent); font-family: var(--font-mono); font-size: 0.62rem; letter-spacing: 0.1em; text-transform: uppercase; color: var(--text-dim); }
  .agent-terminal-window__hint { padding: 8px 12px; border-bottom: 1px solid var(--bg-300); font-family: var(--font-mono); font-size: 0.64rem; letter-spacing: 0.08em; color: color-mix(in srgb, var(--secondary) 88%, #d9e8c9 12%); background: color-mix(in srgb, var(--bg-200) 72%, #071019 28%); text-transform: uppercase; }
  .agent-terminal-window__screen { flex: 1; min-height: 0; min-width: 0; overflow: hidden; background:
      radial-gradient(circle at top, color-mix(in srgb, var(--primary) 10%, transparent), transparent 42%),
      linear-gradient(180deg, rgba(6, 11, 17, 0.96), rgba(3, 8, 14, 0.98)); }
  .agent-terminal-window__screen--live { cursor: text; }
  .agent-terminal-window__composer { display: flex; gap: 8px; padding: 10px 12px; border-top: 1px solid var(--bg-300); background: color-mix(in srgb, var(--bg-200) 84%, transparent); }
  .agent-terminal-window__input { flex: 1; min-width: 0; }
  .window-scroll-container {
    height: 100%;
    overflow-y: auto;
    background: var(--bg);
  }
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
    z-index: 5000 !important;
  }

  /* Agent confirmation stack */
</style>
