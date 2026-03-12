import { get } from 'svelte/store';
import { convertFileSrc } from '@tauri-apps/api/core';
import { workingCopy } from '../stores/workingCopy';
import { activeThreadId, activeVersionId, config, history } from '../stores/domainState';
import { refreshHistory } from '../stores/history';
import { requestQueue } from '../stores/requestQueue';
import { session, syncSessionPhaseFromQueue } from '../stores/sessionStore';
import { paramPanelState } from '../stores/paramPanelState';
import { ensureContext, startRequestHum, stopRequestHum } from '../audio/microwave';
import { startCookingPhraseLoop, startLightReasoningPhraseLoop, stopPhraseLoop } from '../stores/phraseEngine';
import { persistLastSessionSnapshot } from '../modelRuntime/sessionSnapshot';
import { ensureSemanticManifest } from '../modelRuntime/semanticControls';
import type {
  AppConfig,
  Attachment,
  DesignOutput,
  GenerateOutput,
  IntentDecision,
  Request,
  UsageSummary,
} from '../types/domain';
import { estimateBase64Bytes, profileLog } from '../debug/profiler';
import { detectFollowUpAnswer } from './followUpGuard';
import {
  classifyIntent,
  finalizeGenerationAttempt,
  formatBackendError,
  generateDesign,
  getThread,
  getModelManifest,
  getMessStlPath,
  initGenerationAttempt,
  renderModel,
  saveModelManifest,
  saveConfig,
} from '../tauri/client';

// ---------------------------------------------------------------------------
// Constants & Helpers
// ---------------------------------------------------------------------------

const DUPLICATE_REQUEST_WINDOW_MS = 1500;
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

function pickRetryMessage(nextAttempt: number, maxAttempts: number): string {
  const phrase = REPAIR_PHRASES[Math.floor(Math.random() * REPAIR_PHRASES.length)];
  return `${phrase} Retry ${nextAttempt} of ${maxAttempts}.`;
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

function requestAttachmentSignature(attachment: Attachment): string {
  return [
    attachment.type || '',
    attachment.path || '',
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
};

type OpenCodeModalManual = (data: DesignOutput) => void;

let viewerRef: ViewerRef | null = null;
let openCodeModalManual: OpenCodeModalManual | null = null;
let getDrawingCanvas: (() => HTMLCanvasElement | null) | null = null;
let clearDrawing: (() => void) | null = null;

export function initOrchestrator(deps: {
  viewerComponent: ViewerRef | null;
  openCodeModalManual: OpenCodeModalManual;
  getDrawingCanvas?: () => HTMLCanvasElement | null;
  clearDrawing?: () => void;
}) {
  viewerRef = deps.viewerComponent;
  openCodeModalManual = deps.openCodeModalManual;
  getDrawingCanvas = deps.getDrawingCanvas || null;
  clearDrawing = deps.clearDrawing || null;
}

// ---------------------------------------------------------------------------
// Orchestration Logic
// ---------------------------------------------------------------------------

function buildLightReasoningContext(): string {
  const context: string[] = [];
  const wc = get(workingCopy);
  const panel = get(paramPanelState);
  if (wc.title) context.push(`Title: ${wc.title}`);
  if (wc.versionName) context.push(`Version: ${wc.versionName}`);
  if (wc.macroCode) {
    context.push(
      `ACTUAL CURRENT FREECAD MACRO (AUTHORITATIVE, NOT A SAMPLE):\n\`\`\`python\n${wc.macroCode}\n\`\`\``
    );
  } else {
    context.push('ACTUAL CURRENT FREECAD MACRO: [none in working copy]');
  }
  if (panel.uiSpec) {
    context.push(
      `ACTUAL CURRENT UI SPEC (AUTHORITATIVE):\n\`\`\`json\n${JSON.stringify(panel.uiSpec, null, 2)}\n\`\`\``
    );
  }
  if (panel.params && Object.keys(panel.params).length > 0) {
    context.push(
      `ACTUAL CURRENT PARAMETERS (AUTHORITATIVE):\n\`\`\`json\n${JSON.stringify(panel.params, null, 2)}\n\`\`\``
    );
  }
  return context.join('\n\n');
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
    uiSpec: panel.uiSpec || { fields: [] },
    initialParams: panel.params || {}
  };
}

export async function handleGenerate(initialPrompt: string, attachments: Attachment[] = []): Promise<string> {
  session.setError(null);

  // Keep backend AppState config in sync with current UI config before generation.
  await saveConfig(get(config));

  // Capture screenshot with drawing overlay synchronously before clearing
  let preCapture: string | null = null;
  if (viewerRef && get(session).stlUrl) {
    const overlay = getDrawingCanvas?.() ?? null;
    preCapture = viewerRef.captureScreenshot(overlay);
  }
  // Clear drawing immediately so the user sees it disappear on send
  clearDrawing?.();

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
  });

  ensureContext();

  const pipeline = new GenerationPipeline(requestId);
  pipeline.preCapture = preCapture;
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
  currentConfig: AppConfig;
  
  assistantMessageId: string | null = null;
  currentScreenshot: string | null = null;
  preCapture: string | null = null;
  isQuestion: boolean = false;
  forcedQuestionOnly: boolean = false;
  lightResponse: string = '';
  finalResponse: string = '';
  usageSummary: UsageSummary | null = null;
  routeReason: string = 'unclassified';
  followUpQuestion: string | null = null;
  followUpMessageId: string | null = null;

  constructor(requestId: string) {
    this.requestId = requestId;
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
    if (this.preCapture) {
      this.currentScreenshot = this.preCapture;
    } else if (viewerRef && get(session).stlUrl) {
      this.currentScreenshot = viewerRef.captureScreenshot();
    }
    if (this.currentScreenshot) {
      requestQueue.patch(this.requestId, { screenshot: this.currentScreenshot });
    }
    profileLog('generate.classify_image', {
      requestId: this.requestId,
      threadId: this.snapshotThreadId,
      screenshotMb: Number((estimateBase64Bytes(this.currentScreenshot) / (1024 * 1024)).toFixed(2)),
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
    this.updateStatus('Answering question...');
    requestQueue.patch(this.requestId, { phase: 'answering' });
    syncSessionPhaseFromQueue();
    
    const questionReplyText =
      this.finalResponse ||
      this.lightResponse ||
      'Question answered. Geometry unchanged.';

    // Finalize the existing attempt with the answer
    await this.finalizeAttempt('success', undefined, undefined, questionReplyText);

    if (this.isActiveThread()) {
      session.setStatus(questionReplyText);
    }
    
    await refreshHistory();
    this.checkCanceled();

    requestQueue.patch(this.requestId, {
      phase: 'success',
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

    while (attempt <= this.req.maxAttempts) {
      this.checkCanceled();

      if (attempt === 1 && this.isActiveThread()) {
        session.setStlUrl(null);
      }

      requestQueue.patch(this.requestId, { phase: attempt > 1 ? 'repairing' : 'generating', attempt });
      syncSessionPhaseFromQueue();
      this.updateStatus(`Consulting LLM (Attempt ${attempt}/${this.req.maxAttempts})...`);

      try {
        const result = await generateDesign({
          prompt: currentPrompt,
          threadId: this.snapshotThreadId,
          parentMacroCode: this.snapshotParentMacroCode,
          workingDesign: this.snapshotWorkingDesign,
          isRetry: attempt > 1,
          imageData: this.currentScreenshot,
          attachments: this.req.attachments,
          questionMode: false,
          followUpQuestion: attempt === 1 ? this.followUpQuestion : null,
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
        this.updateStatus('Executing FreeCAD engine...');

        try {
          const bundle = await renderModel(data.macroCode, data.initialParams || {});
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

          await this.commitSuccess(data, bundle, manifest);
          return;

        } catch (renderError) {
          this.checkCanceled();
          if (attempt < this.req.maxAttempts) {
            const repairMsg = pickRetryMessage(attempt + 1, this.req.maxAttempts);
            if (this.isActiveThread()) session.setRepairMessage(repairMsg);
            const renderErrorText = toErrorMessage(renderError);
            currentPrompt = `The previous code failed in FreeCAD with this error:\n${renderErrorText}\n\nPlease fix it.`;
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
        attachments: this.req.attachments
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
  ) {
    const stlUrlValue = toAssetUrl(bundle.previewStlPath);
    requestQueue.patch(this.requestId, { phase: 'committing' });
    syncSessionPhaseFromQueue();

    await this.finalizeAttempt('success', data, undefined, undefined, bundle, manifest);
    this.checkCanceled();

    if (this.isActiveThread()) {
      activeThreadId.set(this.snapshotThreadId);
      activeVersionId.set(this.assistantMessageId);
      const currentQ = get(requestQueue);
      if (currentQ.activeId === this.requestId) {
        workingCopy.loadVersion(data, this.assistantMessageId);
        paramPanelState.hydrateFromVersion(data, this.assistantMessageId);
        session.setStlUrl(stlUrlValue);
        session.setModelRuntime(bundle, manifest);
      }
      session.setStatus('Design synthesized successfully.');
    }
    requestQueue.patch(this.requestId, { threadId: this.snapshotThreadId });

    if (this.isActiveThread()) {
      await persistLastSessionSnapshot({
        design: data,
        threadId: this.snapshotThreadId,
        messageId: this.assistantMessageId,
        artifactBundle: bundle,
        modelManifest: manifest,
        selectedPartId: null,
      });
    }

    await refreshHistory();
    requestQueue.patch(this.requestId, {
      phase: 'success',
      result: {
        design: data,
        threadId: this.snapshotThreadId,
        messageId: this.assistantMessageId!,
        stlUrl: stlUrlValue,
        artifactBundle: bundle,
        modelManifest: manifest,
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
    openCodeModalManual?.(data);
    requestQueue.patch(this.requestId, { phase: 'error', error: `Render Error: ${renderError}` });
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
