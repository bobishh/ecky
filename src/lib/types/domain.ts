import type * as Contract from '../tauri/contracts.js';

export type BackendAppError = Contract.AppError;
export type ParamValue = Contract.ParamValue;
export type SelectValue = Contract.SelectValue;
export type SelectOption = Contract.SelectOption;
export type MessageRole = Contract.MessageRole;
export type MessageStatus = Contract.MessageStatus;
export type InteractionMode = Contract.InteractionMode;
export type MessageVisualKind = Contract.MessageVisualKind;
export type MacroDialect = Contract.MacroDialect;
export type SourceLanguage = Contract.SourceLanguage;
export type GeometryBackend = Contract.GeometryBackend;
export type EngineKind = Contract.EngineKind;
export type FinalizeStatus = Contract.FinalizeStatus;
export type UsageSegment = Contract.UsageSegment;
export type UsageSummary = Contract.UsageSummary;
export type EngineConfig = Contract.Engine; // includes enabled: boolean
export type AssetConfig = Contract.Asset;
export type GenieEyeStyle = Contract.EyeStyle;
export type IntentDecision = Contract.IntentDecision;
export type VisualVerificationResult = Contract.VisualVerificationResult;
export type VisualIssue = Contract.VisualIssue;
export type VisualIssueCategory = Contract.VisualIssueCategory;
export type StructuralVerificationResult = Contract.StructuralVerificationResult;
export type StructuralIssue = Contract.StructuralIssue;
export type StructuralMetrics = Contract.StructuralMetrics;
export type VerifierStatus = Contract.VerifierStatus;
export type VerifierSource = Contract.VerifierSource;
export type AgentOrigin = Contract.AgentOrigin;
export type AgentSession = Contract.AgentSession;
export type AgentTerminalSnapshot = Contract.AgentTerminalSnapshot;
export type AgentTerminalInput = Contract.AgentTerminalInput;
export type McpServerStatus = Contract.McpServerStatus;
export type ViewportCameraState = Contract.ViewportCameraState;
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

export type ImageField = {
  type: 'image';
  key: string;
  label: string;
  frozen: boolean;
};

export type UiField = RangeField | NumberField | SelectField | CheckboxField | ImageField;
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
  macroDialect?: MacroDialect;
  engineKind?: EngineKind;
  sourceLanguage: SourceLanguage;
  geometryBackend: GeometryBackend;
  uiSpec: UiSpec;
  initialParams: DesignParams;
  postProcessing?: PostProcessingSpec | null;
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
  agentOrigin?: AgentOrigin | null;
  imageData?: string | null;
  visualKind?: MessageVisualKind | null;
  attachmentImages?: string[];
  timestamp: number;
  deletedAt?: number | null;
}

export interface ThreadMessagesPage {
  messages: Message[];
  nextBefore: number | null;
  hasMore: boolean;
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

export type ThreadStatus = 'active' | 'finalized';

export interface Thread {
  id: string;
  title: string;
  summary: string;
  messages: Message[];
  updatedAt: number;
  versionCount: number;
  pendingCount: number;
  queuedCount: number;
  errorCount: number;
  genieTraits?: GenieTraits | null;
  status?: ThreadStatus;
  finalizedAt?: number | null;
  pendingConfirm?: string | null;
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
  agentOrigin?: AgentOrigin | null;
  timestamp: number;
  imageData?: string | null;
  visualKind?: MessageVisualKind | null;
  attachmentImages?: string[];
  deletedAt: number;
}

export interface RuntimeBackendCapability {
  available: boolean;
  detail: string;
  path?: string | null;
}

export interface RuntimeAuthoringContext {
  engineKind: EngineKind;
  sourceLanguage: SourceLanguage;
  geometryBackend: GeometryBackend;
}

export interface RuntimeCapabilities {
  freecad: RuntimeBackendCapability;
  build123d: RuntimeBackendCapability;
  directOcct: RuntimeBackendCapability;
  mesh: RuntimeBackendCapability;
  recommendedAuthoringContext: RuntimeAuthoringContext;
}

export interface MicrowaveConfig {
  humId: string | null;
  dingId: string | null;
  muted: boolean;
}

export interface AutoAgent {
  id: string;
  label: string;
  cmd: string;
  model?: string | null;
  args: string[];
  enabled: boolean;
  startOnDemand?: boolean;
}

export type McpMode = 'passive' | 'active';

export interface McpConfig {
  port: number | null;
  maxSessions: number | null;
  mode: McpMode;
  primaryAgentId: string | null;
  promptTimeoutSecs: number;
  eckyAstAuthoring: boolean;
  autoAgents: AutoAgent[];
}

export interface VoiceConfig {
  sttLanguageCode: string;
}

export interface AppConfig {
  engines: EngineConfig[];
  selectedEngineId: string;
  freecadCmd: string;
  assets: AssetConfig[];
  microwave: MicrowaveConfig | null;
  voice: VoiceConfig;
  mcp: McpConfig;
  hasSeenOnboarding: boolean;
  connectionType?: string | null;
  defaultEngineKind: EngineKind;
  defaultSourceLanguage: SourceLanguage;
  defaultGeometryBackend: GeometryBackend;
  maxGenerationAttempts: number;
  maxVerifyAttempts: number;
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
export type ViewerEdgePoint = Contract.ViewerEdgePoint;
export type ViewerEdgeTarget = Contract.ViewerEdgeTarget;
export type CalloutAnchor = Contract.CalloutAnchor;
export type MeasurementGuideKind = Contract.MeasurementGuideKind;
export type MeasurementGuide = Contract.MeasurementGuide;
export type MeasurementBasis = Contract.MeasurementBasis;
export type MeasurementAxis = Contract.MeasurementAxis;
export type MeasurementAnnotationSource = Contract.MeasurementAnnotationSource;
export type MeasurementAnnotation = Contract.MeasurementAnnotation;
export type ArtifactBundle = Contract.ArtifactBundle;
export type ExportArtifact = Contract.ExportArtifact;
export type PostProcessingSpec = Contract.PostProcessingSpec;
export type ProjectionType = Contract.ProjectionType;
export type OverflowMode = Contract.OverflowMode;
export type LithophanePlacementMode = Contract.LithophanePlacementMode;
export type LithophaneSide = Contract.LithophaneSide;
export type LithophaneColorMode = Contract.LithophaneColorMode;
export type LithophaneAttachmentSource = Contract.LithophaneAttachmentSource;
export type LithophanePlacement = Contract.LithophanePlacement;
export type LithophaneRelief = Contract.LithophaneRelief;
export type LithophaneColor = Contract.LithophaneColor;
export type LithophaneAttachment = Contract.LithophaneAttachment;
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
  dataUrl?: string | null;
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
  maxVerifyAttempts: number;
  isQuestion: boolean;
  lightResponse: string;
  screenshot: string | null;
  threadId: string | null;
  baseMessageId?: string | null;
  baseModelId?: string | null;
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

function isBuild123dCompat(value: unknown): boolean {
  return value === 'build123d' || value === 'build123D';
}

function isEckyCompat(value: unknown): boolean {
  return value === 'ecky' || value === 'eckyIrV0' || value === 'ecky_ir_v0';
}

function normalizeEngineKindValue(value: unknown): EngineKind | undefined {
  switch (value) {
    case 'freecad':
      return 'freecad';
    case 'build123d':
    case 'build123D':
      return 'build123d';
    case 'ecky':
    case 'eckyIrV0':
    case 'ecky_ir_v0':
      return 'ecky';
    default:
      return undefined;
  }
}

function normalizeMacroDialectValue(value: unknown): MacroDialect | undefined {
  switch (value) {
    case 'legacy':
      return 'legacy';
    case 'cadFrameworkV1':
      return 'cadFrameworkV1';
    case 'build123d':
    case 'build123D':
      return 'build123d';
    case 'ecky':
    case 'eckyIrV0':
    case 'ecky_ir_v0':
      return 'ecky';
    default:
      return undefined;
  }
}

function normalizeSourceLanguageValue(
  value: unknown,
  engineKind?: unknown,
): SourceLanguage | undefined {
  switch (value) {
    case 'legacyPython':
    case 'legacy_python':
      return 'legacyPython';
    case 'ecky':
    case 'eckyIrV0':
    case 'ecky_ir_v0':
      if (engineKind === 'build123d' || engineKind === 'build123D') return 'build123d';
      if (engineKind === 'freecad') return 'legacyPython';
      return 'ecky';
    case 'build123d':
    case 'build123D':
      return 'build123d';
    default:
      if (engineKind === 'build123d' || engineKind === 'build123D') return 'build123d';
      if (isEckyCompat(engineKind)) {
        return 'ecky';
      }
      if (engineKind === 'freecad') return 'legacyPython';
      return undefined;
  }
}

function normalizeGeometryBackendValue(
  value: unknown,
  engineKind?: unknown,
): GeometryBackend | undefined {
  switch (value) {
    case 'freecad':
      return 'freecad';
    case 'build123d':
    case 'build123D':
      return 'build123d';
    case 'mesh':
    case 'eckyRust':
    case 'ecky_rust':
      return 'mesh';
    default:
      if (engineKind === 'build123d' || engineKind === 'build123D') return 'build123d';
      if (isEckyCompat(engineKind)) {
        return 'mesh';
      }
      if (engineKind === 'freecad') return 'freecad';
      return undefined;
  }
}

function slugifyLithophaneId(value: string): string {
  const slug = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  return slug || 'lithophane';
}

function legacyLithophaneAttachmentId(imageParam: string): string {
  return `legacy-${slugifyLithophaneId(imageParam)}`;
}

function normalizeLithophaneAttachmentSource(
  source: LithophaneAttachmentSource | Record<string, unknown> | null | undefined,
): LithophaneAttachmentSource | null {
  if (!source || typeof source !== 'object') return null;
  const kind = (source as { kind?: string }).kind;
  if (kind === 'file') {
    return {
      kind: 'file',
      imagePath:
        (source as { imagePath?: string }).imagePath ??
        (source as { image_path?: string }).image_path ??
        '',
    };
  }
  if (kind === 'param') {
    return {
      kind: 'param',
      imageParam:
        (source as { imageParam?: string }).imageParam ??
        (source as { image_param?: string }).image_param ??
        '',
    };
  }
  return null;
}

function normalizeLithophaneAttachment(
  attachment: LithophaneAttachment | Record<string, unknown> | null | undefined,
): LithophaneAttachment | null {
  if (!attachment || typeof attachment !== 'object') return null;
  const source = normalizeLithophaneAttachmentSource(
    (attachment as { source?: LithophaneAttachmentSource }).source,
  );
  if (!source) return null;
  const placement = (attachment as { placement?: LithophanePlacement }).placement;
  const relief = (attachment as { relief?: LithophaneRelief }).relief;
  const color = (attachment as { color?: LithophaneColor }).color;
  const inferredId =
    source.kind === 'param'
      ? legacyLithophaneAttachmentId(source.imageParam)
      : `litho-${slugifyLithophaneId(source.imagePath.split(/[/\\]/).pop() ?? '')}`;
  return {
    id: (attachment as { id?: string }).id ?? inferredId,
    enabled: (attachment as { enabled?: boolean }).enabled ?? true,
    source,
    targetPartId:
      (attachment as { targetPartId?: string }).targetPartId ??
      (attachment as { target_part_id?: string }).target_part_id ??
      '',
    placement: {
      mode: placement?.mode ?? 'partSidePatch',
      side: placement?.side ?? 'front',
      projection: placement?.projection ?? 'auto',
      widthMm: placement?.widthMm ?? 0,
      heightMm: placement?.heightMm ?? 0,
      offsetXMm: placement?.offsetXMm ?? 0,
      offsetYMm: placement?.offsetYMm ?? 0,
      rotationDeg: placement?.rotationDeg ?? 0,
      overflowMode: placement?.overflowMode ?? 'contain',
      bleedMarginMm: placement?.bleedMarginMm ?? 0,
    },
    relief: {
      depthMm: relief?.depthMm ?? 2,
      invert: relief?.invert ?? false,
    },
    color: {
      mode: color?.mode ?? 'mono',
      channelThicknessMm: color?.channelThicknessMm ?? 0.4,
    },
  };
}

function buildLegacyLithophaneAttachment(
  displacement: Contract.DisplacementSpec | null | undefined,
): LithophaneAttachment | null {
  if (!displacement?.imageParam?.trim()) return null;
  return {
    id: legacyLithophaneAttachmentId(displacement.imageParam),
    enabled: true,
    source: { kind: 'param', imageParam: displacement.imageParam },
    targetPartId: '',
    placement: {
      mode: 'partSidePatch',
      side: 'front',
      projection: displacement.projection ?? 'auto',
      widthMm: 0,
      heightMm: 0,
      offsetXMm: 0,
      offsetYMm: 0,
      rotationDeg: 0,
      overflowMode: 'contain',
      bleedMarginMm: 0,
    },
    relief: {
      depthMm: displacement.depthMm ?? 2,
      invert: displacement.invert ?? false,
    },
    color: {
      mode: 'mono',
      channelThicknessMm: 0.4,
    },
  };
}

export function normalizePostProcessing(
  postProcessing: PostProcessingSpec | Record<string, unknown> | null | undefined,
): PostProcessingSpec | null {
  if (!postProcessing) return null;
  const legacy = postProcessing as Record<string, unknown>;
  const displacement =
    (postProcessing as { displacement?: Contract.DisplacementSpec | null }).displacement ??
    (legacy.displacement as Contract.DisplacementSpec | null | undefined) ??
    null;
  const rawAttachments =
    ((postProcessing as { lithophaneAttachments?: LithophaneAttachment[] | null })
      .lithophaneAttachments ??
      (legacy.lithophane_attachments as LithophaneAttachment[] | undefined) ??
      []) || [];
  const attachments = rawAttachments
    .map((attachment) => normalizeLithophaneAttachment(attachment))
    .filter((attachment): attachment is LithophaneAttachment => attachment !== null);
  const legacyAttachment = buildLegacyLithophaneAttachment(displacement);
  if (legacyAttachment && !attachments.some((attachment) => attachment.id === legacyAttachment.id)) {
    attachments.unshift(legacyAttachment);
  }
  if (!displacement && attachments.length === 0) return null;
  return {
    displacement,
    lithophaneAttachments: attachments,
  };
}

export function hasActiveLithophaneAttachments(
  postProcessing: PostProcessingSpec | null | undefined,
): boolean {
  return (normalizePostProcessing(postProcessing)?.lithophaneAttachments ?? []).some(
    (attachment) => attachment.enabled !== false,
  );
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
    case 'image':
      return {
        type: 'image',
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
    macroDialect:
      normalizeMacroDialectValue(
        output?.macroDialect ?? (legacy.macro_dialect as MacroDialect | undefined),
      ) ?? 'legacy',
    engineKind:
      normalizeEngineKindValue(
        output?.engineKind ?? (legacy.engine_kind as EngineKind | undefined),
      ) ?? 'freecad',
    sourceLanguage:
      normalizeSourceLanguageValue(
        output?.sourceLanguage ?? (legacy.source_language as SourceLanguage | undefined),
        output?.engineKind ?? legacy.engine_kind,
      ) ??
      (isEckyCompat(output?.engineKind ?? legacy.engine_kind)
        ? 'ecky'
        : isBuild123dCompat(output?.engineKind ?? legacy.engine_kind)
          ? 'build123d'
          : 'legacyPython'),
    geometryBackend:
      normalizeGeometryBackendValue(
        output?.geometryBackend ?? (legacy.geometry_backend as GeometryBackend | undefined),
        output?.engineKind ?? legacy.engine_kind,
      ) ??
      (isEckyCompat(output?.engineKind ?? legacy.engine_kind)
        ? 'mesh'
        : isBuild123dCompat(output?.engineKind ?? legacy.engine_kind)
          ? 'build123d'
          : 'freecad'),
    uiSpec: normalizeUiSpec(output?.uiSpec ?? legacy.ui_spec),
    initialParams: normalizeDesignParams(output?.initialParams ?? legacy.initial_params),
    postProcessing: normalizePostProcessing(
      output?.postProcessing ??
        (legacy.post_processing as PostProcessingSpec | undefined) ??
        null,
    ),
  };
}

function healIrGeometryBackend(
  output: DesignOutput | null,
  artifactBundle: ArtifactBundle | null,
  modelManifest: ModelManifest | null,
): DesignOutput | null {
  const runtimeGeometryBackend =
    artifactBundle?.geometryBackend ?? modelManifest?.geometryBackend ?? null;
  if (
    output &&
    output.sourceLanguage === 'ecky' &&
    runtimeGeometryBackend === 'build123d' &&
    output.geometryBackend !== 'build123d'
  ) {
    return {
      ...output,
      geometryBackend: 'build123d',
    };
  }
  return output;
}

export function normalizeMessage(message: Contract.Message | Message): Message {
  const legacy = message as Contract.Message & Record<string, unknown>;
  const artifactBundle =
    message.artifactBundle || legacy.artifact_bundle
      ? normalizeArtifactBundle(
          (message.artifactBundle ?? legacy.artifact_bundle) as ArtifactBundle,
        )
      : null;
  const modelManifest =
    message.modelManifest || legacy.model_manifest
      ? normalizeModelManifest(
          (message.modelManifest ?? legacy.model_manifest) as ModelManifest,
        )
      : null;
  const output = healIrGeometryBackend(
    message.output ? normalizeDesignOutput(message.output) : null,
    artifactBundle,
    modelManifest,
  );
  return {
    id: message.id,
    role: message.role,
    content: message.content,
    status: message.status,
    output,
    usage: normalizeUsageSummary(message.usage),
    artifactBundle,
    modelManifest,
    agentOrigin: (message.agentOrigin ?? (legacy.agent_origin as AgentOrigin | undefined)) ?? null,
    imageData: message.imageData ?? null,
    visualKind:
      (message.visualKind ?? (legacy.visual_kind as MessageVisualKind | undefined)) ?? null,
    attachmentImages: Array.isArray(message.attachmentImages)
      ? [...message.attachmentImages]
      : Array.isArray(legacy.attachment_images)
        ? [...(legacy.attachment_images as string[])]
        : [],
    timestamp: message.timestamp,
  };
}

export function normalizeThreadMessagesPage(
  page: Contract.ThreadMessagesPage | ThreadMessagesPage,
): ThreadMessagesPage {
  return {
    messages: Array.isArray(page.messages) ? page.messages.map(normalizeMessage) : [],
    nextBefore: page.nextBefore ?? null,
    hasMore: Boolean(page.hasMore),
  };
}

export function normalizeRuntimeCapabilities(
  capabilities: Contract.RuntimeCapabilities | RuntimeCapabilities | unknown,
): RuntimeCapabilities {
  const raw = (capabilities ?? {}) as Partial<Contract.RuntimeCapabilities> &
    Record<string, unknown>;
  const rawBuild123d = (raw.build123d ??
    (raw as { build123D?: unknown }).build123D) as
    | Partial<Contract.RuntimeBackendCapability>
    | undefined;
  const rawMesh = (raw.mesh ?? (raw as { eckyRust?: unknown }).eckyRust) as
    | Partial<Contract.RuntimeBackendCapability>
    | undefined;
  const normalizeCapability = (
    capability: Partial<Contract.RuntimeBackendCapability> | undefined,
    fallbackDetail: string,
  ): RuntimeBackendCapability => ({
    available: Boolean(capability?.available),
    detail:
      (typeof capability?.detail === 'string' && capability.detail.trim()) ||
      fallbackDetail,
    path:
      typeof capability?.path === 'string'
        ? capability.path
        : capability?.path === null
          ? null
          : null,
  });

  const recommended = raw.recommendedAuthoringContext as
    | Partial<Contract.RuntimeAuthoringContext>
    | undefined;

  return {
    freecad: normalizeCapability(raw.freecad as Partial<Contract.RuntimeBackendCapability>, 'Unavailable'),
    build123d: normalizeCapability(rawBuild123d, 'Unavailable'),
    directOcct: normalizeCapability(
      raw.directOcct as Partial<Contract.RuntimeBackendCapability>,
      'Unavailable',
    ),
    mesh: normalizeCapability(rawMesh, 'bundled'),
    recommendedAuthoringContext: {
      engineKind: normalizeEngineKindValue(recommended?.engineKind) ?? 'ecky',
      sourceLanguage:
        normalizeSourceLanguageValue(
          recommended?.sourceLanguage,
          recommended?.engineKind,
        ) ?? 'ecky',
      geometryBackend:
        normalizeGeometryBackendValue(
          recommended?.geometryBackend,
          recommended?.engineKind,
        ) ?? 'mesh',
    },
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
    queuedCount: thread.queuedCount ?? (legacy.queued_count as number | undefined) ?? 0,
    errorCount: thread.errorCount ?? (legacy.error_count as number | undefined) ?? 0,
    genieTraits: genieTraits ? normalizeGenieTraits(genieTraits) : null,
    status: thread.status ?? (legacy.thread_status as ThreadStatus | undefined) ?? 'active',
    finalizedAt: thread.finalizedAt ?? (legacy.finalized_at as number | undefined) ?? null,
    pendingConfirm:
      thread.pendingConfirm ?? (legacy.pending_confirm as string | undefined) ?? null,
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
    agentOrigin: (message.agentOrigin ?? (legacy.agent_origin as AgentOrigin | undefined)) ?? null,
    timestamp: message.timestamp,
    imageData: message.imageData ?? null,
    visualKind:
      (message.visualKind ?? (legacy.visual_kind as MessageVisualKind | undefined)) ?? null,
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
  const rawVoice = (config as AppConfig).voice ?? (legacy.voice as VoiceConfig | undefined);
  const sttLanguageCode = `${rawVoice?.sttLanguageCode ?? 'en-US'}`.trim() || 'en-US';
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
    voice: {
      sttLanguageCode,
    },
    mcp: config.mcp
      ? {
          port: config.mcp.port ?? null,
          maxSessions: (config.mcp as any).maxSessions ?? null,
          mode:
            ((config.mcp as any).mode as McpMode | undefined) ??
            ((config.mcp as any).autoAgents?.length ? 'active' : 'passive'),
          primaryAgentId:
            (config.mcp as any).primaryAgentId ??
            (Array.isArray((config.mcp as any).autoAgents) && (config.mcp as any).autoAgents.length > 0
              ? (config.mcp as any).autoAgents.find((agent: AutoAgent) => agent.enabled)?.id ?? null
              : null),
          promptTimeoutSecs: Math.min(
            1800,
            Math.max(10, Number((config.mcp as any).promptTimeoutSecs ?? 1800) || 1800),
          ),
          eckyAstAuthoring: Boolean((config.mcp as any).eckyAstAuthoring),
          autoAgents: Array.isArray((config.mcp as any).autoAgents) ? [...(config.mcp as any).autoAgents] : [],
        }
      : {
          port: null,
          maxSessions: null,
          mode: 'passive',
          primaryAgentId: null,
          promptTimeoutSecs: 1800,
          eckyAstAuthoring: false,
          autoAgents: [],
        },
    hasSeenOnboarding: Boolean(config.hasSeenOnboarding ?? legacy.has_seen_onboarding),
    connectionType: (config as AppConfig).connectionType ?? null,
    defaultEngineKind:
      normalizeEngineKindValue(
        (config as AppConfig).defaultEngineKind ??
          (legacy.default_engine_kind as EngineKind | undefined),
      ) ?? 'freecad',
    defaultSourceLanguage:
      normalizeSourceLanguageValue(
        (config as AppConfig).defaultSourceLanguage ??
          (legacy.default_source_language as SourceLanguage | undefined),
        (config as AppConfig).defaultEngineKind ?? legacy.default_engine_kind,
      ) ??
      (isEckyCompat((config as AppConfig).defaultEngineKind ?? legacy.default_engine_kind)
        ? 'ecky'
        : isBuild123dCompat((config as AppConfig).defaultEngineKind ?? legacy.default_engine_kind)
          ? 'build123d'
          : 'legacyPython'),
    defaultGeometryBackend:
      normalizeGeometryBackendValue(
        (config as AppConfig).defaultGeometryBackend ??
          (legacy.default_geometry_backend as GeometryBackend | undefined),
        (config as AppConfig).defaultEngineKind ?? legacy.default_engine_kind,
      ) ??
      (isEckyCompat((config as AppConfig).defaultEngineKind ?? legacy.default_engine_kind)
        ? 'mesh'
        : isBuild123dCompat((config as AppConfig).defaultEngineKind ?? legacy.default_engine_kind)
          ? 'build123d'
          : 'freecad'),
    maxGenerationAttempts: Math.max(1, Number((config as AppConfig).maxGenerationAttempts ?? (legacy as any).max_generation_attempts ?? 3) || 3),
    maxVerifyAttempts: Math.max(0, Number((config as AppConfig).maxVerifyAttempts ?? (legacy as any).max_verify_attempts ?? 0) || 0),
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
    design: healIrGeometryBackend(
      legacy.design ? normalizeDesignOutput(legacy.design as DesignOutput) : null,
      artifactBundle,
      modelManifest,
    ),
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
    edgeTargets: Array.isArray(bundle.edgeTargets)
      ? bundle.edgeTargets.map((target) => ({
          ...target,
          durableTargetId:
            typeof target.durableTargetId === 'string' && target.durableTargetId.trim()
              ? target.durableTargetId
              : null,
          canonicalTargetId:
            typeof target.canonicalTargetId === 'string' && target.canonicalTargetId.trim()
              ? target.canonicalTargetId
              : null,
          aliasIds: Array.isArray(target.aliasIds) ? [...target.aliasIds] : [],
        }))
      : [],
    faceTargets: Array.isArray(bundle.faceTargets)
      ? bundle.faceTargets.map((target) => ({
          ...target,
          durableTargetId:
            typeof target.durableTargetId === 'string' && target.durableTargetId.trim()
              ? target.durableTargetId
              : null,
          canonicalTargetId:
            typeof target.canonicalTargetId === 'string' && target.canonicalTargetId.trim()
              ? target.canonicalTargetId
              : null,
          aliasIds: Array.isArray(target.aliasIds) ? [...target.aliasIds] : [],
        }))
      : [],
    exportArtifacts: Array.isArray(bundle.exportArtifacts) ? [...bundle.exportArtifacts] : [],
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
      ? manifest.controlPrimitives.map((primitive) => ({
          ...primitive,
          source:
            primitive.source ??
            (primitive.primitiveId?.startsWith('primitive-manual-') ? 'manual' : 'generated'),
        }))
      : [],
    controlRelations: Array.isArray((manifest as Contract.ModelManifest).controlRelations)
      ? [...((manifest as Contract.ModelManifest).controlRelations || [])]
      : [],
    controlViews: Array.isArray(manifest.controlViews)
      ? manifest.controlViews.map((view) => ({
          ...view,
          source: view.source ?? (view.viewId?.startsWith('view-manual-') ? 'manual' : 'generated'),
        }))
      : [],
    advisories: Array.isArray(manifest.advisories) ? [...manifest.advisories] : [],
    selectionTargets: Array.isArray(manifest.selectionTargets)
      ? manifest.selectionTargets.map((target) => ({
          ...target,
          durableTargetId:
            typeof target.durableTargetId === 'string' && target.durableTargetId.trim()
              ? target.durableTargetId
              : null,
          canonicalTargetId:
            typeof target.canonicalTargetId === 'string' && target.canonicalTargetId.trim()
              ? target.canonicalTargetId
              : null,
          aliasIds: Array.isArray(target.aliasIds) ? [...target.aliasIds] : [],
          parameterKeys: Array.isArray(target.parameterKeys) ? [...target.parameterKeys] : [],
          primitiveIds: Array.isArray(target.primitiveIds) ? [...target.primitiveIds] : [],
          viewIds: Array.isArray(target.viewIds) ? [...target.viewIds] : [],
        }))
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
    dataUrl: attachment.dataUrl ?? null,
    kind: attachment.type === 'image' ? 'image' : 'cad',
  };
}

export function normalizeAttachment(attachment: Contract.Attachment): Attachment {
  return {
    path: attachment.path,
    name: attachment.name,
    explanation: attachment.explanation,
    dataUrl: attachment.dataUrl ?? null,
    type: attachment.kind === 'image' ? 'image' : 'cad',
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
    case 'image':
      return {
        type: 'image',
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
    macroDialect: output.macroDialect ?? 'legacy',
    engineKind: output.engineKind,
    sourceLanguage: output.sourceLanguage,
    geometryBackend: output.geometryBackend,
    uiSpec: toContractUiSpec(output.uiSpec),
    initialParams: output.initialParams,
    postProcessing: output.postProcessing ?? null,
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
