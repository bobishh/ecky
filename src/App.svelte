<script lang="ts">
  import DialogueWindowContent from './lib/dialogue/DialogueWindowContent.svelte';
  import Viewer from './lib/Viewer.svelte';
  import VertexGenie from './lib/VertexGenie.svelte';
  import AgentNotificationCenter from './lib/AgentNotificationCenter.svelte';
  import DrawingOverlay from './lib/DrawingOverlay.svelte';
  import ParamPanel from './lib/ParamPanel.svelte';
  import ConfigPanel from './lib/ConfigPanel.svelte';
  import ViewportWorkspace from './lib/workbench/ViewportWorkspace.svelte';
  import WorkbenchWindows from './lib/workbench/WorkbenchWindows.svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { safeSaveDialog } from './lib/safeSaveDialog';
  import { writeTextFile } from '@tauri-apps/plugin-fs';
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { get, writable } from 'svelte/store';
  import { buildGenieTraitsFromSeed, buildModelGenieTraits } from './lib/genie/traits';
  import type { ExplorationCyclePacket } from './lib/explorationCycle';

  import CodeModal from './lib/CodeModal.svelte';
  import SessionActivityWindow from './lib/SessionActivityWindow.svelte';
  import ImportEnrichmentModal from './lib/ImportEnrichmentModal.svelte';
  import ManualImportModal from './lib/ManualImportModal.svelte';
  import AgentTerminalSurface from './lib/AgentTerminalSurface.svelte';
  import DocsHub from './lib/DocsHub.svelte';
  import Modal from './lib/Modal.svelte';
  import Window from './lib/Window.svelte';
  import ProjectSwitcher from './lib/ProjectSwitcher.svelte';
  import CampaignWorkbench from './lib/CampaignWorkbench.svelte';
  import CapturePanel from './lib/CapturePanel.svelte';
  import AnalysisPanel from './lib/AnalysisPanel.svelte';
  import { getAgentActivity, getProjectSource, type FemMeshPreviewResponse, type FemRunResponse } from './lib/tauri/client';
  import type { FemDisplayOptions } from './lib/femDisplay';
  import LibraryPanel from './lib/LibraryPanel.svelte';
  import { campaignDefinitionClient, type CampaignCurrentStepPayload, type CampaignDefinitionSummary } from './lib/projects/campaignDefinitionClient';
  import { campaignRunClient, type CampaignRun } from './lib/projects/campaignRunClient';
  import {
    createCaptureWorkspaceState,
    reduceCaptureWorkspace,
    type CaptureWorkspaceAction,
  } from './lib/capture/captureWorkspaceState';
  import {
    windowStore,
    windowLayoutRemembered,
    loadLayoutForThread,
    loadAppWindowLayout,
    showWindow,
    bringToFront,
    toggleWindow,
    fitVisibleWindowsToViewport,
    setWindowSafeInsets,
    closeWindow as closeWindowStore,
    hardFlush as hardFlushWindowLayout,
    teardown as teardownWindowStore,
    setThreadWindowLayoutRemembered,
    type WindowId,
  } from './lib/stores/windowStore';
  import type { DockLauncherAction } from './lib/workbench/dock';
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
  import { onboarding, shouldAutoStartOnboarding } from './lib/stores/onboarding';
  import { session } from './lib/stores/sessionStore';
  import { startCookingPhraseLoop, stopPhraseLoop } from './lib/stores/phraseEngine';
  import { agentActivityIngestionStore, connectAgentActivityIngestion } from './lib/stores/agentActivity';
  import { agentNotificationsStore } from './lib/stores/agentNotifications';
  import { isActiveLongTaskEvent, isLongTaskEvent, longTasksStore } from './lib/stores/longTasks';
  import { localNotificationActionsStore } from './lib/stores/localNotificationActions';
  import { shouldProjectAgentNotification } from './lib/notificationAggregation';
  import { handleGenerate } from './lib/controllers/requestOrchestrator';
  import { handleParamChange, commitManualVersion, stageParamChange, applyManualCodeDraft, projectManualCodeDraftResult, manualApplyBusyStore } from './lib/controllers/manualController';
  import {
    loadFromHistory,
    createNewThread,
    forkDesign,
    deleteVersion,
    restoreVersion,
    loadVersion,
    refreshHistory,
    refreshThreadHistoryProjection,
    loadOlderThreadMessages,
    rememberCommittedVersionMessage,
    projectWorkspaceProjection,
    activeThreadMessagesLoading,
    activeThreadVersionLoading,
    threadMessagePageState,
  } from './lib/stores/history';
  import { workingCopy, isDirty } from './lib/stores/workingCopy';
  import type { WorkingCopyState } from './lib/stores/workingCopy';
  import {
    historyStore as history,
    activeThreadIdStore as activeThreadId,
    activeVersionId,
    config,
    configLoaded,
    availableModels,
    isLoadingModels,
    runtimeCapabilities,
  } from './lib/stores/domainState';
  import {
    normalizeSketchPreviewDraftScopeId,
  } from './lib/sketchPreviewDraftStore';
  import { selectedCode, selectedTitle, currentView } from './lib/stores/viewState';
  import { boot, saveConfig, fetchModels } from './lib/boot/restore';
  import { requestQueue, allRequests, activeRequests, activeRequestCount, currentActiveRequest, activeThreadBusy, activeThreadRequests } from './lib/stores/requestQueue';
  import { nowSeconds } from './lib/stores/timeEngine';
  import { paramPanelState } from './lib/stores/paramPanelState';
  import { resolveEngineCapabilitySummary } from './lib/modelRuntime/modelCapabilities';
  import { persistLastSessionSnapshot } from './lib/modelRuntime/sessionSnapshot';
  import { getRenderableRuntimeBundle, inspectRuntimeBundle } from './lib/modelRuntime/runtimeBundle';
  import { sameArtifactVersion, shouldPersistVersionPreview } from './lib/versionPreviewPersistence';
  import { resolveDraftPreviewDesign } from './lib/agents/draftPreviewParams';
  import { shouldApplyDraftPreviewToWorkspace } from './lib/agents/draftPreviewProjection';
  import {
    activeRenderSnapshot,
    hydrateActiveRenderSnapshot,
    RenderSnapshotMismatch,
  } from './lib/stores/activeRenderSnapshot';
  import { resolveCodeModalSource, type CodeModalSourceAuthority } from './lib/codeModalSource';
  import { selectProjectFolderWatchEvent } from './lib/projectFolderWatchEvents';
  import {
    deriveThreadAttentionIds,
    deriveMascotStateForThreadAgent,
    derivePrimaryAgentId,
    hasLiveAgentSession,
    resolveActivePendingPrompt,
    shouldAutoFocusAgentWorkingVersion,
    usesAgentDialogueMode,
    usesMcpConnection,
} from './lib/agents/state';
  import { resolveRelayPresence } from './lib/agents/relayPresence';
  import { deriveDialogueState, type DialogueState } from './lib/composables/dialogueState';
  import { applyProviderDialogueAction, createProviderDialogueState, isProviderResultCurrent, mergeProviderPage, mergeProviderSnapshot, projectProviderDialogue, type ProviderDialogueSnapshot } from './lib/providerDialogueSync';
  import type { ProviderCodeReference } from './lib/providerMessagePresentation';
import {
    buildOptimisticQueuedDialogueMessage,
    deriveOptimisticDialogueMessages,
    hasLiveApiEngineConnection,
    mergeOptimisticCodexDialogueMessages,
    mergeOptimisticQueuedDialogueMessages,
    type OptimisticQueuedDialogueMessage,
  } from './lib/composables/apiDialogue';
  import { projectThreadAgentStateFromSessionEvents } from './lib/agents/presentation';
  import {
    deriveViewerBusyState,
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
  import { buildCodeWindowTranspilePrompt } from './lib/cadTranspile';
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
  import { buildImportedEvidence, isForeignCadEvidence } from './lib/modelRuntime/importedEvidence';
  import {
    buildFreecadComponentSource,
    parseFreecadComponentSource,
  } from './lib/modelRuntime/freecadComponentSource';
  import {
    buildPreviewViewTransforms,
    mergePreviewTransforms,
    resolveActivePreviewView,
  } from './lib/modelRuntime/previewViews';
  import {
    provenanceOverlayControls,
  } from './lib/modelRuntime/ownershipSections';
  import {
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
  import { type ExportMode } from './lib/exportOptions';
  import { deriveContextState } from './lib/composables/contextState';
  import { deriveViewportState } from './lib/composables/viewportState';
  import { deriveAgentOpsState, type PendingViewportScreenshotChoice } from './lib/composables/agentOps';
  import { createAgentRuntime } from './lib/composables/agentRuntime';
  import { createModelIo } from './lib/composables/modelIo';
  import { canRepairSavedVersionRuntime, createViewerLoadRuntime, isMissingViewerArtifactError } from './lib/composables/viewerRuntime';
  import { deriveExportState } from './lib/composables/exportOps';
  import {
    composeBubbleEvent,
    composeCodeDiffView,
    composeSessionActivity,
    findNotificationActivityEvent,
    type SessionEvent,
  } from './lib/sessionActivity';
  import {
    recordSessionActivityEvent,
    ingestAgentActivitySessionEvents,
    sessionActivityEvents as sessionActivityEventStore,
  } from './lib/stores/sessionActivityStore';
  import {
    capabilityForAuthoringContext,
    resolveActiveAuthoringContext,
  } from './lib/runtimeCapabilities';
  import { isRenderableVersionTimelineMessage } from './lib/threadTimeline';
  import {
    applyCaptureGuideDraftEdit,
    createCaptureGuideDraftHistory,
    mechanicalGuideReadiness,
    type CaptureFeatureExpectationEdit,
    type CaptureGuideDraftHistory,
    type CaptureLandmarkEdit,
    type CaptureProfileEdit,
  } from './lib/capture/captureGuideDraft';
  import {
    clearSketchPreviewDraft,
    saveSketchPreviewDraft,
    exportFile,
    exportMultipart3mf,
    exportMultipartStlZip,
    formatBackendError,
    getAgyProvider,
    getAgyProviderMessages,
    getCodexTakeover,
    getCodexTakeoverMessages,
    getAgentDraftPreview,
    getActiveAgentSessions,
    getActiveExplorationCycle,
    getAgentTerminalSnapshots,
    getWebContentRecoveryState,
    acknowledgeWebContentRecovery,
    getThreadMessageVersion,
    getVersionSource,
    applyCapturePreview as applyCapturePreviewCommand,
    importModelIntent,
    preparePromptAttachments,
    preparePromptWorkspaceCapture,
    projectFolderRenderActivity,
    prepareCapturePreview,
    adoptLatestCaptureRun,
    listCaptureRuns,
    listExternalShapeSources,
    applyExternalShapeEdit,
    applyInlineComponentImport,
    applySemanticControlValue,
    previewExternalShapeSurfaceTrimPath,
    previewExternalShapeSurfaceTrimLoop,
    previewExternalShapeSurfaceTrimRegion,
    reopenCaptureRun,
    saveCapturePreviewSettings,
    ensureCaptureReconstructionGuide,
    applyCaptureGuideEditIntent,
    validateCaptureGuideIntent,
    queueCaptureGuidedReconstruction,
    retryCaptureReconstruction,
    resumeCaptureSession,
    startCaptureSession as startCaptureSessionCommand,
    getCaptureSessionStatus as getCaptureSessionStatusCommand,
    cancelCaptureSession as cancelCaptureSessionCommand,
    rejectAgentViewportScreenshot,
    resizeAgentTerminal,
    queueAgentPrompt,
    removeAgyQueuedPrompt,
    removeCodexQueuedPrompt,
    retryAgyQueuedPrompt,
    retryCodexQueuedPrompt,
    resolveAgentConfirm,
    submitAgentPromptReply,
    resolveAgentViewportScreenshot,
    saveConfig as persistBackendConfig,
    sendAgentTerminalInput,
    sendAgyProviderPrompt,
    sendCodexTakeoverPrompt,
    steerCodexTakeover,
    stopExplorationRun,
    stopAgyProvider,
    stopCodexTakeover,
    repairVersionRuntime,
    updateVersionPreview,
    type PostProcessingSpec,
    type AgyProviderSnapshot,
    type CodexTakeoverSnapshot,
  } from './lib/tauri/client';
  import { listen } from '@tauri-apps/api/event';
  import type {
    CaptureCropBounds,
    CaptureGuideResultProvenance,
    CaptureGuideEditIntent,
    CaptureGuideSourceMesh,
    CaptureLandmarkRole,
    CaptureMeshPreview,
    CaptureObservedDeviationReport,
    CaptureReconstructionGuide,
    CaptureReconstructionGuideState,
    CaptureRun,
    ExternalShapeSource,
    CaptureSurfaceAnchor,
    SurfaceTrimCapMode,
    SurfaceTrimLoopPreviewResponse,
    SurfaceTrimPathMode,
    SurfaceTrimPathPreviewResponse,
    SurfaceTrimRegionPreviewResponse,
    ReopenedCaptureRun,
    FreecadLibraryItem,
    SketchDocument,
    SketchDraftSource,
  } from './lib/tauri/contracts';
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
    Thread,
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
  type AgentDraftPreviewChangedEvent = {
    sessionId: string;
    threadId: string;
    previewId: string;
    baseMessageId?: string | null;
    modelId?: string | null;
    revision: number;
    feedbackStatus?: 'checking' | 'passed' | 'failed' | 'warning' | null;
    feedbackSummary?: string | null;
  };
  const latestDraftPreviewRevision = new Map<string, number>();
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
    modelStlPath: string;
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
  type ProjectFolderWatchEvent =
    | { kind: 'detected'; slug: string; threadId: string }
    | { kind: 'applied'; slug: string; threadId: string; messageId: string; modelId?: string | null }
    | { kind: 'applyFailed'; slug: string; threadId: string; messageId: string; error: string };
  type ProjectFolderNotice = {
    tone: 'pending' | 'success' | 'error';
    title: string;
    body: string;
    threadId: string;
    messageId: string | null;
  };
  type GeometryRenderActivityEvent = {
    activeCount: number;
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
    sketchDocument?: SketchDocument | null;
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
    messageId?: string | null;
    sourceLanguage?: SourceLanguage | null;
    geometryBackend?: GeometryBackend | null;
    expectedSourcePath?: string | null;
    highlightLine?: number | null;
    throwSourceError?: boolean;
  }) {
    const openRequest = ++codeModalOpenRequest;
    codeModalHighlightLine = seed?.highlightLine ?? null;
    const shouldReopenDocs = $windowStore.docs.visible;
    if (!$activeThreadId) {
      const createdThreadId = await createNewThread({ mode: 'blank' });
      // The history effect loads the per-thread window layout asynchronously.
      // Await that load here, otherwise it can overwrite the just-opened code
      // window with the blank layout.
      if (createdThreadId) await loadLayoutForThread(createdThreadId);
      await tick();
      if (shouldReopenDocs) {
        showWindow('docs');
        await tick();
      }
    }

    const activeId = $activeThreadId;
    const sourceThreadId = activeId;
    if (!sourceThreadId) {
      // Keep the inspector available even when backend thread creation is
      // unavailable (for example during a fresh, offline boot). Docs snippets
      // and manual editing still have useful draft content without a thread.
      codeModalMode = 'version';
      codeModalSourceAuthority = 'draft';
      codeModalSourceThreadId = null;
      codeModalSourceLanguage = seed?.sourceLanguage ?? $config.defaultSourceLanguage;
      codeModalDraftSerial += 1;
      codeModalDraftScopeKey = `draft:no-thread:${codeModalDraftSerial}`;
      selectedCode.set(seed?.code ?? '');
      selectedTitle.set(codeInspectorTitle(
        seed?.title ?? 'Manual Edit',
        codeModalSourceLanguage,
        seed?.geometryBackend ?? $config.defaultGeometryBackend,
      ));
      mountedWindows.code = true;
      showWindow('code');
      return;
    }
    const current = get(workingCopy);
    const versionManifest = activeVersionMessage?.modelManifest ?? activeModelManifest ?? null;
    const foreignEvidenceMode = !seed?.expectedSourcePath && isForeignCadEvidence(versionManifest);
    const nextTitle = seed?.title ?? (current.title || 'Manual Edit');
    const currentHasSource = current.macroCode.trim().length > 0;
    const nextSourceLanguage = seed?.sourceLanguage ?? (
      currentHasSource ? current.sourceLanguage : $config.defaultSourceLanguage
    );
    const nextGeometryBackend = seed?.geometryBackend ?? (
      currentHasSource ? current.geometryBackend : $config.defaultGeometryBackend
    );

    if (foreignEvidenceMode) {
      const versionBundle = activeVersionMessage?.artifactBundle ?? activeArtifactBundle ?? null;
      if (!versionBundle) {
        session.setError('Source Error: imported component artifact bundle is not loaded.');
        return;
      }
      const importedUiSpec = buildImportedUiSpec(versionManifest);
      const importedParams = buildImportedParams(
        versionManifest,
        $paramPanelState.params ?? {},
        importedUiSpec,
      );
      codeModalEvidence = buildImportedEvidence(versionManifest);
      codeModalMode = 'foreign-evidence';
      codeModalSourceAuthority = 'bound';
      codeModalSourceThreadId = sourceThreadId;
      codeModalSourceLanguage = nextSourceLanguage;
      codeModalDraftSerial += 1;
      codeModalDraftScopeKey = [
        'version',
        activeId,
        seed?.messageId ?? $activeVersionId ?? current.sourceVersionId ?? 'draft',
        codeModalDraftSerial,
      ].join(':');
      selectedCode.set(buildFreecadComponentSource({
        artifactBundle: versionBundle,
        manifest: versionManifest,
        parameters: importedParams,
        uiSpec: importedUiSpec,
      }));
      selectedTitle.set(`${nextTitle}.FREECAD-COMPONENT`);
      mountedWindows.code = true;
      showWindow('code');
      return;
    }

    const renderSnapshot = get(activeRenderSnapshot);
    const loadedModelId = get(session).artifactBundle?.modelId ?? null;
    const activeRenderMatchesViewport = Boolean(
      renderSnapshot &&
        renderSnapshot.threadId === sourceThreadId &&
        renderSnapshot.artifactBundle.modelId === loadedModelId,
    );
    let initialCode = seed?.code ?? (
      activeRenderMatchesViewport
        ? renderSnapshot?.design.macroCode ?? current.macroCode
        : current.macroCode
    );
    codeModalEvidence = '';
    codeModalMode = 'version';
    codeModalSourceAuthority = activeRenderMatchesViewport ? 'draft' : 'bound';
    codeModalSourceThreadId = sourceThreadId;
    codeModalSourceLanguage = nextSourceLanguage;
    codeModalDraftSerial += 1;
    codeModalDraftScopeKey = [
      'version',
      $activeThreadId ?? 'no-thread',
      seed?.messageId ?? $activeVersionId ?? current.sourceVersionId ?? 'draft',
      codeModalDraftSerial,
    ].join(':');
    selectedCode.set(initialCode);
    selectedTitle.set(codeInspectorTitle(nextTitle, nextSourceLanguage, nextGeometryBackend));
    // Backend source reads may wait behind a render. The inspector must open
    // immediately from the exact source already associated with the viewport.
    mountedWindows.code = true;
    showWindow('code');
    codeModalSourceMessageId = null;
    if (seed?.messageId && !seed.expectedSourcePath) {
      try {
        initialCode = await getVersionSource(sourceThreadId, seed.messageId) ?? seed.code ?? initialCode;
        if (openRequest !== codeModalOpenRequest || !$windowStore.code.visible) return;
        codeModalMode = 'version';
        codeModalSourceAuthority = 'draft';
        codeModalSourceThreadId = sourceThreadId;
        codeModalSourceMessageId = seed.messageId;
        codeModalSourceLanguage = nextSourceLanguage;
        selectedCode.set(initialCode);
        selectedTitle.set(codeInspectorTitle(nextTitle, nextSourceLanguage, nextGeometryBackend));
        return;
      } catch (error) {
        session.setError(`Source Error: ${formatBackendError(error)}`);
        return;
      }
    }

    let boundSource;
    try {
      boundSource = await getProjectSource(sourceThreadId);
      if (openRequest !== codeModalOpenRequest || !$windowStore.code.visible) return;
    } catch (error) {
      const message = `Source Error: ${formatBackendError(error)}`;
      if (seed?.throwSourceError) throw new Error(message);
      session.setError(message);
      return;
    }

    if (seed?.expectedSourcePath && boundSource.file !== seed.expectedSourcePath) {
      const message = `Source Error: referenced model no longer matches current thread source. Referenced: ${seed.expectedSourcePath}. Current: ${boundSource.file}.`;
      if (seed.throwSourceError) throw new Error(message);
      session.setError(message);
      return;
    }

    const codeSource = seed?.expectedSourcePath
      ? { source: boundSource.source, authority: 'bound' as const }
      : resolveCodeModalSource({
          activeRenderSource: renderSnapshot?.design.macroCode ?? seed?.code,
          boundSource: boundSource.source,
          activeRenderMatchesViewport,
        });
    const nextCode = codeSource.source;
    codeModalSourceAuthority = codeSource.authority;

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
    codeModalSourceThreadId = boundSource.threadId;
    codeModalSourceLanguage = nextSourceLanguage;
    if (get(selectedCode) === initialCode) selectedCode.set(nextCode);
    selectedTitle.set(codeInspectorTitle(nextTitle, nextSourceLanguage, nextGeometryBackend));
  }

  async function refreshOpenCodeModalHead(threadId: string): Promise<void> {
    if (!$windowStore.code.visible || codeModalMode !== 'version') return;
    if (threadId !== get(activeThreadId)) return;
    if (codeModalSourceAuthority !== 'bound' || codeModalSourceMessageId !== null) return;

    try {
      const head = await getProjectSource(threadId);
      if (threadId !== get(activeThreadId) || !$windowStore.code.visible) return;

      const current = get(workingCopy);
      const sourceLanguage: SourceLanguage = head.file.toLowerCase().endsWith('.ecky')
        ? 'ecky'
        : current.sourceLanguage;
      const geometryBackend = sourceLanguage === 'ecky'
        ? $config.defaultGeometryBackend
        : current.geometryBackend;

      codeModalSourceAuthority = 'bound';
      codeModalSourceThreadId = head.threadId;
      codeModalSourceLanguage = sourceLanguage;
      selectedCode.set(head.source);
      selectedTitle.set(codeInspectorTitle(current.title || 'Manual Edit', sourceLanguage, geometryBackend));
      workingCopy.patch({
        macroCode: head.source,
        sourceLanguage,
        geometryBackend,
        dirty: false,
      });
    } catch (error) {
      session.setError(`Source Error: ${formatBackendError(error)}`);
    }
  }

  function closeCodeModal() {
    codeModalOpenRequest += 1;
    closeWindowStore('code');
  }

  // Close the inspector once the version append lands; a failed apply throws
  // before this and keeps the modal open with the raw error.
  async function applyManualVersionAndClose(
    payload: Parameters<typeof commitManualVersion>[0],
  ) {
    await commitManualVersion(payload);
    closeCodeModal();
  }

  async function applyCodeModalSource(
    payload: Parameters<typeof commitManualVersion>[0],
  ) {
    if (codeModalMode !== 'foreign-evidence') {
      return applyManualVersionAndClose(payload);
    }
    if (typeof payload === 'string') throw new Error('Imported component apply payload is invalid.');
    const bundle = activeVersionMessage?.artifactBundle ?? activeArtifactBundle ?? null;
    const manifest = activeVersionMessage?.modelManifest ?? activeModelManifest ?? null;
    if (!bundle || !manifest || !isForeignCadEvidence(manifest)) {
      throw new Error('Imported component runtime is not loaded.');
    }
    if (activeArtifactBundle?.modelId !== bundle.modelId) {
      throw new Error('Imported component runtime is still loading.');
    }
    const uiSpec = buildImportedUiSpec(manifest);
    const parameters = parseFreecadComponentSource(payload.code, bundle, uiSpec);
    workingCopy.patch({
      title: payload.title ?? undefined,
      versionName: payload.versionName ?? undefined,
    });
    const committed = await handleParamChange(parameters, null, true);
    if (!committed) throw new Error('Imported component Apply failed.');
    closeCodeModal();
  }

  function handleDockWindowActivate(id: WindowId, action: DockLauncherAction) {
    if (action === 'focus') {
      bringToFront(id);
      return;
    }
    if (action === 'close') {
      closeWindowStore(id);
      return;
    }
    if (id === 'code') {
      void openVersionCodeModal();
      return;
    }
    showWindow(id);
  }

  async function handleTranslateCodeToEcky(source: string): Promise<void> {
    await handlePromptPanelSubmit(buildCodeWindowTranspilePrompt(source), []);
    closeCodeModal();
    showWindow('dialogue');
  }

  // Local reactive aliases for templates
  const phase = $derived($session.phase);
  const status = $derived($session.status);
  const error = $derived($session.error);
  const errorText = $derived(error ? formatBackendError(error) : null);
  const globalErrorText = $derived(
    $session.globalError ? formatBackendError($session.globalError) : null,
  );
  const sessionAuthoringError = $derived(
    error && typeof error !== 'string' && (error.layer || error.fix)
      ? { layer: error.layer, fix: error.fix }
      : null,
  );
  const stlUrl = $derived($session.stlUrl);
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
  let campaignDefinitions = $state<CampaignDefinitionSummary[]>([]);
  let campaignStep = $state<CampaignCurrentStepPayload | null>(null);
  let campaignRuns = $state<CampaignRun[]>([]);
  let activeCampaignRun = $state<CampaignRun | null>(null);
  let campaignRunError = $state<string | null>(null);
  let projectFolderNotice = $state<ProjectFolderNotice | null>(null);
  let geometryRenderActiveCount = $state(0);
  let projectFolderWatchEventRevision = 0;
  let projectFolderActivitySnapshotInFlight = false;
  const observedProjectFolderRenderThreads = new Set<string>();

  function applyProjectFolderWatchEvent(latest: ProjectFolderWatchEvent): void {
    projectFolderWatchEventRevision += 1;
    if (latest.kind === 'detected') {
      projectFolderNotice = {
        tone: 'pending',
        title: 'SOURCE RENDERING',
        body: `${latest.slug}/model.ecky changed externally. Rendering the settled source.`,
        threadId: latest.threadId,
        messageId: null,
      };
    } else if (latest.kind === 'applied') {
      projectFolderNotice = {
        tone: 'success',
        title: 'SOURCE APPLIED',
        body: `${latest.slug}/model.ecky validated and committed as a new version.`,
        threadId: latest.threadId,
        messageId: latest.messageId,
      };
    } else {
      projectFolderNotice = {
        tone: 'error',
        title: 'SOURCE APPLY FAILED',
        body: latest.error,
        threadId: latest.threadId,
        messageId: latest.messageId,
      };
    }
  }

  async function reconcileProjectFolderRenderActivity(): Promise<void> {
    if (projectFolderActivitySnapshotInFlight) return;
    projectFolderActivitySnapshotInFlight = true;
    const revisionBeforeSnapshot = projectFolderWatchEventRevision;
    try {
      const activeRenders = await projectFolderRenderActivity();
      if (projectFolderWatchEventRevision !== revisionBeforeSnapshot) return;

      const activeThread = get(activeThreadId) ?? null;
      const latest = activeThread
        ? activeRenders.find((activity) => activity.threadId === activeThread)
        : undefined;
      if (latest) {
        observedProjectFolderRenderThreads.add(latest.threadId);
        if (
          projectFolderNotice?.tone !== 'pending' ||
          projectFolderNotice.threadId !== latest.threadId
        ) {
          applyProjectFolderWatchEvent({ kind: 'detected', ...latest });
        }
        return;
      }

      if (
        projectFolderNotice?.tone === 'pending' &&
        observedProjectFolderRenderThreads.delete(projectFolderNotice.threadId)
      ) {
        projectFolderWatchEventRevision += 1;
        projectFolderNotice = null;
      }
    } catch {
      // Recovery snapshot only. Live watcher events remain authoritative.
    } finally {
      projectFolderActivitySnapshotInFlight = false;
    }
  }
  let sketchPreview = $state<SketchPreviewState | null>(null);
  let sketchPreviewDraft = $state<SketchPreviewDraftState | null>(null);
  const sketchWorkspaceAvailable = false;
  let codeModalMode = $state<'version' | 'foreign-evidence' | 'sketch-preview' | 'docs-snippet'>('version');
  let codeModalSourceThreadId = $state<string | null>(null);
  let codeModalSourceMessageId = $state<string | null>(null);
  let codeModalSourceLanguage = $state<SourceLanguage | null>(null);
  let codeModalEvidence = $state('');
  let codeModalSourceAuthority = $state<CodeModalSourceAuthority>('bound');
  let codeModalHighlightLine = $state<number | null>(null);
  let codeModalDraftSerial = $state(0);
  let codeModalDraftScopeKey = $state('');
  let codeModalOpenRequest = 0;
  let activeDraftFeedback = $state<AgentDraftFeedback | null>(null);

  const isBooting = $derived(phase === 'booting');
  const isQuestionFlow = $derived(phase === 'answering');
  const isMcpConnection = $derived(usesMcpConnection($config.connectionType));
  const projectedThreadAgentState = $derived.by(() =>
    projectThreadAgentStateFromSessionEvents($sessionActivityEventStore, $activeThreadId ?? null),
  );
  const usesQueuedAgentDialogue = $derived.by<boolean>(() =>
    usesAgentDialogueMode($config.connectionType, projectedThreadAgentState),
  );
  let activeAgentSessions = $state<AgentSession[]>([]);
  const primaryAgentId = $derived.by<string | null>(() =>
    derivePrimaryAgentId($config.mcp.autoAgents ?? [], $config.mcp.primaryAgentId ?? null),
  );
  const primaryAgentLabel = $derived.by<string | null>(() =>
    $config.mcp.autoAgents.find((agent) => agent.id === primaryAgentId)?.label ?? null,
  );
  const visibleAgentTerminal = $derived($visibleAgentTerminalStore);
  const activeAgentTerminalAttention = $derived($agentTerminalAttentionStore);
  const activeThread = $derived($history.find((t) => t.id === $activeThreadId));
  const codexDialogueStateStore = writable(createProviderDialogueState());
  const agyDialogueStateStore = writable(createProviderDialogueState());
  const codexDialogueState = $derived($codexDialogueStateStore);
  const agyDialogueState = $derived($agyDialogueStateStore);
  const codexTakeoverSnapshot = $derived(codexDialogueState.snapshot as CodexTakeoverSnapshot | null);
  const agyProviderSnapshot = $derived(agyDialogueState.snapshot as AgyProviderSnapshot | null);
  const codexTakeoverLoading = $derived($config.connectionType === 'provider:agy' ? agyDialogueState.loading : codexDialogueState.loading);
  const codexTakeoverError = $derived($config.connectionType === 'provider:agy' ? agyDialogueState.error : codexDialogueState.error);
  const codexTakeoverLoadToken = $derived(codexDialogueState.loadToken);
  let codexTakeoverRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  const agyProviderLoadToken = $derived(agyDialogueState.loadToken);
  let agyProviderRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  let optimisticQueuedAgentMessages = $state<Record<string, OptimisticQueuedDialogueMessage>>({});
  const activeProviderSnapshot = $derived.by<CodexTakeoverSnapshot | AgyProviderSnapshot | null>(() =>
    $config.connectionType === 'provider:agy' ? agyProviderSnapshot : codexTakeoverSnapshot,
  );
  function reduceCodexProvider(action: Parameters<typeof applyProviderDialogueAction>[1]) {
    codexDialogueStateStore.update((state) => applyProviderDialogueAction(state, action));
  }
  function reduceAgyProvider(action: Parameters<typeof applyProviderDialogueAction>[1]) {
    agyDialogueStateStore.update((state) => applyProviderDialogueAction(state, action));
  }
  function setProviderError(error: string | null) {
    if ($config.connectionType === 'provider:agy') {
      reduceAgyProvider(error === null ? { type: 'clearError' } : { type: 'error', error });
      return;
    }
    reduceCodexProvider(error === null ? { type: 'clearError' } : { type: 'error', error });
  }
  const codexDialogueMessages = $derived.by<Message[]>(() =>
    activeProviderSnapshot
      ? projectProviderDialogue({
        ...activeProviderSnapshot,
        providerId: $config.connectionType === 'provider:agy' ? 'agy' : 'codex',
        providerLabel: $config.connectionType === 'provider:agy' ? 'Agy' : 'Codex',
        externalConversationId: 'agyConversationId' in activeProviderSnapshot.binding
          ? activeProviderSnapshot.binding.agyConversationId
          : activeProviderSnapshot.binding.codexThreadId,
      } as ProviderDialogueSnapshot)
      : [],
  );
  const activeThreadDialogueMessages = $derived.by(() => {
    const eckyMessages = deriveOptimisticDialogueMessages(
      activeThread?.messages ?? [],
      $activeThreadRequests,
    );
    if (activeProviderSnapshot) {
      return mergeOptimisticCodexDialogueMessages(
        eckyMessages,
        codexDialogueMessages,
        [],
        $activeThreadId ?? null,
      );
    }
    return mergeOptimisticQueuedDialogueMessages(
      eckyMessages,
      Object.values(optimisticQueuedAgentMessages),
      $activeThreadId ?? null,
    );
  });
  let explorationCycle = $state<ExplorationCyclePacket | null>(null);
  let explorationCycleLoadSequence = 0;
  $effect(() => {
    const threadId = $activeThreadId;
    void activeThreadDialogueMessages.length;
    void $activeThreadRequests.length;
    const sequence = ++explorationCycleLoadSequence;
    if (!threadId) {
      explorationCycle = null;
      return;
    }
    void getActiveExplorationCycle(threadId)
      .then((packet) => {
        if (sequence === explorationCycleLoadSequence) explorationCycle = packet;
      })
      .catch(() => {
        if (sequence === explorationCycleLoadSequence) explorationCycle = null;
      });
  });
  $effect(() => {
    const threadId = $activeThreadId;
    const usesCodexProvider = $config.connectionType === 'provider:codex';
    const usesAgyProvider = $config.connectionType === 'provider:agy';
    untrack(() => {
      reduceCodexProvider({ type: 'snapshot', snapshot: null, preserveLoadedPages: false });
      reduceAgyProvider({ type: 'snapshot', snapshot: null, preserveLoadedPages: false });
      if (threadId && usesCodexProvider) void loadCodexTakeover(threadId, false);
      if (threadId && usesAgyProvider) void loadAgyProvider(threadId, false);
    });
  });

  function applyCodexTakeoverSnapshot(next: CodexTakeoverSnapshot, preserveLoadedPages: boolean, authoritative = false) {
    reduceCodexProvider(authoritative
      ? { type: 'authoritativeSnapshot', snapshot: next as unknown as ProviderDialogueSnapshot, preserveLoadedPages }
      : { type: 'snapshot', snapshot: next as unknown as ProviderDialogueSnapshot, preserveLoadedPages });
  }

  async function loadCodexTakeover(threadId: string, preserveLoadedPages: boolean) {
    const token = codexTakeoverLoadToken + 1;
    reduceCodexProvider({ type: 'loadStarted', token });
    try {
      const snapshot = await getCodexTakeover(threadId);
      if (token !== codexTakeoverLoadToken || get(activeThreadId) !== threadId) return;
      if (snapshot) applyCodexTakeoverSnapshot(snapshot, preserveLoadedPages);
      else reduceCodexProvider({ type: 'snapshot', snapshot: null, preserveLoadedPages: false, token });
    } catch (error) {
      if (token === codexTakeoverLoadToken && get(activeThreadId) === threadId) {
        reduceCodexProvider({ type: 'error', error: formatBackendError(error) });
      }
    } finally {
      if (token === codexTakeoverLoadToken && !codexDialogueState.snapshot) reduceCodexProvider({ type: 'snapshot', snapshot: null, preserveLoadedPages: false, token });
    }
  }

  function applyAgyProviderSnapshot(next: AgyProviderSnapshot, preserveLoadedPages: boolean, authoritative = false) {
    reduceAgyProvider(authoritative
      ? { type: 'authoritativeSnapshot', snapshot: next as unknown as ProviderDialogueSnapshot, preserveLoadedPages }
      : { type: 'snapshot', snapshot: next as unknown as ProviderDialogueSnapshot, preserveLoadedPages });
  }

  function agyProviderError(snapshot: AgyProviderSnapshot | null | undefined): string | null {
    return snapshot?.runtime.error
      ?? snapshot?.queue.find((item) => item.status === 'failed')?.error
      ?? null;
  }

  async function loadAgyProvider(threadId: string, preserveLoadedPages: boolean) {
    const token = agyProviderLoadToken + 1;
    reduceAgyProvider({ type: 'loadStarted', token });
    try {
      const snapshot = await getAgyProvider(threadId);
      if (token !== agyProviderLoadToken || get(activeThreadId) !== threadId) return;
      if (snapshot) applyAgyProviderSnapshot(snapshot, preserveLoadedPages);
      else reduceAgyProvider({ type: 'snapshot', snapshot: null, preserveLoadedPages: false, token });
    } catch (error) {
      if (token === agyProviderLoadToken && get(activeThreadId) === threadId) {
        reduceAgyProvider({ type: 'error', error: formatBackendError(error) });
      }
    } finally {
      // reducer owns loading state; terminal snapshot clears it
    }
  }

  async function handleLoadEarlierCodexMessages() {
    if ($config.connectionType === 'provider:agy') {
      const snapshot = agyProviderSnapshot;
      if (!snapshot?.nextCursor) return;
      try {
        const page = await getAgyProviderMessages({
          eckyThreadId: snapshot.binding.eckyThreadId,
          cursor: snapshot.nextCursor,
          direction: 'older',
        });
        reduceAgyProvider({ type: 'page', page, direction: 'older' });
      } catch (error) {
        setProviderError(formatBackendError(error));
        throw error;
      }
      return;
    }
    const snapshot = codexTakeoverSnapshot;
    if (!snapshot?.nextCursor) return;
    try {
      const page = await getCodexTakeoverMessages({
        eckyThreadId: snapshot.binding.eckyThreadId,
        cursor: snapshot.nextCursor,
        direction: 'older',
      });
      reduceCodexProvider({ type: 'page', page, direction: 'older' });
    } catch (error) {
      setProviderError(formatBackendError(error));
      throw error;
    }
  }

  async function handleCodexSteer(prompt: string) {
    const snapshot = codexTakeoverSnapshot;
    const expectedTurnId = snapshot?.runtime.activeTurnId;
    if (!snapshot || !expectedTurnId) throw new Error('No active Codex turn to steer.');
    try {
      const next = await steerCodexTakeover({
        eckyThreadId: snapshot.binding.eckyThreadId,
        promptText: prompt,
        expectedTurnId,
      });
      applyCodexTakeoverSnapshot(next, true, true);
    } catch (error) {
      setProviderError(formatBackendError(error));
      throw error;
    }
  }

  async function handleStopCodexTakeover() {
    if ($config.connectionType === 'provider:agy') {
      const snapshot = agyProviderSnapshot;
      const turnId = snapshot?.runtime.activeTurnId;
      if (!snapshot || !turnId) return;
      try {
        applyAgyProviderSnapshot(await stopAgyProvider({
          eckyThreadId: snapshot.binding.eckyThreadId,
          turnId,
        }), true, true);
      } catch (error) {
        setProviderError(formatBackendError(error));
        throw error;
      }
      return;
    }
    const snapshot = codexTakeoverSnapshot;
    const turnId = snapshot?.runtime.activeTurnId;
    if (!snapshot || !turnId) return;
    try {
      const next = await stopCodexTakeover({
        eckyThreadId: snapshot.binding.eckyThreadId,
        turnId,
      });
      applyCodexTakeoverSnapshot(next, true, true);
    } catch (error) {
      setProviderError(formatBackendError(error));
      throw error;
    }
  }

  async function handleRetryCodexQueue(queueId: string) {
    if ($config.connectionType === 'provider:agy') {
      const eckyThreadId = agyProviderSnapshot?.binding.eckyThreadId;
      if (!eckyThreadId) return;
      try {
        applyAgyProviderSnapshot(await retryAgyQueuedPrompt(eckyThreadId, queueId), true, true);
      } catch (error) {
        setProviderError(formatBackendError(error));
        throw error;
      }
      return;
    }
    const eckyThreadId = codexTakeoverSnapshot?.binding.eckyThreadId;
    if (!eckyThreadId) return;
    try {
      const next = await retryCodexQueuedPrompt(eckyThreadId, queueId);
      applyCodexTakeoverSnapshot(next, true, true);
    } catch (error) {
      setProviderError(formatBackendError(error));
      throw error;
    }
  }

  async function handleRemoveCodexQueue(queueId: string) {
    if ($config.connectionType === 'provider:agy') {
      const eckyThreadId = agyProviderSnapshot?.binding.eckyThreadId;
      if (!eckyThreadId) return;
      try {
        applyAgyProviderSnapshot(await removeAgyQueuedPrompt(eckyThreadId, queueId), true, true);
      } catch (error) {
        setProviderError(formatBackendError(error));
        throw error;
      }
      return;
    }
    const eckyThreadId = codexTakeoverSnapshot?.binding.eckyThreadId;
    if (!eckyThreadId) return;
    try {
      const next = await removeCodexQueuedPrompt(eckyThreadId, queueId);
      applyCodexTakeoverSnapshot(next, true, true);
    } catch (error) {
      setProviderError(formatBackendError(error));
      throw error;
    }
  }

  function scheduleCodexTakeoverRefresh(event: {
    threadId: string;
    method: string;
    liveMessages?: CodexTakeoverSnapshot['liveMessages'];
    turnTraces?: CodexTakeoverSnapshot['turnTraces'];
    runtime?: CodexTakeoverSnapshot['runtime'];
  }) {
    const snapshot = codexTakeoverSnapshot;
    if (!snapshot || snapshot.binding.codexThreadId !== event.threadId) return;
    const terminalEvent = event.method === 'turn/completed'
      || event.method === 'thread/status/changed' && !event.runtime?.activeTurnId;
    if (event.liveMessages && event.runtime) {
      reduceCodexProvider({ type: 'live', liveMessages: event.liveMessages, turnTraces: event.turnTraces, runtime: event.runtime });
      if (!terminalEvent) return;
    }
    if (codexTakeoverRefreshTimer) clearTimeout(codexTakeoverRefreshTimer);
    codexTakeoverRefreshTimer = setTimeout(() => {
      codexTakeoverRefreshTimer = null;
      const eckyThreadId = snapshot.binding.eckyThreadId;
      const refresh = getCodexTakeover(eckyThreadId);
      void refresh
        .then((next) => {
          if (!isProviderResultCurrent(codexTakeoverSnapshot, eckyThreadId)) return;
          if (next) applyCodexTakeoverSnapshot(next, true);
          setProviderError(next?.runtime.error ?? null);
        })
        .catch((error) => { setProviderError(formatBackendError(error)); });
    }, terminalEvent ? 40 : 250);
  }

  function scheduleAgyProviderRefresh(event: {
    conversationId?: string | null;
    method: string;
    liveMessages?: AgyProviderSnapshot['liveMessages'];
    turnTraces?: AgyProviderSnapshot['turnTraces'];
    runtime?: AgyProviderSnapshot['runtime'];
  }) {
    const snapshot = agyProviderSnapshot;
    if (!snapshot || event.conversationId !== snapshot.binding.agyConversationId) return;
    const terminalEvent = event.method === 'turn/result' || event.method === 'turn/terminal';
    if (event.liveMessages && event.runtime) {
      reduceAgyProvider({ type: 'live', liveMessages: event.liveMessages, turnTraces: event.turnTraces, runtime: event.runtime });
      if (!terminalEvent) return;
    }
    if (agyProviderRefreshTimer) clearTimeout(agyProviderRefreshTimer);
    agyProviderRefreshTimer = setTimeout(() => {
      agyProviderRefreshTimer = null;
      const eckyThreadId = snapshot.binding.eckyThreadId;
      const refresh = getAgyProvider(eckyThreadId);
      void refresh
        .then((next) => {
          if (!isProviderResultCurrent(agyProviderSnapshot, eckyThreadId)) return;
          if (next) applyAgyProviderSnapshot(next, true);
          setProviderError(agyProviderError(next));
        })
        .catch((error) => { setProviderError(formatBackendError(error)); });
    }, terminalEvent ? 40 : 250);
  }

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
      sessionModelManifest: sessionModelManifest ?? activeVersionMessage?.modelManifest ?? null,
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
  const exactProvenanceOverlayControls = $derived.by(() =>
    provenanceOverlayControls({
      manifest: activeModelManifest,
      runtime: activeArtifactBundle,
      fields: effectiveUiSpec.fields || [],
      parameters: effectiveParameters,
      target: selectedTarget,
    }),
  );
  const overlayControls = $derived.by(() =>
    exactProvenanceOverlayControls.length > 0
      ? exactProvenanceOverlayControls
      : contextState.overlayControls,
  );
  const enableViewportContextOverlay = $derived(
    viewerMode === 'select' && exactProvenanceOverlayControls.length > 0,
  );
  const overlayAdvisories = $derived.by(() => contextState.overlayAdvisories);
  const activeMeasurementCallout = $derived.by(() => contextState.activeMeasurementCallout);
  $effect(() => {
    activeControlViewId = contextState.resolvedActiveControlViewId;
  });
  $effect(() => {
    activePreviewViewId = activePreviewView?.viewId ?? null;
  });
  const geometryRenderActive = $derived(geometryRenderActiveCount > 0);
  const suppressViewportBusyUi = $derived(isBooting);
  const projectFolderRenderPending = $derived(
    !suppressViewportBusyUi &&
      projectFolderNotice?.tone === 'pending' &&
      projectFolderNotice.threadId === ($activeThreadId ?? null),
  );
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
      geometryRenderActive,
      projectFolderRenderPending,
    }),
  );
  const showViewerBusyMask = $derived(viewerBusyState.showViewerBusyMask);
  const viewerBusyPhase = $derived<ViewerBusyPhase>(viewerBusyState.viewerBusyPhase);
  const viewerBusyText = $derived<string | null>(viewerBusyState.viewerBusyText);
  let lastViewerBusyTrace = '';
  $effect(() => {
    const trace = {
      event: 'viewer.busy',
      at: Date.now(),
      showMask: showViewerBusyMask,
      busyPhase: viewerBusyPhase,
      busyText: viewerBusyText,
      geometryRenderActiveCount,
    };
    const signature = JSON.stringify(trace, (key, value) => key === 'at' ? undefined : value);
    if (signature === lastViewerBusyTrace) return;
    lastViewerBusyTrace = signature;
    console.warn('[CAD_FLOW][viewer.busy]', trace);
    const flow = ((globalThis as any).__ECKY_CAD_FLOW__ ??= []);
    flow.push(trace);
  });

  const viewportState = $derived.by(() =>
    deriveViewportState({
      activeArtifactBundle,
      activeThreadId: $activeThreadId ?? null,
      activeThreadMessages: activeThreadDialogueMessages,
      activeVersionId: $activeVersionId ?? null,
      activeVersionMessage,
      cameraStateByTarget,
      runtimeRevision: $session.runtimeRevision,
      stlUrl,
      toAssetUrl,
    }),
  );
  const viewerAssets = $derived.by<ViewerAsset[]>(() => viewportState.viewerAssets);
  const hasSketchPreview = $derived(sketchWorkspaceAvailable && Boolean(sketchPreview?.artifactBundle));
  const sketchPreviewStlUrl = $derived.by<string | null>(() =>
    sketchPreview?.artifactBundle ? toAssetUrl(sketchPreview.artifactBundle.modelStlPath) : null,
  );
  const sketchPreviewViewerAssets = $derived.by<ViewerAsset[]>(() =>
    sketchPreview?.artifactBundle
      ? viewerAssetsToUrls(sketchPreview.artifactBundle.viewerAssets ?? [])
      : [],
  );
  const sketchPreviewStatus = $derived.by<SketchViewportStatus | null>(() => {
    if (!hasSketchPreview || !sketchPreview?.artifactBundle) return null;

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
      artifactName: fileBasename(sketchPreview.artifactBundle.modelStlPath) || 'model.stl',
    };
  });
  const sketchPreviewDraftLabel = $derived.by<string | null>(() => {
    if (!hasSketchPreview || !sketchPreviewDraft) return null;
    return sketchPreviewDraft.savedAt ? 'DRAFT SAVED' : 'DRAFT ACTIVE';
  });
  const effectiveViewerStlUrl = $derived.by<string | null>(() =>
    hasSketchPreview ? sketchPreviewStlUrl : ($activeThreadId ? stlUrl : null),
  );
  const effectiveViewerAssets = $derived.by<ViewerAsset[]>(() =>
    hasSketchPreview ? sketchPreviewViewerAssets : viewerAssets,
  );
  const hasRenderableModel = $derived.by(() => viewportState.hasRenderableModel);
  const currentViewportTargetKey = $derived.by<string | null>(
    () => viewportState.currentViewportTargetKey,
  );
  const currentViewerModelKey = $derived.by<string | null>(
    () => viewportState.currentViewerModelKey,
  );
  const effectiveViewerModelKey = $derived.by<string | null>(() =>
    hasSketchPreview && sketchPreview?.artifactBundle
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
      threadAgentState: projectedThreadAgentState,
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
      fallbackAuthoringLints: [],
    });
  });
  const hasLiveMcpSession = $derived.by(() => agentOpsState.hasLiveMcpSession);
  const isActiveMcpMode = $derived(false);
  const isAudioMuted = $derived(Boolean($config?.microwave?.muted));
  const dialogueState = $derived.by<DialogueState>(() => {
    return deriveDialogueState(
      activePendingAgentPrompt,
      usesQueuedAgentDialogue,
      $config.connectionType,
      $config.connectionType === 'provider:agy'
        ? agyProviderSnapshot
          ? {
              providerId: 'agy',
              externalConversationId: agyProviderSnapshot.binding.agyConversationId,
              label: 'Agy',
              supportsSteer: agyProviderSnapshot.capabilities.steer,
              supportsStop: agyProviderSnapshot.capabilities.stop,
            }
          : null
        : codexTakeoverSnapshot?.binding ?? null,
    );
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
  const modelIo = createModelIo({
    save: safeSaveDialog,
    exportFile,
    exportMultipart3mf,
    exportMultipartStlZip,
    setStatus: (message) => session.setStatus(message),
    setError: (message) => session.setError(message),
    formatError: formatBackendError,
  });
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
    const workbenchActive = $currentView === 'workbench';
    if (!overlayActionsEl || !workbenchActive || typeof ResizeObserver === 'undefined' || typeof window === 'undefined') {
      genieSafeRightInset = 360;
      setWindowSafeInsets({});
      return;
    }

    const measure = () => {
      const dockRect = overlayActionsEl?.getBoundingClientRect();
      const width = dockRect?.width ?? 0;
      genieSafeRightInset = Math.max(220, Math.ceil(width) + 28);
      setWindowSafeInsets({
        bottom: dockRect ? Math.ceil(window.innerHeight - dockRect.top + 8) : 0,
      });
      fitVisibleWindowsToViewport({
        width: window.innerWidth,
        height: window.innerHeight,
      });
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
  const terminalWindowState = $derived($windowStore.terminal);
  const codeWindowState = $derived($windowStore.code);
  const projectsWindowState = $derived($windowStore.projects);
  const libraryWindowState = $derived($windowStore.library);
  const captureWindowState = $derived($windowStore.capture);
  const analysisWindowState = $derived($windowStore.analysis);
  const paramsWindowState = $derived($windowStore.params);
  const dialogueWindowState = $derived($windowStore.dialogue);
  const docsWindowState = $derived($windowStore.docs);
  const settingsWindowState = $derived($windowStore.settings);
  const activityWindowState = $derived($windowStore.activity);
  let femResult = $state<FemRunResponse | null>(null);
  let femResultSource = $state('');
  let femMeshPreview = $state<FemMeshPreviewResponse | null>(null);
  let femMeshPreviewSource = $state('');
  let femDisplay = $state<FemDisplayOptions>({
    field: 'vonMises', deformationScale: 1, showMesh: true, showOutline: true, clipFraction: 1,
  });
  const visibleFemResult = $derived(
    femResult
      && femResult.modelId === activeArtifactBundle?.modelId
      && femResultSource === ($workingCopy.macroCode || activeVersionMessage?.output?.macroCode || '')
      ? femResult
      : null,
  );
  const visibleFemMeshPreview = $derived(
    femMeshPreview
      && femMeshPreview.modelId === activeArtifactBundle?.modelId
      && femMeshPreviewSource === ($workingCopy.macroCode || activeVersionMessage?.output?.macroCode || '')
      ? femMeshPreview
      : null,
  );
  let captureWorkspaceState = $state(createCaptureWorkspaceState());
  function transitionCaptureWorkspace(action: CaptureWorkspaceAction) {
    captureWorkspaceState = reduceCaptureWorkspace(captureWorkspaceState, action);
  }
  function patchCaptureWorkspace(patch: NonNullable<Extract<CaptureWorkspaceAction, { type: 'patch' }>['patch']>) {
    transitionCaptureWorkspace({ type: 'patch', patch });
  }
  const captureSessionState = $derived(captureWorkspaceState.phase);
  const captureSessionToken = $derived(captureWorkspaceState.sessionToken);
  const captureRunId = $derived(captureWorkspaceState.runId);
  const capturePairingUrl = $derived('pairingUrl' in captureWorkspaceState ? captureWorkspaceState.pairingUrl : 'No pairing session yet');
  const captureTrustUrl = $derived('trustUrl' in captureWorkspaceState ? captureWorkspaceState.trustUrl : '');
  const captureGuidance = $derived(captureWorkspaceState.guidance);
  const captureCameraStatus = $derived(captureWorkspaceState.cameraStatus);
  const captureAcceptedFrameCount = $derived(captureWorkspaceState.acceptedFrameCount);
  const captureMeshPreview = $derived(captureWorkspaceState.meshPreview);
  const capturePreviewBundle = $derived(captureWorkspaceState.preview?.bundle ?? null);
  const capturePreviewManifest = $derived(captureWorkspaceState.preview?.manifest ?? null);
  let capturePreviewApplied = $state(false);
  let capturePreviewScale = $state(0.05);
  let captureCropEnabled = $state(false);
  let captureCropMode = $state<'translate' | 'scale'>('scale');
  let captureCropBounds = $state<CaptureCropBounds | null>(null);
  let capturePreviewCropBounds = $state<CaptureCropBounds | null>(null);
  let captureCropDirty = $state(false);
  let capturePreparedCropKey = $state<string | null>(null);
  let captureHistoryRuns = $state<CaptureRun[]>([]);
  let captureHistoryLoadToken = 0;
  let externalShapeSources = $state<ExternalShapeSource[]>([]);
  let selectedExternalShapeNodeId = $state<number | null>(null);
  let externalShapeError = $state('');
  let externalShapeLoadToken = 0;
  let externalShapeRefreshKey = '';
  let captureSolidifySource = $state('');
  let captureApplyPending = $state(false);
  let captureTargetThreadId = $state('');
  let captureTargetMessageId = $state<string | null>(null);
  let captureTargetTitle = $state('');
  let captureTargetDraft = $state<WorkingCopyState | null>(null);
  let captureStartedFromUnboundWorkspace = $state(false);
  let capturePreviewPromptedToken = $state('');
  let pendingCaptureProjectSwitch = $state<{
    sessionToken: string;
    targetThreadId: string;
    targetMessageId: string | null;
    message: string;
  } | null>(null);
  let capturePreviewPrepareError = $state('');
  let capturePreviewPreparePromise: Promise<boolean> | null = null;
  let captureGuideMode = $state(false);
  let captureGuideSource = $state<CaptureGuideSourceMesh | null>(null);
  let captureGuide = $state<CaptureReconstructionGuide | null>(null);
  let captureGuideState = $state<CaptureReconstructionGuideState | null>(null);
  let captureGuidePickRole = $state<CaptureLandmarkRole>('calibrationEndpoint');
  let captureGuideKnownDistanceMm = $state(40);
  let captureGuideFeatureDepthMm = $state(18);
  let captureGuideInstruction = $state('');
  let captureGuideError = $state('');
  let captureGuideHistory = $state<CaptureGuideDraftHistory | null>(null);
  let captureGuideSelectedLandmarkId = $state<string | null>(null);
  let captureGuideBackendRevision = 0;
  let captureGuidedMessageId = $state<string | null>(null);
  let captureGuidedModelId = $state<string | null>(null);
  let captureGuidedMessage = $state<Message | null>(null);
  let captureGuidedMessageRequestKey = '';
  let captureComparisonError = $state('');
  let captureGuidedResult = $state<CaptureGuideResultProvenance | null>(null);
  let captureGuidedDeviation = $state<CaptureObservedDeviationReport | null>(null);
  let captureReferenceVisible = $state(true);
  let captureReferenceOpacity = $state(0.28);
  let captureGeneratedVisible = $state(true);
  let captureGeneratedOpacity = $state(1);
  let captureDeviationVisible = $state(true);
  const captureGuideReadiness = $derived(
    captureGuide ? mechanicalGuideReadiness(captureGuide) : { ready: false, reasons: ['Start guided CAD.'] },
  );
  const capturePreviewUrl = $derived(
    capturePreviewBundle ? toAssetUrl(capturePreviewBundle.modelStlPath) : null,
  );
  const selectedExternalShape = $derived(
    externalShapeSources.find(source => source.nodeId === selectedExternalShapeNodeId) ?? null,
  );
  const externalShapePreviewIsCropped = $derived(Boolean(
    selectedExternalShape
      && (
        (selectedExternalShape.planeCrops?.length ?? 0) > 0
        || (selectedExternalShape.surfaceTrims?.length ?? 0) > 0
      )
      && sessionModelManifest?.sourceDigest === selectedExternalShape.sourceDigest
      && stlUrl,
  ));
  const externalShapePreviewUrl = $derived(
    selectedExternalShape?.exists
      ? (
          externalShapePreviewIsCropped
            ? stlUrl
            : toAssetUrl(selectedExternalShape.path)
        )
      : null,
  );
  const externalShapeRawPreviewUrl = $derived(
    selectedExternalShape?.exists ? toAssetUrl(selectedExternalShape.path) : null,
  );
  const captureGeneratedComparisonBundle = $derived.by<ArtifactBundle | null>(() => {
    if (!captureGuidedMessageId || captureGuidedMessage?.id !== captureGuidedMessageId) return null;
    const bundle = captureGuidedMessage.artifactBundle ?? null;
    if (!bundle || (captureGuidedModelId && bundle.modelId !== captureGuidedModelId)) return null;
    return bundle;
  });
  const captureGeneratedComparisonUrl = $derived(
    captureGeneratedComparisonBundle
      ? toAssetUrl(captureGeneratedComparisonBundle.modelStlPath)
      : null,
  );
  let capturePollTimer: ReturnType<typeof setInterval> | null = null;
  const captureStats = $derived([
    { label: 'State', value: captureSessionState.toUpperCase() },
    { label: 'Frames', value: String(captureAcceptedFrameCount) },
    { label: 'Mode', value: 'BATCH PREVIEW' },
  ]);
  let mountedWindows = $state<Record<WindowId, boolean>>({
    code: false,
    projects: false,
    library: false,
    capture: false,
    analysis: false,
    params: false,
    dialogue: false,
    docs: false,
    settings: false,
    terminal: false,
    sketch: false,
    activity: false,
  });
  let windowFitAnimationFrame: number | null = null;

  function handleViewportResize() {
    if (windowFitAnimationFrame !== null) {
      cancelAnimationFrame(windowFitAnimationFrame);
    }
    windowFitAnimationFrame = requestAnimationFrame(() => {
      windowFitAnimationFrame = null;
      fitVisibleWindowsToViewport({
        width: window.innerWidth,
        height: window.innerHeight,
      });
    });
  }

  onDestroy(() => {
    if (windowFitAnimationFrame !== null) {
      cancelAnimationFrame(windowFitAnimationFrame);
    }
    if (capturePollTimer !== null) clearInterval(capturePollTimer);
  });

  $effect(() => {
    const s = $windowStore;
    for (const id of ['code', 'projects', 'library', 'capture', 'analysis', 'params', 'dialogue', 'docs', 'settings', 'terminal', 'activity', 'sketch'] as WindowId[]) {
      if (s[id].visible) {
        mountedWindows[id] = true;
      }
    }
  });

  $effect(() => {
    const threadId = $activeThreadId;
    const sourceIdentity = $workingCopy.macroCode || activeVersionMessage?.output?.macroCode || '';
    if (!captureWindowState.visible || !threadId) {
      if (!threadId) {
        externalShapeSources = [];
        selectedExternalShapeNodeId = null;
        externalShapeError = '';
        externalShapeRefreshKey = '';
      }
      return;
    }
    const refreshKey = `external-shapes-v2:${threadId}:${sourceIdentity}`;
    if (externalShapeRefreshKey === refreshKey) return;
    externalShapeRefreshKey = refreshKey;
    void refreshExternalShapeSources(threadId);
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

  function captureSessionStateFromBackend(
    state: string,
  ): 'pairing' | 'capturing' | 'reconstructing' | 'preview' | 'failed' | 'cancelled' {
    switch (state) {
      case 'pairing':
        return 'pairing';
      case 'capturing':
        return 'capturing';
      case 'reconstructing':
        return 'reconstructing';
      case 'preview':
        return 'preview';
      case 'failed':
        return 'failed';
      case 'cancelled':
        return 'cancelled';
      default:
        return 'failed';
    }
  }

  async function refreshCaptureHistoryRuns(threadId: string | null = get(activeThreadId)) {
    const token = ++captureHistoryLoadToken;
    if (!threadId) {
      captureHistoryRuns = [];
      return;
    }
    try {
      const runs = await listCaptureRuns(threadId);
      if (token === captureHistoryLoadToken) {
        const currentRun = runs.find(run => run.id === captureRunId);
        if (currentRun) syncCaptureGuidedResult(currentRun);
        if (get(activeThreadId) === threadId) captureHistoryRuns = runs;
      }
    } catch (error) {
      if (token === captureHistoryLoadToken) {
        captureHistoryRuns = [];
        console.error('Failed to load capture history:', formatBackendError(error));
      }
    }
  }

  async function refreshExternalShapeSources(threadId: string, authoritative = false) {
    const token = ++externalShapeLoadToken;
    try {
      const sources = await listExternalShapeSources(threadId);
      if ((!authoritative && token !== externalShapeLoadToken) || get(activeThreadId) !== threadId) return;
      externalShapeSources = sources;
      externalShapeError = '';
      if (sources.length === 1) {
        selectedExternalShapeNodeId = sources[0].nodeId;
      } else if (!sources.some(source => source.nodeId === selectedExternalShapeNodeId)) {
        selectedExternalShapeNodeId = null;
      }
    } catch (error) {
      if (token !== externalShapeLoadToken) return;
      externalShapeSources = [];
      selectedExternalShapeNodeId = null;
      externalShapeError = formatBackendError(error);
    }
  }

  function projectExternalShapeEdit(
    threadId: string,
    result: Awaited<ReturnType<typeof applyExternalShapeEdit>>,
  ) {
    const version = result.version;
    if (
      version.status === 'error' ||
      version.error ||
      !version.messageId ||
      !version.artifactBundle ||
      !version.modelManifest ||
      !version.snapshotId
    ) {
      throw version.error ?? new Error('External shape edit returned no persisted runtime.');
    }
    rememberCommittedVersionMessage(threadId, version.designOutput.title, {
      id: version.messageId,
      role: 'assistant',
      content: version.designOutput.response,
      status: version.status,
      output: version.designOutput,
      usage: null,
      artifactBundle: version.artifactBundle,
      modelManifest: version.modelManifest,
      agentOrigin: null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
      timestamp: Date.now() / 1000,
    });
    hydrateActiveRenderSnapshot({
      snapshotId: version.snapshotId,
      threadId,
      messageId: version.messageId,
      design: version.designOutput,
      artifactBundle: version.artifactBundle,
      modelManifest: version.modelManifest,
      selectedPartId: null,
      stlUrl: toAssetUrl(version.artifactBundle.modelStlPath),
      status: 'External shape edit applied.',
    });
    externalShapeSources = result.externalSources;
    if (!externalShapeSources.some(source => source.nodeId === selectedExternalShapeNodeId)) {
      selectedExternalShapeNodeId = externalShapeSources.length === 1
        ? externalShapeSources[0].nodeId
        : null;
    }
    externalShapeRefreshKey = `${threadId}:${result.sourceDigest}`;
  }

  async function applySelectedExternalShapePlaneCrop(
    anchors: CaptureSurfaceAnchor[],
    keepPositive: boolean,
    replaceCropNodeId: number | null,
  ) {
    const threadId = get(activeThreadId);
    const selected = selectedExternalShape;
    if (!threadId || !selected || !selected.contentDigest) {
      throw new Error('Plane crop requires one readable bound STL source.');
    }
    const result = await applyExternalShapeEdit({
      threadId,
      baseMessageId: get(activeVersionId),
      expectedSourceDigest: selected.sourceDigest,
      edit: {
        action: 'applyPlaneCrop',
        nodeId: selected.nodeId,
        expectedMeshContentDigest: selected.contentDigest,
        anchors,
        keepPositive,
        replaceCropNodeId,
      },
    });
    projectExternalShapeEdit(threadId, result);
  }

  async function removeSelectedExternalShapePlaneCrop(cropNodeId: number) {
    const threadId = get(activeThreadId);
    const selected = selectedExternalShape;
    if (!threadId || !selected) {
      throw new Error('Plane crop removal requires one bound STL source.');
    }
    const result = await applyExternalShapeEdit({
      threadId,
      baseMessageId: get(activeVersionId),
      expectedSourceDigest: selected.sourceDigest,
      edit: { action: 'removePlaneCrop', nodeId: selected.nodeId, cropNodeId },
    });
    projectExternalShapeEdit(threadId, result);
  }

  function requireSelectedExternalShape(operation: string) {
    const threadId = get(activeThreadId);
    const selected = selectedExternalShape;
    if (!threadId || !selected || !selected.contentDigest) {
      throw new Error(`${operation} requires one readable bound STL source.`);
    }
    return { threadId, selected, contentDigest: selected.contentDigest };
  }

  async function previewSelectedExternalShapeSurfaceTrimPath(
    fromAnchor: CaptureSurfaceAnchor,
    toAnchor: CaptureSurfaceAnchor,
    pathMode: SurfaceTrimPathMode,
    previewId: number,
    targetMessageId: string | null,
  ): Promise<SurfaceTrimPathPreviewResponse> {
    const { threadId, selected, contentDigest } = requireSelectedExternalShape('Surface trim path preview');
    return previewExternalShapeSurfaceTrimPath({
      schemaVersion: 1,
      threadId,
      targetMessageId,
      nodeId: selected.nodeId,
      expectedSourceDigest: selected.sourceDigest,
      expectedMeshContentDigest: contentDigest,
      fromAnchor,
      toAnchor,
      pathMode,
      previewId,
    });
  }

  async function previewSelectedExternalShapeSurfaceTrimLoop(
    loopAnchors: CaptureSurfaceAnchor[],
    pathMode: SurfaceTrimPathMode,
    previewId: number,
    targetMessageId: string | null,
  ): Promise<SurfaceTrimLoopPreviewResponse> {
    const { threadId, selected, contentDigest } = requireSelectedExternalShape('Surface trim loop preview');
    return previewExternalShapeSurfaceTrimLoop({
      schemaVersion: 1,
      threadId,
      targetMessageId,
      nodeId: selected.nodeId,
      expectedSourceDigest: selected.sourceDigest,
      expectedMeshContentDigest: contentDigest,
      loopAnchors,
      pathMode,
      previewId,
    });
  }

  async function previewSelectedExternalShapeSurfaceTrimRegion(
    loopAnchors: CaptureSurfaceAnchor[],
    keepSeed: CaptureSurfaceAnchor,
    pathMode: SurfaceTrimPathMode,
    capMode: SurfaceTrimCapMode,
    previewId: number,
    targetMessageId: string | null,
  ): Promise<SurfaceTrimRegionPreviewResponse> {
    const { threadId, selected, contentDigest } = requireSelectedExternalShape('Surface trim region preview');
    return previewExternalShapeSurfaceTrimRegion({
      schemaVersion: 1,
      threadId,
      targetMessageId,
      nodeId: selected.nodeId,
      expectedSourceDigest: selected.sourceDigest,
      expectedMeshContentDigest: contentDigest,
      loopAnchors,
      keepSeed,
      pathMode,
      capMode,
      previewId,
    });
  }

  async function applySelectedExternalShapeSurfaceTrim(
    loopAnchors: CaptureSurfaceAnchor[],
    keepSeed: CaptureSurfaceAnchor,
    pathMode: SurfaceTrimPathMode,
    capMode: SurfaceTrimCapMode,
    replaceTrimNodeId: number | null,
    targetMessageId: string | null,
  ) {
    const { threadId, selected, contentDigest } = requireSelectedExternalShape('Surface trim');
    const result = await applyExternalShapeEdit({
      threadId,
      baseMessageId: targetMessageId,
      expectedSourceDigest: selected.sourceDigest,
      edit: {
        action: 'applySurfaceTrim',
        schemaVersion: 1,
        nodeId: selected.nodeId,
        expectedMeshContentDigest: contentDigest,
        loopAnchors,
        keepSeed,
        pathMode,
        capMode,
        replaceTrimNodeId,
      },
    });
    projectExternalShapeEdit(threadId, result);
  }

  async function removeSelectedExternalShapeSurfaceTrim(trimNodeId: number) {
    const { threadId, selected } = requireSelectedExternalShape('Surface trim removal');
    const result = await applyExternalShapeEdit({
      threadId,
      baseMessageId: get(activeVersionId),
      expectedSourceDigest: selected.sourceDigest,
      edit: { action: 'removeSurfaceTrim', nodeId: selected.nodeId, trimNodeId },
    });
    projectExternalShapeEdit(threadId, result);
  }

  function syncCaptureGuidedResult(run: CaptureRun) {
    captureGuidedMessageId = run.guidedReconstructionMessageId ?? null;
    captureGuidedModelId = run.guidedReconstructionModelId ?? null;
    captureGuidedResult = run.guidedReconstructionResult ?? null;
    captureGuidedDeviation = run.guidedReconstructionDeviation ?? null;
    void loadCaptureGuidedMessage(run);
  }

  async function loadCaptureGuidedMessage(run: CaptureRun) {
    const messageId = run.guidedReconstructionMessageId ?? null;
    const modelId = run.guidedReconstructionModelId ?? null;
    if (!messageId) {
      captureGuidedMessageRequestKey = '';
      captureGuidedMessage = null;
      captureComparisonError = '';
      return;
    }
    const requestKey = `${run.id}:${run.targetThreadId}:${messageId}:${modelId ?? ''}`;
    if (captureGuidedMessageRequestKey === requestKey) return;
    captureGuidedMessageRequestKey = requestKey;
    captureGuidedMessage = null;
    captureComparisonError = '';
    try {
      const message = await getThreadMessageVersion(run.targetThreadId, messageId);
      if (captureGuidedMessageRequestKey !== requestKey) return;
      if (!message) {
        throw new Error(`Committed guided reconstruction message ${messageId} is missing from task ${run.targetThreadId}.`);
      }
      const bundle = message.artifactBundle ?? null;
      if (!bundle) {
        throw new Error(`Committed guided reconstruction message ${messageId} has no artifact bundle.`);
      }
      if (modelId && bundle.modelId !== modelId) {
        throw new Error(`Committed guided reconstruction model mismatch: expected ${modelId}, received ${bundle.modelId}.`);
      }
      captureGuidedMessage = message;
    } catch (error) {
      if (captureGuidedMessageRequestKey === requestKey) {
        captureComparisonError = formatBackendError(error);
      }
    }
  }

  $effect(() => {
    const threadId = $activeThreadId;
    void refreshCaptureHistoryRuns(threadId);
  });

  async function hydrateReopenedCapture(reopened: ReopenedCaptureRun) {
    const { run, session: reopenedSession } = reopened;
    patchCaptureWorkspace({ sessionToken: reopenedSession.pairingToken, runId: run.id });
    captureTargetThreadId = run.targetThreadId;
    captureTargetMessageId = run.targetMessageId ?? null;
    captureTargetTitle = run.title;
    captureStartedFromUnboundWorkspace = run.startedFromEmpty;
    const currentDraft = get(workingCopy);
    captureTargetDraft = {
      ...currentDraft,
      title: run.title,
      versionName: 'Capture Draft',
      macroCode: run.targetSource,
      sourceLanguage: run.targetSourceLanguage as WorkingCopyState['sourceLanguage'],
      sourceVersionId: run.targetMessageId ?? null,
      dirty: false,
    };
    patchCaptureWorkspace({ pairingUrl: reopenedSession.pairingUrl, trustUrl: reopenedSession.trustUrl, acceptedFrameCount: reopenedSession.acceptedFrameCount ?? run.acceptedFrameCount });
    const reopenedPhase = captureSessionStateFromBackend(reopenedSession.state);
    patchCaptureWorkspace({
      phase: reopenedPhase,
      meshPreview: reopenedSession.meshPreview ?? run.meshPreview ?? null,
      error: reopenedPhase === 'failed'
        ? reopenedSession.rawError || 'Capture restore failed'
        : null,
    });
    capturePreviewApplied = false;
    capturePreviewScale = run.previewScale;
    capturePreparedCropKey = null;
    captureSolidifySource = '';
    capturePreviewPrepareError = '';
    captureGuide = run.reconstructionGuide ?? null;
    captureGuideInstruction = captureGuide?.instruction ?? '';
    captureGuideHistory = captureGuide ? createCaptureGuideDraftHistory(captureGuide) : null;
    captureGuideSelectedLandmarkId = null;
    captureGuideSource = captureGuide?.sourceMesh ?? null;
    captureGuideState = run.reconstructionGuideState ?? (captureGuide ? { status: 'draft' } : null);
    captureGuideBackendRevision = captureGuide?.revision ?? 0;
    captureGuideMode = captureGuide !== null;
    captureGuideError = captureGuideState?.status === 'stale' ? captureGuideState.reason : '';
    syncCaptureGuidedResult(run);
    capturePreviewPromptedToken = '';
    pendingCaptureProjectSwitch = null;
    if (run.cropBounds) {
      captureCropEnabled = true;
      captureCropMode = 'scale';
      captureCropBounds = cloneCaptureCropBounds(run.cropBounds);
      capturePreviewCropBounds = cloneCaptureCropBounds(run.cropBounds);
      captureCropDirty = false;
    } else {
      resetCaptureCropState();
    }
    patchCaptureWorkspace({ guidance: captureMeshPreview ? 'PREPARING PREVIEW' : 'ADD PHOTOS', cameraStatus: `${captureAcceptedFrameCount} source frames restored`, preview: null });
    showWindow('capture');
    if (capturePollTimer !== null) clearInterval(capturePollTimer);
    capturePollTimer = setInterval(() => void pollCaptureSession(), 1000);
    if (captureMeshPreview) await ensureCapturePreviewPrepared();
  }

  async function openCaptureRunFromHistory(runId: string) {
    try {
      await hydrateReopenedCapture(await reopenCaptureRun(runId));
    } catch (error) {
      showWindow('capture');
      patchCaptureWorkspace({ phase: 'failed', guidance: 'CAPTURE RESTORE FAILED', cameraStatus: formatBackendError(error), error: formatBackendError(error) });
    }
  }

  async function openLastStoredCapture() {
    const currentThreadId = get(activeThreadId);
    const draft = get(workingCopy);
    try {
      const reopened = await adoptLatestCaptureRun(currentThreadId ? {
        threadId: currentThreadId,
        messageId: get(activeVersionId),
        source: draft.macroCode,
        sourceLanguage: draft.sourceLanguage,
      } : null);
      await hydrateReopenedCapture(reopened);
      await refreshHistory();
      void refreshCaptureHistoryRuns(reopened.run.targetThreadId);
    } catch (error) {
      patchCaptureWorkspace({ phase: 'failed', guidance: 'CAPTURE RESTORE FAILED', cameraStatus: formatBackendError(error), error: formatBackendError(error) });
    }
  }

  async function startCaptureSession() {
    patchCaptureWorkspace({ phase: 'capturing', guidance: 'PAIRING', cameraStatus: 'Starting capture session...', error: null });
    try {
      const currentThreadId = get(activeThreadId);
      const draft = get(workingCopy);
      const existingDraft = currentThreadId
        ? { ...draft, params: { ...draft.params }, uiSpec: { ...draft.uiSpec, fields: [...draft.uiSpec.fields] } }
        : null;
      const session = await startCaptureSessionCommand(currentThreadId ? {
        threadId: currentThreadId,
        messageId: get(activeVersionId),
        source: draft.macroCode,
        sourceLanguage: draft.sourceLanguage,
      } : null);
      captureTargetDraft = existingDraft
        ? {
            ...existingDraft,
            title: session.targetTitle,
            macroCode: session.targetSource,
            sourceLanguage: session.targetSourceLanguage as WorkingCopyState['sourceLanguage'],
            sourceVersionId: session.targetMessageId ?? null,
          }
        : {
            title: session.targetTitle,
            versionName: 'Capture Draft',
            macroCode: session.targetSource,
            macroDialect: 'ecky',
            engineKind: 'ecky',
            sourceLanguage: session.targetSourceLanguage as WorkingCopyState['sourceLanguage'],
            geometryBackend: 'mesh',
            uiSpec: { fields: [] },
            params: {},
            postProcessing: null,
            dirty: false,
            sourceVersionId: session.targetMessageId ?? null,
          };
      patchCaptureWorkspace({ sessionToken: session.pairingToken, runId: session.sessionId });
      captureTargetThreadId = session.targetThreadId;
      captureTargetMessageId = session.targetMessageId ?? null;
      captureTargetTitle = session.targetTitle;
      captureStartedFromUnboundWorkspace = session.startedFromEmpty;
      const startedPhase = captureSessionStateFromBackend(session.state);
      patchCaptureWorkspace({
        pairingUrl: session.pairingUrl,
        trustUrl: session.trustUrl,
        phase: startedPhase,
        acceptedFrameCount: session.acceptedFrameCount ?? 0,
        meshPreview: session.meshPreview ?? null,
        preview: null,
        error: startedPhase === 'failed' ? session.rawError || 'Capture failed to start' : null,
      });
      capturePreviewApplied = false;
      capturePreviewScale = 0.05;
      resetCaptureCropState();
      captureSolidifySource = '';
      capturePreviewPromptedToken = '';
      pendingCaptureProjectSwitch = null;
      capturePreviewPrepareError = '';
      captureGuideMode = false;
      captureGuideInstruction = '';
      captureGuideSource = null;
      captureGuide = null;
      captureGuideState = null;
      captureGuideBackendRevision = 0;
      captureGuideError = '';
      captureGuideHistory = null;
      captureGuideSelectedLandmarkId = null;
      captureGuidedMessageId = null;
      captureGuidedModelId = null;
      captureGuidedMessage = null;
      captureGuidedMessageRequestKey = '';
      captureComparisonError = '';
      captureGuidedResult = null;
      captureGuidedDeviation = null;
      patchCaptureWorkspace({ guidance: 'OPEN LINK ON PHONE', cameraStatus: 'Waiting for phone camera' });
      void refreshCaptureHistoryRuns(session.targetThreadId);
      void refreshHistory();
      if (capturePollTimer !== null) clearInterval(capturePollTimer);
      capturePollTimer = setInterval(() => void pollCaptureSession(), 1000);
    } catch (error) {
      patchCaptureWorkspace({ phase: 'failed', guidance: 'PAIR PHONE', cameraStatus: formatBackendError(error), error: formatBackendError(error) });
    }
  }

  async function ensureCapturePreviewPrepared(): Promise<boolean> {
    const requestedCropKey = captureCropBoundsKey(capturePreviewCropBounds);
    if (
      capturePreviewBundle &&
      capturePreviewManifest &&
      capturePreparedCropKey === requestedCropKey
    ) return true;
    if (!captureSessionToken || capturePreviewPrepareError) return false;
    if (capturePreviewPreparePromise) return capturePreviewPreparePromise;

    capturePreviewPreparePromise = (async () => {
      try {
        const prepared = await prepareCapturePreview(captureSessionToken, capturePreviewCropBounds);
        patchCaptureWorkspace({ preview: { bundle: prepared.artifactBundle, manifest: prepared.modelManifest, applied: false, cropBounds: null } });
        capturePreparedCropKey = requestedCropKey;
        patchCaptureWorkspace({ guidance: 'INSPECT MESH', cameraStatus: 'Preview ready inside Capture window' });
        return true;
      } catch (error) {
        capturePreviewPrepareError = formatBackendError(error);
        patchCaptureWorkspace({ cameraStatus: capturePreviewPrepareError, error: capturePreviewPrepareError, phase: 'failed' });
        return false;
      } finally {
        capturePreviewPreparePromise = null;
      }
    })();
    return capturePreviewPreparePromise;
  }

  async function pollCaptureSession() {
    if (!captureSessionToken) return;
    try {
      const session = await getCaptureSessionStatusCommand(captureSessionToken);
      if (!session) throw new Error('Capture session expired or was revoked.');
      const polledPhase = captureSessionStateFromBackend(session.state);
      patchCaptureWorkspace({
        phase: polledPhase,
        acceptedFrameCount: session.acceptedFrameCount ?? 0,
        meshPreview: session.meshPreview ?? null,
        error: polledPhase === 'failed' ? session.rawError || 'Capture failed' : null,
      });
      if (polledPhase === 'capturing') {
        patchCaptureWorkspace({ guidance: session.guidance || 'CAPTURING', cameraStatus: `${captureAcceptedFrameCount} source frames stored` });
      } else if (polledPhase === 'reconstructing') {
        patchCaptureWorkspace({ guidance: `RECONSTRUCTING ${Math.round((session.reconstructionProgress ?? 0) * 100)}%`, cameraStatus: `${captureAcceptedFrameCount} source frames retained`, progress: session.reconstructionProgress ?? 0 });
      } else if (polledPhase === 'preview') {
        patchCaptureWorkspace({ guidance: capturePreviewBundle ? 'INSPECT MESH' : 'PREPARING PREVIEW', cameraStatus: capturePreviewPrepareError
          || (capturePreviewApplied
            ? 'Capture solidify draft applied'
            : 'Source frames retained; no version created') });
        if (
          captureTargetThreadId &&
          !isCaptureTargetCurrent() &&
          capturePreviewPromptedToken !== captureSessionToken
        ) {
          pendingCaptureProjectSwitch = {
            sessionToken: captureSessionToken,
            targetThreadId: captureTargetThreadId,
            targetMessageId: captureTargetMessageId,
            message: `Capture preview ready for ${captureTargetTitle || 'bound project'}. Switch to project?`,
          };
          capturePreviewPromptedToken = captureSessionToken;
          dismissedBubbleText = '';
        }
        void ensureCapturePreviewPrepared();
        if (captureGuideMode && captureTargetThreadId) {
          void refreshCaptureHistoryRuns(captureTargetThreadId);
        }
      } else if (polledPhase === 'failed') {
        patchCaptureWorkspace({ phase: 'failed', guidance: 'RECONSTRUCTION FAILED', cameraStatus: session.rawError || 'Capture failed', error: session.rawError || 'Capture failed' });
      }
    } catch (error) {
      patchCaptureWorkspace({ phase: 'failed', guidance: 'CAPTURE FAILED', cameraStatus: formatBackendError(error), error: formatBackendError(error) });
      if (capturePollTimer !== null) clearInterval(capturePollTimer);
      capturePollTimer = null;
    }
  }

  async function cancelCaptureSession() {
    if (!captureSessionToken) {
      transitionCaptureWorkspace({ type: 'cancel' });
      return;
    }

    try {
      const session = await cancelCaptureSessionCommand(captureSessionToken);
      transitionCaptureWorkspace({ type: 'cancel' });
      capturePreviewApplied = false;
      resetCaptureCropState();
      capturePreviewPrepareError = '';
      captureGuideMode = false;
      captureGuideSource = null;
      captureGuide = null;
      captureGuideState = null;
      captureGuideBackendRevision = 0;
      captureGuideError = '';
      captureGuideHistory = null;
      captureGuideSelectedLandmarkId = null;
      captureGuidedMessageId = null;
      captureGuidedModelId = null;
      captureGuidedResult = null;
      captureGuidedDeviation = null;
      if (capturePollTimer !== null) clearInterval(capturePollTimer);
      capturePollTimer = null;
      patchCaptureWorkspace({ guidance: 'PAIR PHONE', cameraStatus: 'Session cancelled' });
    } catch (error) {
      patchCaptureWorkspace({ phase: 'failed', guidance: 'PAIR PHONE', cameraStatus: formatBackendError(error), error: formatBackendError(error) });
    }
  }

  async function startCaptureGuidedCad() {
    if (!captureRunId || !captureMeshPreview) {
      captureGuideError = 'Capture mesh is not ready.';
      return;
    }
    try {
      const ensured = await ensureCaptureReconstructionGuide(captureRunId);
      captureGuideSource = ensured.guide.sourceMesh;
      captureGuide = ensured.guide;
      captureGuideHistory = createCaptureGuideDraftHistory(captureGuide);
      captureGuideInstruction = captureGuide.instruction;
      captureGuideSelectedLandmarkId = null;
      captureGuideBackendRevision = captureGuide.revision;
      captureGuideState = ensured.state;
      captureGuideMode = true;
      captureCropEnabled = false;
      captureGuideError = '';
      captureGuidePickRole = 'calibrationEndpoint';
      patchCaptureWorkspace({ cameraStatus: 'Guided CAD: pick digest-bound scan evidence' });
    } catch (error) {
      captureGuideError = formatBackendError(error);
      patchCaptureWorkspace({ cameraStatus: captureGuideError });
    }
  }

  function addCaptureGuideAnchor(anchor: CaptureSurfaceAnchor) {
    if (!captureGuide || !captureGuideSource || captureGuideState?.status === 'stale') return;
    void applyCaptureGuideIntent({ kind: 'addLandmark', role: captureGuidePickRole, anchor })
      .then(saved => {
        if (!saved) return;
        captureGuideSelectedLandmarkId = saved.landmarks.at(-1)?.landmarkId ?? null;
        patchCaptureWorkspace({ cameraStatus: `${saved.landmarks.length} guide landmarks; ${captureGuidePickRole}` });
      });
  }

  function undoCaptureGuideEdit() {
    if (!captureGuideHistory || captureGuideHistory.past.length === 0) return;
    const previous = captureGuideHistory.past.at(-1);
    if (!previous) return;
    void applyCaptureGuideIntent({ kind: 'replaceDraft', guide: previous }, true)
      .then(saved => {
        if (saved && !saved.landmarks.some(item => item.landmarkId === captureGuideSelectedLandmarkId)) {
          captureGuideSelectedLandmarkId = null;
        }
      });
  }

  async function applyCaptureGuideIntent(
    edit: CaptureGuideEditIntent,
    undo = false,
  ): Promise<CaptureReconstructionGuide | null> {
    if (!captureGuide || !captureGuideSource || captureGuideState?.status === 'stale') return null;
    try {
      const previous = captureGuide;
      const history = captureGuideHistory ?? createCaptureGuideDraftHistory(previous);
      const result = await applyCaptureGuideEditIntent({
        runId: captureRunId,
        expectedRevision: captureGuideBackendRevision,
        expectedMeshDigest: captureGuideSource.contentDigest,
        edit,
      });
      if (result.guide.revision < captureGuideBackendRevision) return null;
      captureGuideBackendRevision = result.guide.revision;
      captureGuideHistory = undo
        ? { past: history.past.slice(0, -1), present: result.guide }
        : applyCaptureGuideDraftEdit(history, result.guide);
      captureGuide = captureGuideHistory.present;
      captureGuideState = result.state;
      captureGuideError = result.rawEvidence.join('\n');
      return result.guide;
    } catch (error) {
      captureGuideError = formatBackendError(error);
      patchCaptureWorkspace({ cameraStatus: captureGuideError });
      return null;
    }
  }

  function editCaptureGuideLandmark(landmarkId: string, edit: CaptureLandmarkEdit) {
    void applyCaptureGuideIntent({
      kind: 'updateLandmark',
      landmarkId,
      label: edit.label,
      role: edit.role,
    });
  }

  function deleteCaptureGuideLandmark(landmarkId: string) {
    void applyCaptureGuideIntent({ kind: 'deleteLandmark', landmarkId });
    if (captureGuideSelectedLandmarkId === landmarkId) captureGuideSelectedLandmarkId = null;
  }

  function editCaptureGuideProfile(profileId: string, edit: CaptureProfileEdit) {
    if (!captureGuide) return;
    const profile = captureGuide.profiles.find(item => item.profileId === profileId);
    if (!profile) return;
    const next = { ...profile, ...edit };
    void applyCaptureGuideIntent({
      kind: 'configureProfile',
      profileId,
      label: next.label,
      profileKind: next.kind,
      operationHint: next.operationHint,
      supportPlaneId: next.supportPlaneId,
      featureLabel: next.featureLabel ?? null,
      fitRole: next.fitRole ?? null,
    });
  }

  function reorderCaptureGuideProfile(profileId: string, landmarkId: string, targetIndex: number) {
    void applyCaptureGuideIntent({ kind: 'reorderProfileLandmark', profileId, landmarkId, targetIndex });
  }

  function editCaptureGuideExpectation(expectationId: string, edit: CaptureFeatureExpectationEdit) {
    if (!captureGuide) return;
    const expectation = captureGuide.featureExpectations.find(item => item.expectationId === expectationId);
    if (!expectation) return;
    const next = { ...expectation, ...edit };
    void applyCaptureGuideIntent({
      kind: 'updateFeatureExpectation',
      expectationId,
      label: next.label,
      expectedGeometryKind: next.expectedGeometryKind,
      requiredBrepTopologyKind: next.requiredBrepTopologyKind,
      cardinality: next.cardinality,
      partId: next.partId,
      instancePath: next.instancePath ?? null,
      expectedAuthoredSelector: next.expectedAuthoredSelector,
      requiredForAcceptance: next.requiredForAcceptance,
      positionToleranceMm: next.positionToleranceMm ?? null,
      normalToleranceDeg: next.normalToleranceDeg ?? null,
      radialToleranceMm: next.radialToleranceMm ?? null,
    });
  }

  function selectCaptureFeaturePlan(planId: string) {
    void applyCaptureGuideIntent({ kind: 'selectFeaturePlan', planId });
  }

  async function validateCaptureGuide() {
    if (!captureGuide || !captureGuideSource) return;
    try {
      const result = await validateCaptureGuideIntent({
        runId: captureRunId,
        expectedRevision: captureGuideBackendRevision,
        expectedMeshDigest: captureGuideSource.contentDigest,
        knownDistanceMm: captureGuideKnownDistanceMm,
        instruction: captureGuideInstruction,
        featureDepthMm: captureGuideFeatureDepthMm,
      });
      if (result.guide.revision < captureGuideBackendRevision) return;
      captureGuide = result.guide;
      captureGuideHistory = createCaptureGuideDraftHistory(result.guide);
      captureGuideBackendRevision = result.guide.revision;
      captureGuideState = result.state;
      captureGuideError = result.rawEvidence.join('\n');
      patchCaptureWorkspace({ cameraStatus: result.state.status === 'ready'
        ? `Guide revision ${result.guide.revision} ready for parametric BRep`
        : result.state.status === 'stale'
          ? result.state.reason
          : result.guide.reconstructionReadiness?.detail
            ?? 'Deterministic reconstruction needs explicit evidence.' });
    } catch (error) {
      captureGuideState = { status: 'draft' };
      captureGuideError = formatBackendError(error);
      patchCaptureWorkspace({ cameraStatus: captureGuideError });
    }
  }

  async function buildCadFromCaptureGuide() {
    if (!captureGuide || captureGuideState?.status !== 'ready') return;
    try {
      const queued = await queueCaptureGuidedReconstruction(
        captureRunId,
        captureGuide.revision,
        captureGuide.targetSourceDigest,
      );
      captureGuideError = '';
      patchCaptureWorkspace({ cameraStatus: `Guided CAD queued in owning task: ${queued.messageId}` });
      await refreshHistory();
    } catch (error) {
      captureGuideError = formatBackendError(error);
      patchCaptureWorkspace({ cameraStatus: captureGuideError });
    }
  }

  async function applyCapturePreview() {
    if (!captureMeshPreview || captureApplyPending) return;
    captureApplyPending = true;
    try {
      if (captureCropDirty) {
        if (!captureCropBounds) {
          patchCaptureWorkspace({ cameraStatus: 'Crop box is not ready' });
          return;
        }
        await previewCaptureCrop();
        if (captureCropDirty) return;
      }
      capturePreviewPrepareError = '';
      if (!await ensureCapturePreviewPrepared()) return;
      if (!captureTargetThreadId || !isCaptureTargetCurrent()) {
        pendingCaptureProjectSwitch = {
          sessionToken: captureSessionToken,
          targetThreadId: captureTargetThreadId,
          targetMessageId: captureTargetMessageId,
          message: `Capture is bound to ${captureTargetTitle || 'another project'}. Switch before Apply.`,
        };
        patchCaptureWorkspace({ cameraStatus: 'Switch to bound project before Apply' });
        return;
      }
      if (captureStartedFromUnboundWorkspace && get(activeThreadId) === null) {
        activeThreadId.set(captureTargetThreadId);
        activeVersionId.set(null);
        currentView.set('workbench');
        await loadLayoutForThread(captureTargetThreadId);
        showWindow('capture');
      }

      const result = await applyCapturePreviewCommand({ runId: captureRunId });
      projectManualCodeDraftResult(result.draft, result.source);
      captureSolidifySource = result.source;
      capturePreviewApplied = true;
      patchCaptureWorkspace({ cameraStatus: 'Capture solidify draft applied' });
    } catch (error) {
      patchCaptureWorkspace({ cameraStatus: formatBackendError(error) });
    } finally {
      captureApplyPending = false;
    }
  }

  function updateCapturePreviewScale(scale: number) {
    capturePreviewScale = scale;
    capturePreviewApplied = false;
    captureSolidifySource = '';
    if (captureRunId) {
      void saveCapturePreviewSettings(captureRunId, capturePreviewCropBounds, scale).catch((error) => {
        patchCaptureWorkspace({ cameraStatus: formatBackendError(error) });
      });
    }
  }

  function captureCropBoundsKey(bounds: CaptureCropBounds | null): string {
    return bounds ? JSON.stringify(bounds) : 'raw';
  }

  function cloneCaptureCropBounds(bounds: CaptureCropBounds): CaptureCropBounds {
    return { min: [...bounds.min], max: [...bounds.max] } as CaptureCropBounds;
  }

  function resetCaptureCropState() {
    captureCropEnabled = false;
    captureCropMode = 'scale';
    captureCropBounds = null;
    capturePreviewCropBounds = null;
    captureCropDirty = false;
    capturePreparedCropKey = null;
  }

  function updateCaptureCropBounds(bounds: CaptureCropBounds) {
    captureCropBounds = cloneCaptureCropBounds(bounds);
    captureCropDirty = captureCropBoundsKey(captureCropBounds) !== captureCropBoundsKey(capturePreviewCropBounds);
    capturePreviewApplied = false;
    captureSolidifySource = '';
  }

  async function updateCaptureCropEnabled(enabled: boolean) {
    if (enabled) {
      captureCropEnabled = true;
      captureCropMode = 'scale';
      captureCropDirty = true;
      capturePreviewApplied = false;
      captureSolidifySource = '';
      return;
    }
    await resetCaptureCrop();
  }

  async function previewCaptureCrop() {
    if (!captureCropBounds) return;
    const previousBundle = capturePreviewBundle;
    const previousManifest = capturePreviewManifest;
    const previousPreparedCropKey = capturePreparedCropKey;
    const previousPreviewCropBounds = capturePreviewCropBounds;
    capturePreviewCropBounds = cloneCaptureCropBounds(captureCropBounds);
    patchCaptureWorkspace({ preview: null });
    capturePreparedCropKey = null;
    capturePreviewApplied = false;
    captureSolidifySource = '';
    capturePreviewPrepareError = '';
    patchCaptureWorkspace({ guidance: 'PREPARING PREVIEW' });
    if (captureGuide) {
      captureGuideState = { status: 'stale', reason: 'Guide is stale: selected crop/source mesh digest changed.' };
      captureGuideError = captureGuideState.reason;
    }
    if (!await ensureCapturePreviewPrepared()) {
      patchCaptureWorkspace({ preview: previousBundle && previousManifest ? { bundle: previousBundle, manifest: previousManifest, applied: false, cropBounds: null } : null });
      capturePreparedCropKey = previousPreparedCropKey;
      capturePreviewCropBounds = previousPreviewCropBounds;
      captureCropDirty = true;
      patchCaptureWorkspace({ guidance: 'INSPECT MESH' });
      return;
    }
    captureCropDirty = false;
  }

  async function resetCaptureCrop() {
    const previousBundle = capturePreviewBundle;
    const previousManifest = capturePreviewManifest;
    const previousPreparedCropKey = capturePreparedCropKey;
    const previousPreviewCropBounds = capturePreviewCropBounds;
    captureCropEnabled = false;
    captureCropBounds = null;
    capturePreviewCropBounds = null;
    captureCropDirty = false;
    capturePreviewApplied = false;
    captureSolidifySource = '';
    capturePreviewPrepareError = '';
    if (captureGuide) {
      captureGuideState = { status: 'stale', reason: 'Guide is stale: selected crop/source mesh digest changed.' };
      captureGuideError = captureGuideState.reason;
    }
    if (capturePreparedCropKey === 'raw') return;
    patchCaptureWorkspace({ preview: null });
    capturePreparedCropKey = null;
    patchCaptureWorkspace({ guidance: 'PREPARING PREVIEW' });
    if (!await ensureCapturePreviewPrepared()) {
      patchCaptureWorkspace({ preview: previousBundle && previousManifest ? { bundle: previousBundle, manifest: previousManifest, applied: false, cropBounds: null } : null });
      capturePreparedCropKey = previousPreparedCropKey;
      capturePreviewCropBounds = previousPreviewCropBounds;
      captureCropEnabled = previousPreviewCropBounds !== null;
      captureCropDirty = false;
      patchCaptureWorkspace({ guidance: 'INSPECT MESH' });
    }
  }

  function isCaptureTargetCurrent(): boolean {
    const currentThreadId = get(activeThreadId);
    return currentThreadId === captureTargetThreadId
      || (captureStartedFromUnboundWorkspace && currentThreadId === null);
  }

  async function answerCaptureProjectSwitch(choice: 'switch' | 'stay') {
    const pending = pendingCaptureProjectSwitch;
    pendingCaptureProjectSwitch = null;
    if (!pending || choice === 'stay') return;

    try {
      if (pending.targetMessageId) {
        const [thread, targetMessage] = await Promise.all([
          resolveThreadSummary(pending.targetThreadId),
          getThreadMessageVersion(pending.targetThreadId, pending.targetMessageId),
        ]);
        if (!thread) throw new Error(`Capture target thread ${pending.targetThreadId} is unavailable.`);
        if (!targetMessage) throw new Error(`Capture target version ${pending.targetMessageId} is unavailable.`);
        upsertThreadVersionInHistory(thread.id, targetMessage);
        activeThreadId.set(thread.id);
        currentView.set('workbench');
        await loadVersion(targetMessage, thread.id);
      } else {
        activeThreadId.set(pending.targetThreadId);
        activeVersionId.set(null);
        workingCopy.reset();
        paramPanelState.reset();
        session.setStlUrl(null);
        session.clearModelRuntime();
        currentView.set('workbench');
      }
      if (captureTargetDraft) {
        workingCopy.patch({ ...captureTargetDraft });
        paramPanelState.hydrate({
          versionId: captureTargetDraft.sourceVersionId,
          uiSpec: captureTargetDraft.uiSpec,
          params: captureTargetDraft.params,
        });
      }
      await loadLayoutForThread(pending.targetThreadId);
      showWindow('capture');
    } catch (error) {
      patchCaptureWorkspace({ cameraStatus: formatBackendError(error) });
    }
  }

  async function retryCaptureReconstructionFromDesktop() {
    if (!captureSessionToken) return;
    try {
      const sessionInfo = await retryCaptureReconstruction(captureSessionToken);
      const retryPhase = captureSessionStateFromBackend(sessionInfo.state);
      patchCaptureWorkspace({
        phase: retryPhase,
        preview: null,
        error: retryPhase === 'failed' ? sessionInfo.rawError || 'Capture retry failed' : null,
      });
      capturePreviewApplied = false;
      resetCaptureCropState();
      capturePreviewPrepareError = '';
      if (captureGuide) {
        captureGuideState = { status: 'stale', reason: 'Guide is stale: capture reconstruction changed.' };
        captureGuideError = captureGuideState.reason;
      }
      patchCaptureWorkspace({ guidance: 'RECONSTRUCTING 0%', cameraStatus: `${captureAcceptedFrameCount} source frames retained`, progress: 0 });
      if (capturePollTimer === null) capturePollTimer = setInterval(() => void pollCaptureSession(), 1000);
    } catch (error) {
      patchCaptureWorkspace({ cameraStatus: formatBackendError(error) });
    }
  }

  async function addCapturePhotos() {
    if (!captureSessionToken) return;
    try {
      const sessionInfo = await resumeCaptureSession(captureSessionToken);
      const resumedPhase = captureSessionStateFromBackend(sessionInfo.state);
      patchCaptureWorkspace({
        phase: resumedPhase,
        meshPreview: sessionInfo.meshPreview ?? null,
        preview: null,
        error: resumedPhase === 'failed' ? sessionInfo.rawError || 'Capture resume failed' : null,
      });
      capturePreviewApplied = false;
      resetCaptureCropState();
      captureSolidifySource = '';
      capturePreviewPrepareError = '';
      if (captureGuide) {
        captureGuideState = { status: 'stale', reason: 'Guide is stale: capture reconstruction changed.' };
        captureGuideError = captureGuideState.reason;
      }
      patchCaptureWorkspace({ guidance: 'ADD PHOTOS', cameraStatus: `${captureAcceptedFrameCount} frames retained; continue on same phone link` });
      if (capturePollTimer === null) capturePollTimer = setInterval(() => void pollCaptureSession(), 1000);
    } catch (error) {
      patchCaptureWorkspace({ cameraStatus: formatBackendError(error) });
    }
  }

  async function commitCapturePreview() {
    if (!capturePreviewApplied || !captureSolidifySource || !captureTargetThreadId) return;
    if (get(activeThreadId) !== captureTargetThreadId) {
      patchCaptureWorkspace({ cameraStatus: 'Capture conflict: bound project is not active.' });
      return;
    }
    try {
      await commitManualVersion({
        code: captureSolidifySource,
        title: captureTargetTitle || `Capture ${captureSessionToken.slice(0, 8)}`,
        versionName: 'Capture Mesh',
      });
      patchCaptureWorkspace({ cameraStatus: 'Capture model committed' });
    } catch (error) {
      patchCaptureWorkspace({ cameraStatus: formatBackendError(error) });
    }
  }

  $effect(() => {
    const nextConnectionState = projectedThreadAgentState.connectionState;
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
      codeModalSourceAuthority = 'bound';
      codeModalSourceLanguage = seededDraft.sourceLanguage;
      codeModalDraftSerial += 1;
      codeModalDraftScopeKey = [
        'manual-retry',
        $activeThreadId ?? 'no-thread',
        codeModalDraftSerial,
      ].join(':');
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

  const viewerLoadRuntime = createViewerLoadRuntime();

  function waitForViewerLoad(
    kind: 'visible' | 'hidden',
    previousNonce: number,
    timeoutMs = 12000,
  ): Promise<void> {
    return viewerLoadRuntime.waitForLoad(kind, previousNonce, timeoutMs);
  }

  function handleVisibleViewerLoaded() {
    visibleViewerLoadNonce = viewerLoadRuntime.markLoaded('visible');
    if (
      !hasSketchPreview &&
      shouldPersistVersionPreview(activeVersionMessage, get(session).artifactBundle, get(session).stlUrl)
    ) {
      void persistVisibleVersionPreview(visibleViewerLoadNonce);
    }
  }

  function handleHiddenViewerLoaded() {
    hiddenViewerLoadNonce = viewerLoadRuntime.markLoaded('hidden');
  }

  function handleVisibleViewerLoadError(message: string) {
    void recoverVisibleViewerRuntime(message);
  }

  function handleHiddenViewerLoadError(message: string) {
    viewerLoadRuntime.markFailed('hidden', message);
  }

  async function recoverVisibleViewerRuntime(message: string) {
    viewerLoadRuntime.markFailed('visible', message);

    const threadId = get(activeThreadId);
    const messageId = get(activeVersionId);
    const currentSession = get(session);
    const runtimeTarget = get(activeRenderSnapshot)?.targetRef;
    const bundle = currentSession.artifactBundle;
    const recoveryKey =
      threadId && messageId && bundle
        ? `${threadId}:${messageId}:${bundle.modelId}:${bundle.modelStlPath}`
        : null;

    if (
      recoveryKey &&
      threadId &&
      messageId &&
      bundle &&
      canRepairSavedVersionRuntime(runtimeTarget, threadId, messageId) &&
      isMissingViewerArtifactError(message) &&
      visibleViewerRecoveryKey !== recoveryKey
    ) {
      visibleViewerRecoveryKey = recoveryKey;
      session.setError(null);
      session.setStatus('Runtime artifact missing. Rebuilding saved model...');
      try {
        const repaired = await repairVersionRuntime({
          threadId,
          messageId,
          expectedArtifactIdentity: bundle.contentHash || null,
        });
        const repairedVersion = await projectWorkspaceProjection(repaired.workspace);
        if (
          repairedVersion?.id === messageId &&
          get(activeThreadId) === threadId &&
          get(activeVersionId) === messageId &&
          repairedVersion.artifactBundle
        ) {
          session.reloadStlUrl(toAssetUrl(repairedVersion.artifactBundle.modelStlPath));
          session.setStatus('Missing runtime rebuilt from saved source.');
          return;
        }
        throw new Error('Runtime repair returned no canonical selected version.');
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

  async function prepareDialogueAttachments(
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

  async function persistSketchPreviewDraft(
    scopeId: string | null,
    preview: SketchPreviewState,
    newScope = false,
  ) {
    try {
      return await saveSketchPreviewDraft({
        draftScopeId: scopeId,
        newScope,
        draftSource: preview.draft,
        artifactBundle: preview.artifactBundle,
        sketchDocument: preview.sketchDocument ?? null,
      });
    } catch (error) {
      console.warn('[Sketch] Failed to persist preview draft:', error);
      return null;
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
    const saved = await persistSketchPreviewDraft(null, sketchPreview, true);
    if (!saved?.scopeId) return;
    sketchPreviewDraft = { scopeId: saved.scopeId, savedAt: Date.now() };
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
      stlUrl: toAssetUrl(request.modelStlPath),
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
    const [thread, targetMessage] = await Promise.all([
      resolveThreadSummary(request.threadId),
      getThreadMessageVersion(request.threadId, request.messageId),
    ]);
    if (!thread) {
      throw new Error(`Target thread ${request.threadId} is unavailable for screenshot capture.`);
    }
    if (!targetMessage) {
      throw new Error(`Target version ${request.messageId} is unavailable for screenshot capture.`);
    }
    upsertThreadVersionInHistory(thread.id, targetMessage);
    activeThreadId.set(thread.id);
    currentView.set('workbench');
    await loadVersion(targetMessage);
    await waitForViewerLoad('visible', previousNonce);
  }

  function upsertThreadVersionInHistory(threadId: string, message: Message) {
    history.update((items) => {
      return items.map((thread) => ({
        ...thread,
        messages: (thread.id === threadId
          ? [
              ...(thread.messages ?? []).filter((candidate) => candidate.id !== message.id),
              message,
            ]
          : thread.messages ?? []).map((candidate) =>
            thread.id === threadId && candidate.id === message.id
              ? candidate
              : {
                  ...candidate,
                  output: null,
                  artifactBundle: null,
                  modelManifest: null,
                  structuralVerification: null,
                  imageData: null,
                  attachmentImages: [],
                },
          ),
      }));
    });
  }

  async function resolveThreadSummary(threadId: string): Promise<Thread | null> {
    const existing = get(history).find((thread) => thread.id === threadId) ?? null;
    if (existing) return existing;
    await refreshHistory();
    return get(history).find((thread) => thread.id === threadId) ?? null;
  }

  async function focusAgentWorkingVersion(event: AgentWorkingVersionCreatedEvent) {
    const focusKey = `${event.sessionId}:${event.messageId}`;
    if (lastFocusedAgentWorkingVersionKey === focusKey) return;

    const [thread, targetMessage] = await Promise.all([
      resolveThreadSummary(event.threadId),
      getThreadMessageVersion(event.threadId, event.messageId),
    ]);
    if (!thread || !targetMessage) return;
    upsertThreadVersionInHistory(thread.id, targetMessage);

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
    if (shouldAutoStartOnboarding({
      configLoaded: $configLoaded,
      isBooting,
      hasSeenOnboarding: $config.hasSeenOnboarding,
      isActive: $onboarding.isActive,
      isSuppressed: shouldSuppressOnboardingForAutomation(),
    })) {
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
      activeMcpRenderBusy && projectedThreadAgentState.sessionId
        ? `__mcp__:${projectedThreadAgentState.sessionId}`
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
      const prepared = await prepareDialogueAttachments(
        attachments,
        promptThreadId,
      );
      preparedAttachments = prepared.attachments;
      clearDrawingAfterSend = prepared.clearDrawingAfterSend;
      const result = await submitAgentPromptReply({
        requestId,
        threadId: promptThreadId,
        promptText,
        attachments: preparedAttachments,
      });
      if (clearDrawingAfterSend) {
        clearPromptDrawingOverlay();
      }
      if (result.outcome === 'queued') {
        addOptimisticQueuedAgentMessage(
          result.threadId,
          promptText,
          preparedAttachments,
          result.messageId,
        );
        adoptWorkspaceCapturePreference(result.threadId);
        if ($activeThreadId !== result.threadId) {
          activeThreadId.set(result.threadId);
          activeVersionId.set(null);
        }
        await refreshHistory();
        session.setStatus('Prompt expired. Message queued in the thread for any agent to pick up.');
      }
    } catch (e) {
      session.setError(`Agent Prompt Error: ${formatBackendError(e)}`);
    }
  }

  async function handleDialogueSubmit(prompt: string, attachments: Attachment[]) {
    switch (dialogueState.mode) {
      case 'provider': {
        const eckyThreadId = get(activeThreadId);
        if (!eckyThreadId) throw new Error(`Open an Ecky thread before sending to ${dialogueState.label}.`);
        try {
          const prepared = await prepareDialogueAttachments(attachments, eckyThreadId);
          if (dialogueState.providerId === 'agy') {
            applyAgyProviderSnapshot(
              await sendAgyProviderPrompt({
                eckyThreadId,
                promptText: prompt,
                attachments: prepared.attachments,
              }),
              true,
              true,
            );
          } else {
            applyCodexTakeoverSnapshot(
              await sendCodexTakeoverPrompt({
                eckyThreadId,
                promptText: prompt,
                attachments: prepared.attachments,
              }),
              true,
              true,
            );
          }
          if (prepared.clearDrawingAfterSend) clearPromptDrawingOverlay();
        } catch (error) {
          setProviderError(formatBackendError(error));
          throw error;
        }
        break;
      }
      case 'agent-reply': await answerAgentPrompt(dialogueState.requestId, prompt, attachments); break;
      case 'generate':    await handleGenerate(prompt, attachments, { uiDeps: requestOrchestratorUiDeps }); break;
      case 'mcp-idle': {
        let preparedAttachments: Attachment[] = attachments;
        let clearDrawingAfterSend = false;
        try {
          const prepared = await prepareDialogueAttachments(
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

  async function startBlankProject() {
    showNewProjectChooser = false;
    await createNewThread({ mode: 'blank' });
  }

  async function loadCampaignRuns() {
    try {
      campaignRuns = await campaignRunClient.list();
    } catch (error) {
      campaignRunError = formatBackendError(error);
    }
  }

  async function loadCampaignDefinitions() {
    campaignDefinitions = await campaignDefinitionClient.list();
  }

  async function startCampaign() {
    const definition = campaignDefinitions[0];
    if (!definition) {
      campaignRunError = 'Campaign definition has no first step.';
      return;
    }
    try {
      const opened = await campaignRunClient.open({ kind: 'start', definitionId: definition.definitionId });
      const run = opened.run as CampaignRun;
      campaignRuns = [run, ...campaignRuns.filter((candidate) => candidate.id !== run.id)];
      activeCampaignRun = run;
      campaignStep = opened.step as CampaignCurrentStepPayload;
      campaignRunError = null;
      showNewProjectChooser = false;
      await loadAppWindowLayout();
      closeWindowStore('projects');
      currentView.set('campaign');
    } catch (error) {
      campaignRunError = formatBackendError(error);
    }
  }

  async function openCampaignRun(run: CampaignRun) {
    try {
      const opened = await campaignRunClient.open({ kind: 'resume', runId: run.id });
      activeCampaignRun = opened.run as CampaignRun;
      campaignStep = opened.step as CampaignCurrentStepPayload;
      campaignRunError = null;
      await loadAppWindowLayout();
    } catch (error) {
      campaignRunError = formatBackendError(error);
      return;
    }
    closeWindowStore('projects');
    currentView.set('campaign');
  }

  async function transitionCampaignRun(action: import('./lib/tauri/contracts').CampaignRunTransitionAction) {
    if (!activeCampaignRun) throw new Error('Campaign run is unavailable.');
    const result = await campaignRunClient.transition({ runId: activeCampaignRun.id, action });
    const saved = result.run as CampaignRun;
    campaignRuns = campaignRuns.map((run) => run.id === saved.id ? saved : run);
    activeCampaignRun = saved;
    campaignStep = result.step as CampaignCurrentStepPayload;
    return result;
  }

  async function deleteCampaignRun(run: CampaignRun) {
    await campaignRunClient.delete(run.id);
    campaignRuns = campaignRuns.filter((candidate) => candidate.id !== run.id);
    if (activeCampaignRun?.id === run.id) {
      activeCampaignRun = null;
      currentView.set('workbench');
      await loadAppWindowLayout();
      showWindow('projects');
    }
  }

  async function closeCampaignSurface() {
    activeCampaignRun = null;
    await campaignRunClient.clearActiveProjectNavigation();
    currentView.set('workbench');
    await loadAppWindowLayout();
    showWindow('projects');
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

  async function handleTopMacroImport(data: { code: string; title: string }) {
    await createNewThread({ mode: 'macro', ...data });
    showNewProjectImport = false;
  }

  onMount(() => {
    let recoveryResetTimer: ReturnType<typeof setTimeout> | null = null;
    void connectAgentActivityIngestion(
      { listen, getAgentActivity },
      agentActivityIngestionStore,
      {
        onRecoveryError: (recoveryError) => {
          session.setError(`Agent activity recovery failed: ${formatBackendError(recoveryError)}`);
        },
      },
    )
      .then((connection) => {
        agentActivityConnection = connection;
      })
      .catch((connectionError) => {
        session.setError(`Agent activity subscription failed: ${formatBackendError(connectionError)}`);
      });

    agentNotificationProjectionDisconnect = agentActivityIngestionStore.subscribe((events) => {
      const freshEvents = events.filter((event) => !seenAgentActivityEventIds.has(event.eventId));
      if (!freshEvents.length) return;
      for (const event of freshEvents) {
        seenAgentActivityEventIds.add(event.eventId);
      }
      ingestAgentActivitySessionEvents(freshEvents);
      longTasksStore.ingest(freshEvents);
      const notificationEvents = freshEvents.filter((event) => (
        !isActiveLongTaskEvent(event) && shouldProjectAgentNotification(event)
      ));
      agentNotificationsStore.ingest(notificationEvents.filter((event) => !isLongTaskEvent(event)));
      agentNotificationsStore.ingestPriority(notificationEvents.filter(isLongTaskEvent));
    });

    // Initial fetch of agent sessions (push events only fire on changes, not on load)
    void getActiveAgentSessions().then(sessions => { activeAgentSessions = sessions; }).catch(() => {});
    void getAgentTerminalSnapshots()
      .then((snapshots) => {
        replaceAgentTerminalSnapshots(snapshots);
      })
      .catch(() => {});

    const noopUnlisten = Promise.resolve(() => {});
    const canListenToTauri = hasTauriIpc();

    const unlistenGeometryRender = canListenToTauri
      ? listen<GeometryRenderActivityEvent>('geometry-render-activity', (event) => {
          geometryRenderActiveCount = Math.max(0, Number(event.payload.activeCount) || 0);
        })
      : noopUnlisten;

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
    }) : noopUnlisten;
    const unlistenViewportScreenshot = canListenToTauri ? listen<AgentViewportScreenshotEvent>(
      'agent-viewport-screenshot-request',
      (event) => {
        void handleViewportScreenshotEvent(event.payload);
      },
    ) : noopUnlisten;
    const unlistenHistory = canListenToTauri ? listen<{
      threadId?: string | null;
      messageId?: string | null;
      revision?: number;
      kind?: string;
    }>('history-updated', async (event) => {
      const currentThreadId = get(activeThreadId);
      const changedThreadId = event.payload?.threadId ?? null;
      if (currentThreadId && (!changedThreadId || changedThreadId === currentThreadId)) {
        await refreshThreadHistoryProjection(currentThreadId, event.payload?.revision ?? null);
      } else {
        await refreshHistory();
      }
    }) : noopUnlisten;
    const unlistenProjectFolderSync = canListenToTauri ? (async () => {
      const unlistenSync = await listen<ProjectFolderWatchEvent[]>(
        'project-folder-sync',
        (event) => {
          const latest = selectProjectFolderWatchEvent(event.payload, get(activeThreadId) ?? null);
          if (latest) {
            applyProjectFolderWatchEvent(latest);
            void refreshOpenCodeModalHead(latest.threadId);
          }
        },
      );
      await reconcileProjectFolderRenderActivity();
      const activityPollTimer = window.setInterval(() => {
        void reconcileProjectFolderRenderActivity();
      }, 250);
      return () => {
        window.clearInterval(activityPollTimer);
        unlistenSync();
      };
    })() : noopUnlisten;
    const applyDraftPreview = (preview: AgentDraftPreviewUpdatedEvent) => {
        const isActivePreview = shouldApplyDraftPreviewToWorkspace({
          activeThreadId: get(activeThreadId),
          previewThreadId: preview.threadId,
        });

        if (isActivePreview) {
          const previewDesign = resolveDraftPreviewDesign({
            design: preview.design,
            previewThreadId: preview.threadId,
            activeThreadId: preview.threadId,
            currentParams: get(paramPanelState).params,
          });
          try {
            hydrateActiveRenderSnapshot({
              snapshotId: preview.previewId,
              threadId: preview.threadId,
              messageId: preview.previewId,
              eventModelId: preview.modelId ?? null,
              design: previewDesign,
              artifactBundle: preview.artifactBundle,
              modelManifest: preview.modelManifest,
              selectedPartId: null,
              stlUrl: toAssetUrl(preview.artifactBundle.modelStlPath),
              status: preview.feedback?.summary || 'Preview rendered.',
              targetRef: {
                kind: 'savedVersion',
                threadId: preview.threadId,
                messageId: preview.previewId,
              },
            });
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
          } catch (error) {
            const message = error instanceof RenderSnapshotMismatch
              ? error.message
              : `Render snapshot rejected: ${String(error)}`;
            session.setError(message);
            recordSessionActivityEvent({
              threadId: preview.threadId,
              versionId: preview.previewId,
              sessionId: preview.sessionId ?? 'local-session',
              actor: { kind: 'agent', id: preview.sessionId ?? 'agent', label: 'Agent' },
              kind: 'validation_reported',
              title: 'Render snapshot rejected',
              summary: message,
              severity: 'error',
              raw: { previewModelId: preview.modelId, manifestModelId: preview.modelManifest.modelId },
            });
            return;
          }
        }
        recordSessionActivityEvent({
          threadId: preview.threadId,
          versionId: preview.previewId,
          sessionId: preview.sessionId ?? 'local-session',
          actor: {
            kind: 'agent',
            id: preview.sessionId ?? 'agent',
            label: projectedThreadAgentState.agentLabel ?? 'Agent',
          },
          kind: preview.feedback ? 'validation_reported' : 'preview_updated',
          title: preview.feedback ? 'Preview validation reported' : 'Draft preview updated',
          summary: preview.feedback?.summary || 'Draft preview rendered.',
          severity: preview.feedback?.status === 'failed'
            ? 'error'
            : preview.feedback?.status === 'warning'
              ? 'warning'
              : preview.feedback?.status === 'passed'
                ? 'success'
                : 'info',
          state: preview.feedback?.status === 'failed'
            ? 'failed'
            : preview.feedback?.status === 'checking'
              ? 'active'
              : 'resolved',
          requiresAttention: preview.feedback?.status === 'failed',
          artifacts: [
            {
              kind: 'preview_file',
              label: 'Draft model STL',
              value: preview.artifactBundle.modelStlPath ?? preview.artifactBundle.modelId,
              raw: {
                modelId: preview.artifactBundle.modelId,
                artifactVersion: preview.artifactBundle.artifactVersion,
              },
            },
          ],
          raw: preview.feedback ?? null,
        });
    };
    const unlistenDraftPreviewChanged = canListenToTauri ? listen<AgentDraftPreviewChangedEvent>(
      'agent-draft-preview-changed',
      (event) => {
        const changed = event.payload;
        const revisionKey = `${changed.sessionId}:${changed.threadId}`;
        const previousRevision = latestDraftPreviewRevision.get(revisionKey) ?? 0;
        if (changed.revision <= previousRevision) return;
        latestDraftPreviewRevision.set(revisionKey, changed.revision);
        if (!shouldApplyDraftPreviewToWorkspace({
          activeThreadId: get(activeThreadId),
          previewThreadId: changed.threadId,
        })) return;
        void getAgentDraftPreview(changed.threadId, changed.previewId)
          .then((draft) => {
            if (latestDraftPreviewRevision.get(revisionKey) !== changed.revision) return;
            if (draft.previewId !== changed.previewId) return;
            if (!shouldApplyDraftPreviewToWorkspace({
              activeThreadId: get(activeThreadId),
              previewThreadId: draft.threadId,
            })) return;
            applyDraftPreview({
              sessionId: draft.sessionId,
              threadId: draft.threadId,
              previewId: draft.previewId,
              baseMessageId: draft.baseMessageId ?? null,
              modelId: draft.artifactBundle.modelId,
              design: draft.designOutput,
              artifactBundle: draft.artifactBundle,
              modelManifest: draft.modelManifest,
              feedback: draft.draftFeedback ?? null,
            });
          })
          .catch((error) => {
            session.setError(`Draft Preview Load Error: ${formatBackendError(error)}`);
          });
      },
    ) : noopUnlisten;
    const unlistenSessions = canListenToTauri ? listen<AgentSession[]>('agent-sessions-changed', (event) => {
      activeAgentSessions = event.payload;
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
    const unlistenCodexTakeover = canListenToTauri ? listen<{
      threadId: string;
      method: string;
      liveMessages?: CodexTakeoverSnapshot['liveMessages'];
      turnTraces?: CodexTakeoverSnapshot['turnTraces'];
      runtime?: CodexTakeoverSnapshot['runtime'];
    }>(
      'codex-provider-updated',
      (event) => scheduleCodexTakeoverRefresh(event.payload),
    ) : noopUnlisten;
    const unlistenAgyProvider = canListenToTauri ? listen<{
      conversationId?: string | null;
      method: string;
      liveMessages?: AgyProviderSnapshot['liveMessages'];
      turnTraces?: AgyProviderSnapshot['turnTraces'];
      runtime?: AgyProviderSnapshot['runtime'];
    }>(
      'agy-provider-updated',
      (event) => scheduleAgyProviderRefresh(event.payload),
    ) : noopUnlisten;
    void (async () => {
      await boot();
      await loadCampaignDefinitions();
      await loadCampaignRuns();
      if (canListenToTauri) {
        const recovery = await getWebContentRecoveryState().catch(() => null);
        if (recovery?.rawError) {
          session.setError(
            `WebContent ${recovery.blocked ? 'recovery stopped' : 'recovered'}: ${recovery.rawError}`,
          );
          if (!recovery.blocked) {
            recoveryResetTimer = setTimeout(() => {
              void acknowledgeWebContentRecovery();
            }, 30_000);
          }
        }
      }
    })();
    return () => {
      teardownWindowStore();
      resetAgentTerminalStore();
      agentNotificationProjectionDisconnect?.();
      agentNotificationProjectionDisconnect = null;
      void agentActivityConnection?.disconnect();
      agentActivityConnection = null;
      void unlisten.then(fn => fn());
      void unlistenPrompt.then(fn => fn());
      void unlistenPromptClosed.then(fn => fn());
      void unlistenViewportScreenshot.then(fn => fn());
      void unlistenHistory.then(fn => fn());
      void unlistenGeometryRender.then(fn => fn());
      void unlistenProjectFolderSync.then(fn => fn());
      void unlistenDraftPreviewChanged.then(fn => fn());
      void unlistenSessions.then(fn => fn());
      void unlistenTerminal.then(fn => fn());
      void unlistenWorkingVersion.then(fn => fn());
      void unlistenCodexTakeover.then(fn => fn());
      void unlistenAgyProvider.then(fn => fn());
      if (codexTakeoverRefreshTimer) clearTimeout(codexTakeoverRefreshTimer);
      if (agyProviderRefreshTimer) clearTimeout(agyProviderRefreshTimer);
      if (recoveryResetTimer) clearTimeout(recoveryResetTimer);
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
    isMcpConnection
      ? { supportsVision: true, reason: null }
      : resolveEngineCapabilitySummary(selectedEngine),
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
  const eckySeedIdentity = $derived(`thread:${$activeThreadId ?? 'unbound'}`);
  const baseEckyTraits = $derived<GenieTraits>(
    activeThread?.genieTraits ?? buildModelGenieTraits({
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

  const agentRuntime = createAgentRuntime({
    hasIpc: hasTauriIpc,
    setError: (message) => session.setError(message),
  });

  $effect(() => {
    setAgentTerminalSelection(primaryAgentId, projectedThreadAgentState.sessionId);
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
    Boolean(
      hasSketchPreview
        ? sketchPreview?.artifactBundle?.modelStlPath
        : activeArtifactBundle?.modelStlPath,
    ),
  );
  const previewArtifactName = $derived.by<string | null>(() =>
    sketchPreviewStatus?.artifactName ||
    fileBasename(
      hasSketchPreview
        ? sketchPreview?.artifactBundle?.modelStlPath
        : activeArtifactBundle?.modelStlPath,
    ) ||
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
  let genieErrorReactionActive = $state(false);
  let lastGenieErrorReactionKey = '';
  let genieErrorReactionTimer: ReturnType<typeof setTimeout> | null = null;
  const genieErrorReactionSignal = $derived.by(() => {
    if (projectedThreadAgentState.connectionState === 'error') {
      return [
        'agent',
        projectedThreadAgentState.sessionId ?? '',
        projectedThreadAgentState.updatedAt ?? '',
        projectedThreadAgentState.statusText ?? '',
      ].join(':');
    }
    const request = activeThreadLatestErrorRequest;
    return request ? `request:${request.id}:${request.error ?? ''}` : '';
  });

  $effect(() => {
    const latestRequest = $activeThreadRequests.at(-1);
    // A later prompt/success supersedes an older error reaction immediately.
    // Keep the error card/history, but do not leave Ecky visually angry.
    if (latestRequest && latestRequest.phase !== 'error' && latestRequest.phase !== 'canceled') {
      lastGenieErrorReactionKey = '';
      genieErrorReactionActive = false;
      if (genieErrorReactionTimer) {
        clearTimeout(genieErrorReactionTimer);
        genieErrorReactionTimer = null;
      }
      return;
    }
    const reactionKey = genieErrorReactionSignal;
    if (!reactionKey) {
      lastGenieErrorReactionKey = '';
      genieErrorReactionActive = false;
      return;
    }
    if (reactionKey === lastGenieErrorReactionKey) return;
    lastGenieErrorReactionKey = reactionKey;
    genieErrorReactionActive = true;
    if (genieErrorReactionTimer) clearTimeout(genieErrorReactionTimer);
    genieErrorReactionTimer = setTimeout(() => {
      genieErrorReactionTimer = null;
      genieErrorReactionActive = false;
    }, 2600);
  });

  const activeConfirm = $derived(pendingConfirms[0] ?? null);
  const threadAgentMascot = $derived.by(() => deriveMascotStateForThreadAgent(projectedThreadAgentState));

  const genieMode = $derived.by(() => {
    if ($onboarding.isActive) return 'speaking';
    if (pendingCaptureProjectSwitch) return 'speaking';
    if (activeViewportScreenshotChoice) return 'speaking';
    if (activeConfirm) return 'speaking';
    if (isActiveMcpMode && activeAgentTerminalAttention) return 'speaking';
    if (activePendingAgentPrompt) return 'speaking';
    if (hasQueuedAgentMessageWithoutPrompt) return 'light';
    if (projectedThreadAgentState.connectionState === 'waking') return 'waking';
    if (projectedThreadAgentState.connectionState === 'waiting') return 'light';
    if (projectedThreadAgentState.connectionState === 'active') return threadAgentMascot.mode;
    if (projectedThreadAgentState.connectionState === 'error') {
      return genieErrorReactionActive ? 'error' : 'idle';
    }
    if (projectedThreadAgentState.connectionState === 'sleeping') return 'sleeping';
    if (activeMcpRenderBusy) return 'rendering';
    if (activeMcpBusy && activeMcpBubbleSummary) return 'speaking';
    if (activeMcpBusy) return 'thinking';
    if (hasLiveMcpSession) return 'light';
    const atPhase = activeThreadHighestPhase;
    if (atPhase === 'error') return genieErrorReactionActive ? 'error' : 'idle';
    if (atPhase === 'repairing') return 'repairing';
    if (atPhase === 'classifying') return 'light';
    if (atPhase === 'rendering') return 'rendering';
    if (atPhase === 'generating' || atPhase === 'answering') return 'thinking';
    if (assistantFresh && !dismissedBubbleText && lastAdvisorBubble) return 'speaking';
    if (projectedThreadAgentState.connectionState === 'disconnected') return 'idle';
    return 'idle';
  });

  const genieBubbleState = $derived.by(() =>
    resolveGenieBubblePresentation({
      sessionError: errorText,
      sessionAuthoringError,
      onboardingText: $onboarding.isActive ? $onboarding.text : null,
      viewportScreenshotMessage:
        pendingCaptureProjectSwitch?.message ?? activeViewportScreenshotChoice?.message ?? null,
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
      threadAgentState: projectedThreadAgentState,
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
      senderLabel: projectedThreadAgentState.agentLabel ?? null,
    }),
  );
  let selectedSessionActivityEventId = $state<string | null>(null);
  let dialogueFocusRequest = $state(0);
  let agentActivityConnection: { disconnect: () => Promise<void> } | null = null;
  let agentNotificationProjectionDisconnect: (() => void) | null = null;
  let seenAgentActivityEventIds = new Set<string>();
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

  const sessionActivityEvents = $derived.by<SessionEvent[]>(() => $sessionActivityEventStore);
  const sessionActivity = $derived.by(() =>
    composeSessionActivity(sessionActivityEvents, $activeThreadId ?? null, $activeVersionId ?? null),
  );
  const sessionBubbleEvent = $derived.by(() => composeBubbleEvent(sessionActivity));
  const sessionCodeDiffView = $derived.by(() => composeCodeDiffView(sessionActivity, $selectedCode));

  function openSessionActivityFromBubble() {
    const event = sessionBubbleEvent.event;
    if (event) selectedSessionActivityEventId = event.id;
    mountedWindows.activity = true;
    showWindow('activity');
  }

  function selectSessionActivityEvent(id: string) {
    selectedSessionActivityEventId = id;
  }

  async function openNotificationThreadDialogue(threadId: string | null) {
    if (threadId && threadId !== $activeThreadId) {
      let target = $history.find((thread) => thread.id === threadId) ?? null;
      if (!target) {
        try {
          target = await resolveThreadSummary(threadId);
        } catch (error) {
          session.setError(`Project Open Error: ${formatBackendError(error)}`);
          return;
        }
      }
      if (!target) {
        session.setError(`Project Open Error: Thread ${threadId} is unavailable.`);
        return;
      }
      await loadFromHistory(target);
    }
    closeWindowStore('activity');
    showWindow('dialogue');
    await tick();
    dialogueFocusRequest += 1;
  }

  function openNotificationActivityEvent(
    eventId: string,
    threadId: string | null,
    local: boolean,
  ) {
    const exact = sessionActivityEvents.find((event) => event.id === eventId) ?? null;
    const fallback = local
      ? [...sessionActivityEvents].reverse().find((event) =>
          event.threadId === threadId &&
          (event.severity === 'error' || event.kind === 'validation_reported'),
        ) ?? null
      : null;
    selectedSessionActivityEventId = (exact ?? fallback)?.id ?? eventId;
    mountedWindows.activity = true;
    showWindow('activity');
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
    if (pendingCaptureProjectSwitch) {
      return [
        { label: 'SWITCH TO PROJECT', onclick: () => void answerCaptureProjectSwitch('switch') },
        { label: 'STAY HERE', onclick: () => void answerCaptureProjectSwitch('stay') },
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
    if (activeAgentTerminalAttention) {
      return [
        {
          label: 'OPEN TERMINAL',
          onclick: () => {
            if (!terminalWindowState.visible) toggleWindow('terminal');
          },
        },
      ];
    }
    if ($activeThreadId && projectedThreadAgentState.connectionState !== 'none') {
      const connectionState = projectedThreadAgentState.connectionState;
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
            if (visibleAgentTerminal?.active) return;
            if (!terminalWindowState.visible) toggleWindow('terminal');
          },
        });
      } else {
        if (
          visibleAgentTerminal?.active &&
          hasQueuedAgentMessageWithoutPrompt &&
          projectedThreadAgentState.connectionState === 'active'
        ) {
          actions.push({
            label: 'NUDGE AGENT',
            onclick: () => {
              void handleNudgeAgentPromptRearm();
            },
          });
        }
      }
      return actions;
    }
    return null;
  });

  const localNotificationSources = new Set([
    'sessionError',
    'onboarding',
    'viewportScreenshot',
    'confirm',
    'terminalAttention',
    'pendingPrompt',
    'draftFeedback',
    'queuedMessage',
    'threadError',
    'repair',
    'cooking',
    'assistant',
  ]);

  $effect(() => {
    const appError = globalErrorText;
    if (appError) {
      localNotificationActionsStore.set({
        eventId: `local-ui:globalError:${appError}`,
        threadId: null,
        actorLabel: 'ECKY',
        summary: appError,
        detail: null,
        severity: 'error',
        state: 'failed',
        requiresAttention: true,
        actions: [],
      });
      return;
    }

    const folderNotice = projectFolderNotice;
    if (folderNotice) {
      const isError = folderNotice.tone === 'error';
      const isPending = folderNotice.tone === 'pending';
      const activityEvent = findNotificationActivityEvent(sessionActivityEvents, {
        threadId: folderNotice.threadId,
        versionId: folderNotice.messageId,
        summary: folderNotice.body,
      });
      localNotificationActionsStore.set({
        eventId: activityEvent?.id ?? `local-ui:project-folder:${folderNotice.tone}:${folderNotice.threadId}:${folderNotice.messageId ?? folderNotice.body}`,
        activityEventId: activityEvent?.id ?? null,
        threadId: folderNotice.threadId,
        actorLabel: 'ECKY',
        summary: activityEvent?.summary ?? folderNotice.title,
        detail: activityEvent?.detail ?? folderNotice.body,
        severity: activityEvent?.severity ?? (isError ? 'error' : 'info'),
        state: activityEvent?.state === 'failed'
          ? 'failed'
          : activityEvent?.state === 'active'
            ? 'active'
            : isError
              ? 'failed'
              : isPending
                ? 'active'
                : 'resolved',
        requiresAttention: isError,
        actions: [],
      });
      return;
    }

    const source = genieBubbleState.source;
    const text = genieBubbleState.text;
    const actions = genieActions ?? [];
    if (!text || !localNotificationSources.has(source)) {
      localNotificationActionsStore.set(null);
      return;
    }

    const eventId = `local-ui:${source}:${bubbleActivityTimestamp}`;
    const isError = source === 'sessionError' || source === 'threadError';
    const needsAnswer = actions.length > 0 || source === 'pendingPrompt' || source === 'terminalAttention';
    const linkToActivity = source === 'draftFeedback' || source === 'sessionError' || source === 'threadError' || source === 'repair';
    const activityEvent = linkToActivity
      ? findNotificationActivityEvent(sessionActivityEvents, {
          threadId: $activeThreadId ?? null,
          versionId: source === 'draftFeedback'
            ? activeDraftFeedback?.previewId ?? $activeVersionId
            : $activeVersionId,
          summary: text,
        })
      : null;
    localNotificationActionsStore.set({
      eventId: activityEvent?.id ?? eventId,
      activityEventId: activityEvent?.id ?? null,
      threadId: $activeThreadId ?? null,
      actorLabel: 'ECKY',
      summary: activityEvent?.summary ?? text,
      detail: activityEvent?.detail ?? (
        [genieBubbleState.badge, genieBubbleState.layer, genieBubbleState.fix].filter(Boolean).join(' · ') || null
      ),
      severity: activityEvent?.severity ?? (isError ? 'error' : needsAnswer ? 'question' : 'info'),
      state: activityEvent?.state === 'failed'
        ? 'failed'
        : activityEvent?.state === 'active'
          ? 'active'
          : isError
            ? 'failed'
            : needsAnswer
              ? 'active'
              : 'resolved',
      requiresAttention: isError || needsAnswer,
      actions,
    });
  });

  async function applyCompletedRequest(req: Request) {
    if (!req?.result) return;
    const { design, threadId, messageId, stlUrl: reqStlUrl, artifactBundle, modelManifest } =
      req.result;
    const runtime = await inspectRuntimeBundle(
      artifactBundle ?? null,
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

  async function cancelRequest(id: string) {
    const request = get(requestQueue).byId[id];
    try {
      if (request?.threadId) {
        await stopExplorationRun({ requestId: id, threadId: request.threadId });
      }
    } catch (error) {
      session.setError(`Exploration Stop Error: ${formatBackendError(error)}`);
    } finally {
      requestQueue.cancel(id);
    }
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
    showExportChooser = false;
    await modelIo.exportModel(mode, activeArtifactBundle, exportDefaultNames, multipartExportParts, hasMultipartExportModel, exportModelTitle);
  }

  function dismissGenie() {
    if (genieBubble) dismissedBubbleText = genieBubble;
    stopEckySpeech();
  }

  onDestroy(() => {
    localNotificationActionsStore.set(null);
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

  function handleParamPanelChange(nextParams: DesignParams) {
    return handleParamChange(nextParams, null, true);
  }

  async function handleSemanticControlChange(primitiveId: string, value: ParamValue) {
    const threadId = $activeThreadId;
    const targetMessageId = activeVersionMessage?.id ?? $activeVersionId;
    if (!threadId || !targetMessageId) {
      session.setError('Semantic Control Error: exact thread and version target required.');
      return;
    }
    try {
      session.setError(null);
      const result = await applySemanticControlValue({
        threadId,
        targetMessageId,
        primitiveId,
        value,
      });
      if (Object.keys(result.parameterPatch).length === 0) return;
      stageParamChange(result.parameterPatch);
    } catch (error) {
      session.setError(`Semantic Control Error: ${formatBackendError(error)}`);
    }
  }

  function handleSelectControlView(viewId: string | null) {
    activeControlViewId = viewId;
  }

  function projectImportedModel(
    result: Awaited<ReturnType<typeof importModelIntent>>,
    status: string,
  ) {
    if (!result.snapshotId) {
      throw new Error('Imported model returned no backend snapshotId.');
    }
    rememberCommittedVersionMessage(result.threadId, result.title, result.message);
    activeThreadId.set(result.threadId);
    hydrateActiveRenderSnapshot({
      snapshotId: result.snapshotId,
      threadId: result.threadId,
      messageId: result.messageId,
      design: result.designOutput,
      artifactBundle: result.artifactBundle,
      modelManifest: result.modelManifest,
      selectedPartId: null,
      stlUrl: toAssetUrl(result.artifactBundle.modelStlPath),
      status,
    });
    currentView.set('workbench');
    if (result.modelManifest.enrichmentState?.status === 'pending') {
      showEnrichmentModal = true;
    }
  }

  async function handleImportFcstd(sourcePath: string) {
    try {
      if (freecadUnavailableReason) {
        session.setError(`FCStd Import Error: ${freecadUnavailableReason}`);
        return;
      }
      session.setError(null);
      session.setStatus('Importing FCStd...');
      const importedName = sourcePath.split(/[\\/]/).pop() || 'model.FCStd';
      const result = await importModelIntent({ source: { kind: 'fcstd', sourcePath } });
      projectImportedModel(result, `Imported FCStd: ${importedName}`);
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
      const result = await importModelIntent({ source: { kind: 'freecadLibrary', item } });
      projectImportedModel(result, `Imported FreeCAD library part: ${item.name}`);
    } catch (e: unknown) {
      session.setError(`FreeCAD Library Import Error: ${formatBackendError(e)}`);
      throw e;
    }
  }

  async function handleApplyComponentImport(input: {
    packageId: string;
    version: string;
    componentId: string;
    label: string;
  }) {
    const threadId = get(activeThreadId);
    const baseMessageId = get(activeVersionId);
    const expectedSourceDigest =
      sessionModelManifest?.sourceDigest ?? activeVersionMessage?.modelManifest?.sourceDigest;
    if (!threadId || !baseMessageId || !expectedSourceDigest) {
      throw new Error('Component import requires an active persisted design source.');
    }

    session.setStatus(`Copy-inlining component: ${input.label}...`);
    const result = await applyInlineComponentImport({
      threadId,
      baseMessageId,
      expectedSourceDigest,
      packageId: input.packageId,
      version: input.version,
      componentId: input.componentId,
    });
    const imported = result.version;
    if (
      imported.status === 'error' ||
      imported.error ||
      !imported.messageId ||
      !imported.artifactBundle ||
      !imported.modelManifest ||
      !imported.snapshotId
    ) {
      throw imported.error ?? new Error('Component import returned no persisted runtime.');
    }

    rememberCommittedVersionMessage(threadId, imported.designOutput.title, {
      id: imported.messageId,
      role: 'assistant',
      content: imported.designOutput.response,
      status: imported.status,
      output: imported.designOutput,
      usage: null,
      artifactBundle: imported.artifactBundle,
      modelManifest: imported.modelManifest,
      agentOrigin: null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
      timestamp: Date.now() / 1000,
    });
    hydrateActiveRenderSnapshot({
      snapshotId: imported.snapshotId,
      threadId,
      messageId: imported.messageId,
      design: imported.designOutput,
      artifactBundle: imported.artifactBundle,
      modelManifest: imported.modelManifest,
      selectedPartId: null,
      stlUrl: toAssetUrl(imported.artifactBundle.modelStlPath),
      status: `${input.label} imported as a new version.`,
    });
  }

</script>

<svelte:window onbeforeunload={hardFlushWindowLayout} onresize={handleViewportResize} />

<div class="app-page" role="application">
  {#if $onboarding.isActive}
    <div class="onboarding-backdrop"></div>
  {/if}
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
                femResult={visibleFemResult}
                femMeshPreview={visibleFemMeshPreview}
                {femDisplay}
                manifestParts={hasSketchPreview ? [] : activeModelManifest?.parts ?? []}
                edgeTargets={hasSketchPreview ? sketchPreview?.artifactBundle?.edgeTargets ?? [] : activeArtifactBundle?.edgeTargets ?? []}
                faceTargets={hasSketchPreview ? sketchPreview?.artifactBundle?.faceTargets ?? [] : activeArtifactBundle?.faceTargets ?? []}
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
                retainModelWhileLoading={$activeThreadVersionLoading}
                hideModelWhileBusy={showViewerBusyMask}
                busyPhase={viewerBusyPhase}
                busyText={viewerBusyText}
                topologyMode={viewerTopologyMode}
              />
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
              onClearAll={() => { drawMode = false; }}
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
            <div
              class="genie-layer"
              class:onboarding-active={$onboarding.isActive}
              class:choice-active={Boolean(pendingCaptureProjectSwitch || activeViewportScreenshotChoice)}
            >
              <VertexGenie 
                mode={genieMode} 
                safeRightInset={genieSafeRightInset}
                relay={genieRelay}
                traits={eckyTraits} 
                intensity={eckyIntensity} 
                wakeUp={genieWakeUpCount}
                onOpenProjects={() => showWindow('projects')}
                onOpenActivity={openSessionActivityFromBubble}
                agentConnected={
                  threadAgentMascot.connected ||
                  hasLiveMcpSession ||
                  !!visibleAgentTerminal?.active ||
                  hasLiveApiConnection
                }
              />
              <AgentNotificationCenter
                activeThreadId={$activeThreadId ?? null}
                onOpenThreadDialogue={openNotificationThreadDialogue}
                onOpenActivityEvent={openNotificationActivityEvent}
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
                <ViewportWorkspace
                  showCode={false}
                  busy={showViewerBusyMask}
                  showExport={Boolean(activeArtifactBundle)}
                  canExport={canExportModel}
                  hasSketchPreview={hasSketchPreview}
                  onFork={() => void forkDesign()}
                  onExport={() => (showExportChooser = true)}
                />
              </div>
            {/if}
          </main>
        </div>
      </div>
    {:else if $currentView === 'campaign' && activeCampaignRun && campaignStep}
      <main class="campaign-project-page" aria-label="Campaign project">
        <CampaignWorkbench
          campaign={campaignStep}
          bind:run={activeCampaignRun}
          onTransition={transitionCampaignRun}
          onClose={() => void closeCampaignSurface()}
        />
      </main>
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

  {#if isBooting && !geometryRenderActive}
    <div class="boot-overlay">
      <div class="boot-overlay__glass"></div>
      <div class="boot-overlay__content">
        <div class="boot-overlay__title">ECKY CAD</div>
        <div class="boot-overlay__ecky">
          <VertexGenie mode="thinking" fitToCanvas={true} />
        </div>
        <div class="boot-overlay__status">{status || 'Restoring environment...'}</div>
      </div>
    </div>
  {/if}

  <WorkbenchWindows
    currentView={$currentView}
    windowStates={{
      code: codeWindowState,
      projects: projectsWindowState,
      library: libraryWindowState,
      capture: captureWindowState,
      analysis: analysisWindowState,
      params: paramsWindowState,
      dialogue: dialogueWindowState,
      docs: docsWindowState,
      settings: settingsWindowState,
      terminal: terminalWindowState,
      activity: activityWindowState,
      sketch: $windowStore.sketch,
    }}
    {mountedWindows}
    highlightTarget={$onboarding.highlightTarget}
    {drawMode}
    canDraw={selectedModelCapabilities.supportsVision}
    drawUnavailableReason={selectedModelCapabilities.reason}
    terminalDock={visibleAgentTerminal
      ? { agentLabel: visibleAgentTerminal.agentLabel, attentionRequired: visibleAgentTerminal.attentionRequired }
      : null}
    bind:overlayActionsEl
    onActivateWindow={handleDockWindowActivate}
    onDrawToggle={() => {
      if (drawMode) drawingOverlay?.clear();
      drawMode = !drawMode;
    }}
    onCloseView={() => currentView.set('workbench')}
    onCloseWindow={closeWindowStore}
  >
    {#snippet projectsContent()}
      {#if campaignRunError}
        <p class="campaign-project-error" role="alert">{campaignRunError}</p>
      {/if}
      <ProjectSwitcher
        onImportFcstd={handleImportFcstd}
        onOpenNewProjectChooser={() => showNewProjectChooser = true}
        freecadUnavailableReason={freecadUnavailableReason}
        {campaignRuns}
        activeCampaignRunId={activeCampaignRun?.id ?? null}
        onStartCampaign={() => void startCampaign()}
        onOpenCampaignRun={(run) => void openCampaignRun(run)}
        onDeleteCampaignRun={deleteCampaignRun}
      />
    {/snippet}

    {#snippet libraryContent()}
      <LibraryPanel
        onImportFreecadLibraryPart={handleImportFreecadLibraryPart}
        onApplyComponentImport={handleApplyComponentImport}
      />
    {/snippet}

    {#snippet captureContent()}
      <CapturePanel
        sessionState={captureSessionState}
        pairingUrl={capturePairingUrl}
        trustUrl={captureTrustUrl}
        cameraStatus={captureCameraStatus}
        guidance={captureGuidance}
        stats={captureStats}
        onStartCapture={startCaptureSession}
        onOpenLastCapture={openLastStoredCapture}
        onCancelCapture={cancelCaptureSession}
        onApplyPreview={applyCapturePreview}
        onCommitPreview={commitCapturePreview}
        onRetryReconstruction={retryCaptureReconstructionFromDesktop}
        onAddPhotos={addCapturePhotos}
        onPreviewLoadError={(message) => patchCaptureWorkspace({ cameraStatus: message })}
        meshPreview={captureMeshPreview}
        previewModelKey={capturePreviewBundle?.modelId ?? null}
        previewUrl={capturePreviewUrl}
        {externalShapeSources}
        {selectedExternalShapeNodeId}
        {externalShapePreviewUrl}
        {externalShapeRawPreviewUrl}
        externalShapeTargetMessageId={$activeVersionId}
        {externalShapePreviewIsCropped}
        {externalShapeError}
        onSelectExternalShape={(nodeId) => selectedExternalShapeNodeId = nodeId}
        onApplyExternalPlaneCrop={applySelectedExternalShapePlaneCrop}
        onRemoveExternalPlaneCrop={removeSelectedExternalShapePlaneCrop}
        onPreviewExternalSurfaceTrimPath={previewSelectedExternalShapeSurfaceTrimPath}
        onPreviewExternalSurfaceTrimLoop={previewSelectedExternalShapeSurfaceTrimLoop}
        onPreviewExternalSurfaceTrimRegion={previewSelectedExternalShapeSurfaceTrimRegion}
        onApplyExternalSurfaceTrim={applySelectedExternalShapeSurfaceTrim}
        onRemoveExternalSurfaceTrim={removeSelectedExternalShapeSurfaceTrim}
        previewApplied={capturePreviewApplied}
        applyPending={captureApplyPending}
        previewScale={capturePreviewScale}
        cropEnabled={captureCropEnabled}
        cropMode={captureCropMode}
        cropBounds={captureCropBounds}
        cropDirty={captureCropDirty}
        onPreviewScaleChange={updateCapturePreviewScale}
        onCropEnabledChange={updateCaptureCropEnabled}
        onCropModeChange={(mode) => captureCropMode = mode}
        onCropBoundsChange={updateCaptureCropBounds}
        onPreviewCrop={previewCaptureCrop}
        onResetCrop={resetCaptureCrop}
        guideMode={captureGuideMode}
        guide={captureGuide}
        guideState={captureGuideState}
        guidePickRole={captureGuidePickRole}
        guideReady={captureGuideReadiness.ready}
        guideReadinessReasons={captureGuideReadiness.reasons}
        guideKnownDistanceMm={captureGuideKnownDistanceMm}
        guideFeatureDepthMm={captureGuideFeatureDepthMm}
        guideInstruction={captureGuideInstruction}
        guideError={captureGuideError}
        guideComparisonError={captureComparisonError}
        guideComparisonModelKey={captureGeneratedComparisonBundle?.modelId ?? null}
        guideComparisonUrl={captureGeneratedComparisonUrl}
        guideResult={captureGuidedResult}
        guideDeviation={captureGuidedDeviation}
        guideDeviationVisible={captureDeviationVisible}
        guideReferenceVisible={captureReferenceVisible}
        guideReferenceOpacity={captureReferenceOpacity}
        guideGeneratedVisible={captureGeneratedVisible}
        guideGeneratedOpacity={captureGeneratedOpacity}
        guideCanUndo={(captureGuideHistory?.past.length ?? 0) > 0}
        guideSelectedLandmarkId={captureGuideSelectedLandmarkId}
        onStartGuidedCad={startCaptureGuidedCad}
        onGuidePickRoleChange={(role) => captureGuidePickRole = role}
        onGuideAnchor={addCaptureGuideAnchor}
        onGuideAnchorError={(message) => captureGuideError = message}
        onGuideSelectLandmark={(landmarkId) => captureGuideSelectedLandmarkId = landmarkId}
        onGuideEditLandmark={editCaptureGuideLandmark}
        onGuideDeleteLandmark={deleteCaptureGuideLandmark}
        onGuideUndo={undoCaptureGuideEdit}
        onGuideEditProfile={editCaptureGuideProfile}
        onGuideMoveProfileLandmark={reorderCaptureGuideProfile}
        onGuideEditExpectation={editCaptureGuideExpectation}
        onGuideSelectFeaturePlan={selectCaptureFeaturePlan}
        onGuideReferenceVisibleChange={(visible) => captureReferenceVisible = visible}
        onGuideReferenceOpacityChange={(opacity) => captureReferenceOpacity = opacity}
        onGuideGeneratedVisibleChange={(visible) => captureGeneratedVisible = visible}
        onGuideGeneratedOpacityChange={(opacity) => captureGeneratedOpacity = opacity}
        onGuideDeviationVisibleChange={(visible) => captureDeviationVisible = visible}
        onGuideKnownDistanceChange={(value) => captureGuideKnownDistanceMm = value}
        onGuideFeatureDepthChange={(value) => captureGuideFeatureDepthMm = value}
        onGuideInstructionChange={(value) => captureGuideInstruction = value}
        onValidateGuide={validateCaptureGuide}
        onBuildCadFromGuide={buildCadFromCaptureGuide}
      />
    {/snippet}

    {#snippet analysisContent()}
      <AnalysisPanel
        modelId={activeArtifactBundle?.modelId ?? null}
        source={$workingCopy.macroCode || activeVersionMessage?.output?.macroCode || ''}
        onResultChange={(result) => {
          femResult = result;
          femResultSource = result ? ($workingCopy.macroCode || activeVersionMessage?.output?.macroCode || '') : '';
        }}
        onMeshChange={(mesh) => {
          femMeshPreview = mesh;
          femMeshPreviewSource = mesh ? ($workingCopy.macroCode || activeVersionMessage?.output?.macroCode || '') : '';
        }}
        onDisplayChange={(display) => femDisplay = display}
      />
    {/snippet}

    {#snippet paramsContent()}
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
          onpostprocessingchange={(nextPostProcessing) => workingCopy.patch({ postProcessing: nextPostProcessing })}
          onSemanticChange={handleSemanticControlChange}
          onApplyMacroCode={(code) => applyManualCodeDraft(code)}
          onchange={handleParamPanelChange}
          manualApplyBusy={$manualApplyBusyStore}
          onspecchange={(spec, params) => {
            paramPanelState.setUiSpec(spec);
            workingCopy.patch({ uiSpec: spec });
            if (params) {
              paramPanelState.setParams(params);
              workingCopy.patch({ params });
            }
          }}
          activeVersionId={$paramPanelState.versionId}
          threadId={$activeThreadId}
          messageId={$activeVersionId}
          macroCode={viewportCodeWorkingCopyAligned ? $workingCopy.macroCode : activeVersionMessage?.output?.macroCode ?? ''}
          outlineEnabled={viewerOutlineEnabled}
          topologyMode={viewerTopologyMode}
          selectionMode={viewerMode}
          onViewerDisplayChange={(display) => {
            viewerOutlineEnabled = display.outlineEnabled;
            viewerTopologyMode = display.topologyMode;
          }}
          onViewerSelectionModeChange={(mode) => viewerMode = mode}
          onShowCode={() => {
            void openVersionCodeModal({
              code: $workingCopy.macroCode,
              title: $workingCopy.title,
              messageId: $activeVersionId ?? $workingCopy.sourceVersionId,
              sourceLanguage: $workingCopy.sourceLanguage,
              geometryBackend: $workingCopy.geometryBackend,
            });
          }}
        />
      </div>
    {/snippet}

    {#snippet settingsContent()}
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
    {/snippet}

    {#snippet activityContent()}
      <SessionActivityWindow
        events={sessionActivity.events}
        activeThreadId={$activeThreadId ?? null}
        selectedEventId={selectedSessionActivityEventId}
        onSelectEvent={selectSessionActivityEvent}
      />
    {/snippet}

    {#snippet dialogueContent()}
      <DialogueWindowContent
        rememberLayout={$windowLayoutRemembered}
        onRememberLayoutChange={(remember) => void setThreadWindowLayoutRemembered(remember)}
        activeThreadId={$activeThreadId}
        bind:activeVersionId={$activeVersionId}
        promptProps={{
          onGenerate: handlePromptPanelSubmit,
          isGenerating: $activeThreadBusy,
          generationUnavailableReason,
          imageAttachmentUnavailableReason: imageInputUnavailableReason,
          dialogueState,
          codexTakeover: activeProviderSnapshot,
          codexTakeoverError,
          onLoadEarlierCodexMessages: handleLoadEarlierCodexMessages,
          onSteerCodexTakeover: handleCodexSteer,
          onStopCodexTakeover: handleStopCodexTakeover,
          onRetryCodexQueue: handleRetryCodexQueue,
          onRemoveCodexQueue: handleRemoveCodexQueue,
          messages: activeThreadDialogueMessages,
          captureRuns: captureHistoryRuns,
          messagesLoading: $activeThreadMessagesLoading,
          messagesHasMore: activeThread ? ($threadMessagePageState[activeThread.id]?.hasMore ?? false) : false,
          messagesPageLoading: activeThread ? ($threadMessagePageState[activeThread.id]?.isLoading ?? false) : false,
          requests: $activeThreadRequests,
          explorationCycle,
          onLoadOlderMessages: () => activeThread ? loadOlderThreadMessages(activeThread.id) : undefined,
          activeThreadId: $activeThreadId,
          sendWorkspaceCapture: sendWorkspaceCaptureForActiveThread,
          workspaceCaptureHint,
          sttLanguageCode: $config.voice?.sttLanguageCode ?? 'en-US',
          onToggleWorkspaceCapture: setWorkspaceCaptureForActiveThread,
          onShowCode: (m) => {
            void openVersionCodeModal({
              code: m.output.macroCode,
              title: m.output.title,
              messageId: m.id,
              sourceLanguage: m.artifactBundle?.sourceLanguage ?? m.modelManifest?.sourceLanguage ?? m.output.sourceLanguage ?? null,
              geometryBackend: m.artifactBundle?.geometryBackend ?? m.modelManifest?.geometryBackend ?? m.output.geometryBackend ?? null,
            });
          },
          onOpenCodeReference: (reference: ProviderCodeReference) => openVersionCodeModal({
            expectedSourcePath: reference.path,
            highlightLine: reference.line,
            throwSourceError: true,
          }),
          onDeleteVersion: deleteVersion,
          onRestoreVersion: restoreVersion,
          onAuthoredVerifyFocus: handlePromptPanelAuthoredVerifyFocus,
          onVersionChange: loadVersion,
          onOpenCapture: openCaptureRunFromHistory,
          focusRequest: dialogueFocusRequest,
        }}
      />
    {/snippet}

    {#snippet docsContent()}
      <DocsHub
        onOpenAttempt={({ code, title }) => {
          void openVersionCodeModal({
            code,
            title,
            sourceLanguage: 'ecky',
            geometryBackend: 'mesh',
          });
        }}
      />
    {/snippet}

    {#snippet terminalContent()}
      {#if visibleAgentTerminal}
        <div class="agent-terminal-window">
          <div class="agent-terminal-window__meta">
            <div class="agent-terminal-window__status">{visibleAgentTerminal.active ? 'LIVE PTY' : 'LAST SESSION'}</div>
            {#if activeAgentTerminalMetaSummary}<div class="agent-terminal-window__summary">{activeAgentTerminalMetaSummary}</div>{/if}
          </div>
          {#if projectedThreadAgentState.sessionId}
            <div class="agent-terminal-window__trace-meta">
              <span>SESSION {shortSessionId(projectedThreadAgentState.sessionId)}</span>
              <span>THREAD {activeThread?.title ?? 'UNKNOWN'}</span>
              {#if projectedThreadAgentState.providerKind}<span>PROVIDER {projectedThreadAgentState.providerKind.toUpperCase()}</span>{/if}
              {#if projectedThreadAgentState.waitingOnPrompt}<span>WAITING ON PROMPT</span>
              {:else if activeMcpBusy}<span>TURN ACTIVE</span>
              {:else if projectedThreadAgentState.phase}<span>{formatAgentPhase(projectedThreadAgentState.phase)}</span>{/if}
            </div>
          {/if}
          <div class="agent-terminal-window__hint">{visibleAgentTerminal.active ? 'CLICK TERMINAL TO TYPE DIRECTLY. ARROWS, TAB, ESC, CTRL+C AND PASTE GO STRAIGHT TO THE PTY.' : 'LAST CAPTURED TERMINAL OUTPUT'}</div>
          <div class="agent-terminal-window__screen" class:agent-terminal-window__screen--live={visibleAgentTerminal.active} aria-label={visibleAgentTerminal.agentLabel + ' terminal'}>
            <AgentTerminalSurface
              bind:this={agentTerminalSurface}
              snapshot={visibleAgentTerminal}
              visible={terminalWindowState.visible}
              onRawInput={(data) => void handleAgentTerminalRawInput(data)}
              onResize={({ cols, rows }) => void handleAgentTerminalResize(visibleAgentTerminal.agentId, cols, rows)}
            />
          </div>
          <div class="agent-terminal-window__composer">
            <input
              class="input-mono agent-terminal-window__input"
              bind:value={agentTerminalInput}
              placeholder={'Paste or send a full line to ' + visibleAgentTerminal.agentLabel + '...'}
              disabled={!visibleAgentTerminal.active}
              onkeydown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  void submitAgentTerminalInput();
                }
              }}
            />
            <button class="btn btn-xs btn-secondary" onclick={() => void submitAgentTerminalInput(true)} disabled={!visibleAgentTerminal.active} title="Send Enter">ENTER</button>
            <button class="btn btn-xs btn-primary" onclick={() => void submitAgentTerminalInput()} disabled={!visibleAgentTerminal.active || !agentTerminalInput.length}>SEND</button>
          </div>
        </div>
      {/if}
    {/snippet}
  </WorkbenchWindows>

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

  <CodeModal
      bind:code={$selectedCode}
      evidence={codeModalEvidence}
      mode={codeModalMode}
      sourceLanguage={codeModalSourceLanguage}
      macroDiffView={sessionCodeDiffView}
      title={$selectedTitle}
      draftScopeKey={codeModalDraftScopeKey}
      defaultTitle={$workingCopy.title}
      defaultVersionName={$workingCopy.versionName || 'V-manual'}
      sourceThreadId={codeModalSourceThreadId}
      sourceMessageId={codeModalSourceMessageId ?? $activeVersionId}
      sourceAuthority={codeModalSourceAuthority}
      highlightLine={codeModalHighlightLine}
      onApplyVersion={codeModalMode === 'version' || codeModalMode === 'foreign-evidence' ? applyCodeModalSource : undefined}
      onTranslateToEcky={codeModalMode === 'version' ? handleTranslateCodeToEcky : undefined}
      z={codeWindowState.z}
      hidden={!codeWindowState.visible}
      focused={codeWindowState.active}
      onclose={closeCodeModal}
  />

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
  .campaign-project-page { height: 100%; min-height: 0; padding: 14px; overflow: hidden; }
  .main-workbench { flex: 1; display: flex; flex-direction: column; min-width: 0; overflow: hidden; }
  .viewport-area { flex: 1; min-height: 100px; background: #0b0f1a; position: relative; overflow: hidden; }
  .viewer-shell {
    position: absolute;
    inset: 0;
    z-index: 5;
    transition: opacity 180ms ease, filter 180ms ease;
    overflow: hidden;
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
  .genie-layer { position: absolute; left: 10px; top: 10px; z-index: 120; pointer-events: auto; max-width: min(56vw, 380px); }
  .genie-layer.choice-active { z-index: 7000; }

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
