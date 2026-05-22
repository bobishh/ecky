import { get } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { workingCopy } from '../stores/workingCopy';
import { activeThreadIdStore as activeThreadId, activeVersionId, config, historyStore as history } from '../stores/domainState';
import { refreshHistory } from '../stores/history';
import { requestQueue } from '../stores/requestQueue';
import { session, syncSessionPhaseFromQueue } from '../stores/sessionStore';
import { paramPanelState } from '../stores/paramPanelState';
import { ensureContext, startRequestHum, stopRequestHum } from '../audio/microwave';
import { startCookingPhraseLoop, startLightReasoningPhraseLoop, stopPhraseLoop } from '../stores/phraseEngine';
import { persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import { getRenderableRuntimeBundle, inspectRuntimeBundle } from '../modelRuntime/runtimeBundle';
import { ensureSemanticManifest } from '../modelRuntime/semanticControls';
import type {
  AppConfig,
  Attachment,
  DesignOutput,
  GenerateOutput,
  IntentDecision,
  Message,
  Request,
  RuntimeAuthoringContext,
  StructuralMetrics,
  StructuralVerificationResult,
  UsageSummary,
} from '../types/domain';
import { estimateBase64Bytes, profileLog } from '../debug/profiler';
import { detectFollowUpAnswer } from './followUpGuard';
import { needsGeneratedQuestionAnswer, pendingQuestionCopy } from './questionAnswer';
import { runStructuralCheck } from './structuralVerification';
import { runVerificationRound } from './verificationLoop';
import { buildAuthoringDigest } from '../llmContextDigest';
import { resolveActiveAuthoringContext } from '../runtimeCapabilities';
import {
  classifyIntent,
  finalizeGenerationAttempt,
  formatBackendError,
  generateDesign,
  getThread,
  getModelManifest,
  getMessStlPath,
  initGenerationAttempt,
  persistStructuralVerification,
  renderModel,
  saveModelManifest,
  saveConfig,
  verifyRender,
  verifyGeneratedModel,
} from '../tauri/client';

// ---------------------------------------------------------------------------
// Constants & Helpers
// ---------------------------------------------------------------------------

const DUPLICATE_REQUEST_WINDOW_MS = 1500;
const TEXT_ONLY_NVIDIA_NIM_REASON =
  'Selected NVIDIA NIM model looks text-only. Image attachments, concept-preview reuse, and screenshot verification are unavailable.';
const MODEL_CAPABILITIES_MODULE_SPECIFIER = '../modelRuntime/modelCapabilities';
const GENERIC_ROUTING_RESPONSE_MARKERS = [
  'this looks like a geometry change request',
  'intent looks like a design change request',
  'thinking not deep enough',
  'answering the question without generating geometry',
  'treating this as a question',
  'question answered. geometry unchanged',
];

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

type ModelCapabilitySummary = {
  supportsVision: boolean;
  reason: string | null;
};

type ModelCapabilitiesModule = {
  inferModelCapabilities?: (
    provider: string,
    baseUrl: string,
    model: string,
  ) => Partial<ModelCapabilitySummary> | null | undefined;
  isVisionCapableModel?: (provider: string, baseUrl: string, model: string) => boolean;
  visionUnavailableReason?: (
    provider: string,
    baseUrl: string,
    model: string,
  ) => string | null | undefined;
};

let modelCapabilitiesModulePromise: Promise<ModelCapabilitiesModule | null> | null = null;

function pickRetryMessage(nextAttempt: number, maxAttempts: number): string {
  const phrase = REPAIR_PHRASES[Math.floor(Math.random() * REPAIR_PHRASES.length)];
  return `${phrase} Retry ${nextAttempt} of ${maxAttempts}.`;
}

function renderBackendLabel(design: DesignOutput): string {
  if (design.geometryBackend === 'build123d' || design.sourceLanguage === 'build123d') return 'build123d';
  if (design.macroDialect === 'ecky' || design.sourceLanguage === 'ecky') {
    return 'Ecky';
  }
  return 'FreeCAD';
}

export function isExplicitQuestionOnlyIntent(promptText: string): boolean {
  const prompt = `${promptText ?? ''}`.trim().toLowerCase();
  if (!prompt) return false;
  if (prompt.startsWith('/ask ')) return true;

  return [
    'answer only',
    'just answer',
    'only answer',
    'do not generate',
    "don't generate",
    'without generating',
    'no generation',
    'do not change the model',
    "don't change the model",
    'without changing the model',
    'только ответь',
    'только ответ',
    'просто ответь',
    'без генерации',
    'не генерируй',
    'не меняй модель',
    'не трогай модель',
  ].some((marker) => prompt.includes(marker));
}

export function isQuestionIntent(promptText: string): boolean {
  const prompt = `${promptText ?? ''}`.trim().toLowerCase();
  if (!prompt) return false;
  if (isExplicitQuestionOnlyIntent(prompt)) return true;
  const hasQuestionSignal = prompt.includes('?') || /\b(explain|why|how|what|which)\b/.test(prompt);
  const hasDesignAction = /\b(generate|create|make|add|remove|change|update|resize)\b/.test(prompt);
  return hasQuestionSignal && !hasDesignAction;
}

function isGenericRoutingResponse(responseText: string): boolean {
  const normalized = `${responseText ?? ''}`.trim().toLowerCase();
  if (!normalized) return true;
  return GENERIC_ROUTING_RESPONSE_MARKERS.some((marker) => normalized.includes(marker));
}

function toErrorMessage(err: unknown): string {
  return formatBackendError(err);
}

function toAssetUrl(path: string | null | undefined): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

function formatStructuralSummary(metrics: StructuralMetrics): string {
  const lines = [`Structural checks passed.`, `Parts: ${metrics.partCount}`];
  if (metrics.previewStlTriangleCount != null) lines.push(`Triangles: ${metrics.previewStlTriangleCount}`);
  if (metrics.previewStlComponentCount != null) lines.push(`Components: ${metrics.previewStlComponentCount}`);
  if (metrics.previewStlNonManifoldEdgeCount != null) lines.push(`Non-manifold edges: ${metrics.previewStlNonManifoldEdgeCount}`);
  if (metrics.previewStlOverhangTriangleCount != null) lines.push(`Overhang triangles: ${metrics.previewStlOverhangTriangleCount}`);
  if (metrics.previewStlOverhangRatio != null) lines.push(`Overhang ratio: ${metrics.previewStlOverhangRatio.toFixed(3)}`);
  if (metrics.totalVolume != null) lines.push(`Volume: ${metrics.totalVolume.toFixed(2)}mm³`);
  if (metrics.totalArea != null) lines.push(`Area: ${metrics.totalArea.toFixed(2)}mm²`);
  if (metrics.bbox) {
    const b = metrics.bbox;
    lines.push(`BBox: [${b.xMin.toFixed(1)}, ${b.yMin.toFixed(1)}, ${b.zMin.toFixed(1)}] → [${b.xMax.toFixed(1)}, ${b.yMax.toFixed(1)}, ${b.zMax.toFixed(1)}]`);
  }
  return lines.join('\n');
}

function mergeUsageSummary(
  left: UsageSummary | null | undefined,
  right: UsageSummary | null | undefined,
): UsageSummary | null {
  if (!left && !right) return null;
  if (!left) return right ?? null;
  if (!right) return left;

  return {
    inputTokens: (left.inputTokens ?? 0) + (right.inputTokens ?? 0),
    outputTokens: (left.outputTokens ?? 0) + (right.outputTokens ?? 0),
    totalTokens: (left.totalTokens ?? 0) + (right.totalTokens ?? 0),
    cachedInputTokens: (left.cachedInputTokens ?? 0) + (right.cachedInputTokens ?? 0),
    reasoningTokens: (left.reasoningTokens ?? 0) + (right.reasoningTokens ?? 0),
    estimatedCostUsd:
      typeof left.estimatedCostUsd === 'number' || typeof right.estimatedCostUsd === 'number'
        ? (left.estimatedCostUsd ?? 0) + (right.estimatedCostUsd ?? 0)
        : null,
    segments: [...(left.segments || []), ...(right.segments || [])],
  };
}

function normalizeCapabilitySummary(
  summary: Partial<ModelCapabilitySummary> | null | undefined,
): ModelCapabilitySummary | null {
  if (!summary || typeof summary.supportsVision !== 'boolean') return null;
  return {
    supportsVision: summary.supportsVision,
    reason: summary.reason ?? null,
  };
}

function isMissingModelCapabilitiesModule(error: unknown): boolean {
  const message = formatBackendError(error).toLowerCase();
  return (
    message.includes('modelcapabilities') &&
    (
      message.includes('cannot find module') ||
      message.includes('failed to fetch dynamically imported module') ||
      message.includes('failed to resolve module specifier') ||
      message.includes('error loading dynamically imported module')
    )
  );
}

async function loadModelCapabilitiesModule(): Promise<ModelCapabilitiesModule | null> {
  if (!modelCapabilitiesModulePromise) {
    modelCapabilitiesModulePromise = (async () => {
      try {
        const specifier = MODEL_CAPABILITIES_MODULE_SPECIFIER;
        return await import(/* @vite-ignore */ specifier) as ModelCapabilitiesModule;
      } catch (error) {
        if (!isMissingModelCapabilitiesModule(error)) {
          console.warn('[Orchestrator] modelCapabilities helper unavailable:', error);
        }
        return null;
      }
    })();
  }

  return modelCapabilitiesModulePromise;
}

function isNvidiaNimEndpoint(provider: string, baseUrl: string): boolean {
  if (`${provider ?? ''}`.trim().toLowerCase() !== 'openai') return false;
  const normalizedBaseUrl = `${baseUrl ?? ''}`.trim();
  if (!normalizedBaseUrl) return false;

  try {
    return new URL(normalizedBaseUrl).hostname.toLowerCase() === 'integrate.api.nvidia.com';
  } catch {
    return /integrate\.api\.nvidia\.com/i.test(normalizedBaseUrl);
  }
}

function isLikelyVisionModel(model: string): boolean {
  const normalizedModel = `${model ?? ''}`.trim().toLowerCase();
  if (!normalizedModel) return false;
  return (
    normalizedModel.includes('multimodal') ||
    normalizedModel.includes('multi-modal') ||
    /(^|[\s/_-])(vision|vl)($|[\s/_-])/.test(normalizedModel)
  );
}

function fallbackInferModelCapabilities(
  provider: string,
  baseUrl: string,
  model: string,
): ModelCapabilitySummary {
  if (!isNvidiaNimEndpoint(provider, baseUrl)) {
    return { supportsVision: true, reason: null };
  }
  if (!`${model ?? ''}`.trim()) {
    return { supportsVision: true, reason: null };
  }
  if (isLikelyVisionModel(model)) {
    return { supportsVision: true, reason: null };
  }
  return {
    supportsVision: false,
    reason: TEXT_ONLY_NVIDIA_NIM_REASON,
  };
}

function selectedEngineFromConfig(
  currentConfig: AppConfig,
): AppConfig['engines'][number] | null {
  return currentConfig.engines.find((engine) => engine.id === currentConfig.selectedEngineId) ?? null;
}

async function inferSelectedModelCapabilities(
  currentConfig: AppConfig,
): Promise<ModelCapabilitySummary> {
  const selectedEngine = selectedEngineFromConfig(currentConfig);
  if (!selectedEngine) return { supportsVision: true, reason: null };

  const { provider, baseUrl, model } = selectedEngine;
  const fallback = fallbackInferModelCapabilities(provider, baseUrl, model);
  const helper = await loadModelCapabilitiesModule();
  if (!helper) return fallback;

  const inferred =
    normalizeCapabilitySummary(helper.inferModelCapabilities?.(provider, baseUrl, model)) ??
    (
      typeof helper.isVisionCapableModel === 'function'
        ? {
            supportsVision: helper.isVisionCapableModel(provider, baseUrl, model),
            reason: helper.visionUnavailableReason?.(provider, baseUrl, model) ?? null,
          }
        : null
    );

  if (!inferred) return fallback;
  if (inferred.supportsVision) return { supportsVision: true, reason: null };
  return {
    supportsVision: false,
    reason: inferred.reason ?? fallback.reason ?? TEXT_ONLY_NVIDIA_NIM_REASON,
  };
}

function filterModelFacingAttachments(
  attachments: Attachment[],
  supportsVision: boolean,
): Attachment[] {
  if (supportsVision) return attachments;
  return attachments.filter((attachment) => attachment.type !== 'image');
}

function requestAttachmentSignature(attachment: Attachment): string {
  return [
    attachment.type || '',
    attachment.path || '',
    attachment.dataUrl || '',
    attachment.name || '',
    attachment.explanation || '',
  ].join('|');
}

function requestSignature(
  prompt: string,
  attachments: Attachment[],
  threadId: string | null,
): string {
  const normalizedPrompt = `${prompt ?? ''}`.trim();
  const attachmentSignature = attachments
    .map(requestAttachmentSignature)
    .sort()
    .join('||');
  return `${threadId ?? 'new-thread'}::${normalizedPrompt}::${attachmentSignature}`;
}

function findRecentDuplicateRequest(
  prompt: string,
  attachments: Attachment[],
  threadId: string | null,
): Request | null {
  const now = Date.now();
  const targetSignature = requestSignature(prompt, attachments, threadId);
  const queue = get(requestQueue);

  for (const id of queue.order) {
    const existing = queue.byId[id];
    if (!existing) continue;
    if (now - existing.createdAt > DUPLICATE_REQUEST_WINDOW_MS) continue;
    if (requestSignature(existing.prompt, existing.attachments, existing.threadId) !== targetSignature) {
      continue;
    }
    return existing;
  }

  return null;
}

class CancelError extends Error {
  constructor() {
    super('Request canceled');
    this.name = 'CancelError';
  }
}

// ---------------------------------------------------------------------------
// Dependencies (Injected from UI)
// ---------------------------------------------------------------------------

type ViewerRef = {
  captureScreenshot: (overlayCanvas?: HTMLCanvasElement | null) => string | null;
  captureMultiAngleScreenshots: () => string[];
};

type OpenCodeModalManual = (data: DesignOutput) => void;

type OrchestratorUiDeps = {
  viewerComponent?: ViewerRef | null;
  openCodeModalManual?: OpenCodeModalManual | null;
  getDrawingCanvas?: (() => HTMLCanvasElement | null) | null;
  clearDrawing?: (() => void) | null;
};

// ---------------------------------------------------------------------------
// Orchestration Logic
// ---------------------------------------------------------------------------

function buildLightReasoningContext(): string {
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  return buildAuthoringDigest({
    title: wc.title,
    versionName: wc.versionName,
    sourceLanguage: wc.sourceLanguage,
    uiSpec: panel.uiSpec,
    params: panel.params,
    modelManifest: get(session).modelManifest,
  });
}

function buildWorkingDesignSnapshot(): DesignOutput | null {
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  if (!wc.macroCode) return null;
  return {
    title: wc.title || 'Untitled Design',
    versionName: wc.versionName || 'Working Copy',
    response: '',
    interactionMode: 'design',
    macroCode: wc.macroCode,
    macroDialect: wc.macroDialect ?? 'legacy',
    engineKind: wc.engineKind ?? 'freecad',
    sourceLanguage: wc.sourceLanguage ?? 'legacyPython',
    geometryBackend: wc.geometryBackend ?? 'freecad',
    uiSpec: panel.uiSpec || { fields: [] },
    initialParams: panel.params || {},
    postProcessing: wc.postProcessing ?? null,
  };
}

function selectedVersionMessage(): Pick<Message, 'output' | 'artifactBundle' | 'modelManifest'> | null {
  const threadId = get(activeThreadId);
  const versionId = get(activeVersionId);
  if (!threadId || !versionId) return null;
  const thread = get(history).find((candidate) => candidate.id === threadId);
  return thread?.messages.find((message) => message.id === versionId) ?? null;
}

function currentAuthoringContext(currentConfig: AppConfig): RuntimeAuthoringContext {
  const currentSession = get(session);
  return resolveActiveAuthoringContext({
    config: currentConfig,
    activeVersionMessage: selectedVersionMessage(),
    sessionArtifactBundle: currentSession.artifactBundle,
    sessionModelManifest: currentSession.modelManifest,
  });
}

type GenerateSubmissionOptions = {
  imageDataOverride?: string | null;
  uiDeps?: OrchestratorUiDeps;
};

export async function handleGenerate(
  initialPrompt: string,
  attachments: Attachment[] = [],
  options: GenerateSubmissionOptions = {},
): Promise<string> {
  const uiDeps = options.uiDeps ?? {};
  session.setError(null);

  // Keep backend AppState config in sync with current UI config before generation.
  const currentConfig = get(config);
  await saveConfig(currentConfig);
  const modelCapabilities = await inferSelectedModelCapabilities(currentConfig);

  // Capture screenshot with drawing overlay synchronously before clearing
  let preCapture: string | null = modelCapabilities.supportsVision
    ? options.imageDataOverride ?? null
    : null;
  if (!preCapture && modelCapabilities.supportsVision && uiDeps.viewerComponent && get(session).stlUrl) {
    const overlay = uiDeps.getDrawingCanvas?.() ?? null;
    preCapture = uiDeps.viewerComponent.captureScreenshot(overlay);
  }
  // Clear drawing immediately so the user sees it disappear on send
  uiDeps.clearDrawing?.();

  const currentThreadId = get(activeThreadId);
  const currentVersionId = get(activeVersionId);
  const currentModelId = get(session).artifactBundle?.modelId ?? null;
  const duplicateRequest = findRecentDuplicateRequest(initialPrompt, attachments, currentThreadId);
  if (duplicateRequest) {
    requestQueue.setActive(duplicateRequest.id);
    session.setStatus('Request already in flight.');
    return duplicateRequest.id;
  }
  const requestId = requestQueue.submit(
    initialPrompt,
    attachments,
    currentThreadId,
    currentVersionId,
    currentModelId,
  );
  requestQueue.setActive(requestId);

  if (preCapture) {
    requestQueue.patch(requestId, { screenshot: preCapture });
  }

  profileLog('generate.submit', {
    requestId,
    threadId: currentThreadId,
    promptChars: initialPrompt.length,
    attachments: attachments.length,
    screenshotMb: Number((estimateBase64Bytes(preCapture) / (1024 * 1024)).toFixed(2)),
    supportsVision: modelCapabilities.supportsVision,
  });
  if (!modelCapabilities.supportsVision) {
    console.info('[Orchestrator] vision inputs suppressed for selected model', {
      requestId,
      reason: modelCapabilities.reason,
    });
  }

  ensureContext();

  const pipeline = new GenerationPipeline(requestId, uiDeps);
  pipeline.preCapture = preCapture;
  pipeline.modelCapabilities = modelCapabilities;
  pipeline.modelFacingAttachments = filterModelFacingAttachments(
    attachments,
    modelCapabilities.supportsVision,
  );
  pipeline.execute().catch(err => {
    console.error("[Orchestrator] Pipeline hard failure:", err);
  });

  return requestId;
}

/**
 * Encapsulates the entire generation lifecycle for a single request,
 * providing strict isolation, cancellation checks, and immutable persistence.
 */
class GenerationPipeline {
  requestId: string;
  req: Request;
  snapshotThreadId: string;
  snapshotParentMacroCode: string | null;
  snapshotWorkingDesign: DesignOutput | null;
  snapshotAuthoringContext: RuntimeAuthoringContext;
  currentConfig: AppConfig;
  
  assistantMessageId: string | null = null;
  currentScreenshot: string | null = null;
  preCapture: string | null = null;
  modelCapabilities: ModelCapabilitySummary = { supportsVision: true, reason: null };
  modelFacingAttachments: Attachment[] = [];
  isQuestion: boolean = false;
  forcedQuestionOnly: boolean = false;
  lightResponse: string = '';
  finalResponse: string = '';
  usageSummary: UsageSummary | null = null;
  routeReason: string = 'unclassified';
  followUpQuestion: string | null = null;
  followUpMessageId: string | null = null;
  uiDeps: OrchestratorUiDeps;

  constructor(requestId: string, uiDeps: OrchestratorUiDeps = {}) {
    this.requestId = requestId;
    this.uiDeps = uiDeps;
    const q = get(requestQueue);
    this.req = q.byId[requestId];
    
    // Ensure thread ID exists immediately
    this.snapshotThreadId = this.req.threadId || crypto.randomUUID();
    if (!this.req.threadId) {
      requestQueue.patch(requestId, { threadId: this.snapshotThreadId });
      // If we are in a 'New Session' state (no active thread), claim this new thread as active
      if (get(activeThreadId) === null) {
        activeThreadId.set(this.snapshotThreadId);
      }
    }

    this.snapshotParentMacroCode = get(workingCopy).macroCode || null;
    this.snapshotWorkingDesign = buildWorkingDesignSnapshot();
    this.currentConfig = get(config);
    this.snapshotAuthoringContext = currentAuthoringContext(this.currentConfig);
    this.modelFacingAttachments = this.req.attachments;
  }

  // --- Main Execution ---

  async execute() {
    try {
      await this.stepClassify();
      
      if (this.isQuestion) {
        await this.stepAnswerQuestion();
      } else {
        await this.stepGenerateAndRender();
      }
    } catch (err) {
      if (err instanceof CancelError) {
        await this.finalizeAttempt('discarded');
        this.stopMicrowave(false);
        return;
      }
      await this.handleGlobalError(err);
    } finally {
      stopPhraseLoop();
    }
  }

  // --- Discrete Steps ---

  private async stepClassify() {
    this.checkCanceled();
    requestQueue.patch(this.requestId, { phase: 'classifying' });
    syncSessionPhaseFromQueue();
    startLightReasoningPhraseLoop();

    this.forcedQuestionOnly = isExplicitQuestionOnlyIntent(this.req.prompt);
    this.isQuestion = this.forcedQuestionOnly || isQuestionIntent(this.req.prompt);
    this.routeReason = this.forcedQuestionOnly
      ? 'explicit-question-only marker'
      : this.isQuestion
        ? 'local question heuristic'
        : 'local design heuristic';

    // Use pre-captured screenshot (with drawing overlay composited) from handleGenerate
    if (this.modelCapabilities.supportsVision) {
      if (this.preCapture) {
        this.currentScreenshot = this.preCapture;
      } else if (this.uiDeps.viewerComponent && get(session).stlUrl) {
        this.currentScreenshot = this.uiDeps.viewerComponent.captureScreenshot();
      }
    } else {
      this.currentScreenshot = null;
    }
    if (this.currentScreenshot) {
      requestQueue.patch(this.requestId, { screenshot: this.currentScreenshot });
    }
    profileLog('generate.classify_image', {
      requestId: this.requestId,
      threadId: this.snapshotThreadId,
      screenshotMb: Number((estimateBase64Bytes(this.currentScreenshot) / (1024 * 1024)).toFixed(2)),
      supportsVision: this.modelCapabilities.supportsVision,
      screenshotSuppressedReason: this.modelCapabilities.supportsVision ? null : this.modelCapabilities.reason,
    });

    const followUpMatched = await this.applyFollowUpAnswerGuard();
    await this.initDatabaseRecord();
    if (!followUpMatched) {
      await this.classifyIntent();
    }
    console.info('[Pipeline] route decision', {
      requestId: this.requestId,
      threadId: this.snapshotThreadId,
      finalMode: this.isQuestion ? 'question' : 'design',
      reason: this.routeReason,
      classifierPreview: this.lightResponse,
      finalResponse: this.finalResponse,
    });
    profileLog('generate.route', {
      requestId: this.requestId,
      threadId: this.snapshotThreadId,
      finalMode: this.isQuestion ? 'question' : 'design',
      reason: this.routeReason,
      classifierPreview: this.lightResponse,
      finalResponse: this.finalResponse,
    });
  }

  private async stepAnswerQuestion() {
    const pendingCopy = pendingQuestionCopy(this.finalResponse);
    this.updateStatus(pendingCopy);
    requestQueue.patch(this.requestId, { phase: 'answering', lightResponse: pendingCopy });
    syncSessionPhaseFromQueue();

    let questionReplyText = this.finalResponse.trim();
    if (needsGeneratedQuestionAnswer(questionReplyText)) {
      const authoringContext = this.snapshotWorkingDesign ?? this.snapshotAuthoringContext;
      try {
        const result = await generateDesign({
          prompt: this.req.prompt,
          threadId: this.snapshotThreadId,
          parentMacroCode: this.snapshotParentMacroCode,
          workingDesign: this.snapshotWorkingDesign,
          isRetry: false,
          imageData: this.currentScreenshot,
          attachments: this.modelFacingAttachments,
          questionMode: true,
          followUpQuestion: null,
          engineKind: authoringContext.engineKind ?? null,
          sourceLanguage: authoringContext.sourceLanguage,
          geometryBackend: authoringContext.geometryBackend,
        });
        this.checkCanceled();
        this.usageSummary = mergeUsageSummary(this.usageSummary, result.usage);
        questionReplyText =
          result.design.response?.trim() ||
          'Question answered. Geometry unchanged.';
      } catch (e) {
        this.checkCanceled();
        await this.handleGenerationFailure(toErrorMessage(e));
        return;
      }
    }

    // Finalize the existing attempt with the answer
    await this.finalizeAttempt('success', undefined, undefined, questionReplyText);

    if (this.isActiveThread()) {
      session.setStatus(questionReplyText);
    }
    
    await refreshHistory();
    this.checkCanceled();

    requestQueue.patch(this.requestId, {
      phase: 'success',
      lightResponse: questionReplyText,
      result: {
        design: null,
        threadId: this.snapshotThreadId,
        messageId: this.assistantMessageId || '',
        stlUrl: '',
        artifactBundle: null,
        modelManifest: null,
      },
    });
    syncSessionPhaseFromQueue();
  }

  private async stepGenerateAndRender() {
    this.checkCanceled();
    console.info('[Pipeline] starting generate path', {
      requestId: this.requestId,
      threadId: this.snapshotThreadId,
      reason: this.routeReason,
      prompt: this.req.prompt,
    });
    stopPhraseLoop();
    startCookingPhraseLoop();
    requestQueue.patch(this.requestId, { cookingStartTime: Date.now() });
    startRequestHum(this.requestId, this.currentConfig, this.snapshotThreadId);

    let attempt = 1;
    let currentPrompt = this.req.prompt;
    // Screenshot/VLM verification attempts (0 = disabled).
    // Structural verification always runs regardless.
    const maxVerifyAttempts = this.req.maxVerifyAttempts;

    while (attempt <= this.req.maxAttempts) {
      this.checkCanceled();

      if (attempt === 1 && this.isActiveThread()) {
        session.setStlUrl(null);
      }

      requestQueue.patch(this.requestId, { phase: attempt > 1 ? 'repairing' : 'generating', attempt });
      syncSessionPhaseFromQueue();
      this.updateStatus(`Consulting LLM (Attempt ${attempt}/${this.req.maxAttempts})...`);

      try {
        const authoringContext = this.snapshotWorkingDesign ?? this.snapshotAuthoringContext;
        const result = await generateDesign({
          prompt: currentPrompt,
          threadId: this.snapshotThreadId,
          parentMacroCode: this.snapshotParentMacroCode,
          workingDesign: this.snapshotWorkingDesign,
          isRetry: attempt > 1,
          imageData: this.currentScreenshot,
          attachments: this.modelFacingAttachments,
          questionMode: false,
          followUpQuestion: attempt === 1 ? this.followUpQuestion : null,
          engineKind: authoringContext.engineKind ?? null,
          sourceLanguage: authoringContext.sourceLanguage,
          geometryBackend: authoringContext.geometryBackend,
        });
        this.checkCanceled();
        this.usageSummary = mergeUsageSummary(this.usageSummary, result.usage);

        const data = result.design;
        const interactionMode = `${data.interactionMode ?? ''}`.toLowerCase();

        if (interactionMode === 'question') {
          await this.handleFallbackQuestion(data, currentPrompt);
          return;
        }

        // --- Render Step ---
        requestQueue.patch(this.requestId, { phase: 'rendering' });
        syncSessionPhaseFromQueue();
        const backendLabel = renderBackendLabel(data);
        this.updateStatus(`Executing ${backendLabel} engine...`);

        try {
          const bundle = await renderModel(
            data.macroCode,
            data.initialParams || {},
            data.macroDialect ?? null,
            data.geometryBackend ?? null,
            data.postProcessing ?? null,
            get(activeThreadId) === this.snapshotThreadId ? get(session).modelManifest : null,
          );
          const rawManifest = await getModelManifest(bundle.modelId);
          const previousManifest =
            get(activeThreadId) === this.snapshotThreadId ? get(session).modelManifest : null;
          const manifest =
            ensureSemanticManifest(
              rawManifest,
              data.uiSpec,
              data.initialParams || {},
              previousManifest,
            ) ?? rawManifest;
          if (JSON.stringify(manifest) !== JSON.stringify(rawManifest)) {
            await saveModelManifest(bundle.modelId, manifest);
          }
          this.checkCanceled();

          // Collect reference image paths from attachments for verification
          const referenceImagePaths = (this.modelFacingAttachments ?? [])
            .filter((a) => a.type === 'image')
            .map((a) => a.path);
          const visionVerificationSkipReason = this.modelCapabilities.supportsVision
            ? null
            : this.modelCapabilities.reason;

          // ── Stage 1: Structural verification (always runs) ──────────────
          let structuralSummary: string | null = null;
          let structuralMetrics: StructuralMetrics | null = null;
          let structuralVerification: StructuralVerificationResult | null = null;
          {
            this.updateStatus('Structural verification...');
            const structResult = await runStructuralCheck({
              modelId: bundle.modelId,
              originalPrompt: this.req.prompt,
              currentGenerationAttempt: attempt,
              maxGenerationAttempts: this.req.maxAttempts,
              verify: (modelId, prompt) => verifyGeneratedModel(modelId, prompt),
            });

            console.info('[Pipeline] structural verify:', structResult.kind);
            structuralVerification =
              structResult.kind === 'structural_skipped' ? null : structResult.verification;

            if (structResult.kind === 'repair_needed') {
              currentPrompt = structResult.repairPrompt;
              attempt++;
              if (attempt <= this.req.maxAttempts) {
                const firstIssue = structResult.repairPrompt.split('\n').find((l: string) => l.startsWith('- [')) ?? structResult.repairPrompt.split('\n')[0];
                if (this.isActiveThread()) session.setRepairMessage(`Structural: ${firstIssue}`);
                this.checkCanceled();
                continue;
              }
              // attempt cap hit — fall through to commit best-effort
            } else if (structResult.kind === 'failed_terminal') {
              console.warn('[Pipeline] structural terminal failure:', structResult.issues);
              await this.handleVerificationFailure(data, `Structural verification failed:\n${structResult.issues}`);
              return;
            } else if (structResult.kind === 'structural_passed') {
              structuralMetrics = structResult.metrics;
              structuralSummary = formatStructuralSummary(structResult.metrics);
            }
            // structural_passed or structural_skipped → proceed
            this.checkCanceled();
          }

          // ── Stage 2: Screenshot/VLM verification (gated by config) ───────
          if (maxVerifyAttempts > 0 && (this.uiDeps.viewerComponent || visionVerificationSkipReason)) {
            if (this.uiDeps.viewerComponent && !visionVerificationSkipReason) {
              // Give Three.js one frame to render the new STL before capturing
              await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
            }

            let verifyAttempt = 0;
            while (verifyAttempt < maxVerifyAttempts) {
              this.checkCanceled();
              verifyAttempt++;

              requestQueue.patch(this.requestId, { phase: 'rendering' });
              this.updateStatus(
                visionVerificationSkipReason
                  ? 'Vision verify skipped for selected model.'
                  : `Vision verify (${verifyAttempt}/${maxVerifyAttempts})...`,
              );

              const vResult = await runVerificationRound(verifyAttempt, {
                originalPrompt: this.req.prompt,
                maxVerifyAttempts,
                currentGenerationAttempt: attempt,
                maxGenerationAttempts: this.req.maxAttempts,
                skipReason: visionVerificationSkipReason,
                capture: () => this.uiDeps.viewerComponent?.captureMultiAngleScreenshots() ?? [],
                verify: (prompt, screenshots, refImages, structSummary) =>
                  verifyRender(prompt, screenshots, refImages, structSummary),
                referenceImages: referenceImagePaths,
                structuralSummary,
                structuralMetrics,
              });

              if (vResult.kind === 'skipped') {
                console.info('[Pipeline] vision verify skipped:', vResult.reason);
              } else {
                console.info('[Pipeline] vision verify:', vResult.kind);
              }

              if (vResult.kind === 'passed' || vResult.kind === 'skipped') break;

              if (vResult.kind === 'repair_needed') {
                currentPrompt = vResult.repairPrompt;
                attempt++;
                if (attempt > this.req.maxAttempts) break;
                if (this.isActiveThread()) session.setRepairMessage(`Vision: ${vResult.repairPrompt.split('\n')[0]}`);
                break; // break verify loop, continue generate loop
              }

              // failed_terminal → stop; generated geometry is known bad.
              if (vResult.kind === 'failed_terminal') {
                console.warn('[Pipeline] vision terminal failure:', vResult.issues);
                await this.handleVerificationFailure(data, `Vision verification failed:\n${vResult.issues}`);
                return;
              }
              break;
            }
            this.checkCanceled();
          }
          // ── End verification ──────────────────────────────────────────────

          await this.commitSuccess(data, bundle, manifest, structuralVerification);
          return;

        } catch (renderError) {
          this.checkCanceled();
          if (attempt < this.req.maxAttempts) {
            const repairMsg = pickRetryMessage(attempt + 1, this.req.maxAttempts);
            if (this.isActiveThread()) session.setRepairMessage(repairMsg);
            const renderErrorText = toErrorMessage(renderError);
            currentPrompt = `The previous code failed in ${backendLabel} with this error:\n${renderErrorText}\n\nPlease fix it.`;
            attempt++;
            continue;
          } else {
            await this.handleRenderFailure(data, toErrorMessage(renderError));
            return;
          }
        }
      } catch (e) {
        this.checkCanceled();
        await this.handleGenerationFailure(toErrorMessage(e));
        return;
      }
    }
  }

  // --- Logic Sub-methods ---

  private async initDatabaseRecord() {
    this.checkCanceled();
    this.assistantMessageId = await initGenerationAttempt({
      threadId: this.snapshotThreadId,
      prompt: this.req.prompt,
      attachments: this.req.attachments,
      imageData: this.currentScreenshot
    });
    
    requestQueue.patch(this.requestId, { 
      result: {
        design: null,
        threadId: this.snapshotThreadId,
        messageId: this.assistantMessageId,
        stlUrl: '',
        artifactBundle: null,
        modelManifest: null,
      } 
    });
    await refreshHistory();
  }

  private async classifyIntent() {
    this.checkCanceled();
    try {
      const intent = await classifyIntent({
        prompt: this.req.prompt,
        threadId: this.snapshotThreadId,
        context: buildLightReasoningContext(),
        imageData: this.currentScreenshot,
        attachments: this.modelFacingAttachments
      });
      this.checkCanceled();
      this.usageSummary = mergeUsageSummary(this.usageSummary, intent.usage);

      const heuristicQuestion = isQuestionIntent(this.req.prompt);
      const classifierIntent = `${intent?.intentMode ?? ''}`.trim().toLowerCase();
      const classifierResponse = `${intent?.response ?? ''}`.trim();
      const classifierFinalResponse = `${intent?.finalResponse ?? ''}`.trim();
      const classifierConfidence = Number.isFinite(intent?.confidence) ? intent.confidence : 0;
      this.finalResponse = classifierFinalResponse;

      if (this.forcedQuestionOnly) {
        this.isQuestion = true;
        this.routeReason = 'explicit-question-only marker';
      } else if (classifierFinalResponse) {
        this.isQuestion = true;
        this.routeReason = `classifier provided final_response (${classifierConfidence.toFixed(2)})`;
      } else if (classifierIntent === 'question') {
        this.isQuestion = true;
        this.routeReason = `classifier chose question (${classifierConfidence.toFixed(2)})`;
      } else if (classifierIntent === 'design') {
        this.isQuestion = false;
        this.routeReason = `classifier chose design (${classifierConfidence.toFixed(2)})`;
      } else {
        this.routeReason = heuristicQuestion
          ? 'classifier fallback kept question heuristic'
          : 'classifier fallback kept design heuristic';
      }

      const bubbleCandidate = this.finalResponse || classifierResponse;
      if (bubbleCandidate) {
        this.lightResponse = classifierResponse;
        const bubbleText =
          this.finalResponse || this.isQuestion || !isGenericRoutingResponse(classifierResponse)
            ? bubbleCandidate
            : 'Routing request...';
        if (this.isActiveThread()) session.setCookingPhrase(bubbleText);
      }
      requestQueue.patch(this.requestId, { isQuestion: this.isQuestion, lightResponse: this.lightResponse });
    } catch (e) {
      console.warn(`[Pipeline:${this.requestId}] Intent classification fallback:`, e);
      this.routeReason = this.isQuestion
        ? 'classifier failed; kept local question heuristic'
        : 'classifier failed; kept local design heuristic';
    }
  }

  private async resolveThreadForFollowUpGuard(): Promise<{
    thread: import('../types/domain').Thread | null;
    source: 'none' | 'cached' | 'fetched' | 'fetch-failed';
  }> {
    if (!this.req.threadId) {
      return { thread: null, source: 'none' };
    }

    const cachedThread = get(history).find((thread) => thread.id === this.snapshotThreadId) ?? null;
    const hasCachedMessages = Boolean(cachedThread?.messages?.length);

    try {
      const fullThread = await getThread(this.snapshotThreadId);
      history.update((items) => {
        const hasExisting = items.some((thread) => thread.id === this.snapshotThreadId);
        if (!hasExisting) {
          return [...items, fullThread];
        }
        return items.map((thread) =>
          thread.id === this.snapshotThreadId ? { ...thread, messages: fullThread.messages } : thread,
        );
      });
      return { thread: fullThread, source: 'fetched' };
    } catch (error) {
      console.warn('[Pipeline] follow-up guard failed to load full thread', {
        requestId: this.requestId,
        threadId: this.snapshotThreadId,
        error: toErrorMessage(error),
      });
      if (hasCachedMessages) {
        return { thread: cachedThread, source: 'cached' };
      }
      return { thread: cachedThread, source: 'fetch-failed' };
    }
  }

  private async applyFollowUpAnswerGuard(): Promise<boolean> {
    const { thread: activeThread, source } = await this.resolveThreadForFollowUpGuard();
    const guard = detectFollowUpAnswer({
      promptText: this.req.prompt,
      attachments: this.req.attachments,
      activeThread,
      explicitQuestionOnly: this.forcedQuestionOnly,
    });

    const details = {
      requestId: this.requestId,
      threadId: this.snapshotThreadId,
      evaluated: true,
      matched: guard.matched,
      matchedMessageId: guard.messageId,
      reason: guard.reason,
      threadSource: source,
      classifySkipped: guard.matched,
    };
    console.info('[Pipeline] follow-up guard', details);
    profileLog('generate.followup_guard', details);

    if (!guard.matched || !guard.question) {
      return false;
    }

    this.followUpQuestion = guard.question;
    this.followUpMessageId = guard.messageId;
    this.isQuestion = false;
    this.routeReason = 'follow-up answer to last assistant question';
    this.lightResponse = 'Using your answer to continue the design.';
    this.finalResponse = '';
    if (this.isActiveThread()) {
      session.setCookingPhrase(this.lightResponse);
    }
    requestQueue.patch(this.requestId, {
      isQuestion: false,
      lightResponse: this.lightResponse,
    });
    return true;
  }

  private async commitSuccess(
    data: DesignOutput,
    bundle: import('../types/domain').ArtifactBundle,
    manifest: import('../types/domain').ModelManifest,
    structuralVerification: StructuralVerificationResult | null,
  ) {
    const runtime = await inspectRuntimeBundle(
      bundle,
      undefined,
      undefined,
      data.postProcessing ?? null,
      data.initialParams ?? {},
    );
    const renderableBundle =
      runtime.bundle ??
      getRenderableRuntimeBundle(bundle, data.postProcessing ?? null, data.initialParams ?? {}) ??
      bundle;
    const stlUrlValue = toAssetUrl(renderableBundle.previewStlPath);
    requestQueue.patch(this.requestId, { phase: 'committing' });
    syncSessionPhaseFromQueue();

    await this.finalizeAttempt('success', data, undefined, undefined, bundle, manifest);
    this.checkCanceled();
    if (this.assistantMessageId && structuralVerification) {
      await persistStructuralVerification(this.assistantMessageId, structuralVerification);
      this.checkCanceled();
    }

    if (this.isActiveThread()) {
      activeThreadId.set(this.snapshotThreadId);
      activeVersionId.set(this.assistantMessageId);
      const currentQ = get(requestQueue);
      if (currentQ.activeId === this.requestId) {
        workingCopy.loadVersion(data, this.assistantMessageId);
        paramPanelState.hydrateFromVersion(data, this.assistantMessageId);
        session.setStlUrl(stlUrlValue);
        session.setModelRuntime(renderableBundle, manifest);
      }
      session.setStatus(
        runtime.skippedOversizedPreview
          ? 'Design synthesized successfully. Lithophane preview was skipped in the viewer; base part meshes are shown instead.'
          : 'Design synthesized successfully.',
      );
    }
    requestQueue.patch(this.requestId, { threadId: this.snapshotThreadId });

    if (this.isActiveThread()) {
      await persistLastSessionSnapshot({
        design: data,
        threadId: this.snapshotThreadId,
        messageId: this.assistantMessageId,
        artifactBundle: renderableBundle,
        modelManifest: manifest,
        selectedPartId: null,
      });
    }

    await refreshHistory();
    requestQueue.patch(this.requestId, {
      phase: 'success',
      lightResponse: data.response?.trim() || 'Design synthesized successfully.',
      result: {
        design: data,
        threadId: this.snapshotThreadId,
        messageId: this.assistantMessageId!,
        stlUrl: stlUrlValue,
        artifactBundle: renderableBundle,
        modelManifest: manifest,
        structuralVerification,
      }
    });
    this.stopMicrowave(true);
    syncSessionPhaseFromQueue();
  }

  private async handleFallbackQuestion(data: DesignOutput, currentPrompt: string) {
    void currentPrompt;
    const responseText = data.response || 'Question answered.';
    await this.finalizeAttempt('success', undefined, undefined, responseText);

    if (this.isActiveThread()) {
      session.setStatus(responseText);
    }
    if (this.isActiveThread()) {
      await persistLastSessionSnapshot({
        design: data,
        threadId: this.snapshotThreadId,
        messageId: this.assistantMessageId,
        artifactBundle: null,
        modelManifest: null,
        selectedPartId: null,
      });
    }
    await refreshHistory();
    requestQueue.patch(this.requestId, {
      phase: 'success',
      lightResponse: responseText,
      result: {
        design: data,
        threadId: this.snapshotThreadId,
        messageId: this.assistantMessageId || '',
        stlUrl: '',
        artifactBundle: null,
        modelManifest: null,
      },
    });
    this.stopMicrowave(true);
    syncSessionPhaseFromQueue();
  }

  // --- Utility & Error Handlers ---

  private checkCanceled() {
    const r = get(requestQueue).byId[this.requestId];
    if (!r || r.phase === 'canceled') {
      throw new CancelError();
    }
  }

  private isActiveThread() {
    return get(activeThreadId) === this.snapshotThreadId;
  }

  private updateStatus(msg: string) {
    if (this.isActiveThread()) session.setStatus(msg);
  }

  private updateError(err: string) {
    if (this.isActiveThread()) session.setError(err);
  }

  private stopMicrowave(success: boolean) {
    const currentQ = get(requestQueue);
    const activeIds = currentQ.order.filter(id => {
      const r = currentQ.byId[id];
      return r && !['success', 'error', 'canceled'].includes(r.phase);
    });
    const slot = activeIds.indexOf(this.requestId);
    stopRequestHum(this.requestId, success, this.currentConfig, Math.max(0, slot));
  }

  private async finalizeAttempt(
    status: 'success' | 'error' | 'discarded',
    design?: DesignOutput,
    errorMessage?: string,
    responseText?: string,
    artifactBundle?: import('../types/domain').ArtifactBundle | null,
    modelManifest?: import('../types/domain').ModelManifest | null,
  ) {
    if (!this.assistantMessageId) return;
    try {
      await finalizeGenerationAttempt({
        messageId: this.assistantMessageId,
        status,
        design,
        usage: this.usageSummary,
        artifactBundle,
        modelManifest,
        errorMessage,
        responseText
      });
    } catch (e) {
      console.error("[Pipeline] Failed to finalize attempt:", e);
    }
  }

  private async handleGlobalError(err: unknown) {
    const errText = toErrorMessage(err);
    this.updateError(`Pipeline Error: ${errText}`);
    if (this.isActiveThread()) {
      try {
        const messPath = await getMessStlPath();
        session.setStlUrl(toAssetUrl(messPath));
        session.clearModelRuntime();
      } catch (e) {
        session.setStlUrl(null);
      }
    }
    requestQueue.patch(this.requestId, { phase: 'error', error: errText });
    await this.finalizeAttempt('error', undefined, errText);
    this.stopMicrowave(false);
    syncSessionPhaseFromQueue();
  }

  private async handleRenderFailure(data: DesignOutput, renderError: string) {
    this.updateError(`Render Error: ${renderError}`);
    if (this.isActiveThread()) {
      try {
        const messPath = await getMessStlPath();
        session.setStlUrl(toAssetUrl(messPath));
        session.clearModelRuntime();
      } catch (e) {
        session.setStlUrl(null);
      }
    }

    await this.finalizeAttempt('error', data, `Render Error: ${renderError}`);
    this.uiDeps.openCodeModalManual?.(data);
    requestQueue.patch(this.requestId, { phase: 'error', error: `Render Error: ${renderError}` });
    this.stopMicrowave(false);
    syncSessionPhaseFromQueue();
  }

  private async handleVerificationFailure(data: DesignOutput, verificationError: string) {
    this.updateError(verificationError);
    if (this.isActiveThread()) {
      try {
        const messPath = await getMessStlPath();
        session.setStlUrl(toAssetUrl(messPath));
        session.clearModelRuntime();
      } catch (e) {
        session.setStlUrl(null);
      }
    }

    await this.finalizeAttempt('error', data, verificationError);
    this.uiDeps.openCodeModalManual?.(data);
    requestQueue.patch(this.requestId, { phase: 'error', error: verificationError });
    this.stopMicrowave(false);
    syncSessionPhaseFromQueue();
  }

  private async handleGenerationFailure(e: string) {
    this.updateError(`Generation Failed: ${e}`);
    if (this.isActiveThread()) {
      try {
        const messPath = await getMessStlPath();
        session.setStlUrl(toAssetUrl(messPath));
        session.clearModelRuntime();
      } catch (err) {
        session.setStlUrl(null);
      }
    }

    await this.finalizeAttempt('error', undefined, `Generation Failed: ${e}`);
    requestQueue.patch(this.requestId, { phase: 'error', error: `Generation Failed: ${e}` });
    this.stopMicrowave(false);
    syncSessionPhaseFromQueue();
  }
}
