import type * as Contract from '../tauri/contracts.js';

export type BackendAppError = Contract.AppError;
export type ParamValue = Contract.ParamValue;
export type SelectValue = Contract.SelectValue;
export type SelectOption = Contract.SelectOption;
export type MessageRole = Contract.MessageRole;
export type MessageStatus = Contract.MessageStatus;
export type InteractionMode = Contract.InteractionMode;
export type FinalizeStatus = Contract.FinalizeStatus;
export type UsageSegment = Contract.UsageSegment;
export type UsageSummary = Contract.UsageSummary;
export type EngineConfig = Contract.Engine;
export type AssetConfig = Contract.Asset;
export type GenieEyeStyle = Contract.EyeStyle;
export type IntentDecision = Contract.IntentDecision;
export type GenerateOutput = {
  design: DesignOutput;
  threadId: string;
  messageId: string;
  usage: UsageSummary | null;
};

export type DesignParams = Record<string, ParamValue>;

export type RangeField = {
  type: 'range';
  key: string;
  label: string;
  min?: number;
  max?: number;
  step?: number;
  minFrom?: string;
  maxFrom?: string;
  frozen: boolean;
};

export type NumberField = {
  type: 'number';
  key: string;
  label: string;
  min?: number;
  max?: number;
  step?: number;
  minFrom?: string;
  maxFrom?: string;
  frozen: boolean;
};

export type SelectField = {
  type: 'select';
  key: string;
  label: string;
  options: SelectOption[];
  frozen: boolean;
};

export type CheckboxField = {
  type: 'checkbox';
  key: string;
  label: string;
  frozen: boolean;
};

export type UiField = RangeField | NumberField | SelectField | CheckboxField;
export type ResolvedUiField = UiField & { _auto?: boolean };

export interface UiSpec {
  fields: UiField[];
}

export interface DesignOutput {
  title: string;
  versionName: string;
  response: string;
  interactionMode: InteractionMode;
  macroCode: string;
  uiSpec: UiSpec;
  initialParams: DesignParams;
}

export interface Message {
  id: string;
  role: MessageRole;
  content: string;
  status: MessageStatus;
  output?: DesignOutput | null;
  usage?: UsageSummary | null;
  artifactBundle?: ArtifactBundle | null;
  modelManifest?: ModelManifest | null;
  imageData?: string | null;
  attachmentImages?: string[];
  timestamp: number;
}

export interface GenieTraits {
  version: number;
  seed: number;
  colorHue: number;
  vertexCount: number;
  radiusBase: number;
  stretchY: number;
  asymmetry: number;
  chordSkip: number;
  jitterScale: number;
  pulseScale: number;
  hoverScale: number;
  warpScale: number;
  glowHueShift: number;
  eyeStyle: GenieEyeStyle;
  eyeSpacing: number;
  eyeSize: number;
  mouthCurve: number;
  thinkingBias: number;
  repairBias: number;
  renderBias: number;
  expressiveness: number;
}

export interface Thread {
  id: string;
  title: string;
  summary: string;
  messages: Message[];
  updatedAt: number;
  versionCount: number;
  pendingCount: number;
  errorCount: number;
  genieTraits?: GenieTraits | null;
}

export interface DeletedMessage {
  id: string;
  threadId: string;
  threadTitle: string;
  role: MessageRole;
  content: string;
  output?: DesignOutput | null;
  usage?: UsageSummary | null;
  artifactBundle?: ArtifactBundle | null;
  modelManifest?: ModelManifest | null;
  timestamp: number;
  imageData?: string | null;
  attachmentImages?: string[];
  deletedAt: number;
}

export interface MicrowaveConfig {
  humId: string | null;
  dingId: string | null;
  muted: boolean;
}

export interface AppConfig {
  engines: EngineConfig[];
  selectedEngineId: string;
  freecadCmd: string;
  assets: AssetConfig[];
  microwave: MicrowaveConfig | null;
}

export type ModelSourceKind = Contract.ModelSourceKind;
export type ViewerAssetFormat = Contract.ViewerAssetFormat;
export type SelectionTargetKind = Contract.SelectionTargetKind;
export type EnrichmentStatus = Contract.EnrichmentStatus;
export type ControlPrimitiveKind = Contract.ControlPrimitiveKind;
export type ControlRelationMode = Contract.ControlRelationMode;
export type ControlViewScope = Contract.ControlViewScope;
export type ControlViewSource = Contract.ControlViewSource;
export type AdvisorySeverity = Contract.AdvisorySeverity;
export type AdvisoryCondition = Contract.AdvisoryCondition;
export type ViewerAsset = Contract.ViewerAsset;
export type ArtifactBundle = Contract.ArtifactBundle;
export type ManifestBounds = Contract.ManifestBounds;
export type DocumentMetadata = Contract.DocumentMetadata;
export type PartBinding = Contract.PartBinding;
export type ParameterGroup = Contract.ParameterGroup;
export type SelectionTarget = Contract.SelectionTarget;
export type EnrichmentProposal = Contract.EnrichmentProposal;
export type PrimitiveBinding = Contract.PrimitiveBinding;
export type ControlPrimitive = Contract.ControlPrimitive;
export type ControlRelation = Contract.ControlRelation;
export type ControlViewSection = Contract.ControlViewSection;
export type ControlView = Contract.ControlView;
export type Advisory = Contract.Advisory;
export type ManifestEnrichmentState = Contract.ManifestEnrichmentState;
export type ModelManifest = Contract.ModelManifest;

export interface LastDesignSnapshot {
  design: DesignOutput | null;
  threadId: string | null;
  messageId: string | null;
  artifactBundle: ArtifactBundle | null;
  modelManifest: ModelManifest | null;
  selectedPartId: string | null;
}

export interface ParsedParamsResult {
  fields: UiField[];
  params: DesignParams;
}

export interface Attachment {
  path: string;
  name: string;
  explanation: string;
  type: 'image' | 'cad' | string;
}

export type RequestPhase =
  | 'classifying'
  | 'answering'
  | 'generating'
  | 'queued_for_render'
  | 'rendering'
  | 'committing'
  | 'repairing'
  | 'success'
  | 'error'
  | 'canceled';

export interface RequestResult {
  design: DesignOutput | null;
  threadId: string;
  messageId: string;
  stlUrl: string;
  artifactBundle: ArtifactBundle | null;
  modelManifest: ModelManifest | null;
}

export interface Request {
  id: string;
  prompt: string;
  attachments: Attachment[];
  createdAt: number;
  phase: RequestPhase;
  attempt: number;
  maxAttempts: number;
  isQuestion: boolean;
  lightResponse: string;
  screenshot: string | null;
  threadId: string | null;
  result: RequestResult | null;
  error: string | null;
  cookingStartTime: number | null;
  cookingElapsed: number;
}

function optionalNumber(value: number | null | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function optionalString(value: string | null | undefined): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

export function normalizeUsageSummary(
  usage: Contract.UsageSummary | UsageSummary | null | undefined,
): UsageSummary | null {
  if (!usage || typeof usage !== 'object') {
    return null;
  }

  return {
    inputTokens: typeof usage.inputTokens === 'number' ? usage.inputTokens : 0,
    outputTokens: typeof usage.outputTokens === 'number' ? usage.outputTokens : 0,
    totalTokens: typeof usage.totalTokens === 'number' ? usage.totalTokens : 0,
    cachedInputTokens:
      typeof usage.cachedInputTokens === 'number' ? usage.cachedInputTokens : 0,
    reasoningTokens:
      typeof usage.reasoningTokens === 'number' ? usage.reasoningTokens : 0,
    estimatedCostUsd:
      typeof usage.estimatedCostUsd === 'number' ? usage.estimatedCostUsd : null,
    segments: Array.isArray(usage.segments)
      ? usage.segments.map((segment) => ({
          stage: segment.stage,
          provider: segment.provider,
          model: segment.model,
          inputTokens: typeof segment.inputTokens === 'number' ? segment.inputTokens : 0,
          outputTokens: typeof segment.outputTokens === 'number' ? segment.outputTokens : 0,
          totalTokens: typeof segment.totalTokens === 'number' ? segment.totalTokens : 0,
          cachedInputTokens:
            typeof segment.cachedInputTokens === 'number' ? segment.cachedInputTokens : 0,
          reasoningTokens:
            typeof segment.reasoningTokens === 'number' ? segment.reasoningTokens : 0,
          estimatedCostUsd:
            typeof segment.estimatedCostUsd === 'number' ? segment.estimatedCostUsd : null,
        }))
      : [],
  };
}

function normalizeParamValue(value: unknown): ParamValue | undefined {
  if (
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean' ||
    value === null
  ) {
    return value;
  }
  return undefined;
}

export function normalizeDesignParams(params: unknown): DesignParams {
  if (!params || typeof params !== 'object' || Array.isArray(params)) {
    return {};
  }
  const normalized: DesignParams = {};
  for (const [key, value] of Object.entries(params as Record<string, unknown>)) {
    const param = normalizeParamValue(value);
    if (param !== undefined) {
      normalized[key] = param;
    }
  }
  return normalized;
}

export function normalizeUiField(field: Contract.UiField | UiField | unknown): UiField | null {
  if (!field || typeof field !== 'object') {
    return null;
  }

  const raw = field as Partial<Contract.UiField> & Record<string, unknown>;
  const key = typeof raw.key === 'string' ? raw.key : '';
  if (!key) {
    return null;
  }
  const label = typeof raw.label === 'string' && raw.label.trim() ? raw.label : key;
  const frozen = Boolean(raw.frozen ?? raw.freezed);

  switch (raw.type) {
    case 'range':
      return {
        type: 'range',
        key,
        label,
        min: optionalNumber(raw.min as number | null | undefined),
        max: optionalNumber(raw.max as number | null | undefined),
        step: optionalNumber(raw.step as number | null | undefined),
        minFrom: optionalString((raw.minFrom ?? raw.min_from) as string | null | undefined),
        maxFrom: optionalString((raw.maxFrom ?? raw.max_from) as string | null | undefined),
        frozen,
      };
    case 'number':
      return {
        type: 'number',
        key,
        label,
        min: optionalNumber(raw.min as number | null | undefined),
        max: optionalNumber(raw.max as number | null | undefined),
        step: optionalNumber(raw.step as number | null | undefined),
        minFrom: optionalString((raw.minFrom ?? raw.min_from) as string | null | undefined),
        maxFrom: optionalString((raw.maxFrom ?? raw.max_from) as string | null | undefined),
        frozen,
      };
    case 'select':
      return {
        type: 'select',
        key,
        label,
        options: Array.isArray(raw.options) ? [...raw.options] : [],
        frozen,
      };
    case 'checkbox':
      return {
        type: 'checkbox',
        key,
        label,
        frozen,
      };
    default:
      return null;
  }
}

export function normalizeUiSpec(uiSpec: Contract.UiSpec | UiSpec | unknown): UiSpec {
  if (!uiSpec || typeof uiSpec !== 'object') {
    return { fields: [] };
  }
  const raw = uiSpec as Partial<Contract.UiSpec> & { fields?: unknown[] };
  const fields = Array.isArray(raw.fields)
    ? raw.fields
        .map((field: unknown) => normalizeUiField(field))
        .filter((field: UiField | null): field is UiField => field !== null)
    : [];
  return { fields };
}

export function normalizeDesignOutput(
  output: Contract.DesignOutput | DesignOutput | null | undefined,
): DesignOutput {
  const legacy = (output ?? {}) as Partial<Contract.DesignOutput> & Record<string, unknown>;
  return {
    title: (output?.title ?? legacy.title) as string | undefined ?? 'Untitled Design',
    versionName:
      (output?.versionName ?? (legacy.version_name as string | undefined)) ?? 'V1',
    response: (output?.response ?? legacy.response) as string | undefined ?? '',
    interactionMode:
      (output?.interactionMode ??
        (legacy.interaction_mode as InteractionMode | undefined)) ?? 'design',
    macroCode: (output?.macroCode ?? (legacy.macro_code as string | undefined)) ?? '',
    uiSpec: normalizeUiSpec(output?.uiSpec ?? legacy.ui_spec),
    initialParams: normalizeDesignParams(output?.initialParams ?? legacy.initial_params),
  };
}

export function normalizeMessage(message: Contract.Message | Message): Message {
  const legacy = message as Contract.Message & Record<string, unknown>;
  return {
    id: message.id,
    role: message.role,
    content: message.content,
    status: message.status,
    output: message.output ? normalizeDesignOutput(message.output) : null,
    usage: normalizeUsageSummary(message.usage),
    artifactBundle:
      message.artifactBundle || legacy.artifact_bundle
        ? normalizeArtifactBundle(
            (message.artifactBundle ?? legacy.artifact_bundle) as ArtifactBundle,
          )
        : null,
    modelManifest:
      message.modelManifest || legacy.model_manifest
        ? normalizeModelManifest(
            (message.modelManifest ?? legacy.model_manifest) as ModelManifest,
          )
        : null,
    imageData: message.imageData ?? null,
    attachmentImages: Array.isArray(message.attachmentImages)
      ? [...message.attachmentImages]
      : Array.isArray(legacy.attachment_images)
        ? [...(legacy.attachment_images as string[])]
        : [],
    timestamp: message.timestamp,
  };
}

export function normalizeGenieTraits(
  traits: Contract.GenieTraits | GenieTraits | null | undefined,
): GenieTraits | null {
  if (!traits) return null;
  return {
    version: traits.version ?? 2,
    seed: traits.seed,
    colorHue: traits.colorHue,
    vertexCount: traits.vertexCount,
    radiusBase: traits.radiusBase,
    stretchY: traits.stretchY,
    asymmetry: traits.asymmetry,
    chordSkip: traits.chordSkip,
    jitterScale: traits.jitterScale,
    pulseScale: traits.pulseScale,
    hoverScale: traits.hoverScale,
    warpScale: traits.warpScale,
    glowHueShift: traits.glowHueShift,
    eyeStyle: traits.eyeStyle,
    eyeSpacing: traits.eyeSpacing,
    eyeSize: traits.eyeSize,
    mouthCurve: traits.mouthCurve,
    thinkingBias: traits.thinkingBias,
    repairBias: traits.repairBias,
    renderBias: traits.renderBias,
    expressiveness: traits.expressiveness,
  };
}

export function normalizeThread(thread: Contract.Thread | Thread): Thread {
  const legacy = thread as Contract.Thread & Record<string, unknown>;
  const genieTraits =
    thread.genieTraits ?? (legacy.genie_traits as GenieTraits | undefined) ?? null;
  return {
    id: thread.id,
    title: thread.title,
    summary: thread.summary ?? '',
    messages: Array.isArray(thread.messages) ? thread.messages.map(normalizeMessage) : [],
    updatedAt: thread.updatedAt ?? (legacy.updated_at as number | undefined) ?? 0,
    versionCount: thread.versionCount ?? (legacy.version_count as number | undefined) ?? 0,
    pendingCount: thread.pendingCount ?? (legacy.pending_count as number | undefined) ?? 0,
    errorCount: thread.errorCount ?? (legacy.error_count as number | undefined) ?? 0,
    genieTraits: genieTraits ? normalizeGenieTraits(genieTraits) : null,
  };
}

export function normalizeDeletedMessage(
  message: Contract.DeletedMessage | DeletedMessage,
): DeletedMessage {
  const legacy = message as Contract.DeletedMessage & Record<string, unknown>;
  return {
    id: message.id,
    threadId: message.threadId,
    threadTitle: message.threadTitle,
    role: message.role,
    content: message.content,
    output: message.output ? normalizeDesignOutput(message.output) : null,
    usage: normalizeUsageSummary(message.usage),
    artifactBundle:
      message.artifactBundle || legacy.artifact_bundle
        ? normalizeArtifactBundle(
            (message.artifactBundle ?? legacy.artifact_bundle) as ArtifactBundle,
          )
        : null,
    modelManifest:
      message.modelManifest || legacy.model_manifest
        ? normalizeModelManifest(
            (message.modelManifest ?? legacy.model_manifest) as ModelManifest,
          )
        : null,
    timestamp: message.timestamp,
    imageData: message.imageData ?? null,
    attachmentImages: Array.isArray(message.attachmentImages)
      ? [...message.attachmentImages]
      : Array.isArray(legacy.attachment_images)
        ? [...(legacy.attachment_images as string[])]
        : [],
    deletedAt: message.deletedAt,
  };
}

export function normalizeConfig(config: Contract.Config | AppConfig): AppConfig {
  const legacy = config as Contract.Config & Record<string, unknown>;
  return {
    engines: Array.isArray(config.engines) ? [...config.engines] : [],
    selectedEngineId:
      config.selectedEngineId ?? (legacy.selected_engine_id as string | undefined) ?? '',
    freecadCmd: typeof config.freecadCmd === 'string' ? config.freecadCmd : '',
    assets: Array.isArray(config.assets) ? [...config.assets] : [],
    microwave: config.microwave
      ? {
          humId: config.microwave.humId ?? null,
          dingId: config.microwave.dingId ?? null,
          muted: Boolean(config.microwave.muted),
        }
      : null,
  };
}

export function normalizeLastDesignSnapshot(
  snapshot: Contract.LastDesignSnapshot | LastDesignSnapshot | unknown,
): LastDesignSnapshot | null {
  if (!snapshot) {
    return null;
  }
  if (Array.isArray(snapshot)) {
    const [design, threadId] = snapshot as [unknown, unknown];
    return {
      design: normalizeDesignOutput(design as DesignOutput),
      threadId: typeof threadId === 'string' ? threadId : null,
      messageId: null,
      artifactBundle: null,
      modelManifest: null,
      selectedPartId: null,
    };
  }
  const legacy = snapshot as Partial<Contract.LastDesignSnapshot> & Record<string, unknown>;
  const artifactBundle =
    legacy.artifactBundle || legacy.artifact_bundle
      ? normalizeArtifactBundle(
          (legacy.artifactBundle ?? legacy.artifact_bundle) as ArtifactBundle,
        )
      : null;
  const modelManifest =
    legacy.modelManifest || legacy.model_manifest
      ? normalizeModelManifest(
          (legacy.modelManifest ?? legacy.model_manifest) as ModelManifest,
        )
      : null;
  if (!legacy.design && !artifactBundle && !modelManifest) {
    return null;
  }
  return {
    design: legacy.design ? normalizeDesignOutput(legacy.design as DesignOutput) : null,
    threadId:
      (legacy.threadId as string | null | undefined) ??
      (legacy.thread_id as string | null | undefined) ??
      null,
    messageId:
      (legacy.messageId as string | null | undefined) ??
      (legacy.message_id as string | null | undefined) ??
      null,
    artifactBundle,
    modelManifest,
    selectedPartId:
      (legacy.selectedPartId as string | null | undefined) ??
      (legacy.selected_part_id as string | null | undefined) ??
      null,
  };
}

export function normalizeParsedParamsResult(
  result: Contract.ParsedParamsResult | ParsedParamsResult,
): ParsedParamsResult {
  return {
    fields: Array.isArray(result.fields)
      ? result.fields
          .map((field: unknown) => normalizeUiField(field))
          .filter((field: UiField | null): field is UiField => field !== null)
      : [],
    params: normalizeDesignParams(result.params),
  };
}

export function normalizeArtifactBundle(
  bundle: Contract.ArtifactBundle | ArtifactBundle,
): ArtifactBundle {
  return {
    ...bundle,
    viewerAssets: Array.isArray(bundle.viewerAssets) ? [...bundle.viewerAssets] : [],
  };
}

export function normalizeModelManifest(
  manifest: Contract.ModelManifest | ModelManifest,
): ModelManifest {
  return {
    ...manifest,
    parts: Array.isArray(manifest.parts) ? [...manifest.parts] : [],
    parameterGroups: Array.isArray(manifest.parameterGroups)
      ? [...manifest.parameterGroups]
      : [],
    controlPrimitives: Array.isArray(manifest.controlPrimitives)
      ? [...manifest.controlPrimitives]
      : [],
    controlRelations: Array.isArray((manifest as Contract.ModelManifest).controlRelations)
      ? [...((manifest as Contract.ModelManifest).controlRelations || [])]
      : [],
    controlViews: Array.isArray(manifest.controlViews)
      ? [...manifest.controlViews]
      : [],
    advisories: Array.isArray(manifest.advisories) ? [...manifest.advisories] : [],
    selectionTargets: Array.isArray(manifest.selectionTargets)
      ? [...manifest.selectionTargets]
      : [],
    warnings: Array.isArray(manifest.warnings) ? [...manifest.warnings] : [],
    enrichmentState: {
      status: manifest.enrichmentState?.status ?? 'none',
      proposals: Array.isArray(manifest.enrichmentState?.proposals)
        ? [...manifest.enrichmentState.proposals]
        : [],
    },
  };
}

export function toContractAttachment(attachment: Attachment): Contract.Attachment {
  return {
    path: attachment.path,
    name: attachment.name,
    explanation: attachment.explanation,
    kind: attachment.type === 'image' ? 'image' : 'cad',
  };
}

export function toContractUiField(field: UiField): Contract.UiField {
  switch (field.type) {
    case 'range':
      return {
        type: 'range',
        key: field.key,
        label: field.label,
        min: field.min,
        max: field.max,
        step: field.step,
        minFrom: field.minFrom,
        maxFrom: field.maxFrom,
        frozen: field.frozen,
      };
    case 'number':
      return {
        type: 'number',
        key: field.key,
        label: field.label,
        min: field.min,
        max: field.max,
        step: field.step,
        minFrom: field.minFrom,
        maxFrom: field.maxFrom,
        frozen: field.frozen,
      };
    case 'select':
      return {
        type: 'select',
        key: field.key,
        label: field.label,
        options: field.options,
        frozen: field.frozen,
      };
    case 'checkbox':
      return {
        type: 'checkbox',
        key: field.key,
        label: field.label,
        frozen: field.frozen,
      };
  }
}

export function toContractUiSpec(uiSpec: UiSpec): Contract.UiSpec {
  return {
    fields: uiSpec.fields.map(toContractUiField),
  };
}

export function toContractDesignOutput(output: DesignOutput): Contract.DesignOutput {
  return {
    title: output.title,
    versionName: output.versionName,
    response: output.response,
    interactionMode: output.interactionMode,
    macroCode: output.macroCode,
    uiSpec: toContractUiSpec(output.uiSpec),
    initialParams: output.initialParams,
  };
}

export function toContractUsageSummary(
  usage: UsageSummary | null | undefined,
): Contract.UsageSummary | null {
  if (!usage) {
    return null;
  }

  return {
    inputTokens: usage.inputTokens,
    outputTokens: usage.outputTokens,
    totalTokens: usage.totalTokens,
    cachedInputTokens: usage.cachedInputTokens,
    reasoningTokens: usage.reasoningTokens,
    estimatedCostUsd: usage.estimatedCostUsd ?? null,
    segments: Array.isArray(usage.segments)
      ? usage.segments.map((segment) => ({
          stage: segment.stage,
          provider: segment.provider,
          model: segment.model,
          inputTokens: segment.inputTokens,
          outputTokens: segment.outputTokens,
          totalTokens: segment.totalTokens,
          cachedInputTokens: segment.cachedInputTokens,
          reasoningTokens: segment.reasoningTokens,
          estimatedCostUsd: segment.estimatedCostUsd ?? null,
        }))
      : [],
  };
}

export function toContractLastDesignSnapshot(
  snapshot: LastDesignSnapshot,
): Contract.LastDesignSnapshot {
  return {
    design: snapshot.design ? toContractDesignOutput(snapshot.design) : null,
    threadId: snapshot.threadId,
    messageId: snapshot.messageId,
    artifactBundle: snapshot.artifactBundle,
    modelManifest: snapshot.modelManifest,
    selectedPartId: snapshot.selectedPartId,
  };
}
