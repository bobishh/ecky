import {
  commands,
  type ActiveProjectNavigation,
  type AgentDraftProjection,
  type AppError,
  type AppLogEntry,
  type CampaignRun,
  type OpenCampaignProjectIntent,
  type OpenCampaignProjectResult,
  type TransitionCampaignRunInput,
  type TransitionCampaignRunResult,
  type CampaignStepPayload,
  type CampaignSummary,
  type CaptureSessionInfo,
  type ExistingCaptureTarget,
  type CaptureSessionState,
  type CaptureRun,
  type ApplyCapturePreviewResult,
  type ManualCodeApplyResponse,
  type PersistControlDefaultsInput,
  type ExternalShapeSource,
  type ApplyExternalShapeEditInput,
  type ApplyInlineComponentImportInput,
  type LibraryPanelIntent,
  type LibraryPanelProjection,
  type SurfaceTrimPathPreviewRequest,
  type SurfaceTrimPathPreviewResponse,
  type SurfaceTrimLoopPreviewRequest,
  type SurfaceTrimLoopPreviewResponse,
  type SurfaceTrimRegionPreviewRequest,
  type SurfaceTrimRegionPreviewResponse,
  type ApplyCaptureGuideEditInput,
  type ApplyCaptureGuideEditResult,
  type ValidateCaptureGuideIntentInput,
  type ValidateCaptureGuideIntentResult,
  type ReopenedCaptureRun,
  type QueuedCaptureGuidedReconstruction,
  type FemCancelResponse,
  type FemConvergenceIntentInput,
  type FemConvergenceResponse,
  type FemMeshPreviewResponse,
  type FemMeshPreviewIntentResponse,
  type FemRunIntentInput,
  type FemRunIntentResponse,
  type FemRunResponse,
  type FemStudyValidationResponse,
  type FemVtuExportResponse,
  type FemVtuExportIntentInput,
  type DeletedThreadsPage,
  type DenseTopologyItem,
  type DenseTopologyKind,
  type DenseTopologyPage,
  type ApplySemanticManifestEditInput,
  type SemanticManifestEditResult,
  type ApplySemanticControlValueInput,
  type ApplySemanticControlValueResult,
  type EnsureCaptureReconstructionGuideResult,
  type Result,
  type AgentActivityCatchUp,
  type ThreadWindowLayout,
  type WebContentRecoveryState,
  type AgyMessagePage,
  type AgyMessagePageInput,
  type AgyPromptInput,
  type AgyProviderSnapshot,
  type AgyStopInput,
  type ProviderWriterActivationInput,
  type CodexMessagePage,
  type CodexMessagePageInput,
  type CodexPromptInput,
  type ClassificationResult,
  type CodexSteerInput,
  type CodexStopInput,
  type CodexVoiceConnection,
  type CodexVoiceStartInput,
  type CodexVoiceStopInput,
  type CodexTakeoverSnapshot,
  type ExplorationRunOutput,
  type StartExplorationRunInput,
  type StopExplorationRunInput,
  type SketchPreviewSubmissionPacket,
  type SketchPreviewSubmissionRequest,
} from './contracts';
import {
  normalizeArtifactBundle,
  normalizeAttachment,
  normalizeConfig,
  normalizeDeletedMessage,
  normalizeDesignOutput,
  normalizeLastDesignSnapshot,
  normalizeModelManifest,
  normalizeMessage,
  normalizeParsedParamsResult,
  normalizeRuntimeCapabilities,
  normalizeThreadMessagesPage,
  normalizeThread,
  normalizeUsageSummary,
  toContractAttachment,
  toContractDesignOutput,
  toContractLastDesignSnapshot,
  toContractUiSpec,
  type AgentSession,
  type AgentTerminalInput,
  type AgentTerminalSnapshot,
  type ArtifactBundle,
  type AppConfig,
  type Attachment,
  type DeletedMessage,
  type DesignOutput,
  type DesignParams,
  type GeometryBackend,
  type MessageStatus,
  type LastDesignSnapshot,
  type MacroDialect,
  type Message,
  type ModelManifest,
  type McpServerStatus,
  type ParsedParamsResult,
  type RuntimeCapabilities,
  type StructuralVerificationResult,
  type VisualVerificationResult,
  type SourceLanguage,
  type Thread,
  type ThreadMessagesPage,
  type UiSpec,
  type UsageSummary,
  type ViewportCameraState,
} from '../types/domain';
import { resolveSketchPreviewDraftScopeId } from '../sketchPreviewDraftStore';
import { projectExplorationCyclePacket, type ExplorationCyclePacket } from '../explorationCycle';
import type { AuthoringGraph, AuthoringGraphRequest } from '../authoringGraph';
import type {
  ComponentPackage,
  ClearSketchPreviewDraftRequest,
  ExportPartInput,
  FreecadLibraryItem,
  PostProcessingSpec,
  PromptTranscription,
  QueueAgentPromptInput,
  RasterTraceRequest,
  RasterTraceResponse,
  RejectViewportScreenshotInput,
  SubmitAgentPromptReplyInput,
  SubmitAgentPromptReplyResult,
  ResolveViewportScreenshotInput,
  SketchAcceptedBrepComponentPackageRequest,
  SketchBrepCandidateAcceptRequest,
  SketchBrepCandidateAcceptResponse,
  SketchDraftRequest,
  SketchDraftSource,
  SketchDocument,
  SaveSketchPreviewDraftRequest,
  SketchPreviewDraft,
  SketchSuggestionRequest,
  SketchSuggestionResponse,
  TranscribePromptAudioInput,
  VersionPreviewRuntime,
} from './contracts';

export type {
  FemRunIntentInput,
  FemRunIntentResponse,
  FemRunResponse,
  FemConvergenceIntentInput,
  FemConvergenceResponse,
  FemMeshPreviewResponse,
  FemMeshPreviewIntentResponse,
  FemStudyValidationResponse,
  FemVtuExportResponse,
  FemVtuExportIntentInput,
  CodexMessagePage,
  CodexTakeoverSnapshot,
  AgyMessagePage,
  AgyProviderSnapshot,
};

export async function activateProviderWriter(
  input: ProviderWriterActivationInput,
): Promise<void> {
  await invokeCommand(commands.activateProviderWriter(input));
}

export async function getAgyProvider(
  eckyThreadId: string,
): Promise<AgyProviderSnapshot | null> {
  return invokeCommand(commands.getAgyProvider(eckyThreadId));
}

export async function getAgyProviderMessages(
  input: AgyMessagePageInput,
): Promise<AgyMessagePage> {
  return invokeCommand(commands.getAgyProviderMessages(input));
}

export async function sendAgyProviderPrompt(
  input: Omit<AgyPromptInput, 'attachments'> & { attachments?: Attachment[] },
): Promise<AgyProviderSnapshot> {
  return invokeCommand(commands.sendAgyProviderPrompt({
    ...input,
    attachments: (input.attachments ?? []).map(toContractAttachment),
  } as AgyPromptInput));
}

export async function stopAgyProvider(
  input: AgyStopInput,
): Promise<AgyProviderSnapshot> {
  return invokeCommand(commands.stopAgyProvider(input));
}

export async function retryAgyQueuedPrompt(
  eckyThreadId: string,
  queueId: string,
): Promise<AgyProviderSnapshot> {
  return invokeCommand(commands.retryAgyQueuedPrompt(eckyThreadId, queueId));
}

export async function removeAgyQueuedPrompt(
  eckyThreadId: string,
  queueId: string,
): Promise<AgyProviderSnapshot> {
  return invokeCommand(commands.removeAgyQueuedPrompt(eckyThreadId, queueId));
}

export async function compactAgyProvider(
  eckyThreadId: string,
): Promise<AgyProviderSnapshot> {
  return invokeCommand(commands.compactAgyProvider(eckyThreadId));
}

export async function getCodexTakeover(
  eckyThreadId: string,
): Promise<CodexTakeoverSnapshot | null> {
  return invokeCommand(commands.getCodexTakeover(eckyThreadId));
}

export async function getJevClassificationResults(
  threadId: string,
): Promise<ClassificationResult[]> {
  return invokeCommand(commands.getJevClassificationResults(threadId));
}

export async function getCodexTakeoverMessages(
  input: CodexMessagePageInput,
): Promise<CodexMessagePage> {
  return invokeCommand(commands.getCodexTakeoverMessages(input));
}

export async function sendCodexTakeoverPrompt(
  input: Omit<CodexPromptInput, 'attachments'> & { attachments?: Attachment[] },
): Promise<CodexTakeoverSnapshot> {
  return invokeCommand(commands.sendCodexTakeoverPrompt({
    ...input,
    attachments: (input.attachments ?? []).map(toContractAttachment),
  } as CodexPromptInput));
}

export async function steerCodexTakeover(
  input: Omit<CodexSteerInput, 'attachments'> & { attachments?: Attachment[] },
): Promise<CodexTakeoverSnapshot> {
  return invokeCommand(commands.steerCodexTakeover({
    ...input,
    attachments: (input.attachments ?? []).map(toContractAttachment),
  }));
}

export async function stopCodexTakeover(
  input: CodexStopInput,
): Promise<CodexTakeoverSnapshot> {
  return invokeCommand(commands.stopCodexTakeover(input));
}

export async function retryCodexQueuedPrompt(
  eckyThreadId: string,
  queueId: string,
): Promise<CodexTakeoverSnapshot> {
  return invokeCommand(commands.retryCodexQueuedPrompt(eckyThreadId, queueId));
}

export async function removeCodexQueuedPrompt(
  eckyThreadId: string,
  queueId: string,
): Promise<CodexTakeoverSnapshot> {
  return invokeCommand(commands.removeCodexQueuedPrompt(eckyThreadId, queueId));
}

export type AppErrorDiagnosticField = {
  key: string;
  value: string;
};

export type AppErrorDiagnosticContext = {
  detailText: string | null;
  rawTail: string | null;
  stableNodeKey: string | null;
  partKey: string | null;
  operation: string | null;
  startLine: number | null;
  endLine: number | null;
  fields: AppErrorDiagnosticField[];
};

function unwrapResult<T>(result: Result<T, AppError>): T {
  if (result.status === 'ok') {
    return result.data;
  }
  throw result.error;
}

async function invokeCommand<T>(command: Promise<Result<T, AppError>>): Promise<T>;
async function invokeCommand<T, R>(
  command: Promise<Result<T, AppError>>,
  transform: (value: T) => R,
): Promise<R>;
async function invokeCommand<T, R>(
  command: Promise<Result<T, AppError>>,
  transform?: (value: T) => R,
): Promise<T | R> {
  const value = unwrapResult(await command);
  return transform ? transform(value) : value;
}

function canUseLegacyE2eCompatibility(): boolean {
  return import.meta.env.DEV && typeof navigator !== 'undefined' && navigator.webdriver;
}

function isBackendError(error: unknown): error is AppError {
  return Boolean(
    error &&
      typeof error === 'object' &&
      'code' in error &&
      'message' in error &&
      typeof (error as { message?: unknown }).message === 'string',
  );
}

function parseDiagnosticTail(rawDetails: string | null | undefined): {
  detailText: string | null;
  rawTail: string | null;
  fields: AppErrorDiagnosticField[];
} {
  const details = `${rawDetails ?? ''}`.trim();
  if (!details) {
    return { detailText: null, rawTail: null, fields: [] };
  }
  const lines = details
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  const rawTail = lines.at(-1) ?? null;
  if (!rawTail) {
    return { detailText: details, rawTail: null, fields: [] };
  }
  const fields = rawTail
    .split(/\s+/)
    .map((token) => {
      const equalIndex = token.indexOf('=');
      if (equalIndex <= 0 || equalIndex === token.length - 1) return null;
      return {
        key: token.slice(0, equalIndex),
        value: token.slice(equalIndex + 1),
      } satisfies AppErrorDiagnosticField;
    })
    .filter((field): field is AppErrorDiagnosticField => Boolean(field));
  if (fields.length === 0 || fields.length !== rawTail.split(/\s+/).length) {
    return { detailText: details, rawTail: null, fields: [] };
  }
  const detailText = lines.slice(0, -1).join('\n').trim() || null;
  return { detailText, rawTail, fields };
}

function formatDiagnosticLine(context: AppErrorDiagnosticContext): string | null {
  const parts = [...context.fields.map((field) => `${field.key}=${field.value}`)];
  if (context.partKey && !context.fields.some((field) => field.key === 'part')) {
    parts.unshift(`part=${context.partKey}`);
  }
  if (context.operation && !context.fields.some((field) => field.key === 'op')) {
    parts.push(`op=${context.operation}`);
  }
  if (context.startLine !== null && !context.fields.some((field) => field.key === 'lines')) {
    parts.push(
      context.endLine !== null && context.endLine !== context.startLine
        ? `lines=${context.startLine}-${context.endLine}`
        : `lines=${context.startLine}`,
    );
  }
  return parts.length > 0 ? parts.join(' | ') : null;
}

export function getAppErrorDiagnosticContext(error: unknown): AppErrorDiagnosticContext | null {
  if (!isBackendError(error)) return null;
  const parsed = parseDiagnosticTail(error.details);
  const resolvedParamFields = (error.diagnosticContext?.resolvedParams ?? []).map((param) => ({
    key: param.key,
    value:
      typeof param.value === 'number' || typeof param.value === 'boolean'
        ? `${param.value}`
        : param.value === null
          ? 'null'
          : `${param.value}`,
  }));
  return {
    detailText: parsed.detailText,
    rawTail: parsed.rawTail,
    stableNodeKey: error.stableNodeKey ?? null,
    partKey: error.diagnosticContext?.partKey ?? null,
    operation: error.diagnosticContext?.opName ?? error.operation ?? null,
    startLine: error.diagnosticContext?.startLine ?? error.startLine ?? null,
    endLine: error.diagnosticContext?.endLine ?? error.endLine ?? null,
    fields: resolvedParamFields.length > 0 ? resolvedParamFields : parsed.fields,
  };
}

export function formatBackendError(error: unknown): string {
  if (isBackendError(error)) {
    const context = getAppErrorDiagnosticContext(error);
    const sections = [error.message];
    if (context?.detailText) {
      sections.push(context.detailText);
    } else if (error.details && !context) {
      sections.push(error.details);
    }
    const diagnosticLine = context ? formatDiagnosticLine(context) : null;
    if (diagnosticLine) {
      sections.push(`Context: ${diagnosticLine}`);
    } else if (error.details && !context?.detailText) {
      sections.push(error.details);
    }
    return sections.join('\n');
  }
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

export async function getConfig(): Promise<AppConfig> {
  return invokeCommand(commands.getConfig(), normalizeConfig);
}

export type BootProjection = {
  config: AppConfig;
  history: Thread[];
  workspace: WorkspaceProjection | null;
  selectedPartId: string | null;
};

export async function getBootProjection(): Promise<BootProjection> {
  let projection = await invokeCommand(commands.getBootProjection());
  if (!projection && canUseLegacyE2eCompatibility()) {
    projection = await import('./e2eBootCompatibility').then((module) => module.legacyBootProjection());
  }
  if (!projection) throw new Error('get_boot_projection returned an empty response');
  return {
    config: normalizeConfig(projection.config),
    history: projection.history.map(normalizeThread),
    workspace: projection.workspace
      ? normalizeWorkspaceProjection(projection.workspace)
      : null,
    selectedPartId: projection.selectedPartId,
  };
}

export async function getBootRuntimeProjection(): Promise<{
  config: AppConfig;
  capabilities: RuntimeCapabilities;
}> {
  let projection = await invokeCommand(commands.getBootRuntimeProjection());
  if (!projection && canUseLegacyE2eCompatibility()) {
    projection = await import('./e2eBootCompatibility').then((module) => module.legacyBootRuntimeProjection());
  }
  if (!projection) throw new Error('get_boot_runtime_projection returned an empty response');
  return {
    config: normalizeConfig(projection.config),
    capabilities: normalizeRuntimeCapabilities(projection.capabilities),
  };
}

export async function getRuntimeCapabilities(): Promise<RuntimeCapabilities> {
  return invokeCommand(commands.getRuntimeCapabilities(), normalizeRuntimeCapabilities);
}

export async function saveConfig(config: AppConfig): Promise<void> {
  await invokeCommand(commands.saveConfig(config));
}

export async function saveConfigProjection(config: AppConfig): Promise<{
  config: AppConfig;
  capabilities: RuntimeCapabilities;
}> {
  let projection = await invokeCommand(commands.saveConfigProjection(config));
  if (!projection && canUseLegacyE2eCompatibility()) {
    projection = await import('./e2eBootCompatibility').then((module) => module.legacySaveConfigProjection(config));
  }
  if (!projection) throw new Error('save_config_projection returned an empty response');
  return {
    config: normalizeConfig(projection.config),
    capabilities: normalizeRuntimeCapabilities(projection.capabilities),
  };
}

export async function refreshModelCatalog(): Promise<{
  config: AppConfig;
  models: string[];
}> {
  let projection = await invokeCommand(commands.refreshModelCatalog());
  if (!projection && canUseLegacyE2eCompatibility()) {
    projection = await import('./e2eBootCompatibility').then((module) => module.legacyModelCatalogProjection());
  }
  if (!projection) throw new Error('refresh_model_catalog returned an empty response');
  return {
    config: normalizeConfig(projection.config),
    models: projection.models,
  };
}

export async function listModels(
  provider: string,
  apiKey: string,
  baseUrl: string,
): Promise<string[]> {
  return invokeCommand(commands.listModels(provider, apiKey, baseUrl));
}

export async function getDesignSystemPrompt(provider?: string | null): Promise<string> {
  return invokeCommand(commands.getDesignSystemPrompt(provider ?? null));
}

export async function listAgentModels(cmd: string): Promise<{ models: string[]; isLive: boolean }> {
  return invokeCommand(commands.listAgentModels(cmd));
}

export async function listProviderModels(provider: string): Promise<{ models: string[]; isLive: boolean }> {
  return invokeCommand(commands.listProviderModels(provider));
}

export async function getHistory(): Promise<Thread[]> {
  return invokeCommand(commands.getHistory(), (threads) => threads.map(normalizeThread));
}

export async function getThread(id: string): Promise<Thread> {
  return invokeCommand(commands.getThread(id), normalizeThread);
}

export async function getActiveExplorationCycle(
  threadId: string,
): Promise<ExplorationCyclePacket | null> {
  const packet = await invokeCommand(commands.getActiveExplorationCycle(threadId));
  return packet ? projectExplorationCyclePacket(packet) : null;
}

export type StartExplorationRunRequest = Omit<
  StartExplorationRunInput,
  'attachments' | 'workingDesign'
> & {
  attachments?: Attachment[];
  workingDesign?: DesignOutput | null;
};

export async function startExplorationRun(
  input: StartExplorationRunRequest,
): Promise<ExplorationRunOutput & { message: Message | null; snapshotId: string | null }> {
  const projection = await invokeCommand(commands.startExplorationRun({
    ...input,
    attachments: (input.attachments ?? []).map(toContractAttachment),
    workingDesign: input.workingDesign ? toContractDesignOutput(input.workingDesign) : null,
  }));
  const output = projection.run;
  return {
    ...output,
    design: output.design ? normalizeDesignOutput(output.design) : null,
    artifactBundle: output.artifactBundle
      ? normalizeArtifactBundle(output.artifactBundle)
      : null,
    modelManifest: output.modelManifest
      ? normalizeModelManifest(output.modelManifest)
      : null,
    usage: normalizeUsageSummary(output.usage),
    message: projection.message ? normalizeMessage(projection.message) : null,
    snapshotId: projection.snapshotId ?? null,
  };
}

export async function stopExplorationRun(input: StopExplorationRunInput): Promise<void> {
  await invokeCommand(commands.stopExplorationRun(input));
}

export async function getThreadLatestVersion(threadId: string): Promise<Message | null> {
  const headId = await invokeCommand(commands.getThreadHeadVersionId(threadId));
  if (headId) {
    const detail = await invokeCommand(commands.getVersionDetail(threadId, headId));
    return detail ? hydrateVersionDetail(threadId, headId, detail) : null;
  }
  const fallback = await invokeCommand(commands.getThreadLatestVersion(threadId));
  if (!fallback) return null;
  return getHydratedVersionDetail(threadId, fallback.id, fallback);
}

export async function getThreadMessageVersion(
  threadId: string,
  messageId: string,
): Promise<Message | null> {
  const detail = await invokeCommand(commands.getVersionDetail(threadId, messageId));
  if (!detail) {
    const message = await invokeCommand(commands.getThreadMessageVersion(threadId, messageId));
    return message ? normalizeMessage(message) : null;
  }
  return hydrateVersionDetail(threadId, messageId, detail);
}

async function getHydratedVersionDetail(
  threadId: string,
  messageId: string,
  fallback: import('./contracts').Message,
): Promise<Message> {
  const detail = await invokeCommand(commands.getVersionDetail(threadId, messageId));
  return detail
    ? hydrateVersionDetail(threadId, messageId, detail)
    : normalizeMessage(fallback);
}

async function hydrateVersionDetail(
  threadId: string,
  messageId: string,
  detail: import('./contracts').VersionDetail,
): Promise<Message> {
  const message = structuredClone(detail.message);
  await hydrateBoundedTopology(
    detail.edgeCount,
    detail.faceCount,
    detail.selectionTargetCount,
    async (kind, cursor) => invokeCommand(
      commands.getDenseTopologyPage(threadId, messageId, kind, cursor, 500),
    ),
    message,
  );
  return normalizeMessage(message);
}

const MAX_INTERACTIVE_TOPOLOGY_TARGETS = 1_000;

async function readTopologyKind(
  kind: DenseTopologyKind,
  loadPage: (kind: DenseTopologyKind, cursor: string | null) => Promise<DenseTopologyPage>,
): Promise<DenseTopologyItem[]> {
  const items: DenseTopologyItem[] = [];
  let cursor: string | null = null;
  do {
    const page = await loadPage(kind, cursor);
    items.push(...page.items);
    cursor = page.nextCursor;
  } while (cursor && items.length < MAX_INTERACTIVE_TOPOLOGY_TARGETS);
  return items;
}

async function hydrateBoundedTopology(
  edgeCount: number,
  faceCount: number,
  selectionTargetCount: number,
  loadPage: (kind: DenseTopologyKind, cursor: string | null) => Promise<DenseTopologyPage>,
  target: {
    artifactBundle?: { edgeTargets?: unknown[]; faceTargets?: unknown[] } | null;
    modelManifest?: { selectionTargets?: unknown[] } | null;
  },
): Promise<void> {
  const total = edgeCount + faceCount + selectionTargetCount;
  if (total === 0 || total > MAX_INTERACTIVE_TOPOLOGY_TARGETS) return;
  const [edges, faces, selections] = await Promise.all([
    edgeCount > 0 ? readTopologyKind('edge', loadPage) : [],
    faceCount > 0 ? readTopologyKind('face', loadPage) : [],
    selectionTargetCount > 0 ? readTopologyKind('selection', loadPage) : [],
  ]);
  if (target.artifactBundle) {
    target.artifactBundle.edgeTargets = edges
      .filter((item) => item.kind === 'edge')
      .map((item) => item.value);
    target.artifactBundle.faceTargets = faces
      .filter((item) => item.kind === 'face')
      .map((item) => item.value);
  }
  if (target.modelManifest) {
    target.modelManifest.selectionTargets = selections
      .filter((item) => item.kind === 'selection')
      .map((item) => item.value);
  }
}

export async function materializeVersionPreview(
  threadId: string,
  messageId: string,
): Promise<VersionPreviewRuntime> {
  const runtime = await invokeCommand(commands.materializeVersionPreview(threadId, messageId));
  return {
    ...runtime,
    artifactBundle: normalizeArtifactBundle(runtime.artifactBundle),
    modelManifest: normalizeModelManifest(runtime.modelManifest),
  };
}

export async function releaseVersionPreview(leaseId: string): Promise<void> {
  await invokeCommand(commands.releaseVersionPreview(leaseId));
}

export async function getThreadMessagesPage(
  threadId: string,
  before: string | null = null,
  limit = 50,
  includeVisualPayloads = false,
): Promise<ThreadMessagesPage> {
  return invokeCommand(
    commands.getThreadMessagesPage(threadId, before, limit, includeVisualPayloads),
    normalizeThreadMessagesPage,
  );
}

export type WorkspaceProjection = {
  thread: Thread;
  messagesPage: ThreadMessagesPage;
  selectedVersion: Message | null;
  requestedMessageFound: boolean;
};

function normalizeWorkspaceProjection(
  projection: import('./contracts').WorkspaceProjection,
): WorkspaceProjection {
  return {
    thread: normalizeThread(projection.thread),
    messagesPage: normalizeThreadMessagesPage(projection.messagesPage),
    selectedVersion: projection.selectedVersion
      ? normalizeMessage(projection.selectedVersion)
      : null,
    requestedMessageFound: projection.requestedMessageFound,
  };
}

export async function getWorkspaceProjection(
  threadId: string,
  preferredMessageId: string | null = null,
  messageLimit = 20,
): Promise<WorkspaceProjection> {
  const projection = await invokeCommand(
    commands.getWorkspaceProjection(threadId, preferredMessageId, messageLimit),
  );
  return normalizeWorkspaceProjection(projection);
}

export async function createDesignThreadIntent(input: {
  mode: 'blank' | 'macro';
  title?: string | null;
  source?: string | null;
  baseThreadId?: string | null;
  baseMessageId?: string | null;
}): Promise<{
  threadId: string;
  sourceDocument: import('./contracts').CreatedThreadSourceDocument;
  initialVersionId: string | null;
  snapshotId: string | null;
  parserMatched: boolean | null;
  initialVersionError: import('./contracts').AppError | null;
  workspace: WorkspaceProjection;
}> {
  const response = await invokeCommand(commands.createDesignThread({
    mode: input.mode,
    title: input.title ?? null,
    source: input.source ?? null,
    baseThreadId: input.baseThreadId ?? null,
    baseMessageId: input.baseMessageId ?? null,
  }));
  return {
    ...response,
    initialVersionId: response.initialVersionId ?? null,
    snapshotId: response.snapshotId ?? null,
    parserMatched: response.parserMatched ?? null,
    initialVersionError: response.initialVersionError ?? null,
    workspace: normalizeWorkspaceProjection(response.workspace),
  };
}

export async function forkDesignIntent(input: {
  sourceThreadId: string;
  sourceMessageId: string;
  title?: string | null;
  versionName?: string | null;
  messageLimit?: number | null;
}): Promise<{ threadId: string; messageId: string; workspace: WorkspaceProjection }> {
  const response = await invokeCommand(commands.forkDesign({
    sourceThreadId: input.sourceThreadId,
    sourceMessageId: input.sourceMessageId,
    title: input.title ?? null,
    versionName: input.versionName ?? null,
    messageLimit: input.messageLimit ?? null,
  }));
  return {
    ...response,
    workspace: normalizeWorkspaceProjection(response.workspace),
  };
}

export async function getVersionSource(threadId: string, messageId: string): Promise<string | null> {
  const chunks: string[] = [];
  let startByte = 0;
  let expectedDigest: string | null = null;
  while (true) {
    const window = await invokeCommand(
      commands.getVersionSourceWindow(threadId, messageId, startByte, 256 * 1024),
    );
    if (!window) return null;
    if (expectedDigest && window.digest !== expectedDigest) {
      throw new Error(
        `Version source changed during windowed read: expected ${expectedDigest}, received ${window.digest}.`,
      );
    }
    expectedDigest = window.digest;
    chunks.push(window.content);
    if (window.nextStartByte === null) break;
    if (window.nextStartByte <= startByte) {
      throw new Error(`Version source window did not advance beyond byte ${startByte}.`);
    }
    startByte = window.nextStartByte;
  }
  return chunks.join('');
}

export type ThreadLifecycleProjection = {
  threadId: string;
  history: Thread[];
};

function normalizeThreadLifecycleProjection(
  projection: import('./contracts').ThreadLifecycleProjection,
): ThreadLifecycleProjection {
  return {
    threadId: projection.threadId,
    history: projection.history.map(normalizeThread),
  };
}

export async function deleteThreadIntent(
  threadId: string,
  selectedMessageId: string | null,
): Promise<ThreadLifecycleProjection> {
  const projection = await invokeCommand(commands.deleteThreadIntent({
    threadId,
    selectedMessageId,
  }));
  return normalizeThreadLifecycleProjection(projection);
}

export async function renameThread(id: string, title: string): Promise<void> {
  await invokeCommand(commands.renameThread(id, title));
}

export type VersionHistoryIntentInput = {
  messageId: string;
  selectedThreadId?: string | null;
  selectedMessageId?: string | null;
  messageLimit?: number | null;
};

export type VersionHistoryIntentResponse = {
  threadId: string;
  workspace: WorkspaceProjection | null;
  threadRemoved: boolean;
};

function normalizeVersionHistoryIntentResponse(
  response: import('./contracts').VersionHistoryIntentResponse,
): VersionHistoryIntentResponse {
  return {
    threadId: response.threadId,
    workspace: response.workspace ? normalizeWorkspaceProjection(response.workspace) : null,
    threadRemoved: response.threadRemoved,
  };
}

export async function deleteVersionIntent(
  input: VersionHistoryIntentInput,
): Promise<VersionHistoryIntentResponse> {
  const response = await invokeCommand(commands.deleteVersionIntent({
    messageId: input.messageId,
    selectedThreadId: input.selectedThreadId ?? null,
    selectedMessageId: input.selectedMessageId ?? null,
    messageLimit: input.messageLimit ?? null,
  }));
  return normalizeVersionHistoryIntentResponse(response);
}

export async function restoreVersionIntent(
  input: VersionHistoryIntentInput,
): Promise<VersionHistoryIntentResponse> {
  const response = await invokeCommand(commands.restoreVersionIntent({
    messageId: input.messageId,
    selectedThreadId: input.selectedThreadId ?? null,
    selectedMessageId: input.selectedMessageId ?? null,
    messageLimit: input.messageLimit ?? null,
  }));
  return normalizeVersionHistoryIntentResponse(response);
}

export async function getDeletedMessages(): Promise<DeletedMessage[]> {
  return invokeCommand(commands.getDeletedMessages(), (messages) =>
    messages.map(normalizeDeletedMessage),
  );
}

export async function getDeletedThreadsPage(
  before: string | null = null,
  limit = 24,
): Promise<DeletedThreadsPage> {
  return invokeCommand(commands.getDeletedThreadsPage(before, limit));
}

export async function getDeletedThreadPreview(id: string): Promise<string | null> {
  return invokeCommand(commands.getDeletedThreadPreview(id));
}

export async function getThreadPreview(id: string): Promise<string | null> {
  return invokeCommand(commands.getThreadPreview(id));
}

export async function restoreDeletedThread(id: string): Promise<void> {
  await invokeCommand(commands.restoreDeletedThread(id));
}

export async function hideDeletedMessage(messageId: string): Promise<void> {
  await invokeCommand(commands.hideDeletedMessage(messageId));
}

export async function finalizeThreadIntent(
  threadId: string,
  selectedMessageId: string | null,
): Promise<ThreadLifecycleProjection> {
  const projection = await invokeCommand(commands.finalizeThreadIntent({
    threadId,
    selectedMessageId,
  }));
  return normalizeThreadLifecycleProjection(projection);
}

export async function reopenThreadIntent(threadId: string): Promise<ThreadLifecycleProjection> {
  const projection = await invokeCommand(commands.reopenThreadIntent({
    threadId,
    selectedMessageId: null,
  }));
  return normalizeThreadLifecycleProjection(projection);
}

export async function openInventoryThreadIntent(
  threadId: string,
  messageLimit = 20,
): Promise<WorkspaceProjection> {
  const projection = await invokeCommand(commands.openInventoryThreadIntent({
    threadId,
    messageLimit,
  }));
  return normalizeWorkspaceProjection(projection);
}

export async function getInventory(): Promise<Thread[]> {
  return invokeCommand(commands.getInventory(), (threads) => threads.map(normalizeThread));
}

export type { MacroAstSourceNode } from './contracts';

export async function openProjectInEditor(
  threadId: string | null,
  messageId: string | null,
): Promise<import('./contracts').ProjectEditorLink> {
  return invokeCommand(commands.openProjectInEditor(threadId, messageId));
}

export async function openImportedCadSource(
  threadId: string | null,
  messageId: string | null,
): Promise<import('./contracts').ProjectEditorLink> {
  return invokeCommand(commands.openImportedCadSource(threadId, messageId));
}

export async function getProjectSource(
  threadId: string,
): Promise<import('./contracts').ProjectSourceDocument> {
  return invokeCommand(commands.getProjectSource(threadId));
}

export async function listExternalShapeSources(
  threadId: string,
): Promise<ExternalShapeSource[]> {
  return invokeCommand(commands.listExternalShapeSources(threadId));
}

export async function applyExternalShapeEdit(
  input: ApplyExternalShapeEditInput,
): Promise<{
  version: ManualCodeApplyResult;
  sourceDigest: string;
  externalSources: ExternalShapeSource[];
}> {
  const result = await invokeCommand(commands.applyExternalShapeEdit(input));
  return {
    ...result,
    version: {
      ...result.version,
      baseMessageId: result.version.baseMessageId ?? null,
      messageId: result.version.messageId ?? null,
      designOutput: normalizeDesignOutput(result.version.designOutput),
      artifactBundle: result.version.artifactBundle
        ? normalizeArtifactBundle(result.version.artifactBundle)
        : null,
      modelManifest: result.version.modelManifest
        ? normalizeModelManifest(result.version.modelManifest)
        : null,
      snapshotId: result.version.snapshotId ?? null,
      error: result.version.error ?? null,
    },
  };
}

export async function applyInlineComponentImport(
  input: ApplyInlineComponentImportInput,
): Promise<{
  version: ManualCodeApplyResult;
  sourceDigest: string;
  entrySymbol: string;
  partKey: string;
}> {
  const result = await invokeCommand(commands.applyInlineComponentImport(input));
  return {
    ...result,
    version: {
      ...result.version,
      baseMessageId: result.version.baseMessageId ?? null,
      messageId: result.version.messageId ?? null,
      designOutput: normalizeDesignOutput(result.version.designOutput),
      artifactBundle: result.version.artifactBundle
        ? normalizeArtifactBundle(result.version.artifactBundle)
        : null,
      modelManifest: result.version.modelManifest
        ? normalizeModelManifest(result.version.modelManifest)
        : null,
      snapshotId: result.version.snapshotId ?? null,
      error: result.version.error ?? null,
    },
  };
}

export async function libraryPanelIntent(
  intent: LibraryPanelIntent,
): Promise<LibraryPanelProjection> {
  return invokeCommand(commands.libraryPanelIntent(intent));
}

export async function submitSketchPreview(
  request: SketchPreviewSubmissionRequest,
): Promise<SketchPreviewSubmissionPacket> {
  const packet = await invokeCommand(commands.submitSketchPreview(request));
  return {
    ...packet,
    artifactBundle: packet.artifactBundle
      ? normalizeArtifactBundle(packet.artifactBundle)
      : null,
  };
}

export async function previewExternalShapeSurfaceTrimPath(
  request: SurfaceTrimPathPreviewRequest,
): Promise<SurfaceTrimPathPreviewResponse> {
  return invokeCommand(commands.previewExternalShapeSurfaceTrimPath(request));
}

export async function previewExternalShapeSurfaceTrimLoop(
  request: SurfaceTrimLoopPreviewRequest,
): Promise<SurfaceTrimLoopPreviewResponse> {
  return invokeCommand(commands.previewExternalShapeSurfaceTrimLoop(request));
}

export async function previewExternalShapeSurfaceTrimRegion(
  request: SurfaceTrimRegionPreviewRequest,
): Promise<SurfaceTrimRegionPreviewResponse> {
  return invokeCommand(commands.previewExternalShapeSurfaceTrimRegion(request));
}

export async function revealProjectFolder(
  threadId: string | null,
  messageId: string | null,
): Promise<import('./contracts').ProjectEditorLink> {
  return invokeCommand(commands.revealProjectFolder(threadId, messageId));
}

export type {
  ProjectFolderExportResult,
  ProjectFolderStatus,
  ProjectFolderApplyResult,
  ProjectFolderRenderActivity,
  ProjectManifest,
  ProjectSyncState,
} from './contracts';

export async function projectFolderExport(
  threadId: string | null,
  messageId: string | null,
): Promise<import('./contracts').ProjectFolderExportResult> {
  return invokeCommand(commands.projectFolderExport(threadId, messageId));
}

export async function projectFolderStatus(
  threadId: string | null,
  messageId: string | null,
): Promise<import('./contracts').ProjectFolderStatus> {
  return invokeCommand(commands.projectFolderStatus(threadId, messageId));
}

export async function projectFolderRenderActivity(): Promise<import('./contracts').ProjectFolderRenderActivity[]> {
  return invokeCommand(commands.projectFolderRenderActivity());
}

export async function projectFolderApply(
  threadId: string | null,
  messageId: string | null,
  force = false,
  title: string | null = null,
  versionName: string | null = null,
): Promise<import('./contracts').ProjectFolderApplyResult> {
  return invokeCommand(
    commands.projectFolderApply(threadId, messageId, force, title, versionName),
  );
}

export async function macroAstSourceMap(macroCode: string): Promise<import('./contracts').MacroAstSourceNode[]> {
  return invokeCommand(commands.macroAstSourceMap(macroCode));
}

export async function renderModel(
  macroCode: string,
  parameters: DesignParams,
  macroDialect?: MacroDialect | null,
  geometryBackend?: GeometryBackend | null,
  postProcessing?: PostProcessingSpec | null,
  previousManifest?: ModelManifest | null,
): Promise<ArtifactBundle> {
  const trace = {
    event: 'render.invoke',
    at: Date.now(),
    macroLength: macroCode.length,
    macroDialect: macroDialect ?? null,
    geometryBackend: geometryBackend ?? null,
    parameterKeys: Object.keys(parameters),
    previousModelId: previousManifest?.modelId ?? null,
    stack: new Error().stack,
  };
  console.warn('[CAD_FLOW][render.invoke]', trace);
  const flow = ((globalThis as any).__ECKY_CAD_FLOW__ ??= []);
  flow.push(trace);
  const bundle = await invokeCommand(
    commands.renderModel(
      macroCode,
      parameters,
      macroDialect ?? null,
      geometryBackend ?? null,
      postProcessing ?? null,
      previousManifest ?? null,
    ),
    normalizeArtifactBundle,
  );
  console.warn('[CAD_FLOW][render.result]', {
    modelId: bundle.modelId,
    modelStlPath: bundle.modelStlPath,
    contentHash: bundle.contentHash,
  });
  return bundle;
}

export type { PostProcessingSpec };

export type ImportedModelIntentResult = {
  threadId: string;
  messageId: string;
  title: string;
  message: Message;
  designOutput: DesignOutput;
  artifactBundle: ArtifactBundle;
  modelManifest: ModelManifest;
  snapshotId: string | null;
};

export async function importModelIntent(input: {
  source:
    | { kind: 'fcstd'; sourcePath: string }
    | { kind: 'freecadLibrary'; item: FreecadLibraryItem };
  threadId?: string | null;
  title?: string | null;
}): Promise<ImportedModelIntentResult> {
  const response = await invokeCommand(commands.importModelIntent({
    source: input.source,
    threadId: input.threadId ?? null,
    title: input.title ?? null,
  }));
  return {
    ...response,
    message: normalizeMessage(response.message),
    designOutput: normalizeDesignOutput(response.designOutput),
    artifactBundle: normalizeArtifactBundle(response.artifactBundle),
    modelManifest: normalizeModelManifest(response.modelManifest),
    snapshotId: response.snapshotId ?? null,
  };
}

export async function getAuthoringGraph(request: AuthoringGraphRequest): Promise<AuthoringGraph> {
  return invokeCommand(commands.getAuthoringGraph(request));
}

export async function applySemanticManifestEdit(
  input: ApplySemanticManifestEditInput,
): Promise<SemanticManifestEditResult> {
  const result = await invokeCommand(commands.applySemanticManifestEdit(input));
  return { ...result, manifest: normalizeModelManifest(result.manifest) };
}

export type SemanticControlValueResult = Omit<ApplySemanticControlValueResult, 'parameterPatch'> & {
  parameterPatch: DesignParams;
};

export async function applySemanticControlValue(
  input: ApplySemanticControlValueInput,
): Promise<SemanticControlValueResult> {
  const result = await invokeCommand(commands.applySemanticControlValue(input));
  const parameterPatch: DesignParams = {};
  for (const [key, value] of Object.entries(result.parameterPatch)) {
    if (value !== undefined) parameterPatch[key] = value;
  }
  return { ...result, parameterPatch };
}

export async function getMessStlPath(): Promise<string> {
  return invokeCommand(commands.getMessStlPath());
}

export async function exportFile(sourcePath: string, targetPath: string): Promise<void> {
  await invokeCommand(commands.exportFile(sourcePath, targetPath));
}

export async function exportEckyMcpSkillZip(targetPath: string): Promise<void> {
  await invokeCommand(commands.exportEckyMcpSkillZip(targetPath));
}

export async function exportDocsBookEpub(targetPath: string): Promise<void> {
  await invokeCommand(commands.exportDocsBookEpub(targetPath));
}

export async function suggestSketchFeatures(
  request: SketchSuggestionRequest,
): Promise<SketchSuggestionResponse> {
  return invokeCommand(commands.suggestSketchFeatures(request));
}

export async function evaluateSketchDocumentConstraints(
  request: import('./contracts').SketchConstraintEvaluationRequest,
): Promise<import('./contracts').SketchConstraintEvaluationResponse> {
  return invokeCommand(commands.evaluateSketchDocumentConstraints(request));
}

export async function generateSketchDraftPreview(
  request: SketchDraftRequest,
): Promise<{ draft: SketchDraftSource; artifactBundle: ArtifactBundle }> {
  const [draft, bundle] = await invokeCommand(commands.generateSketchDraftPreview(request));
  return { draft, artifactBundle: normalizeArtifactBundle(bundle) };
}

export async function traceRasterReference(
  request: RasterTraceRequest,
): Promise<RasterTraceResponse> {
  return invokeCommand(commands.traceRasterReference(request));
}

export async function saveSketchPreviewDraft(input: {
  scopeId?: string | null;
  draftScopeId?: string | null;
  newScope?: boolean;
  draftSource: SketchDraftSource;
  artifactBundle: ArtifactBundle;
  sketchDocument?: SketchDocument | null;
}): Promise<SketchPreviewDraft> {
  const scopeId = resolveSketchPreviewDraftScopeId(input);
  return invokeCommand(
    commands.saveSketchPreviewDraft({
      scopeId,
      newScope: input.newScope ?? false,
      draftSource: input.draftSource,
      artifactBundle: input.artifactBundle,
      sketchDocument: input.sketchDocument ?? null,
    } satisfies SaveSketchPreviewDraftRequest),
  );
}

export async function clearSketchPreviewDraft(input: {
  scopeId?: string | null;
  draftScopeId?: string | null;
}): Promise<void> {
  const scopeId = resolveSketchPreviewDraftScopeId(input);
  await invokeCommand(
    commands.clearSketchPreviewDraft({
      scopeId,
    } satisfies ClearSketchPreviewDraftRequest),
  );
}

export async function acceptSketchBrepCandidateSolution(
  request: SketchBrepCandidateAcceptRequest,
): Promise<Omit<SketchBrepCandidateAcceptResponse, 'artifactBundle'> & { artifactBundle: ArtifactBundle }> {
  const response = await invokeCommand(commands.acceptSketchBrepCandidateSolution(request));
  return {
    ...response,
    artifactBundle: normalizeArtifactBundle(response.artifactBundle),
  };
}

export async function acceptedBrepCandidateToComponentPackage(
  request: SketchAcceptedBrepComponentPackageRequest,
): Promise<ComponentPackage> {
  return invokeCommand(commands.acceptedBrepCandidateToComponentPackage(request));
}

export async function exportMultipartStlZip(
  parts: ExportPartInput[],
  targetPath: string,
  modelName: string,
): Promise<void> {
  await invokeCommand(commands.exportMultipartStlZip(parts, targetPath, modelName));
}

export async function exportMultipart3mf(
  parts: ExportPartInput[],
  targetPath: string,
  modelName: string,
): Promise<void> {
  await invokeCommand(commands.exportMultipart3mf(parts, targetPath, modelName));
}

export type ManualParameterApplyResult = {
  threadId: string;
  baseMessageId: string;
  messageId: string | null;
  status: MessageStatus;
  designOutput: DesignOutput;
  artifactBundle: ArtifactBundle | null;
  modelManifest: ModelManifest | null;
  snapshotId: string | null;
  error: AppError | null;
};

export async function applyManualParameters(input: {
  threadId: string;
  targetMessageId: string;
  parameters: DesignParams;
  persist: boolean;
  title?: string | null;
  versionName?: string | null;
}): Promise<ManualParameterApplyResult> {
  const response = await invokeCommand(
    commands.applyManualParameters({
      threadId: input.threadId,
      targetMessageId: input.targetMessageId,
      parameters: input.parameters,
      persist: input.persist,
      title: input.title ?? null,
      versionName: input.versionName ?? null,
    }),
  );
  return {
    ...response,
    messageId: response.messageId ?? null,
    designOutput: normalizeDesignOutput(response.designOutput),
    artifactBundle: response.artifactBundle
      ? normalizeArtifactBundle(response.artifactBundle)
      : null,
    modelManifest: response.modelManifest
      ? normalizeModelManifest(response.modelManifest)
      : null,
    snapshotId: response.snapshotId ?? null,
    error: response.error ?? null,
  };
}

export async function applyImportedParameters(input: {
  threadId: string;
  targetMessageId: string;
  parameters: DesignParams;
  persist: boolean;
  title?: string | null;
  versionName?: string | null;
}): Promise<ManualParameterApplyResult> {
  const response = await invokeCommand(commands.applyImportedParameters({
    threadId: input.threadId,
    targetMessageId: input.targetMessageId,
    parameters: input.parameters,
    persist: input.persist,
    title: input.title ?? null,
    versionName: input.versionName ?? null,
  }));
  return {
    ...response,
    messageId: response.messageId ?? null,
    designOutput: normalizeDesignOutput(response.designOutput),
    artifactBundle: response.artifactBundle
      ? normalizeArtifactBundle(response.artifactBundle)
      : null,
    modelManifest: response.modelManifest
      ? normalizeModelManifest(response.modelManifest)
      : null,
    snapshotId: response.snapshotId ?? null,
    error: response.error ?? null,
  };
}

export type ManualCodeApplyResult = {
  threadId: string;
  baseMessageId: string | null;
  messageId: string | null;
  status: MessageStatus;
  designOutput: DesignOutput;
  artifactBundle: ArtifactBundle | null;
  modelManifest: ModelManifest | null;
  snapshotId: string | null;
  parserMatched: boolean;
  error: AppError | null;
};

function normalizeManualCodeApplyResponse(response: ManualCodeApplyResponse): ManualCodeApplyResult {
  return {
    ...response,
    baseMessageId: response.baseMessageId ?? null,
    messageId: response.messageId ?? null,
    designOutput: normalizeDesignOutput(response.designOutput),
    artifactBundle: response.artifactBundle
      ? normalizeArtifactBundle(response.artifactBundle)
      : null,
    modelManifest: response.modelManifest
      ? normalizeModelManifest(response.modelManifest)
      : null,
    snapshotId: response.snapshotId ?? null,
    error: response.error ?? null,
  };
}

export async function applyManualCode(input: {
  threadId: string;
  baseMessageId?: string | null;
  source: string;
  persist: boolean;
  title?: string | null;
  versionName?: string | null;
  uiSpec: UiSpec;
  parameters: DesignParams;
  postProcessing?: PostProcessingSpec | null;
  sourceLanguage?: SourceLanguage | null;
  geometryBackend?: GeometryBackend | null;
}): Promise<ManualCodeApplyResult> {
  const response = await invokeCommand(commands.applyManualCode({
    threadId: input.threadId,
    baseMessageId: input.baseMessageId ?? null,
    source: input.source,
    persist: input.persist,
    title: input.title ?? null,
    versionName: input.versionName ?? null,
    uiSpec: toContractUiSpec(input.uiSpec),
    parameters: input.parameters,
    postProcessing: input.postProcessing ?? null,
    sourceLanguage: input.sourceLanguage ?? null,
    geometryBackend: input.geometryBackend ?? null,
  }));
  return normalizeManualCodeApplyResponse(response);
}

export async function applyCapturePreview(input: { runId: string }): Promise<{
  source: string;
  draft: ManualCodeApplyResult;
}> {
  const response: ApplyCapturePreviewResult = await invokeCommand(commands.applyCapturePreview(input));
  return {
    source: response.source,
    draft: normalizeManualCodeApplyResponse(response.draft),
  };
}

export async function persistControlDefaults(input: {
  messageId: string;
  mutation:
    | { action: 'readFromMacro' }
    | { action: 'saveSchema'; uiSpec: UiSpec; parameters: DesignParams }
    | { action: 'saveValues'; parameters: DesignParams };
}): Promise<{ uiSpec: UiSpec; parameters: DesignParams; workspace: WorkspaceProjection }> {
  const mutation: PersistControlDefaultsInput['mutation'] = input.mutation.action === 'saveSchema'
    ? {
        action: 'saveSchema',
        uiSpec: toContractUiSpec(input.mutation.uiSpec),
        parameters: input.mutation.parameters,
      }
    : input.mutation;
  const response = await invokeCommand(commands.persistControlDefaults({
    messageId: input.messageId,
    mutation,
  }));
  return {
    uiSpec: response.uiSpec as UiSpec,
    parameters: response.parameters as DesignParams,
    workspace: normalizeWorkspaceProjection(response.workspace),
  };
}

export async function repairVersionRuntime(input: {
  threadId: string;
  messageId: string;
  expectedArtifactIdentity?: string | null;
}): Promise<{
  snapshotId: string;
  artifactIdentity: string;
  workspace: WorkspaceProjection;
}> {
  const response = await invokeCommand(commands.repairVersionRuntime(input));
  return {
    ...response,
    workspace: normalizeWorkspaceProjection(response.workspace),
  };
}

export async function updateVersionPreview(
  messageId: string,
  imageData: string,
  artifactBundle: ArtifactBundle,
): Promise<void> {
  await invokeCommand(commands.updateVersionPreview(messageId, imageData, artifactBundle));
}

export async function parseMacroParams(macroCode: string): Promise<ParsedParamsResult> {
  return normalizeParsedParamsResult(await commands.parseMacroParams(macroCode));
}

export async function listCampaignDefinitions(): Promise<CampaignSummary[]> {
  return invokeCommand(commands.listCampaignDefinitions());
}

export async function getCampaignStep(
  definitionId: string,
  stepId: string,
): Promise<CampaignStepPayload> {
  return invokeCommand(commands.getCampaignStep(definitionId, stepId));
}

export async function openCampaignProject(
  intent: OpenCampaignProjectIntent,
): Promise<OpenCampaignProjectResult> {
  return invokeCommand(commands.openCampaignProject(intent));
}

export async function listCampaignRuns(): Promise<CampaignRun[]> {
  return invokeCommand(commands.listCampaignRuns());
}

export async function getCampaignRun(id: string): Promise<CampaignRun> {
  return invokeCommand(commands.getCampaignRun(id));
}

export async function transitionCampaignRun(
  input: TransitionCampaignRunInput,
): Promise<TransitionCampaignRunResult> {
  return invokeCommand(commands.transitionCampaignRun(input));
}

export async function deleteCampaignRun(id: string): Promise<void> {
  await invokeCommand(commands.deleteCampaignRun(id));
}

export async function getActiveProjectNavigation(): Promise<ActiveProjectNavigation | null> {
  return invokeCommand(commands.getActiveProjectNavigation());
}

export async function clearActiveProjectNavigation(): Promise<void> {
  await invokeCommand(commands.clearActiveProjectNavigation());
}

export async function getAppWindowLayout(): Promise<ThreadWindowLayout | null> {
  return invokeCommand(commands.getAppWindowLayout());
}

export async function saveAppWindowLayout(layout: ThreadWindowLayout): Promise<void> {
  await invokeCommand(commands.saveAppWindowLayout(layout));
}

export async function uploadAsset(input: {
  sourcePath: string;
  name: string;
  format: string;
}) {
  return invokeCommand(commands.uploadAsset(input.sourcePath, input.name, input.format));
}

export async function saveRecordedAudio(input: { base64Data: string; name: string }) {
  return invokeCommand(commands.saveRecordedAudio(input.base64Data, input.name));
}

export async function transcribePromptAudio(input: TranscribePromptAudioInput): Promise<PromptTranscription> {
  return invokeCommand(commands.transcribePromptAudio(input));
}

export async function getLastDesign(): Promise<LastDesignSnapshot | null> {
  return invokeCommand(commands.getLastDesign(), normalizeLastDesignSnapshot);
}

export async function getWebContentRecoveryState(): Promise<WebContentRecoveryState> {
  return invokeCommand(commands.getWebContentRecoveryState());
}

export async function acknowledgeWebContentRecovery(): Promise<void> {
  await invokeCommand(commands.acknowledgeWebContentRecovery());
}

export type HydratedAgentDraft = Omit<
  AgentDraftProjection,
  'designOutput' | 'artifactBundle' | 'modelManifest'
> & {
  designOutput: DesignOutput;
  artifactBundle: ArtifactBundle;
  modelManifest: ModelManifest;
};

export async function getAgentDraftPreview(
  threadId: string,
  previewId: string,
): Promise<HydratedAgentDraft> {
  const draft = await invokeCommand(commands.getAgentDraftPreview(threadId, previewId));
  const projected = structuredClone(draft);
  await hydrateBoundedTopology(
    draft.edgeCount,
    draft.faceCount,
    draft.selectionTargetCount,
    async (kind, cursor) => invokeCommand(
      commands.getAgentDraftTopologyPage(threadId, previewId, kind, cursor, 500),
    ),
    projected,
  );
  return {
    ...projected,
    designOutput: normalizeDesignOutput(projected.designOutput),
    artifactBundle: normalizeArtifactBundle(projected.artifactBundle),
    modelManifest: normalizeModelManifest(projected.modelManifest),
  };
}

export async function saveLastDesign(snapshot: LastDesignSnapshot | null): Promise<void> {
  await invokeCommand(commands.saveLastDesign(snapshot ? toContractLastDesignSnapshot(snapshot) : null));
}

export async function getActiveAgentSessions(): Promise<AgentSession[]> {
  return invokeCommand(commands.getActiveAgentSessions());
}

export async function getMcpServerStatus(): Promise<McpServerStatus> {
  return invokeCommand(commands.getMcpServerStatus());
}

export async function getAgentTerminalSnapshots(): Promise<AgentTerminalSnapshot[]> {
  return invokeCommand(commands.getAgentTerminalSnapshots());
}

export async function startCaptureSession(
  target: ExistingCaptureTarget | null,
): Promise<CaptureSessionInfo> {
  return invokeCommand(commands.startCaptureSession(target));
}

export async function listCaptureRuns(threadId: string): Promise<CaptureRun[]> {
  return invokeCommand(commands.listCaptureRuns(threadId));
}

export async function reopenCaptureRun(runId: string): Promise<ReopenedCaptureRun> {
  return invokeCommand(commands.reopenCaptureRun(runId));
}

export async function adoptLatestCaptureRun(
  target: ExistingCaptureTarget | null,
): Promise<ReopenedCaptureRun> {
  return invokeCommand(commands.adoptLatestCaptureRun(target));
}

export async function saveCapturePreviewSettings(
  runId: string,
  cropBounds: import('./contracts').CaptureCropBounds | null,
  previewScale: number,
): Promise<void> {
  await invokeCommand(commands.saveCapturePreviewSettings(runId, cropBounds, previewScale));
}

export async function ensureCaptureReconstructionGuide(
  runId: string,
): Promise<EnsureCaptureReconstructionGuideResult> {
  return invokeCommand(commands.ensureCaptureReconstructionGuide(runId));
}

export async function applyCaptureGuideEditIntent(
  input: ApplyCaptureGuideEditInput,
): Promise<ApplyCaptureGuideEditResult> {
  return invokeCommand(commands.applyCaptureGuideEdit(input));
}

export async function validateCaptureGuideIntent(
  input: ValidateCaptureGuideIntentInput,
): Promise<ValidateCaptureGuideIntentResult> {
  return invokeCommand(commands.validateCaptureGuideIntent(input));
}

export async function queueCaptureGuidedReconstruction(
  runId: string,
  expectedGuideRevision: number,
  expectedTargetSourceDigest: string,
): Promise<QueuedCaptureGuidedReconstruction> {
  return invokeCommand(commands.queueCaptureGuidedReconstruction(
    runId,
    expectedGuideRevision,
    expectedTargetSourceDigest,
  ));
}

export async function showSafeSaveDialog(
  defaultPath: string,
  filterName: string,
  extensions: string[],
): Promise<string | null> {
  return invokeCommand(commands.safeSaveDialog(defaultPath, filterName, extensions));
}

export async function getCaptureSessionStatus(token: string): Promise<CaptureSessionInfo | null> {
  return invokeCommand(commands.getCaptureSessionStatus(token));
}

export async function cancelCaptureSession(token: string): Promise<CaptureSessionInfo> {
  return invokeCommand(commands.cancelCaptureSession(token));
}

export async function resumeCaptureSession(token: string): Promise<CaptureSessionInfo> {
  return invokeCommand(commands.resumeCaptureSession(token));
}

export async function retryCaptureReconstruction(token: string): Promise<CaptureSessionInfo> {
  return invokeCommand(commands.retryCaptureReconstruction(token));
}

export async function prepareCapturePreview(
  token: string,
  cropBounds: import('./contracts').CaptureCropBounds | null,
): Promise<{
  artifactBundle: ArtifactBundle;
  modelManifest: ModelManifest;
}> {
  const prepared = await invokeCommand(commands.prepareCapturePreview(token, cropBounds));
  return {
    artifactBundle: normalizeArtifactBundle(prepared.artifactBundle),
    modelManifest: normalizeModelManifest(prepared.modelManifest),
  };
}

export async function runFemStudyIntent(
  input: FemRunIntentInput,
): Promise<FemRunIntentResponse> {
  return invokeCommand(commands.runFemStudyIntent(input));
}

export async function validateFemStudyIntent(
  input: FemRunIntentInput,
): Promise<FemStudyValidationResponse> {
  return invokeCommand(commands.validateFemStudyIntent(input));
}

export async function previewFemMeshIntent(
  input: FemRunIntentInput,
): Promise<FemMeshPreviewIntentResponse> {
  return invokeCommand(commands.previewFemMeshIntent(input));
}

export async function runFemConvergenceIntent(
  input: FemConvergenceIntentInput,
): Promise<FemConvergenceResponse> {
  return invokeCommand(commands.runFemConvergenceIntent(input));
}

export async function getCachedFemConvergenceIntent(
  input: FemConvergenceIntentInput,
): Promise<FemConvergenceResponse | null> {
  return invokeCommand(commands.getCachedFemConvergenceIntent(input));
}

export async function cancelFemStudy(jobId: string): Promise<FemCancelResponse> {
  return invokeCommand(commands.cancelFemStudy(jobId));
}

export async function exportFemResultVtuIntent(
  input: FemVtuExportIntentInput,
  targetPath: string,
): Promise<FemVtuExportResponse> {
  return invokeCommand(commands.exportFemResultVtuIntent(input, targetPath));
}

export async function sendAgentTerminalInput(input: AgentTerminalInput): Promise<void> {
  await invokeCommand(
    commands.sendAgentTerminalInput({
      agentId: input.agentId,
      text: input.text ?? '',
      key: input.key ?? null,
      ctrl: input.ctrl ?? false,
      alt: input.alt ?? false,
      shift: input.shift ?? false,
      meta: input.meta ?? false,
      submit: input.submit ?? false,
    }),
  );
}

export async function resizeAgentTerminal(
  agentId: string,
  cols: number,
  rows: number,
): Promise<void> {
  await invokeCommand(commands.resizeAgentTerminal(agentId, cols, rows));
}

export async function resolveAgentConfirm(requestId: string, choice: string) {
  await invokeCommand(commands.resolveAgentConfirm(requestId, choice));
}

export async function preparePromptAttachments(
  attachments: Attachment[],
): Promise<Attachment[]> {
  if (attachments.length === 0) {
    return [];
  }
  return invokeCommand(
    commands.preparePromptAttachments(attachments.map(toContractAttachment)),
    (value) => value.map(normalizeAttachment),
  );
}

export async function preparePromptWorkspaceCapture(input: {
  dataUrl: string;
  threadId?: string | null;
  name?: string | null;
  explanation?: string | null;
}): Promise<Attachment> {
  return invokeCommand(
    commands.preparePromptWorkspaceCapture({
      dataUrl: input.dataUrl,
      threadId: input.threadId ?? null,
      name: input.name ?? null,
      explanation: input.explanation ?? null,
    }),
    normalizeAttachment,
  );
}

export async function submitAgentPromptReply(input: {
  requestId: string;
  threadId: string;
  promptText: string;
  attachments: Attachment[];
}): Promise<SubmitAgentPromptReplyResult> {
  return invokeCommand(
    commands.submitAgentPromptReply({
      requestId: input.requestId,
      threadId: input.threadId,
      promptText: input.promptText,
      attachments: input.attachments.map(toContractAttachment),
    } as SubmitAgentPromptReplyInput),
  );
}

export async function queueAgentPrompt(input: {
  threadId?: string | null;
  promptText: string;
  attachments: Attachment[];
}): Promise<{ threadId: string; messageId: string }> {
  return invokeCommand(
    commands.queueAgentPrompt({
      threadId: input.threadId ?? null,
      promptText: input.promptText,
      attachments: input.attachments.map(toContractAttachment),
    } as QueueAgentPromptInput),
  );
}

export async function resolveAgentViewportScreenshot(input: {
  requestId: string;
  dataUrl: string;
  width: number;
  height: number;
  camera: ViewportCameraState;
  source: string;
  threadId: string;
  messageId: string;
  modelId?: string | null;
  includeOverlays: boolean;
}) {
  await invokeCommand(commands.resolveAgentViewportScreenshot(input as ResolveViewportScreenshotInput));
}

export async function rejectAgentViewportScreenshot(requestId: string, error: string) {
  await invokeCommand(
    commands.rejectAgentViewportScreenshot({
      requestId,
      error,
    } as RejectViewportScreenshotInput),
  );
}

export async function getAgentActivity(afterCursor: number | null): Promise<AgentActivityCatchUp> {
  return invokeCommand(commands.getAgentActivity(afterCursor));
}

export async function getAppLogs(): Promise<AppLogEntry[]> {
  return invokeCommand(commands.getAppLogs());
}

export async function getThreadWindowLayout(threadId: string): Promise<ThreadWindowLayout | null> {
  return invokeCommand(commands.getThreadWindowLayout(threadId));
}

export async function saveThreadWindowLayout(threadId: string, layout: ThreadWindowLayout): Promise<void> {
  await invokeCommand(commands.saveThreadWindowLayout(threadId, layout));
}

export type { AppLogEntry };
export type { VisualVerificationResult };
export type { StructuralVerificationResult };

export async function startCodexVoice(input: CodexVoiceStartInput): Promise<CodexVoiceConnection> {
  return invokeCommand(commands.startCodexVoice(input));
}

export async function stopCodexVoice(input: CodexVoiceStopInput): Promise<void> {
  await invokeCommand(commands.stopCodexVoice(input));
}
