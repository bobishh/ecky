import { commands, type AppError, type Result, type ThreadAgentState } from './contracts';
import {
  normalizeAgentDraft,
  normalizeArtifactBundle,
  normalizeConfig,
  normalizeDeletedMessage,
  normalizeDesignOutput,
  normalizeLastDesignSnapshot,
  normalizeModelManifest,
  normalizeParsedParamsResult,
  normalizeThread,
  normalizeUsageSummary,
  toContractAttachment,
  toContractDesignOutput,
  toContractLastDesignSnapshot,
  toContractUsageSummary,
  toContractUiSpec,
  type AgentSession,
  type ArtifactBundle,
  type AppConfig,
  type Attachment,
  type DeletedMessage,
  type DesignOutput,
  type DesignParams,
  type FinalizeStatus,
  type GenerateOutput,
  type IntentDecision,
  type LastDesignSnapshot,
  type ModelManifest,
  type McpServerStatus,
  type ParsedParamsResult,
  type Thread,
  type UiSpec,
  type UsageSummary,
} from '../types/domain';

export type { ThreadAgentState };

function unwrapResult<T>(result: Result<T, AppError>): T {
  if (result.status === 'ok') {
    return result.data;
  }
  throw result.error;
}

export function isBackendError(error: unknown): error is AppError {
  return Boolean(
    error &&
      typeof error === 'object' &&
      'code' in error &&
      'message' in error &&
      typeof (error as { message?: unknown }).message === 'string',
  );
}

export function formatBackendError(error: unknown): string {
  if (isBackendError(error)) {
    return error.details ? `${error.message}\n${error.details}` : error.message;
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
  return normalizeConfig(unwrapResult(await commands.getConfig()));
}

export async function checkFreecad(): Promise<boolean> {
  return unwrapResult(await commands.checkFreecad());
}

export async function saveConfig(config: AppConfig): Promise<void> {
  unwrapResult(await commands.saveConfig(config));
}

export async function getSystemPrompt(): Promise<string> {
  return unwrapResult(await commands.getSystemPrompt());
}

export async function listModels(
  provider: string,
  apiKey: string,
  baseUrl: string,
): Promise<string[]> {
  return unwrapResult(await commands.listModels(provider, apiKey, baseUrl));
}

export async function getHistory(): Promise<Thread[]> {
  return unwrapResult(await commands.getHistory()).map(normalizeThread);
}

export async function getThread(id: string): Promise<Thread> {
  return normalizeThread(unwrapResult(await commands.getThread(id)));
}

export async function clearHistory(): Promise<void> {
  unwrapResult(await commands.clearHistory());
}

export async function deleteThread(id: string): Promise<void> {
  unwrapResult(await commands.deleteThread(id));
}

export async function renameThread(id: string, title: string): Promise<void> {
  unwrapResult(await commands.renameThread(id, title));
}

export async function deleteVersion(messageId: string): Promise<void> {
  unwrapResult(await commands.deleteVersion(messageId));
}

export async function restoreVersion(messageId: string): Promise<void> {
  unwrapResult(await commands.restoreVersion(messageId));
}

export async function getDeletedMessages(): Promise<DeletedMessage[]> {
  return unwrapResult(await commands.getDeletedMessages()).map(normalizeDeletedMessage);
}

export async function hideDeletedMessage(messageId: string): Promise<void> {
  unwrapResult(await commands.hideDeletedMessage(messageId));
}

export async function finalizeThread(id: string): Promise<void> {
  unwrapResult(await commands.finalizeThread(id));
}

export async function reopenThread(id: string): Promise<void> {
  unwrapResult(await commands.reopenThread(id));
}

export async function getInventory(): Promise<Thread[]> {
  return unwrapResult(await commands.getInventory()).map(normalizeThread);
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
}): Promise<GenerateOutput> {
  const result = unwrapResult(
    await commands.generateDesign(
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
  return unwrapResult(
    await commands.initGenerationAttempt(
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
  unwrapResult(
    await commands.finalizeGenerationAttempt(
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

export async function classifyIntent(input: {
  prompt: string;
  threadId: string | null;
  context: string | null;
  imageData: string | null;
  attachments: Attachment[];
}): Promise<IntentDecision> {
  const result = unwrapResult(
    await commands.classifyIntent(
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

export async function renderStl(macroCode: string, parameters: DesignParams): Promise<string> {
  return unwrapResult(await commands.renderStl(macroCode, parameters));
}

export async function renderModel(
  macroCode: string,
  parameters: DesignParams,
): Promise<ArtifactBundle> {
  return normalizeArtifactBundle(unwrapResult(await commands.renderModel(macroCode, parameters)));
}

export async function importFcstd(sourcePath: string): Promise<ArtifactBundle> {
  return normalizeArtifactBundle(unwrapResult(await commands.importFcstd(sourcePath)));
}

export async function applyImportedModel(
  artifactBundle: ArtifactBundle,
  manifest: ModelManifest,
  parameters: DesignParams,
  messageId?: string | null,
): Promise<ArtifactBundle> {
  return normalizeArtifactBundle(
    unwrapResult(
      await commands.applyImportedModel(
        artifactBundle,
        manifest,
        parameters,
        messageId ?? null,
      ),
    ),
  );
}

export async function getModelManifest(modelId: string): Promise<ModelManifest> {
  return normalizeModelManifest(unwrapResult(await commands.getModelManifest(modelId)));
}

export async function saveModelManifest(
  modelId: string,
  manifest: ModelManifest,
  messageId?: string | null,
): Promise<void> {
  unwrapResult(await commands.saveModelManifest(modelId, manifest, messageId ?? null));
}

export async function getDefaultMacro(): Promise<string> {
  return unwrapResult(await commands.getDefaultMacro());
}

export async function getMessStlPath(): Promise<string> {
  return unwrapResult(await commands.getMessStlPath());
}

export async function exportFile(sourcePath: string, targetPath: string): Promise<void> {
  unwrapResult(await commands.exportFile(sourcePath, targetPath));
}

export async function addManualVersion(input: {
  threadId: string;
  title: string;
  versionName: string;
  macroCode: string;
  parameters: DesignParams;
  uiSpec: UiSpec;
  artifactBundle?: ArtifactBundle | null;
  modelManifest?: ModelManifest | null;
}): Promise<string> {
  return unwrapResult(
    await commands.addManualVersion(
      input.threadId,
      input.title,
      input.versionName,
      input.macroCode,
      input.parameters,
      toContractUiSpec(input.uiSpec),
      input.artifactBundle ?? null,
      input.modelManifest ?? null,
    ),
  );
}

export async function addImportedModelVersion(input: {
  threadId: string;
  title: string;
  artifactBundle: ArtifactBundle;
  modelManifest: ModelManifest;
}): Promise<string> {
  return unwrapResult(
    await commands.addImportedModelVersion(
      input.threadId,
      input.title,
      input.artifactBundle,
      input.modelManifest,
    ),
  );
}

export async function updateUiSpec(messageId: string, uiSpec: UiSpec): Promise<void> {
  unwrapResult(await commands.updateUiSpec(messageId, toContractUiSpec(uiSpec)));
}

export async function updateParameters(
  messageId: string,
  parameters: DesignParams,
): Promise<void> {
  unwrapResult(await commands.updateParameters(messageId, parameters));
}

export async function updateVersionRuntime(
  messageId: string,
  artifactBundle: ArtifactBundle,
  modelManifest: ModelManifest,
): Promise<void> {
  unwrapResult(await commands.updateVersionRuntime(messageId, artifactBundle, modelManifest));
}

export async function parseMacroParams(macroCode: string): Promise<ParsedParamsResult> {
  return normalizeParsedParamsResult(await commands.parseMacroParams(macroCode));
}

export async function uploadAsset(input: {
  sourcePath: string;
  name: string;
  format: string;
}) {
  return unwrapResult(await commands.uploadAsset(input.sourcePath, input.name, input.format));
}

export async function saveRecordedAudio(input: { base64Data: string; name: string }) {
  return unwrapResult(await commands.saveRecordedAudio(input.base64Data, input.name));
}

export async function getLastDesign(): Promise<LastDesignSnapshot | null> {
  return normalizeLastDesignSnapshot(unwrapResult(await commands.getLastDesign()));
}

export async function saveLastDesign(snapshot: LastDesignSnapshot | null): Promise<void> {
  unwrapResult(
    await commands.saveLastDesign(snapshot ? toContractLastDesignSnapshot(snapshot) : null),
  );
}

export async function getActiveAgentSessions(): Promise<AgentSession[]> {
  return unwrapResult(await commands.getActiveAgentSessions());
}

export async function getMcpServerStatus(): Promise<McpServerStatus> {
  return unwrapResult(await commands.getMcpServerStatus());
}

export async function getAgentDraft(
  threadId: string,
  baseMessageId: string,
) {
  return normalizeAgentDraft(unwrapResult(await commands.getAgentDraft(threadId, baseMessageId)));
}

export async function deleteAgentDraft(
  threadId: string,
  baseMessageId: string,
) {
  unwrapResult(await commands.deleteAgentDraft(threadId, baseMessageId));
}

export async function resolveAgentConfirm(requestId: string, choice: string) {
  unwrapResult(await commands.resolveAgentConfirm(requestId, choice));
}

export async function resolveAgentPrompt(requestId: string, promptText: string) {
  unwrapResult(await commands.resolveAgentPrompt(requestId, promptText));
}

export async function getThreadAgentState(threadId: string): Promise<ThreadAgentState> {
  return unwrapResult(await commands.getThreadAgentState(threadId));
}
