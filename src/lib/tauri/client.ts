import { commands, type AppError, type AppLogEntry, type Result, type ThreadAgentState, type ThreadWindowLayout } from './contracts';
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
  toContractUsageSummary,
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
  type EngineKind,
  type FinalizeStatus,
  type GeometryBackend,
  type GenerateOutput,
  type IntentDecision,
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
import type {
  ComponentPackage,
  ComponentPackageHeader,
  BrepHiddenLineProjectionRequest,
  BrepHiddenLineProjectionResponse,
  ClearSketchPreviewDraftRequest,
  ExportPartInput,
  FreecadLibraryImportRequest,
  FreecadLibraryItem,
  FreecadLibrarySearchRequest,
  InstalledComponentPackage,
  LoadSketchPreviewDraftRequest,
  PostProcessingSpec,
  PromptTranscription,
  QueueAgentPromptInput,
  RejectViewportScreenshotInput,
  ResolveAgentPromptInput,
  ResolveViewportScreenshotInput,
  SketchAcceptedBrepComponentPackageRequest,
  SketchBrepCandidateRequest,
  SketchBrepCandidateAcceptRequest,
  SketchBrepCandidateAcceptResponse,
  SketchBrepCandidateResponse,
  SketchDraftRequest,
  SketchDraftSource,
  SaveSketchPreviewDraftRequest,
  SketchPreviewDraft,
  SketchPreviewHullRequest,
  SketchSuggestionRequest,
  SketchSuggestionResponse,
  TranscribePromptAudioInput,
} from './contracts';

export type { ThreadAgentState };

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

export async function getRuntimeCapabilities(): Promise<RuntimeCapabilities> {
  return invokeCommand(commands.getRuntimeCapabilities(), normalizeRuntimeCapabilities);
}

export async function saveConfig(config: AppConfig): Promise<void> {
  await invokeCommand(commands.saveConfig(config));
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

export async function getHistory(): Promise<Thread[]> {
  return invokeCommand(commands.getHistory(), (threads) => threads.map(normalizeThread));
}

export async function getThread(id: string): Promise<Thread> {
  return invokeCommand(commands.getThread(id), normalizeThread);
}

export async function getThreadLatestVersion(threadId: string): Promise<Message | null> {
  const message = await invokeCommand(commands.getThreadLatestVersion(threadId));
  return message ? normalizeMessage(message) : null;
}

export async function getThreadMessageVersion(
  threadId: string,
  messageId: string,
): Promise<Message | null> {
  const message = await invokeCommand(commands.getThreadMessageVersion(threadId, messageId));
  return message ? normalizeMessage(message) : null;
}

export async function getThreadMessagesPage(
  threadId: string,
  before: number | null = null,
  limit = 50,
  includeVisualPayloads = false,
): Promise<ThreadMessagesPage> {
  return invokeCommand(
    commands.getThreadMessagesPage(threadId, before, limit, includeVisualPayloads),
    normalizeThreadMessagesPage,
  );
}

export async function deleteThread(id: string): Promise<void> {
  await invokeCommand(commands.deleteThread(id));
}

export async function renameThread(id: string, title: string): Promise<void> {
  await invokeCommand(commands.renameThread(id, title));
}

export async function deleteVersion(messageId: string): Promise<void> {
  await invokeCommand(commands.deleteVersion(messageId));
}

export async function restoreVersion(messageId: string): Promise<void> {
  await invokeCommand(commands.restoreVersion(messageId));
}

export async function getDeletedMessages(): Promise<DeletedMessage[]> {
  return invokeCommand(commands.getDeletedMessages(), (messages) =>
    messages.map(normalizeDeletedMessage),
  );
}

export async function hideDeletedMessage(messageId: string): Promise<void> {
  await invokeCommand(commands.hideDeletedMessage(messageId));
}

export async function finalizeThread(id: string, messageId: string | null = null): Promise<void> {
  await invokeCommand(commands.finalizeThread(id, messageId));
}

export async function reopenThread(id: string): Promise<void> {
  await invokeCommand(commands.reopenThread(id));
}

export async function getInventory(): Promise<Thread[]> {
  return invokeCommand(commands.getInventory(), (threads) => threads.map(normalizeThread));
}

export async function generateDesign(input: {
  prompt: string;
  threadId: string | null;
  parentMacroCode: string | null;
  workingDesign: DesignOutput | null;
  isRetry: boolean;
  imageData: string | null;
  attachments: Attachment[];
  questionMode: boolean | null;
  followUpQuestion: string | null;
  engineKind?: EngineKind | null;
  sourceLanguage?: SourceLanguage | null;
  geometryBackend?: GeometryBackend | null;
}): Promise<GenerateOutput> {
  const result = await invokeCommand(
    commands.generateDesign(
      input.prompt,
      input.threadId,
      input.parentMacroCode,
      input.workingDesign ? toContractDesignOutput(input.workingDesign) : null,
      input.isRetry,
      input.imageData,
      input.attachments.map(toContractAttachment),
      {
        questionMode: input.questionMode,
        followUpQuestion: input.followUpQuestion,
        engineKind: input.engineKind ?? null,
        sourceLanguage: input.sourceLanguage ?? null,
        geometryBackend: input.geometryBackend ?? null,
      },
    ),
  );
  return {
    design: normalizeDesignOutput(result.design),
    threadId: result.threadId,
    messageId: result.messageId,
    usage: normalizeUsageSummary(result.usage),
  };
}

export async function initGenerationAttempt(input: {
  threadId: string;
  prompt: string;
  attachments: Attachment[];
  imageData: string | null;
}): Promise<string> {
  return invokeCommand(
    commands.initGenerationAttempt(
      input.threadId,
      input.prompt,
      input.attachments.map(toContractAttachment),
      input.imageData,
    ),
  );
}

export async function finalizeGenerationAttempt(input: {
  messageId: string;
  status: FinalizeStatus;
  design?: DesignOutput;
  usage?: UsageSummary | null;
  artifactBundle?: ArtifactBundle | null;
  modelManifest?: ModelManifest | null;
  errorMessage?: string;
  responseText?: string;
}): Promise<void> {
  await invokeCommand(
    commands.finalizeGenerationAttempt(
      input.messageId,
      input.status,
      input.design ? toContractDesignOutput(input.design) : null,
      toContractUsageSummary(input.usage),
      input.artifactBundle ?? null,
      input.modelManifest ?? null,
      input.errorMessage ?? null,
      input.responseText ?? null,
    ),
  );
}

export async function persistStructuralVerification(
  messageId: string,
  structuralVerification: StructuralVerificationResult,
): Promise<void> {
  await invokeCommand(
    commands.persistStructuralVerification(messageId, structuralVerification),
  );
}

export async function classifyIntent(input: {
  prompt: string;
  threadId: string | null;
  context: string | null;
  imageData: string | null;
  attachments: Attachment[];
}): Promise<IntentDecision> {
  const result = await invokeCommand(
    commands.classifyIntent(
      input.prompt,
      input.threadId,
      input.context,
      input.imageData,
      input.attachments.map(toContractAttachment),
    ),
  );
  return {
    ...result,
    usage: normalizeUsageSummary(result.usage),
  };
}

export type { MacroAstSourceNode } from './contracts';

export async function openProjectInEditor(
  threadId: string | null,
  messageId: string | null,
): Promise<import('./contracts').ProjectEditorLink> {
  return invokeCommand(commands.openProjectInEditor(threadId, messageId));
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
  return invokeCommand(
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
}

export type { PostProcessingSpec };

export async function importFcstd(sourcePath: string): Promise<ArtifactBundle> {
  return invokeCommand(commands.importFcstd(sourcePath), normalizeArtifactBundle);
}

export async function searchFreecadLibrary(
  request: FreecadLibrarySearchRequest,
): Promise<FreecadLibraryItem[]> {
  return invokeCommand(commands.searchFreecadLibrary(request));
}

export async function importFreecadLibraryPart(
  request: FreecadLibraryImportRequest,
): Promise<ArtifactBundle> {
  return invokeCommand(commands.importFreecadLibraryPart(request), normalizeArtifactBundle);
}

export async function applyImportedModel(
  artifactBundle: ArtifactBundle,
  manifest: ModelManifest,
  parameters: DesignParams,
  messageId?: string | null,
): Promise<ArtifactBundle> {
  return invokeCommand(
    commands.applyImportedModel(artifactBundle, manifest, parameters, messageId ?? null),
    normalizeArtifactBundle,
  );
}

export async function getModelManifest(modelId: string): Promise<ModelManifest> {
  return invokeCommand(commands.getModelManifest(modelId), normalizeModelManifest);
}

export async function saveModelManifest(
  modelId: string,
  manifest: ModelManifest,
  messageId?: string | null,
): Promise<void> {
  await invokeCommand(commands.saveModelManifest(modelId, manifest, messageId ?? null));
}

export async function getDefaultMacro(): Promise<string> {
  return invokeCommand(commands.getDefaultMacro());
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

export async function installComponentPackageArchive(
  archivePath: string,
): Promise<InstalledComponentPackage> {
  return invokeCommand(commands.installComponentPackageArchive(archivePath));
}

export async function listInstalledComponentPackageHeaders(): Promise<ComponentPackageHeader[]> {
  return invokeCommand(commands.listInstalledComponentPackageHeaders());
}

export async function suggestSketchFeatures(
  request: SketchSuggestionRequest,
): Promise<SketchSuggestionResponse> {
  return invokeCommand(commands.suggestSketchFeatures(request));
}

export async function generateSketchDraftPreview(
  request: SketchDraftRequest,
): Promise<{ draft: SketchDraftSource; artifactBundle: ArtifactBundle }> {
  const [draft, bundle] = await invokeCommand(commands.generateSketchDraftPreview(request));
  return { draft, artifactBundle: normalizeArtifactBundle(bundle) };
}

export async function generateSketchPreviewHull(
  request: SketchPreviewHullRequest,
): Promise<{ draft: SketchDraftSource; artifactBundle: ArtifactBundle }> {
  const [draft, bundle] = await invokeCommand(commands.generateSketchPreviewHull(request));
  return { draft, artifactBundle: normalizeArtifactBundle(bundle) };
}

export async function saveSketchPreviewDraft(input: {
  scopeId?: string | null;
  draftScopeId?: string | null;
  draftSource: SketchDraftSource;
  artifactBundle: ArtifactBundle;
}): Promise<SketchPreviewDraft> {
  const scopeId = resolveSketchPreviewDraftScopeId(input);
  return invokeCommand(
    commands.saveSketchPreviewDraft({
      scopeId,
      draftSource: input.draftSource,
      artifactBundle: input.artifactBundle,
    } satisfies SaveSketchPreviewDraftRequest),
  );
}

export async function loadSketchPreviewDraft(input: {
  scopeId?: string | null;
  draftScopeId?: string | null;
}): Promise<SketchPreviewDraft | null> {
  const scopeId = resolveSketchPreviewDraftScopeId(input);
  return invokeCommand(
    commands.loadSketchPreviewDraft({
      scopeId,
    } satisfies LoadSketchPreviewDraftRequest),
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

export async function analyzeSketchBrepCandidates(
  request: SketchBrepCandidateRequest,
): Promise<SketchBrepCandidateResponse> {
  return invokeCommand(commands.analyzeSketchBrepCandidates(request));
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

export async function extractBrepHiddenLineProjections(
  request: BrepHiddenLineProjectionRequest,
): Promise<BrepHiddenLineProjectionResponse> {
  return invokeCommand(commands.extractBrepHiddenLineProjections(request));
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

export async function addManualVersion(input: {
  threadId: string;
  title: string;
  versionName: string;
  macroCode: string;
  sourceLanguage?: SourceLanguage | null;
  geometryBackend?: GeometryBackend | null;
  parameters: DesignParams;
  uiSpec: UiSpec;
  postProcessing?: PostProcessingSpec | null;
  artifactBundle?: ArtifactBundle | null;
  modelManifest?: ModelManifest | null;
}): Promise<string> {
  return invokeCommand(
    commands.addManualVersion({
      threadId: input.threadId,
      title: input.title,
      versionName: input.versionName,
      macroCode: input.macroCode,
      sourceLanguage: input.sourceLanguage ?? null,
      geometryBackend: input.geometryBackend ?? null,
      parameters: input.parameters,
      uiSpec: toContractUiSpec(input.uiSpec),
      postProcessing: input.postProcessing ?? null,
      artifactBundle: input.artifactBundle ?? null,
      modelManifest: input.modelManifest ?? null,
    }),
  );
}

export async function addImportedModelVersion(input: {
  threadId: string;
  title: string;
  artifactBundle: ArtifactBundle;
  modelManifest: ModelManifest;
}): Promise<string> {
  return invokeCommand(
    commands.addImportedModelVersion(
      input.threadId,
      input.title,
      input.artifactBundle,
      input.modelManifest,
    ),
  );
}

export async function updateUiSpec(messageId: string, uiSpec: UiSpec): Promise<void> {
  await invokeCommand(commands.updateUiSpec(messageId, toContractUiSpec(uiSpec)));
}

export async function updateParameters(
  messageId: string,
  parameters: DesignParams,
): Promise<void> {
  await invokeCommand(commands.updateParameters(messageId, parameters));
}

export async function updateVersionRuntime(
  messageId: string,
  artifactBundle: ArtifactBundle,
  modelManifest: ModelManifest,
): Promise<void> {
  await invokeCommand(commands.updateVersionRuntime(messageId, artifactBundle, modelManifest));
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

export async function getMessageAttachments(messageId: string): Promise<Attachment[]> {
  return invokeCommand(commands.getMessageAttachments(messageId), (value) =>
    value.map(normalizeAttachment),
  );
}

export async function resolveAgentPrompt(input: {
  requestId: string;
  promptText: string;
  messageIds?: string[];
  messageId?: string | null;
  attachments: Attachment[];
}) {
  await invokeCommand(
    commands.resolveAgentPrompt({
      requestId: input.requestId,
      promptText: input.promptText,
      messageIds: input.messageIds ?? [],
      messageId: input.messageId ?? null,
      attachments: input.attachments.map(toContractAttachment),
    } as ResolveAgentPromptInput),
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

export async function getThreadAgentState(threadId: string): Promise<ThreadAgentState> {
  return invokeCommand(commands.getThreadAgentState(threadId));
}

export async function getAppLogs(): Promise<AppLogEntry[]> {
  return invokeCommand(commands.getAppLogs());
}

export async function wakePrimaryAutoAgent(
  threadId?: string | null,
  messageId?: string | null,
  modelId?: string | null,
): Promise<void> {
  await invokeCommand(commands.wakePrimaryAutoAgent(threadId ?? null, messageId ?? null, modelId ?? null));
}

export async function stopPrimaryAutoAgent(
  threadId?: string | null,
  messageId?: string | null,
  modelId?: string | null,
): Promise<void> {
  await invokeCommand(commands.stopPrimaryAutoAgent(threadId ?? null, messageId ?? null, modelId ?? null));
}

export async function restartPrimaryAutoAgent(
  threadId?: string | null,
  messageId?: string | null,
  modelId?: string | null,
): Promise<void> {
  await invokeCommand(commands.restartPrimaryAutoAgent(threadId ?? null, messageId ?? null, modelId ?? null));
}

export async function verifyRender(
  originalPrompt: string,
  screenshots: string[],
  referenceImagePaths: string[] = [],
  structuralSummary: string | null = null,
): Promise<VisualVerificationResult> {
  return invokeCommand(commands.verifyRender(originalPrompt, screenshots, referenceImagePaths, structuralSummary));
}

export async function verifyGeneratedModel(
  modelId: string,
  originalPrompt: string,
): Promise<StructuralVerificationResult> {
  return invokeCommand(commands.verifyGeneratedModel(modelId, originalPrompt));
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
