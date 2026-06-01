<script lang="ts">
  import { get } from 'svelte/store';
  import { tick } from 'svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { uiHighlightStore } from './stores/uiHighlightStore';
  import { open } from '@tauri-apps/plugin-dialog';
  import {
    formatBackendError,
    getAppErrorDiagnosticContext,
    parseMacroParams,
    saveModelManifest,
    updateParameters,
    updateUiSpec,
  } from './tauri/client';
  import { buildImportedSyntheticDesign } from './modelRuntime/importedRuntime';
  import MacroAstMap from './MacroAstMap.svelte';
  import {
    filterFieldsBySearch,
    resolveContextSections,
    resolveTargetParameterKeys,
    type MeasurementControlFocus,
    type ContextSelectionTarget,
  } from './modelRuntime/contextualEditing';
  import { normalizePostProcessing } from './types/domain';
  import { type TopologyMode } from './viewerDisplayMode';
  import type {
    MaterializedSemanticControl,
    MaterializedSemanticView,
  } from './modelRuntime/semanticControls';
  import { persistLastSessionSnapshot } from './modelRuntime/sessionSnapshot';
  import { activeThreadIdStore as activeThreadId, historyStore as history } from './stores/domainState';
  import { refreshHistory } from './stores/history';
  import { liveApply } from './stores/paramPanelState';
  import ParamPanelToolbar from './components/ParamPanelToolbar.svelte';
  import ParamPanelModeTabs from './components/ParamPanelModeTabs.svelte';
  import ParamPanelEditFields from './components/ParamPanelEditFields.svelte';
  import ParamPanelImportedProposals from './components/ParamPanelImportedProposals.svelte';
  import ParamPanelLithophaneTab from './components/ParamPanelLithophaneTab.svelte';
  import ParamPanelRawTab from './components/ParamPanelRawTab.svelte';
  import ParamPanelViewsTab from './components/ParamPanelViewsTab.svelte';
  import { session } from './stores/sessionStore';
  import type {
    CheckboxField,
    ImageField,
    AdvisoryCondition,
    AdvisorySeverity,
    ControlPrimitive,
    ControlPrimitiveKind,
    ControlRelationMode,
    ControlView,
    ControlViewScope,
    ControlViewSource,
    ArtifactBundle,
    DesignParams,
    EnrichmentProposal,
    EnrichmentStatus,
    LithophaneAttachment,
    PostProcessingSpec,
    ProjectionType,
    NumberField,
    ParamValue,
    ParameterGroup,
    PartBinding,
    PrimitiveBinding,
    RangeField,
    ResolvedUiField,
    SelectField,
    ModelManifest,
    UiField,
    UiSpec,
  } from './types/domain';

  type EditableNumber = number | '' | undefined;
  type EditableRangeField = Omit<RangeField, 'min' | 'max' | 'step'> & {
    min?: EditableNumber;
    max?: EditableNumber;
    step?: EditableNumber;
    _auto?: boolean;
  };
  type EditableNumberField = Omit<NumberField, 'min' | 'max' | 'step'> & {
    min?: EditableNumber;
    max?: EditableNumber;
    step?: EditableNumber;
    _auto?: boolean;
  };
  type EditableSelectField = SelectField & { _auto?: boolean };
  type EditableCheckboxField = CheckboxField & { _auto?: boolean };
  type EditableImageField = ImageField & { _auto?: boolean };
  type EditableUiField =
    | EditableRangeField
    | EditableNumberField
    | EditableSelectField
    | EditableCheckboxField
    | EditableImageField;
  type RangeLikeField =
    | Extract<ResolvedUiField, { type: 'range' | 'number' }>
    | Extract<EditableUiField, { type: 'range' | 'number' }>;
  type CadTone = 'neutral' | 'size' | 'x' | 'y' | 'z' | 'angle' | 'state' | 'mode';
  type CadHint = {
    tone: CadTone;
    tag: string;
    glyph: string;
    note: string;
  };
  type ViewerMode = 'orbit' | 'select' | 'measure';
  type PrimitiveBindingDraft = {
    parameterKey: string;
    scale: string;
    offset: string;
    min: string;
    max: string;
  };
  const PARAM_UNDO_LIMIT = 50;

  let {
    uiSpec = $bindable(null),
    parameters = {},
    modelManifest = null,
    controlViews = [],
    activeControlViewId = null,
    selectedTarget = null,
    selectedPartId = null,
    searchQuery = $bindable(''),
    onSelectControlView,
    onSelectPart,
    onSemanticChange,
    onControlFocusChange,
    onchange,
    oncommit,
    onspecchange,
    onpostprocessingchange,
    onShowCode = undefined,
    onOpenInEditor = undefined,
    outlineEnabled = true,
    topologyMode = 'mesh',
    selectionMode = 'orbit',
    onViewerDisplayChange,
    onViewerSelectionModeChange,
    activeVersionId = null,
    messageId = null,
    macroCode = '',
    postProcessing = null,
    artifactBundle = null,
    onApplyMacroCode = undefined,
  }: {
    uiSpec?: UiSpec | null;
    parameters?: DesignParams;
    modelManifest?: ModelManifest | null;
    postProcessing?: PostProcessingSpec | null;
    artifactBundle?: ArtifactBundle | null;
    controlViews?: MaterializedSemanticView[];
    activeControlViewId?: string | null;
    selectedTarget?: ContextSelectionTarget | null;
    selectedPartId?: string | null;
    searchQuery?: string;
    onSelectControlView?: (viewId: string | null) => void;
    onSelectPart?: (partId: string | null) => void;
    onSemanticChange?: (primitiveId: string, value: ParamValue) => Promise<void> | void;
    onControlFocusChange?: (focus: MeasurementControlFocus | null) => void;
    onchange?: (params: DesignParams) => Promise<boolean | void> | boolean | void;
    oncommit?: (params: DesignParams) => Promise<boolean | void> | boolean | void;
    onspecchange?: (uiSpec: UiSpec, params: DesignParams) => void;
    onpostprocessingchange?: (postProcessing: PostProcessingSpec | null) => void;
    onShowCode?: () => void;
    onOpenInEditor?: () => void;
    outlineEnabled?: boolean;
    topologyMode?: TopologyMode;
    selectionMode?: ViewerMode;
    onViewerDisplayChange?: (display: { outlineEnabled: boolean; topologyMode: TopologyMode }) => void;
    onViewerSelectionModeChange?: (mode: ViewerMode) => void;
    activeVersionId?: string | null;
    messageId?: string | null;
    macroCode?: string;
    onApplyMacroCode?: (code: string) => Promise<unknown>;
  } = $props();

  let editing = $state(false);
  let editFields = $state<EditableUiField[]>([]);
  let localParams = $state<DesignParams>({});
  let pendingParamDrafts = $state<DesignParams>({});
  let paramUndoStack = $state<DesignParams[]>([]);
  let paramUndoDepth = $derived(paramUndoStack.length);
  const effectiveLocalParams = $derived.by(() => ({ ...localParams, ...pendingParamDrafts }));
  let hasPendingChanges = $derived(JSON.stringify(effectiveLocalParams) !== JSON.stringify(parameters));
  let saveValuesState = $state<'idle' | 'saving' | 'saved'>('idle');
  let macroParamKeys = $state<Set<string> | null>(null);
  let macroParseSeq = 0;
  let localSelectedPartId = $state<string | null>(null);
  let proposalMutationId = $state<string | null>(null);
  let activeTab = $state<'views' | 'raw' | 'litho' | 'newParams'>('views');
  let highlightedParamKey = $state<string | null>(null);
  let highlightTimeoutId: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const highlight = $uiHighlightStore;
    if (highlight?.action === 'highlightParam') {
      highlightedParamKey = highlight.target;
      
      // Scroll into view
      void tick().then(() => {
        const el = document.querySelector(`[data-param-key="${highlight?.target}"]`);
        if (el) {
          el.scrollIntoView({ behavior: 'smooth', block: 'center' });
        }
      });

      if (highlightTimeoutId) clearTimeout(highlightTimeoutId);
      highlightTimeoutId = setTimeout(() => {
        highlightedParamKey = null;
        highlightTimeoutId = null;
      }, 2000);
    } else if (highlight?.action === 'focusMacroNode') {
      activeTab = 'newParams';
      pendingMacroFocusNodeId = highlight.target;
    }
  });
  let pendingMacroFocusNodeId = $state<string | null>(null);
  let sectionOverrides = $state<Record<string, boolean>>({});
  let hadSemanticViews = $state(false);
  let composerOpen = $state(false);
  let composerMode = $state<'create' | 'edit'>('create');
  let composerViewId = $state<string | null>(null);
  let composerViewLabel = $state('');
  let composerViewScope = $state<ControlViewScope>('global');
  let composerViewPartId = $state<string | null>(null);
  let composerPrimitiveIds = $state<string[]>([]);
  let primitiveComposerOpen = $state(false);
  let primitiveComposerMode = $state<'create' | 'edit'>('create');
  let primitiveEditingId = $state<string | null>(null);
  let primitiveLabel = $state('');
  let primitiveScope = $state<'global' | 'part'>('global');
  let primitivePartId = $state<string | null>(null);
  let primitiveParameterKeys = $state<string[]>([]);
  let primitiveBindingDrafts = $state<Record<string, PrimitiveBindingDraft>>({});
  let primitiveAttachToView = $state(true);
  let advisoryComposerOpen = $state(false);
  let advisoryLabel = $state('');
  let advisoryMessage = $state('');
  let advisorySeverity = $state<AdvisorySeverity>('warning');
  let advisoryCondition = $state<AdvisoryCondition>('always');
  let advisoryThreshold = $state('');
  let advisoryPrimitiveIds = $state<string[]>([]);
  let relationComposerOpen = $state(false);
  let relationSourcePrimitiveId = $state<string | null>(null);
  let relationTargetPrimitiveId = $state<string | null>(null);
  let relationMode = $state<ControlRelationMode>('mirror');
  let relationScale = $state('1');
  let relationOffset = $state('0');

  let lastVersionId = $state<string | null>(null);
  let lastHistorySourceSignature = $state<string | null>(null);
  let lastIncomingParamsSignature = $state('');
  let lastIncomingParamsSnapshot = $state<DesignParams | null>(null);
  let suppressNextIncomingHistory = $state(false);
  let localPostProcessing = $state<PostProcessingSpec | null>(null);
  let lastIncomingPostProcessingSignature = $state('');
  let selectedLithoId = $state<string | null>(null);

  function clonePostProcessing(value: PostProcessingSpec | null | undefined): PostProcessingSpec | null {
    return value ? normalizePostProcessing(JSON.parse(JSON.stringify(value))) : null;
  }

  function ensurePostProcessingDraft(): PostProcessingSpec {
    return clonePostProcessing(localPostProcessing) ?? {
      displacement: null,
      lithophaneAttachments: [],
    };
  }

  function nextLithoId() {
    return `litho-${crypto.randomUUID().slice(0, 8)}`;
  }

  function defaultLithophaneAttachment(): LithophaneAttachment {
    return {
      id: nextLithoId(),
      enabled: true,
      source: { kind: 'file', imagePath: '' },
      targetPartId: localSelectedPartId ?? modelManifest?.parts?.[0]?.partId ?? '',
      placement: {
        mode: 'partSidePatch',
        side: 'front',
        projection: 'auto',
        widthMm: 0,
        heightMm: 0,
        offsetXMm: 0,
        offsetYMm: 0,
        rotationDeg: 0,
        overflowMode: 'contain',
        bleedMarginMm: 0,
      },
      relief: {
        depthMm: 2,
        invert: false,
      },
      color: {
        mode: 'mono',
        channelThicknessMm: 0.4,
      },
    };
  }

  function commitPostProcessing(next: PostProcessingSpec | null, statusText = 'Lithophane staged. Apply to rerender.') {
    localPostProcessing = clonePostProcessing(next);
    onpostprocessingchange?.(localPostProcessing);
    if (!$liveApply) {
      session.setStatus(statusText);
    }
  }

  function parseOptionalNumber(raw: EditableNumber): number | undefined {
    if (raw === null || raw === undefined || raw === '') return undefined;
    const number = Number(raw);
    return Number.isFinite(number) ? number : undefined;
  }

  function toEditableField(field: ResolvedUiField | UiField): EditableUiField {
    switch (field.type) {
      case 'range':
        return {
          ...field,
          min: field.min,
          max: field.max,
          step: field.step,
        };
      case 'number':
        return {
          ...field,
          min: field.min,
          max: field.max,
          step: field.step,
        };
      case 'select':
      case 'checkbox':
      case 'image':
        return { ...field };
    }
  }

  function toPersistedField(field: EditableUiField): UiField | null {
    const key = field.key.trim();
    if (!key) return null;
    const label = field.label || field.key;

    switch (field.type) {
      case 'range': {
        const result: RangeField = {
          type: 'range',
          key,
          label,
          frozen: !!field.frozen,
        };
        const min = parseOptionalNumber(field.min);
        const max = parseOptionalNumber(field.max);
        const step = parseOptionalNumber(field.step);
        if (min !== undefined) result.min = min;
        if (max !== undefined) result.max = max;
        if (step !== undefined && step > 0) result.step = step;
        if (field.minFrom) result.minFrom = field.minFrom;
        if (field.maxFrom) result.maxFrom = field.maxFrom;
        return result;
      }
      case 'number': {
        const result: NumberField = {
          type: 'number',
          key,
          label,
          frozen: !!field.frozen,
        };
        const min = parseOptionalNumber(field.min);
        const max = parseOptionalNumber(field.max);
        const step = parseOptionalNumber(field.step);
        if (min !== undefined) result.min = min;
        if (max !== undefined) result.max = max;
        if (step !== undefined && step > 0) result.step = step;
        if (field.minFrom) result.minFrom = field.minFrom;
        if (field.maxFrom) result.maxFrom = field.maxFrom;
        return result;
      }
      case 'select':
        return {
          type: 'select',
          key,
          label,
          frozen: !!field.frozen,
          options: (field.options || [])
            .map((option) => ({
              label: `${option.label ?? ''}`.trim(),
              value: typeof option.value === 'number' ? option.value : `${option.value ?? ''}`,
            }))
            .filter((option) => option.label.length > 0 || `${option.value}`.trim().length > 0),
        };
      case 'checkbox':
        return {
          type: 'checkbox',
          key,
          label,
          frozen: !!field.frozen,
        };
      case 'image':
        return {
          type: 'image',
          key,
          label,
          frozen: !!field.frozen,
        };
    }
  }

  function asNumber(value: ParamValue | undefined, fallback = 0): number {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : fallback;
  }

  function firstSelectedPath(selection: string | string[] | null): string | null {
    if (typeof selection === 'string') return selection;
    if (Array.isArray(selection)) return selection[0] ?? null;
    return null;
  }

  function setFocusedControl(primitiveId: string | null, parameterKey: string | null) {
    onControlFocusChange?.({ primitiveId, parameterKey });
  }

  function clearFocusedControl(event: MouseEvent | FocusEvent) {
    const current = event.currentTarget as HTMLElement | null;
    const related = (event as FocusEvent).relatedTarget as Node | null;
    if (current && related && current.contains(related)) return;
    onControlFocusChange?.(null);
  }

  function updateViewerDisplay(next: Partial<{ outlineEnabled: boolean; topologyMode: TopologyMode }>) {
    onViewerDisplayChange?.({
      outlineEnabled: next.outlineEnabled ?? outlineEnabled,
      topologyMode: next.topologyMode ?? topologyMode,
    });
  }

  function focusDiagnosticMacroNode(error: unknown) {
    const diagnostic = getAppErrorDiagnosticContext(error);
    if (diagnostic?.partKey) {
      pendingMacroFocusNodeId = `part:${diagnostic.partKey}`;
      return;
    }
    if (diagnostic?.stableNodeKey) {
      pendingMacroFocusNodeId = diagnostic.stableNodeKey;
    }
  }

  $effect(() => {
    const code = `${macroCode ?? ''}`.trim();
    const seq = ++macroParseSeq;

    if (!code) {
      macroParamKeys = null;
      return;
    }

    (async () => {
      try {
        const parsed = await parseMacroParams(code);
        if (seq !== macroParseSeq) return;
        const keys = new Set<string>();
        for (const field of parsed?.fields || []) {
          if (field?.key) keys.add(field.key);
        }
        for (const key of Object.keys(parsed?.params || {})) {
          keys.add(key);
        }
        macroParamKeys = keys.size > 0 ? keys : null;
      } catch (e: unknown) {
        if (seq === macroParseSeq) {
          macroParamKeys = null;
        }
      }
    })();
  });

  $effect(() => {
    const isBlankSession =
      activeVersionId === null &&
      !macroCode.trim() &&
      (uiSpec?.fields?.length ?? 0) === 0 &&
      Object.keys(parameters || {}).length === 0;
    const incomingParamsSignature = JSON.stringify(parameters ?? {});

    if (isBlankSession) {
      localParams = {};
      pendingParamDrafts = {};
      paramUndoStack = [];
      localPostProcessing = null;
      selectedLithoId = null;
      lastVersionId = null;
      lastHistorySourceSignature = null;
      lastIncomingParamsSignature = incomingParamsSignature;
      lastIncomingParamsSnapshot = null;
      suppressNextIncomingHistory = false;
      lastIncomingPostProcessingSignature = JSON.stringify(null);
      editing = false;
      editFields = [];
      return;
    }

    const historySourceSignature = currentHistorySourceSignature();
    if (historySourceSignature !== lastHistorySourceSignature) {
      pendingParamDrafts = {};
      paramUndoStack = [];
      lastHistorySourceSignature = historySourceSignature;
    }

    // If we switched to a different version/thread, we must reset everything
    if (activeVersionId !== lastVersionId) {
      localParams = { ...parameters };
      pendingParamDrafts = {};
      lastVersionId = activeVersionId;
      lastIncomingParamsSignature = incomingParamsSignature;
      lastIncomingParamsSnapshot = { ...parameters };
      suppressNextIncomingHistory = false;
      editing = false;
      editFields = [];
      return;
    }

    if (incomingParamsSignature !== lastIncomingParamsSignature && !editing) {
      if (suppressNextIncomingHistory) {
        suppressNextIncomingHistory = false;
      } else if (lastIncomingParamsSnapshot && !paramsEqual(lastIncomingParamsSnapshot, parameters)) {
        pushParamHistory(lastIncomingParamsSnapshot);
      }
      localParams = { ...parameters };
      pendingParamDrafts = {};
      lastIncomingParamsSignature = incomingParamsSignature;
      lastIncomingParamsSnapshot = { ...parameters };
      return;
    }

    // Same version: keep local edits intact while user has pending changes or edits controls.
    // Otherwise, hard-sync to canonical persisted parameters (prunes stale keys).
    if (editing || hasPendingChanges) {
      return;
    }

    if (JSON.stringify(localParams) !== incomingParamsSignature) {
      localParams = { ...parameters };
      pendingParamDrafts = {};
    }
    lastIncomingParamsSignature = incomingParamsSignature;
    lastIncomingParamsSnapshot = { ...parameters };
  });

  $effect(() => {
    const normalized = clonePostProcessing(postProcessing);
    const incomingSignature = JSON.stringify(normalized ?? null);
    if (incomingSignature === lastIncomingPostProcessingSignature) return;
    localPostProcessing = normalized;
    lastIncomingPostProcessingSignature = incomingSignature;
    const nextSelectedId =
      normalized?.lithophaneAttachments?.find((attachment) => attachment.id === selectedLithoId)?.id ??
      normalized?.lithophaneAttachments?.[0]?.id ??
      null;
    selectedLithoId = nextSelectedId;
  });

  // Merge: each key in localParams not covered by uiSpec.fields gets a generated "number" field
  const mergedFields = $derived.by(() => {
    const specFields = uiSpec?.fields || [];
    const keys = macroParamKeys;
    const filteredSpecFields = keys
      ? specFields.filter((field) => field && keys.has(field.key))
      : specFields.filter(Boolean);
    const declaredKeys = new Set(filteredSpecFields.map((field) => field.key));
    
    const extraFields: ResolvedUiField[] = Object.entries(localParams)
      .filter(([key]) => !macroParamKeys || macroParamKeys.has(key))
      .filter(([key]) => !declaredKeys.has(key))
      .map(([key, val]) => ({
        key,
        label: key.replace(/[_-]/g, ' '),
        type: typeof val === 'boolean' ? 'checkbox' : 'number',
        frozen: false,
        _auto: true,
      }));
    
    const all: ResolvedUiField[] = [...filteredSpecFields, ...extraFields];
    // Sort: non-frozen first, then frozen
    return all.sort((a, b) => {
      if (a.frozen === b.frozen) return 0;
      return a.frozen ? 1 : -1;
    });
  });

  function startEditing() {
    editFields = mergedFields.map(toEditableField);
    editing = true;
  }

  function cancelEditing() {
    editing = false;
    editFields = [];
  }

  function addField() {
    editFields = [
      ...editFields,
      {
        key: '',
        label: '',
        type: 'number',
        min: undefined,
        max: undefined,
        step: undefined,
        minFrom: '',
        maxFrom: '',
        frozen: false,
      },
    ];
  }

  function updateEditField(index: number, patch: Partial<EditableUiField>) {
    editFields = editFields.map((field, i) =>
      i === index ? ({ ...field, ...patch } as EditableUiField) : field,
    );
  }

  function removeField(index: number) {
    editFields = editFields.filter((_, i) => i !== index);
  }

  function mergeParsedEditFields(
    existingFields: EditableUiField[],
    parsedFields: UiField[],
  ): EditableUiField[] {
    const merged = [...existingFields.filter(Boolean)];
    const seenKeys = new Set(
      merged.map((field) => field.key.trim()).filter((key) => key.length > 0),
    );

    for (const parsedField of parsedFields) {
      if (!parsedField) continue;
      const key = parsedField.key.trim();
      if (!key || seenKeys.has(key)) continue;
      merged.push(toEditableField(parsedField));
      seenKeys.add(key);
    }

    return merged;
  }

  function addSelectOption(index: number) {
    const field = editFields[index];
    if (!field || field.type !== 'select') return;
    field.options = [...(field.options || []), { label: '', value: '' }];
  }

  function removeSelectOption(fieldIndex: number, optionIndex: number) {
    const field = editFields[fieldIndex];
    if (!field || field.type !== 'select') return;
    field.options = (field.options || []).filter((_, index) => index !== optionIndex);
  }

  function updateSelectOption(
    fieldIndex: number,
    optionIndex: number,
    patch: { label?: string; value?: string | number },
  ) {
    const field = editFields[fieldIndex];
    if (!field || field.type !== 'select') return;
    field.options = (field.options || []).map((option, index) =>
      index === optionIndex ? { ...option, ...patch } : option,
    );
    updateEditField(fieldIndex, { options: field.options } as Partial<EditableUiField>);
  }

  let reading = $state(false);
  let applying = $state(false);
  let committing = $state(false);

  const filteredFields = $derived.by(() => {
    return filterFieldsBySearch(mergedFields, searchQuery);
  });

  const filteredEditFields = $derived.by(() => {
    return filterFieldsBySearch(editFields, searchQuery);
  });

  const filteredEditFieldEntries = $derived.by(() =>
    filteredEditFields.map((field) => ({
      field,
      index: editFields.indexOf(field),
    })),
  );

  const isSelectMode = $derived(selectionMode === 'select');

  $effect(() => {
    if (editing) return;
    if (controlViews.length === 0) {
      hadSemanticViews = false;
      return;
    }
    if (!hadSemanticViews) {
      hadSemanticViews = true;
      activeTab = 'views';
    }
  });

  $effect(() => {
    localSelectedPartId = selectedTarget?.partId ?? selectedPartId;
  });

  $effect(() => {
    if (!isSelectMode || localSelectedPartId) return;
    const fallbackTarget = (modelManifest?.selectionTargets || []).length === 1
      ? modelManifest?.selectionTargets?.[0]
      : null;
    const fallbackPartId = fallbackTarget?.partId ?? null;
    if (!fallbackPartId) return;
    selectPart(fallbackPartId);
  });

  const partCount = $derived(modelManifest?.parts?.length ?? 0);

  $effect(() => {
    if (!modelManifest?.parts?.length || modelManifest.parts.length !== 1) return;
    const onlyPartId = modelManifest.parts[0]?.partId ?? null;
    if (!onlyPartId || localSelectedPartId === onlyPartId) return;
    selectPart(onlyPartId);
  });

  const selectedPart = $derived.by<PartBinding | null>(() => {
    if (!localSelectedPartId || !modelManifest?.parts?.length) return null;
    return modelManifest.parts.find((part) => part.partId === localSelectedPartId) ?? null;
  });

  const lithophaneAttachments = $derived.by<LithophaneAttachment[]>(() =>
    clonePostProcessing(localPostProcessing)?.lithophaneAttachments ?? [],
  );

  const selectedLithophaneAttachment = $derived.by<LithophaneAttachment | null>(() => {
    if (!lithophaneAttachments.length) return null;
    return (
      lithophaneAttachments.find((attachment) => attachment.id === selectedLithoId) ??
      lithophaneAttachments[0] ??
      null
    );
  });

  const selectedLithophaneExportArtifacts = $derived.by(() => {
    const attachment = selectedLithophaneAttachment;
    const exports = artifactBundle?.exportArtifacts ?? [];
    if (!attachment) return exports;
    return exports.filter((item) => item.label.includes(attachment.id));
  });

  const manifestWarnings = $derived.by(() => {
    const warnings = new Set<string>();
    for (const warning of modelManifest?.warnings || []) {
      if (warning?.trim()) warnings.add(warning);
    }
    for (const warning of modelManifest?.document?.warnings || []) {
      if (warning?.trim()) warnings.add(warning);
    }
    return [...warnings];
  });

  const enrichmentProposals = $derived<EnrichmentProposal[]>(
    modelManifest?.enrichmentState?.proposals || [],
  );

  const selectedGroups = $derived.by<ParameterGroup[]>(() => {
    if (!localSelectedPartId || !modelManifest?.parameterGroups?.length) return [];
    const selectedId = localSelectedPartId;
    return modelManifest.parameterGroups.filter((group) =>
      (group.partIds || []).includes(selectedId),
    );
  });

  const selectedParameterKeys = $derived.by(() => {
    const exactKeys = resolveTargetParameterKeys(modelManifest, selectedTarget);
    if (exactKeys.size > 0) {
      return exactKeys;
    }
    const keys = new Set<string>();
    for (const group of selectedGroups) {
      for (const key of group.parameterKeys || []) {
        keys.add(key);
      }
    }
    if (keys.size === 0 && selectedPart) {
      for (const key of selectedPart.parameterKeys || []) {
        keys.add(key);
      }
    }
    return keys;
  });

  const contextualSelectionKeys = $derived.by(() => {
    const keys = new Set<string>(selectedParameterKeys);
    if (keys.size > 0) return keys;
    if (!isSelectMode) return keys;

    for (const key of selectedTarget?.parameterKeys || []) {
      keys.add(key);
    }
    if (keys.size > 0) return keys;

    const fallbackTarget = (modelManifest?.selectionTargets || []).length === 1
      ? modelManifest?.selectionTargets?.[0]
      : null;
    for (const key of fallbackTarget?.parameterKeys || []) {
      keys.add(key);
    }
    if (keys.size > 0) return keys;

    const fallbackPart = localSelectedPartId
      ? (modelManifest?.parts || []).find((part) => part.partId === localSelectedPartId)
      : null;
    for (const key of fallbackPart?.parameterKeys || []) {
      keys.add(key);
    }
    return keys;
  });

  const focusedFields = $derived.by(() => {
    if (!localSelectedPartId || selectedParameterKeys.size === 0) return [];
    return filteredFields.filter((field) => selectedParameterKeys.has(field.key));
  });

  const remainingFields = $derived.by(() => {
    if (!localSelectedPartId || selectedParameterKeys.size === 0) return filteredFields;
    return filteredFields.filter((field) => !selectedParameterKeys.has(field.key));
  });

  const activeSemanticView = $derived.by<MaterializedSemanticView | null>(() => {
    if (!controlViews.length) return null;
    return controlViews.find((view) => view.viewId === activeControlViewId) ?? controlViews[0] ?? null;
  });

  const filteredSemanticSections = $derived.by(() => {
    if (isSelectMode && !selectedPart && (modelManifest?.selectionTargets?.length ?? 0) > 1) {
      return [];
    }
    const sections = resolveContextSections(activeSemanticView, selectedTarget, searchQuery);
    if (!isSelectMode || contextualSelectionKeys.size === 0) {
      return sections;
    }
    return sections
      .map((section) => ({
        ...section,
        controls: section.controls.filter((control) => {
          if (control.rawField && contextualSelectionKeys.has(control.rawField.key)) return true;
          return (control.bindings || []).some((binding) =>
            contextualSelectionKeys.has(binding.parameterKey),
          );
        }),
      }))
      .filter((section) => section.controls.length > 0);
  });

  const primitiveCatalog = $derived.by(() => {
    const partsById = new Map((modelManifest?.parts || []).map((part) => [part.partId, part]));
    return (modelManifest?.controlPrimitives || [])
      .map((primitive) => ({
        primitiveId: primitive.primitiveId,
        label: primitive.label,
        editable: primitive.editable,
        partIds: primitive.partIds || [],
        parameterKeys: (primitive.bindings || []).map((binding) => binding.parameterKey),
        partLabels: (primitive.partIds || [])
          .map((partId) => partsById.get(partId)?.label || partId)
          .filter(Boolean),
      }))
      .sort((left, right) => left.label.localeCompare(right.label));
  });

  const composerVisiblePrimitives = $derived.by(() => {
    const scopeFiltered =
      composerViewScope !== 'part' || !composerViewPartId
        ? primitiveCatalog
        : primitiveCatalog.filter(
            (primitive) =>
              primitive.partIds.length === 0 || primitive.partIds.includes(composerViewPartId as string),
          );
    if (!isSelectMode || contextualSelectionKeys.size === 0) {
      return scopeFiltered;
    }
    return scopeFiltered.filter(
      (primitive) =>
        primitive.parameterKeys.some((key) => contextualSelectionKeys.has(key)),
    );
  });

  const composerCanSave = $derived(
    Boolean(composerViewLabel.trim()) &&
      composerPrimitiveIds.length > 0 &&
      (composerViewScope !== 'part' || Boolean(composerViewPartId)),
  );

  const primitiveCandidateFields = $derived.by(() => {
    let candidates = mergedFields;
    if (primitiveScope === 'part' && primitivePartId) {
      const scopedPartId = primitivePartId;
      const scopedKeys = new Set<string>();
      const scopedGroups = (modelManifest?.parameterGroups || []).filter((group) =>
        (group.partIds || []).includes(scopedPartId),
      );
      for (const group of scopedGroups) {
        for (const key of group.parameterKeys || []) {
          scopedKeys.add(key);
        }
      }
      const scopedPart = (modelManifest?.parts || []).find((part) => part.partId === scopedPartId);
      for (const key of scopedPart?.parameterKeys || []) {
        scopedKeys.add(key);
      }
      if (scopedKeys.size > 0) {
        candidates = candidates.filter((field) => scopedKeys.has(field.key));
      }
    }
    if (!searchQuery.trim()) return candidates;
    const query = searchQuery.toLowerCase();
    return candidates.filter((field) =>
      field.key.toLowerCase().includes(query) ||
      (field.label || '').toLowerCase().includes(query),
    );
  });

  const selectedPrimitiveFields = $derived.by(() =>
    primitiveCandidateFields.filter((field) => primitiveParameterKeys.includes(field.key)),
  );

  const primitiveKindPreview = $derived.by<ControlPrimitiveKind | null>(() => {
    const kinds = new Set<ControlPrimitiveKind>();
    for (const field of selectedPrimitiveFields) {
      if (field.type === 'checkbox') {
        kinds.add('toggle');
      } else if (field.type === 'select') {
        kinds.add('choice');
      } else {
        kinds.add('number');
      }
    }
    if (kinds.size === 1) {
      return [...kinds][0] ?? null;
    }
    return null;
  });

  const primitiveCanSave = $derived(
    Boolean(primitiveLabel.trim()) &&
      primitiveParameterKeys.length > 0 &&
      Boolean(primitiveKindPreview) &&
      (primitiveScope !== 'part' || Boolean(primitivePartId)),
  );
  const advisoryCandidateControls = $derived.by(() =>
    activeSemanticView
      ? [...new Map(
          activeSemanticView.sections
            .flatMap((section) => section.controls)
            .map((control) => [control.primitiveId, control] as const),
        ).values()]
      : [],
  );
  const advisoryCanSave = $derived(
    Boolean(advisoryLabel.trim()) &&
      Boolean(advisoryMessage.trim()) &&
      advisoryPrimitiveIds.length > 0 &&
      (advisoryCondition === 'always' || advisoryThreshold.trim().length > 0),
  );
  const activeViewRelations = $derived.by(() => {
    if (!activeSemanticView || !modelManifest?.controlRelations?.length) return [];
    const primitiveIds = new Set(
      activeSemanticView.sections.flatMap((section) => section.controls.map((control) => control.primitiveId)),
    );
    const labels = new Map(
      activeSemanticView.sections
        .flatMap((section) => section.controls)
        .map((control) => [control.primitiveId, control.label] as const),
    );
    return modelManifest.controlRelations
      .filter(
        (relation) =>
          primitiveIds.has(relation.sourcePrimitiveId) && primitiveIds.has(relation.targetPrimitiveId),
      )
      .map((relation) => ({
        ...relation,
        sourceLabel: labels.get(relation.sourcePrimitiveId) || relation.sourcePrimitiveId,
        targetLabel: labels.get(relation.targetPrimitiveId) || relation.targetPrimitiveId,
      }));
  });
  const relationCanSave = $derived(
    Boolean(relationSourcePrimitiveId) &&
      Boolean(relationTargetPrimitiveId) &&
      relationSourcePrimitiveId !== relationTargetPrimitiveId,
  );

  $effect(() => {
    if (!composerOpen) return;
    const visibleIds = new Set(composerVisiblePrimitives.map((primitive) => primitive.primitiveId));
    if (composerPrimitiveIds.some((primitiveId) => !visibleIds.has(primitiveId))) {
      composerPrimitiveIds = composerPrimitiveIds.filter((primitiveId) => visibleIds.has(primitiveId));
    }
  });

  $effect(() => {
    if (!primitiveComposerOpen) return;
    const visibleKeys = new Set(primitiveCandidateFields.map((field) => field.key));
    if (primitiveParameterKeys.some((key) => !visibleKeys.has(key))) {
      primitiveParameterKeys = primitiveParameterKeys.filter((key) => visibleKeys.has(key));
    }
  });

  async function readFromMacro() {
    if (!macroCode) {
      session.setStatus('No macro code available to read from.');
      return;
    }
    reading = true;
    try {
      const result = await parseMacroParams(macroCode);
      const { fields, params } = result;

      if (fields && fields.length > 0) {
        const before = editFields.length;
        editFields = mergeParsedEditFields(editFields, fields);
        localParams = { ...params, ...localParams };
        const added = editFields.length - before;
        if (added > 0) {
          session.setStatus(`${added} new field${added === 1 ? '' : 's'} added from macro.`);
        } else {
          session.setStatus('All fields already up to date.');
        }
      } else {
        session.setStatus('No parameters detected in macro.');
      }
    } catch (e: unknown) {
      console.error('ParamPanel: Failed to parse macro params:', e);
      session.setError('Failed to read from macro.');
    } finally {
      reading = false;
    }
  }

  async function saveFields() {
    const cleaned = editFields
      .map(toPersistedField)
      .filter((field): field is UiField => field !== null);

    const newSpec: UiSpec = { fields: cleaned };
    uiSpec = newSpec;

    if (activeVersionId) {
      console.log('ParamPanel: Saving uiSpec to messageId:', activeVersionId, newSpec);
      try {
        await updateUiSpec(activeVersionId, newSpec);
        console.log('ParamPanel: update_ui_spec success');
        
        // Also save parameters since readFromMacro might have updated them
        await updateParameters(activeVersionId, localParams);
        console.log('ParamPanel: update_parameters success');
        
        // Notify parent, but do not rerender geometry for control-only edits.
        if (onspecchange) onspecchange(newSpec, localParams);
        session.setStatus('Controls saved.');
      } catch (e: unknown) {
        console.error('ParamPanel: Failed to save ui_spec/params:', formatBackendError(e));
        session.setError(`Control Save Failed: ${formatBackendError(e)}`);
      }
    } else {
      if (onspecchange) onspecchange(newSpec, localParams);
      session.setStatus('Controls updated.');
    }

    editing = false;
    editFields = [];
  }

  function paramsEqual(left: DesignParams, right: DesignParams): boolean {
    return JSON.stringify(left) === JSON.stringify(right);
  }

  function cloneParams(params: DesignParams): DesignParams {
    return { ...params };
  }

  function currentHistorySourceSignature(): string {
    return JSON.stringify({
      macroCode: macroCode ?? '',
      fields: (uiSpec?.fields || []).map((field) => field?.key ?? ''),
    });
  }

  function pushParamHistory(snapshot: DesignParams) {
    lastHistorySourceSignature = currentHistorySourceSignature();
    const cloned = cloneParams(snapshot);
    const previous = paramUndoStack[paramUndoStack.length - 1];
    if (previous && paramsEqual(previous, cloned)) return;
    paramUndoStack = [...paramUndoStack, cloned].slice(-PARAM_UNDO_LIMIT);
  }

  function clearPendingParamDraft(key: string) {
    if (!(key in pendingParamDrafts)) return;
    const nextDrafts = { ...pendingParamDrafts };
    delete nextDrafts[key];
    pendingParamDrafts = nextDrafts;
  }

  function stageParamDraft(key: string, value: ParamValue) {
    const nextParams = { ...effectiveLocalParams, [key]: value };
    if (!paramsEqual(nextParams, effectiveLocalParams)) {
      pushParamHistory(effectiveLocalParams);
    }
    pendingParamDrafts = { ...pendingParamDrafts, [key]: value };
  }

  function update(key: string, value: ParamValue) {
    let clampedValue = value;
    const field = mergedFields.find(f => f.key === key);
    if (field && (field.type === 'range' || field.type === 'number')) {
      if (typeof value !== 'number' || !Number.isFinite(value)) return;
      const props = getRangeProps(field);
      clampedValue = Math.max(props.min, Math.min(props.max, value));
    }

    const nextParams: DesignParams = { ...effectiveLocalParams, [key]: clampedValue };

    // Cascade clamping for dependent fields
    for (const otherField of mergedFields) {
      if (otherField.type !== 'range' && otherField.type !== 'number') continue;
      if (otherField.key !== key && (otherField.minFrom === key || otherField.maxFrom === key)) {
        const otherVal = asNumber(nextParams[otherField.key], 0);
        let oMin = otherField.min ?? 0;
        if (otherField.minFrom && nextParams[otherField.minFrom] !== undefined) {
          oMin = asNumber(nextParams[otherField.minFrom], oMin);
        }
        let oMax = otherField.max ?? Math.max(200, otherVal * 4);
        if (otherField.maxFrom && nextParams[otherField.maxFrom] !== undefined) {
          oMax = asNumber(nextParams[otherField.maxFrom], oMax);
        }
        
        const nextClamped = Math.max(oMin, Math.min(oMax, otherVal));
        if (nextClamped !== otherVal) {
          nextParams[otherField.key] = nextClamped;
        }
      }
    }

    if (!paramsEqual(nextParams, effectiveLocalParams)) {
      pushParamHistory(effectiveLocalParams);
    }
    localParams = nextParams;
    clearPendingParamDraft(key);
    if ($liveApply && onchange) {
      onchange(localParams);
    } else {
      session.setStatus('Parameters staged. Apply to rerender.');
    }
  }

  function updateSemanticControl(control: MaterializedSemanticControl, value: ParamValue) {
    onSemanticChange?.(control.primitiveId, value);
  }

  function isManualPrimitive(control: MaterializedSemanticControl): boolean {
    return control.source === 'manual';
  }

  function semanticSourceLabel(source: ControlViewSource | undefined): string {
    switch (source) {
      case 'llm':
        return 'LLM';
      case 'manual':
        return 'MANUAL';
      case 'inherited':
        return 'INHERITED';
      default:
        return 'GENERATED';
    }
  }

  function shouldShowSemanticSource(source: ControlViewSource | undefined): boolean {
    return source === 'manual' || source === 'inherited' || source === 'llm';
  }

  function isSectionExpanded(sectionId: string, collapsedByDefault: boolean) {
    const explicit = sectionOverrides[sectionId];
    if (explicit !== undefined) return explicit;
    return !collapsedByDefault;
  }

  function toggleSection(sectionId: string, collapsedByDefault: boolean) {
    const nextExpanded = !isSectionExpanded(sectionId, collapsedByDefault);
    sectionOverrides = {
      ...sectionOverrides,
      [sectionId]: nextExpanded,
    };
  }

  async function applyChanges() {
    if (applying) return;
    const paramsToApply = cloneParams(effectiveLocalParams);
    console.log('ParamPanel: applyChanges clicked', { localParams: paramsToApply, hasPendingChanges, live: $liveApply });
    if (onchange) {
      applying = true;
      session.setError(null);
      try {
        const applied = await onchange(paramsToApply);
        if (applied === false) return;
        if (!paramsEqual(paramsToApply, parameters)) {
          pushParamHistory(parameters);
        }
        localParams = paramsToApply;
        pendingParamDrafts = {};
      } catch (e: unknown) {
        console.error('ParamPanel: onchange failed', e);
        session.setError(`Apply Failed: ${formatBackendError(e)}`);
        focusDiagnosticMacroNode(e);
      } finally {
        applying = false;
      }
    } else {
      console.warn('ParamPanel: onchange prop is missing!');
      session.setError('Apply Failed: parameter change handler is missing.');
    }
  }

  async function undoParams() {
    if (applying || paramUndoStack.length === 0) return;
    const previousParams = cloneParams(paramUndoStack[paramUndoStack.length - 1]);
    paramUndoStack = paramUndoStack.slice(0, -1);
    localParams = previousParams;
    pendingParamDrafts = {};

    if (!onchange) {
      session.setStatus('Parameters restored. Apply to rerender.');
      return;
    }

    applying = true;
    session.setError(null);
    try {
      suppressNextIncomingHistory = true;
      await onchange(previousParams);
      session.setStatus('Parameters restored.');
    } catch (e: unknown) {
      suppressNextIncomingHistory = false;
      console.error('ParamPanel: undo apply failed', e);
      session.setError(`Undo Failed: ${formatBackendError(e)}`);
      focusDiagnosticMacroNode(e);
    } finally {
      applying = false;
    }
  }

  async function commitChanges() {
    if (committing) return;
    const paramsToCommit = cloneParams(effectiveLocalParams);
    if (oncommit) {
      committing = true;
      session.setError(null);
      try {
        const committed = await oncommit(paramsToCommit);
        if (committed === false) return;
        localParams = paramsToCommit;
        pendingParamDrafts = {};
      } catch (e: unknown) {
        console.error('ParamPanel: oncommit failed', e);
        session.setError(`Commit Failed: ${formatBackendError(e)}`);
        focusDiagnosticMacroNode(e);
      } finally {
        committing = false;
      }
    } else {
      console.warn('ParamPanel: oncommit prop is missing!');
      session.setError('Commit Failed: parameter commit handler is missing.');
    }
  }

  async function saveValues() {
    if (!activeVersionId) return;
    saveValuesState = 'saving';
    try {
      await updateParameters(activeVersionId, localParams);
      // Sync in-memory state so that isDirty=true and paramPanelState reflects saved values.
      // This prevents stale state from being used in subsequent renders or overwritten by agent drafts.
      if (onspecchange) onspecchange(uiSpec ?? { fields: [] }, localParams);
      await refreshHistory();
      saveValuesState = 'saved';
      setTimeout(() => {
        if (saveValuesState === 'saved') saveValuesState = 'idle';
      }, 1500);
    } catch (e: unknown) {
      console.error('Failed to save defaults:', formatBackendError(e));
      session.setError(`Save Values Failed: ${formatBackendError(e)}`);
      saveValuesState = 'idle';
    }
  }

  function getRangeProps(field: RangeLikeField) {
    const rawVal = Number(effectiveLocalParams[field.key]);
    const val = Number.isFinite(rawVal) ? rawVal : 0;
    let min = parseOptionalNumber(field.min) ?? 0;
    if (field.minFrom && effectiveLocalParams[field.minFrom] !== undefined) {
      min = asNumber(effectiveLocalParams[field.minFrom], min);
    }

    let max = parseOptionalNumber(field.max) ?? Math.max(200, val * 4);
    if (field.maxFrom && effectiveLocalParams[field.maxFrom] !== undefined) {
      max = asNumber(effectiveLocalParams[field.maxFrom], max);
    }

    if (max < min) max = min;
    if (max === min) max = min + 1;
    const stepCandidate = parseOptionalNumber(field.step) ?? (max - min > 50 ? 1 : 0.1);
    const step = Number.isFinite(stepCandidate) && stepCandidate > 0 ? stepCandidate : 1;
    return { min, max, step };
  }

  function getAvailableTypes(field: EditableUiField | ResolvedUiField) {
    const preferredTypes: EditableUiField['type'][] = [];
    const paramValue = parameters[field.key];

    preferredTypes.push(field.type);
    if (typeof paramValue === 'boolean') {
      preferredTypes.push('checkbox');
    } else if (typeof paramValue === 'string') {
      preferredTypes.push('select');
    } else if (typeof paramValue === 'number' || paramValue === null) {
      preferredTypes.push('number');
    }

    return [...new Set<EditableUiField['type']>([
      ...preferredTypes,
      'number',
      'select',
      'checkbox',
      'image',
      'range',
    ])];
  }

  function getCadHint(field: UiField | ResolvedUiField | EditableUiField): CadHint {
    const signature = `${field.key} ${field.label}`.toLowerCase();
    const tokens = new Set(signature.split(/[^a-z0-9]+/).filter(Boolean));
    const hasToken = (...candidates: string[]) => candidates.some((candidate) => tokens.has(candidate));
    const hasFragment = (...fragments: string[]) =>
      fragments.some((fragment) => signature.includes(fragment));

    if (field.type === 'checkbox') {
      return { tone: 'state', tag: 'STATE', glyph: '[ ]', note: 'binary latch' };
    }
    if (field.type === 'image') {
      return { tone: 'state', tag: 'FILE', glyph: '[@]', note: 'asset path' };
    }
    if (field.type === 'select') {
      return { tone: 'mode', tag: 'MODE', glyph: '::', note: 'discrete set' };
    }
    if (hasFragment('diameter') || hasToken('dia', 'diameter')) {
      return { tone: 'size', tag: 'DIA', glyph: 'O/', note: 'radial span' };
    }
    if (hasFragment('radius') || hasToken('radius', 'fillet')) {
      return { tone: 'size', tag: 'RAD', glyph: 'R', note: 'radial span' };
    }
    if (
      hasFragment('angle') ||
      hasToken('angle', 'tilt', 'rotation', 'rotate', 'yaw', 'pitch')
    ) {
      return { tone: 'angle', tag: 'ANG', glyph: '/_', note: 'angular sweep' };
    }
    if (
      hasFragment('height') ||
      hasFragment('vertical') ||
      hasFragment('elevation') ||
      hasToken('height', 'z', 'rise', 'top', 'bottom')
    ) {
      return { tone: 'z', tag: 'Z', glyph: '^v', note: 'vertical span' };
    }
    if (
      hasFragment('depth') ||
      hasFragment('length') ||
      hasFragment('offset') ||
      hasToken('depth', 'length', 'y', 'front', 'back', 'reach')
    ) {
      return { tone: 'y', tag: 'Y', glyph: '<>', note: 'forward run' };
    }
    if (
      hasFragment('width') ||
      hasFragment('size') ||
      hasFragment('scale') ||
      hasToken('width', 'size', 'span', 'scale', 'x')
    ) {
      return { tone: 'x', tag: 'X', glyph: '<>', note: 'lateral span' };
    }
    if (field.type === 'range' || field.type === 'number') {
      return { tone: 'size', tag: 'DIM', glyph: '<>', note: 'dimension' };
    }
    return { tone: 'neutral', tag: 'CTRL', glyph: '::', note: 'tunable value' };
  }

  function selectPart(partId: string | null) {
    localSelectedPartId = partId;
    session.setSelectedPartId(partId);
    onSelectPart?.(partId);
  }

  function patchLithophaneAttachment(
    attachmentId: string,
    mutate: (attachment: LithophaneAttachment) => LithophaneAttachment,
    statusText = 'Lithophane staged. Apply to rerender.',
  ) {
    const draft = ensurePostProcessingDraft();
    draft.displacement = null;
    draft.lithophaneAttachments = (draft.lithophaneAttachments || []).map((attachment) =>
      attachment.id === attachmentId ? mutate(attachment) : attachment,
    );
    commitPostProcessing(draft, statusText);
  }

  function addLithophane() {
    const draft = ensurePostProcessingDraft();
    const attachment = defaultLithophaneAttachment();
    draft.displacement = null;
    draft.lithophaneAttachments = [...(draft.lithophaneAttachments || []), attachment];
    selectedLithoId = attachment.id;
    commitPostProcessing(draft, 'Lithophane patch added. Apply to rerender.');
    activeTab = 'litho';
  }

  function duplicateLithophane(attachment: LithophaneAttachment | null) {
    if (!attachment) return;
    const draft = ensurePostProcessingDraft();
    const clone = {
      ...JSON.parse(JSON.stringify(attachment)),
      id: nextLithoId(),
      targetPartId: attachment.targetPartId || localSelectedPartId || '',
    } as LithophaneAttachment;
    draft.displacement = null;
    draft.lithophaneAttachments = [...(draft.lithophaneAttachments || []), clone];
    selectedLithoId = clone.id;
    commitPostProcessing(draft, 'Lithophane patch duplicated. Apply to rerender.');
  }

  function deleteLithophane(attachmentId: string) {
    const draft = ensurePostProcessingDraft();
    draft.displacement = null;
    draft.lithophaneAttachments = (draft.lithophaneAttachments || []).filter(
      (attachment) => attachment.id !== attachmentId,
    );
    selectedLithoId = draft.lithophaneAttachments[0]?.id ?? null;
    commitPostProcessing(
      draft.lithophaneAttachments.length ? draft : null,
      'Lithophane patch removed.',
    );
  }

  function setLithophaneImage(attachmentId: string, path: string) {
    patchLithophaneAttachment(attachmentId, (attachment) => ({
      ...attachment,
      source: {
        kind: 'file',
        imagePath: path,
      },
    }));
  }

  function clearLithophaneImage(attachmentId: string) {
    patchLithophaneAttachment(
      attachmentId,
      (attachment) => ({
        ...attachment,
        source: {
          kind: 'file',
          imagePath: '',
        },
      }),
      'Lithophane image cleared. Apply to rerender.',
    );
  }

  async function pickLithophaneImage(attachmentId: string) {
    const file = await open({
      multiple: false,
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }],
    });
    const selected = firstSelectedPath(file);
    if (selected) setLithophaneImage(attachmentId, selected);
  }

  async function pickRawImage(parameterKey: string) {
    const file = await open({
      multiple: false,
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }],
    });
    const selected = firstSelectedPath(file);
    if (selected) update(parameterKey, selected);
  }

  async function pickSemanticControlImage(control: MaterializedSemanticControl) {
    const file = await open({
      multiple: false,
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }],
    });
    const selected = firstSelectedPath(file);
    if (selected && control.rawField) updateSemanticControl(control, selected);
  }

  function setLithophaneProjection(
    attachmentId: string,
    projection: ProjectionType,
  ) {
    patchLithophaneAttachment(attachmentId, (attachment) => ({
      ...attachment,
      placement: {
        ...attachment.placement,
        projection,
      },
      color:
        projection === 'planar'
          ? attachment.color
          : {
              ...attachment.color,
              mode: 'mono',
            },
    }));
  }

  function setLithophaneColorMode(
    attachmentId: string,
    mode: 'mono' | 'cmyk',
  ) {
    patchLithophaneAttachment(attachmentId, (attachment) => ({
      ...attachment,
      color: {
        ...attachment.color,
        mode,
      },
    }));
  }

  function previewImageUrl(path: string | null | undefined) {
    if (!path) return null;
    try {
      return convertFileSrc(path);
    } catch {
      return path;
    }
  }

  function slugify(value: string): string {
    return value
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '');
  }

  function flattenViewPrimitiveIds(view: MaterializedSemanticView | null): string[] {
    if (!view) return [];
    return [...new Set(view.sections.flatMap((section) => section.controls.map((control) => control.primitiveId)))];
  }

  function resetComposer() {
    composerOpen = false;
    composerMode = 'create';
    composerViewId = null;
    composerViewLabel = '';
    composerViewScope = 'global';
    composerViewPartId = null;
    composerPrimitiveIds = [];
  }

  function resetPrimitiveComposer() {
    primitiveComposerOpen = false;
    primitiveComposerMode = 'create';
    primitiveEditingId = null;
    primitiveLabel = '';
    primitiveScope = 'global';
    primitivePartId = null;
    primitiveParameterKeys = [];
    primitiveBindingDrafts = {};
    primitiveAttachToView = true;
  }

  function resetAdvisoryComposer() {
    advisoryComposerOpen = false;
    advisoryLabel = '';
    advisoryMessage = '';
    advisorySeverity = 'warning';
    advisoryCondition = 'always';
    advisoryThreshold = '';
    advisoryPrimitiveIds = [];
  }

  function resetRelationComposer() {
    relationComposerOpen = false;
    relationSourcePrimitiveId = null;
    relationTargetPrimitiveId = null;
    relationMode = 'mirror';
    relationScale = '1';
    relationOffset = '0';
  }

  function openCreateViewComposer() {
    resetPrimitiveComposer();
    resetAdvisoryComposer();
    resetRelationComposer();
    const sourceView = activeSemanticView;
    composerMode = 'create';
    composerViewId = null;
    composerViewLabel = sourceView ? `${sourceView.label} Copy` : (selectedPart?.label || 'New View');
    composerViewScope = selectedPart ? 'part' : (sourceView?.scope || 'global');
    composerViewPartId =
      composerViewScope === 'part'
        ? (selectedPart?.partId || sourceView?.partIds?.[0] || null)
        : null;
    composerPrimitiveIds = flattenViewPrimitiveIds(sourceView);
    composerOpen = true;
  }

  function openPrimitiveComposer() {
    resetComposer();
    resetAdvisoryComposer();
    resetRelationComposer();
    primitiveComposerMode = 'create';
    primitiveEditingId = null;
    primitiveLabel = selectedPart ? `${selectedPart.label} ` : '';
    primitiveScope = selectedPart ? 'part' : 'global';
    primitivePartId = selectedPart?.partId || null;
    primitiveParameterKeys = [];
    primitiveBindingDrafts = {};
    primitiveAttachToView = true;
    primitiveComposerOpen = true;
  }

  function bindingDraftFromBinding(binding: PrimitiveBinding): PrimitiveBindingDraft {
    return {
      parameterKey: binding.parameterKey,
      scale: `${binding.scale ?? 1}`,
      offset: `${binding.offset ?? 0}`,
      min: binding.min === null || binding.min === undefined ? '' : `${binding.min}`,
      max: binding.max === null || binding.max === undefined ? '' : `${binding.max}`,
    };
  }

  function ensurePrimitiveDraft(parameterKey: string) {
    if (primitiveBindingDrafts[parameterKey]) return;
    primitiveBindingDrafts = {
      ...primitiveBindingDrafts,
      [parameterKey]: {
        parameterKey,
        scale: '1',
        offset: '0',
        min: '',
        max: '',
      },
    };
  }

  function openEditPrimitiveComposer(control: MaterializedSemanticControl) {
    resetComposer();
    resetAdvisoryComposer();
    resetRelationComposer();
    const primitive = (modelManifest?.controlPrimitives || []).find(
      (entry) => entry.primitiveId === control.primitiveId,
    );
    if (!primitive || !primitive.primitiveId.startsWith('primitive-manual-')) return;

    primitiveComposerMode = 'edit';
    primitiveEditingId = primitive.primitiveId;
    primitiveLabel = primitive.label;
    primitiveScope = primitive.partIds?.length ? 'part' : 'global';
    primitivePartId = primitive.partIds?.[0] || null;
    primitiveParameterKeys = (primitive.bindings || []).map((binding) => binding.parameterKey);
    primitiveBindingDrafts = Object.fromEntries(
      (primitive.bindings || []).map((binding) => [binding.parameterKey, bindingDraftFromBinding(binding)]),
    );
    primitiveAttachToView = false;
    primitiveComposerOpen = true;
  }

  function openAdvisoryComposer() {
    resetComposer();
    resetPrimitiveComposer();
    resetRelationComposer();
    advisoryLabel = activeSemanticView ? `${activeSemanticView.label} Rule` : 'New Rule';
    advisoryMessage = '';
    advisorySeverity = 'warning';
    advisoryCondition = 'always';
    advisoryThreshold = '';
    advisoryPrimitiveIds = advisoryCandidateControls.map((control) => control.primitiveId).slice(0, 1);
    advisoryComposerOpen = true;
  }

  function openRelationComposer() {
    resetComposer();
    resetPrimitiveComposer();
    resetAdvisoryComposer();
    const controls = advisoryCandidateControls;
    relationSourcePrimitiveId = controls[0]?.primitiveId || null;
    relationTargetPrimitiveId = controls[1]?.primitiveId || controls[0]?.primitiveId || null;
    relationMode = 'mirror';
    relationScale = '1';
    relationOffset = '0';
    relationComposerOpen = true;
  }

  function openEditViewComposer(view: MaterializedSemanticView) {
    resetPrimitiveComposer();
    composerMode = 'edit';
    composerViewId = view.viewId;
    composerViewLabel = view.label;
    composerViewScope = view.scope;
    composerViewPartId = view.scope === 'part' ? (view.partIds?.[0] || null) : null;
    composerPrimitiveIds = flattenViewPrimitiveIds(view);
    composerOpen = true;
  }

  function toggleComposerPrimitive(primitiveId: string, checked: boolean) {
    if (checked) {
      composerPrimitiveIds = [...new Set([...composerPrimitiveIds, primitiveId])];
    } else {
      composerPrimitiveIds = composerPrimitiveIds.filter((id) => id !== primitiveId);
    }
  }

  function togglePrimitiveParameter(parameterKey: string, checked: boolean) {
    if (checked) {
      primitiveParameterKeys = [...new Set([...primitiveParameterKeys, parameterKey])];
      ensurePrimitiveDraft(parameterKey);
    } else {
      primitiveParameterKeys = primitiveParameterKeys.filter((key) => key !== parameterKey);
      const nextDrafts = { ...primitiveBindingDrafts };
      delete nextDrafts[parameterKey];
      primitiveBindingDrafts = nextDrafts;
    }
  }

  function toggleAdvisoryPrimitive(primitiveId: string, checked: boolean) {
    if (checked) {
      advisoryPrimitiveIds = [...new Set([...advisoryPrimitiveIds, primitiveId])];
    } else {
      advisoryPrimitiveIds = advisoryPrimitiveIds.filter((id) => id !== primitiveId);
    }
  }

  function updatePrimitiveDraft(
    parameterKey: string,
    key: keyof Omit<PrimitiveBindingDraft, 'parameterKey'>,
    value: string,
  ) {
    ensurePrimitiveDraft(parameterKey);
    primitiveBindingDrafts = {
      ...primitiveBindingDrafts,
      [parameterKey]: {
        ...primitiveBindingDrafts[parameterKey],
        [key]: value,
      },
    };
  }

  function inferManualSections(primitiveIds: string[]): { sectionId: string; label: string; primitiveIds: string[]; collapsed: boolean }[] {
    const buckets = new Map<string, { sectionId: string; label: string; primitiveIds: string[]; collapsed: boolean; order: number }>();
    const sourceViews = activeSemanticView
      ? [activeSemanticView, ...controlViews.filter((view) => view.viewId !== activeSemanticView.viewId)]
      : controlViews;

    for (const primitiveId of primitiveIds) {
      let matchedSection: { sectionId: string; label: string; collapsed: boolean; order: number } | null = null;
      for (const view of sourceViews) {
        const sectionIndex = view.sections.findIndex((section) =>
          section.controls.some((control) => control.primitiveId === primitiveId),
        );
        if (sectionIndex === -1) continue;
        const section = view.sections[sectionIndex];
        matchedSection = {
          sectionId: section.sectionId,
          label: section.label,
          collapsed: section.collapsed,
          order: sectionIndex,
        };
        break;
      }

      const bucketKey = matchedSection?.sectionId || 'main';
      const bucket = buckets.get(bucketKey) || {
        sectionId: bucketKey,
        label: matchedSection?.label || 'Main',
        primitiveIds: [],
        collapsed: matchedSection?.collapsed || false,
        order: matchedSection?.order || 0,
      };
      bucket.primitiveIds.push(primitiveId);
      buckets.set(bucketKey, bucket);
    }

    return [...buckets.values()]
      .map((bucket) => ({
        sectionId: bucket.sectionId,
        label: bucket.label,
        primitiveIds: bucket.primitiveIds,
        collapsed: bucket.collapsed,
        order: bucket.order,
      }))
      .sort((left, right) => left.order - right.order || left.label.localeCompare(right.label))
      .map(({ order: _order, ...section }) => section);
  }

  function buildManualViewFromBase(
    baseView: MaterializedSemanticView | null,
    primitiveIds: string[],
    fallbackScope: ControlViewScope,
    fallbackPartId: string | null,
    existingViews: ControlView[],
  ): { view: ControlView; selectViewId: string } {
    const maxOrder = existingViews.reduce((max, view) => Math.max(max, view.order || 0), 0);

    if (baseView?.source === 'manual') {
      const existing = existingViews.find((view) => view.viewId === baseView.viewId);
      return {
        view: {
          viewId: baseView.viewId,
          label: baseView.label,
          scope: baseView.scope,
          partIds: [...(baseView.partIds || [])],
          primitiveIds,
          sections: inferManualSections(primitiveIds),
          default: existing?.default ?? false,
          source: 'manual',
          status: 'accepted',
          order: existing?.order ?? baseView.order ?? (maxOrder + 1),
        },
        selectViewId: baseView.viewId,
      };
    }

    const label = baseView ? `${baseView.label} Custom` : (fallbackScope === 'part' ? 'Part Custom' : 'Custom');
    const viewId = `view-manual-${slugify(label)}-${Date.now().toString(36)}`;
    return {
      view: {
        viewId,
        label,
        scope: baseView?.scope || fallbackScope,
        partIds: [...(baseView?.partIds || (fallbackScope === 'part' && fallbackPartId ? [fallbackPartId] : []))],
        primitiveIds,
        sections: inferManualSections(primitiveIds),
        default: false,
        source: 'manual',
        status: 'accepted',
        order: maxOrder + 1,
      },
      selectViewId: viewId,
    };
  }

  async function persistManifest(nextManifest: ModelManifest, nextViewId: string | null = null) {
    const versionMessageId = messageId ?? activeVersionId;
    await saveModelManifest(nextManifest.modelId, nextManifest, versionMessageId);
    updateCachedManifest(nextManifest, versionMessageId);
    const currentSession = get(session);
    session.setModelRuntime(currentSession.artifactBundle, nextManifest);
    await persistLastSessionSnapshot({
      modelManifest: nextManifest,
      messageId: versionMessageId ?? null,
    });
    if (nextViewId) {
      onSelectControlView?.(nextViewId);
    }
  }

  async function saveManualView() {
    if (!modelManifest || !composerCanSave) return;

    const existingViews = (modelManifest.controlViews || []).filter((view) => view.viewId !== composerViewId);
    const maxOrder = existingViews.reduce((max, view) => Math.max(max, view.order || 0), 0);
    const nextViewId =
      composerMode === 'edit' && composerViewId
        ? composerViewId
        : `view-manual-${slugify(composerViewLabel)}-${Date.now().toString(36)}`;
    const nextView: ControlView = {
      viewId: nextViewId,
      label: composerViewLabel.trim(),
      scope: composerViewScope,
      partIds: composerViewScope === 'part' && composerViewPartId ? [composerViewPartId] : [],
      primitiveIds: [...composerPrimitiveIds],
      sections: inferManualSections(composerPrimitiveIds),
      default: false,
      source: 'manual',
      status: 'accepted',
      order: composerMode === 'edit'
        ? modelManifest.controlViews?.find((view) => view.viewId === composerViewId)?.order ?? (maxOrder + 1)
        : maxOrder + 1,
    };

    const nextManifest: ModelManifest = {
      ...modelManifest,
      controlViews: [...existingViews, nextView].sort(
        (left, right) => (left.order ?? 0) - (right.order ?? 0) || left.label.localeCompare(right.label),
      ),
    };

    try {
      await persistManifest(nextManifest, nextView.viewId);
      activeTab = 'views';
      resetComposer();
    } catch (e: unknown) {
      session.setError(`View Save Failed: ${formatBackendError(e)}`);
    }
  }

  async function saveManualPrimitive() {
    if (!modelManifest || !primitiveCanSave || !primitiveKindPreview) return;
    const nextPrimitiveId =
      primitiveComposerMode === 'edit' && primitiveEditingId
        ? primitiveEditingId
        : `primitive-manual-${slugify(primitiveLabel)}-${Date.now().toString(36)}`;
    const nextBindings: PrimitiveBinding[] = primitiveParameterKeys.map((parameterKey) => {
      const draft = primitiveBindingDrafts[parameterKey];
      const numeric = (value: string, fallback: number) => {
        const parsed = Number(value);
        return Number.isFinite(parsed) ? parsed : fallback;
      };
      const optional = (value: string) => {
        const parsed = Number(value);
        return Number.isFinite(parsed) ? parsed : null;
      };
      return {
        parameterKey,
        scale: numeric(draft?.scale ?? '1', 1),
        offset: numeric(draft?.offset ?? '0', 0),
        min: optional(draft?.min ?? ''),
        max: optional(draft?.max ?? ''),
      };
    });

    const nextPrimitive: ControlPrimitive = {
      primitiveId: nextPrimitiveId,
      label: primitiveLabel.trim(),
      kind: primitiveKindPreview,
      source: 'manual',
      partIds: primitiveScope === 'part' && primitivePartId ? [primitivePartId] : [],
      bindings: nextBindings,
      editable: true,
      order:
        primitiveComposerMode === 'edit'
          ? (modelManifest.controlPrimitives || []).find((primitive) => primitive.primitiveId === primitiveEditingId)?.order ??
            ((modelManifest.controlPrimitives || []).reduce((max, primitive) => Math.max(max, primitive.order || 0), 0) + 1)
          : (modelManifest.controlPrimitives || []).reduce((max, primitive) => Math.max(max, primitive.order || 0), 0) + 1,
    };

    let nextControlViews = [...(modelManifest.controlViews || [])];
    let selectViewId = activeControlViewId;

    if (primitiveAttachToView) {
      const baseIds = activeSemanticView ? flattenViewPrimitiveIds(activeSemanticView) : [];
      const combinedIds = [...new Set([...baseIds, nextPrimitiveId])];
      const existingWithoutBase =
        activeSemanticView?.source === 'manual'
          ? nextControlViews.filter((view) => view.viewId !== activeSemanticView.viewId)
          : nextControlViews;
      const { view, selectViewId: nextSelectedViewId } = buildManualViewFromBase(
        activeSemanticView,
        combinedIds,
        primitiveScope === 'part' ? 'part' : 'global',
        primitivePartId,
        existingWithoutBase,
      );
      nextControlViews = [...existingWithoutBase, view].sort(
        (left, right) => (left.order ?? 0) - (right.order ?? 0) || left.label.localeCompare(right.label),
      );
      selectViewId = nextSelectedViewId;
    }

    const nextManifest: ModelManifest = {
      ...modelManifest,
      controlPrimitives: [
        ...(modelManifest.controlPrimitives || []).filter((primitive) => primitive.primitiveId !== nextPrimitiveId),
        nextPrimitive,
      ].sort(
        (left, right) => (left.order ?? 0) - (right.order ?? 0) || left.label.localeCompare(right.label),
      ),
      controlViews: nextControlViews,
    };

    try {
      await persistManifest(nextManifest, selectViewId);
      activeTab = 'views';
      resetPrimitiveComposer();
    } catch (e: unknown) {
      session.setError(`Knob Save Failed: ${formatBackendError(e)}`);
    }
  }

  async function deleteManualPrimitive(primitiveId: string) {
    if (!modelManifest || !primitiveId.startsWith('primitive-manual-')) return;

    const nextViews = (modelManifest.controlViews || [])
      .map((view) => {
        const primitiveIds = (view.primitiveIds || []).filter((id) => id !== primitiveId);
        const sections = (view.sections || [])
          .map((section) => ({
            ...section,
            primitiveIds: (section.primitiveIds || []).filter((id) => id !== primitiveId),
          }))
          .filter((section) => section.primitiveIds.length > 0);
        return {
          ...view,
          primitiveIds,
          sections,
        };
      })
      .filter((view) => (view.primitiveIds || []).length > 0);

    const nextManifest: ModelManifest = {
      ...modelManifest,
      controlPrimitives: (modelManifest.controlPrimitives || []).filter(
        (primitive) => primitive.primitiveId !== primitiveId,
      ),
      controlViews: nextViews,
    };

    try {
      await persistManifest(nextManifest, null);
      if (primitiveEditingId === primitiveId) {
        resetPrimitiveComposer();
      }
    } catch (e: unknown) {
      session.setError(`Knob Delete Failed: ${formatBackendError(e)}`);
    }
  }

  async function saveManualAdvisory() {
    if (!modelManifest || !advisoryCanSave) return;
    const thresholdValue = Number(advisoryThreshold);
    const nextManifest: ModelManifest = {
      ...modelManifest,
      advisories: [
        ...(modelManifest.advisories || []),
        {
          advisoryId: `advisory-manual-${slugify(advisoryLabel)}-${Date.now().toString(36)}`,
          label: advisoryLabel.trim(),
          severity: advisorySeverity,
          primitiveIds: [...advisoryPrimitiveIds],
          viewIds: activeControlViewId ? [activeControlViewId] : [],
          message: advisoryMessage.trim(),
          condition: advisoryCondition,
          threshold:
            advisoryCondition === 'always' || !Number.isFinite(thresholdValue)
              ? null
              : thresholdValue,
        },
      ],
    };

    try {
      await persistManifest(nextManifest, activeControlViewId);
      resetAdvisoryComposer();
    } catch (e: unknown) {
      session.setError(`Rule Save Failed: ${formatBackendError(e)}`);
    }
  }

  async function deleteManualAdvisory(advisoryId: string) {
    if (!modelManifest || !advisoryId.startsWith('advisory-manual-')) return;
    const nextManifest: ModelManifest = {
      ...modelManifest,
      advisories: (modelManifest.advisories || []).filter((advisory) => advisory.advisoryId !== advisoryId),
    };

    try {
      await persistManifest(nextManifest, activeControlViewId);
    } catch (e: unknown) {
      session.setError(`Rule Delete Failed: ${formatBackendError(e)}`);
    }
  }

  async function saveControlRelation() {
    if (!modelManifest || !relationCanSave || !relationSourcePrimitiveId || !relationTargetPrimitiveId) return;
    const scale = Number(relationScale);
    const offset = Number(relationOffset);
    const nextManifest: ModelManifest = {
      ...modelManifest,
      controlRelations: [
        ...(modelManifest.controlRelations || []),
        {
          relationId: `relation-manual-${Date.now().toString(36)}`,
          sourcePrimitiveId: relationSourcePrimitiveId,
          targetPrimitiveId: relationTargetPrimitiveId,
          mode: relationMode,
          scale: Number.isFinite(scale) ? scale : 1,
          offset: Number.isFinite(offset) ? offset : 0,
          enabled: true,
        },
      ],
    };

    try {
      await persistManifest(nextManifest, activeControlViewId);
      resetRelationComposer();
    } catch (e: unknown) {
      session.setError(`Link Save Failed: ${formatBackendError(e)}`);
    }
  }

  async function deleteControlRelation(relationId: string) {
    if (!modelManifest) return;
    const nextManifest: ModelManifest = {
      ...modelManifest,
      controlRelations: (modelManifest.controlRelations || []).filter(
        (relation) => relation.relationId !== relationId,
      ),
    };

    try {
      await persistManifest(nextManifest, activeControlViewId);
    } catch (e: unknown) {
      session.setError(`Link Delete Failed: ${formatBackendError(e)}`);
    }
  }

  async function deleteManualView(viewId: string) {
    if (!modelManifest) return;
    const nextManifest: ModelManifest = {
      ...modelManifest,
      controlViews: (modelManifest.controlViews || []).filter((view) => view.viewId !== viewId),
    };

    try {
      await persistManifest(nextManifest, null);
      if (activeControlViewId === viewId) {
        onSelectControlView?.(null);
      }
      resetComposer();
    } catch (e: unknown) {
      session.setError(`View Delete Failed: ${formatBackendError(e)}`);
    }
  }

  function deriveEnrichmentStatus(proposals: EnrichmentProposal[]): EnrichmentStatus {
    if (proposals.some((proposal) => proposal.status === 'pending')) return 'pending';
    if (proposals.some((proposal) => proposal.status === 'accepted')) return 'accepted';
    if (proposals.some((proposal) => proposal.status === 'rejected')) return 'rejected';
    return 'none';
  }

  function proposalGroupId(proposalId: string) {
    return `proposal-bind-${proposalId}`;
  }

  function rebuildImportedProposalBindings(
    manifest: ModelManifest,
    proposals: EnrichmentProposal[],
  ): ModelManifest {
    if (manifest.sourceKind !== 'importedFcstd') {
      return manifest;
    }

    const accepted = proposals.filter((proposal) => proposal.status === 'accepted');
    const autoGroupIds = new Set(
      (manifest.parameterGroups || [])
        .filter((group) => group.groupId.startsWith('proposal-bind-'))
        .map((group) => group.groupId),
    );
    const autoKeysByPart = new Map<string, Set<string>>();

    for (const group of manifest.parameterGroups || []) {
      if (!autoGroupIds.has(group.groupId)) continue;
      for (const partId of group.partIds || []) {
        const bucket = autoKeysByPart.get(partId) ?? new Set<string>();
        for (const key of group.parameterKeys || []) {
          bucket.add(key);
        }
        autoKeysByPart.set(partId, bucket);
      }
    }

    const acceptedKeysByPart = new Map<string, Set<string>>();
    for (const proposal of accepted) {
      for (const partId of proposal.partIds || []) {
        const bucket = acceptedKeysByPart.get(partId) ?? new Set<string>();
        for (const key of proposal.parameterKeys || []) {
          bucket.add(key);
        }
        acceptedKeysByPart.set(partId, bucket);
      }
    }

    const nextParts = (manifest.parts || []).map((part) => {
      const preservedKeys = (part.parameterKeys || []).filter(
        (key) => !autoKeysByPart.get(part.partId)?.has(key),
      );
      const acceptedKeys = [...(acceptedKeysByPart.get(part.partId) ?? new Set<string>())];
      const parameterKeys = [...new Set([...preservedKeys, ...acceptedKeys])];
      const editable = parameterKeys.length > 0;
      return {
        ...part,
        parameterKeys,
        editable,
      };
    });

    const editablePartIds = new Set(
      nextParts.filter((part) => part.editable).map((part) => part.partId),
    );
    const nextGroups = [
      ...(manifest.parameterGroups || []).filter(
        (group) => !group.groupId.startsWith('proposal-bind-'),
      ),
      ...accepted.map((proposal) => ({
        groupId: proposalGroupId(proposal.proposalId),
        label: proposal.label,
        parameterKeys: [...new Set(proposal.parameterKeys || [])],
        partIds: [...new Set(proposal.partIds || [])],
        editable: true,
      })),
    ];
    const nextTargets = (manifest.selectionTargets || []).map((target) => ({
      ...target,
      editable: editablePartIds.has(target.partId),
    }));

    const nextWarnings = (manifest.warnings || []).filter(
      (warning) =>
        warning !== 'Imported FCStd models are inspect-only until bindings are confirmed.' &&
        warning !== 'Imported FCStd bindings were accepted from heuristic proposals.',
    );

    if (accepted.length === 0) {
      nextWarnings.push('Imported FCStd models are inspect-only until bindings are confirmed.');
    } else {
      nextWarnings.push('Imported FCStd bindings were accepted from heuristic proposals.');
    }

    return {
      ...manifest,
      parts: nextParts,
      parameterGroups: nextGroups,
      selectionTargets: nextTargets,
      warnings: nextWarnings,
    };
  }

  function labelPartIds(partIds: string[] | undefined) {
    if (!partIds?.length || !modelManifest?.parts?.length) return 'No parts';
    const parts = modelManifest.parts || [];
    return partIds
      .map((partId) => parts.find((part) => part.partId === partId)?.label || partId)
      .join(', ');
  }

  function updateCachedManifest(nextManifest: ModelManifest, versionMessageId: string | null) {
    const threadId = get(activeThreadId);
    if (!threadId || !versionMessageId) return;
    const nextOutput = buildImportedSyntheticDesign(nextManifest, localParams, uiSpec);

    history.update((threads) =>
      threads.map((thread) => {
        if (thread.id !== threadId || !thread.messages?.length) {
          return thread;
        }

        return {
          ...thread,
          messages: thread.messages.map((message) =>
            message.id === versionMessageId
              ? {
                  ...message,
                  output: nextOutput ?? message.output ?? null,
                  modelManifest: nextManifest,
                }
              : message,
          ),
        };
      }),
    );
  }

  async function updateProposalStatus(proposalId: string, status: EnrichmentStatus) {
    if (!modelManifest || proposalMutationId) return;

    const nextProposals = enrichmentProposals.map((proposal) =>
      proposal.proposalId === proposalId ? { ...proposal, status } : proposal,
    );
    const nextManifestBase: ModelManifest = {
      ...modelManifest,
      enrichmentState: {
        status: deriveEnrichmentStatus(nextProposals),
        proposals: nextProposals,
      },
    };
    const nextManifest = rebuildImportedProposalBindings(nextManifestBase, nextProposals);

    proposalMutationId = proposalId;
    try {
      await persistManifest(nextManifest);
    } catch (e: unknown) {
      session.setError(`Manifest Save Failed: ${formatBackendError(e)}`);
    } finally {
      proposalMutationId = null;
    }
  }
</script>

<div class="param-panel">
  <ParamPanelToolbar
    searchQuery={searchQuery}
    editing={editing}
    applying={applying}
    committing={committing}
    reading={reading}
    undoDepth={paramUndoDepth}
    saveValuesState={saveValuesState}
    liveApply={$liveApply}
    activeVersionId={activeVersionId}
    onSearchQueryChange={(value) => searchQuery = value}
    onApplyChanges={applyChanges}
    onUndoParams={undoParams}
    onCommitChanges={commitChanges}
    onSaveValues={saveValues}
    onStartEditing={startEditing}
    onSaveFields={saveFields}
    onCancelEditing={cancelEditing}
    onReadFromMacro={readFromMacro}
    onLiveApplyChange={(checked) => liveApply.set(checked)}
  />

  <div class="param-panel-body">
    {#if editing}
      <ParamPanelEditFields
        fieldEntries={filteredEditFieldEntries}
        {getAvailableTypes}
        onFieldChange={updateEditField}
        onAddSelectOption={addSelectOption}
        onRemoveSelectOption={removeSelectOption}
        onOptionChange={updateSelectOption}
        onRemoveField={removeField}
        onAddField={addField}
      />
    {:else}
      {#if modelManifest}
        {#if manifestWarnings.length > 0}
          <div class="warning-stack">
            {#each manifestWarnings as warning}
              <div class="warning-chip">{warning}</div>
            {/each}
          </div>
        {/if}
      {/if}

    {#if modelManifest?.parts?.length && (partCount > 1 || modelManifest?.sourceKind === 'importedFcstd')}
      <div class="part-strip">
        <div class="section-label">PARTS</div>
        <div class="part-strip-list">
          {#each modelManifest.parts as part}
            <button
              class="part-chip"
              class:part-chip-active={part.partId === localSelectedPartId}
              class:part-chip-readonly={!part.editable}
              aria-label={part.label}
              onclick={() => selectPart(part.partId)}
              title={part.editable ? 'Select part controls' : 'Inspect-only part'}
            >
              {part.label.toLowerCase()}
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <ParamPanelModeTabs
      activeTab={activeTab}
      outlineEnabled={outlineEnabled}
      topologyMode={topologyMode}
      selectionMode={selectionMode}
      macroCode={macroCode}
      onActiveTabChange={(tab) => activeTab = tab}
      onShowCode={onShowCode}
      onOpenInEditor={onOpenInEditor}
      onViewerDisplayChange={updateViewerDisplay}
      onViewerSelectionModeChange={onViewerSelectionModeChange}
    />

    {#if enrichmentProposals.length > 0 && modelManifest?.sourceKind === 'importedFcstd'}
      <ParamPanelImportedProposals
        proposals={enrichmentProposals}
        mutationId={proposalMutationId}
        {labelPartIds}
        onUpdateProposalStatus={updateProposalStatus}
      />
    {/if}

    {#if activeTab === 'newParams'}
      <MacroAstMap
        {macroCode}
        {modelManifest}
        {uiSpec}
        parameters={effectiveLocalParams}
        fields={mergedFields}
        {highlightedParamKey}
        liveApply={$liveApply}
        focusNodeId={pendingMacroFocusNodeId}
        onFocusNodeHandled={() => (pendingMacroFocusNodeId = null)}
        {onApplyMacroCode}
        onDraftValue={(key, value) => stageParamDraft(key, value)}
        onUpdate={(key, value) => update(key, value)}
        onControlFocusChange={(primitiveId, parameterKey) => setFocusedControl(primitiveId, parameterKey)}
      />
    {:else if activeTab === 'litho'}
      <ParamPanelLithophaneTab
        {modelManifest}
        attachments={lithophaneAttachments}
        selectedAttachment={selectedLithophaneAttachment}
        selectedAttachmentId={selectedLithoId}
        exportArtifacts={selectedLithophaneExportArtifacts}
        {previewImageUrl}
        onSelectAttachment={(attachmentId) => selectedLithoId = attachmentId}
        onAddAttachment={addLithophane}
        onDuplicateAttachment={duplicateLithophane}
        onDeleteAttachment={deleteLithophane}
        onPatchAttachment={patchLithophaneAttachment}
        onPickImage={pickLithophaneImage}
        onClearImage={clearLithophaneImage}
        onSetProjection={setLithophaneProjection}
        onSetColorMode={setLithophaneColorMode}
      />
    {:else if activeTab === 'views'}
      <ParamPanelViewsTab
        {controlViews}
        {activeControlViewId}
        {activeSemanticView}
        {advisoryComposerOpen}
        {advisoryLabel}
        {advisoryMessage}
        {advisorySeverity}
        {advisoryCondition}
        {advisoryThreshold}
        {advisoryCandidateControls}
        {advisoryPrimitiveIds}
        {advisoryCanSave}
        {relationComposerOpen}
        {relationSourcePrimitiveId}
        {relationTargetPrimitiveId}
        {relationMode}
        {relationScale}
        {relationOffset}
        {relationCanSave}
        {primitiveComposerOpen}
        {primitiveComposerMode}
        {primitiveEditingId}
        {primitiveLabel}
        {primitiveScope}
        {primitivePartId}
        {primitiveAttachToView}
        modelParts={modelManifest?.parts || []}
        {primitiveCandidateFields}
        {primitiveParameterKeys}
        {selectedPrimitiveFields}
        {primitiveBindingDrafts}
        {primitiveKindPreview}
        {primitiveCanSave}
        {composerOpen}
        {composerMode}
        {composerViewLabel}
        {composerViewScope}
        {composerViewPartId}
        {composerVisiblePrimitives}
        {composerPrimitiveIds}
        {composerCanSave}
        advisories={activeSemanticView?.advisories || []}
        {activeViewRelations}
        {filteredSemanticSections}
        {selectedPart}
        {isSelectMode}
        selectionTargetCount={modelManifest?.selectionTargets?.length ?? 0}
        {highlightedParamKey}
        liveApply={$liveApply}
        onSelectControlView={onSelectControlView}
        onOpenCreateViewComposer={openCreateViewComposer}
        onOpenPrimitiveComposer={openPrimitiveComposer}
        onOpenAdvisoryComposer={openAdvisoryComposer}
        onOpenRelationComposer={openRelationComposer}
        onOpenEditViewComposer={openEditViewComposer}
        onDeleteManualView={deleteManualView}
        {shouldShowSemanticSource}
        {semanticSourceLabel}
        onAdvisoryLabelChange={(value) => advisoryLabel = value}
        onAdvisoryMessageChange={(value) => advisoryMessage = value}
        onAdvisorySeverityChange={(value) => advisorySeverity = value}
        onAdvisoryConditionChange={(value) => advisoryCondition = value}
        onAdvisoryThresholdChange={(value) => advisoryThreshold = value}
        onToggleAdvisoryPrimitive={toggleAdvisoryPrimitive}
        onCancelAdvisory={resetAdvisoryComposer}
        onSaveAdvisory={saveManualAdvisory}
        onRelationSourceChange={(value) => relationSourcePrimitiveId = value}
        onRelationTargetChange={(value) => relationTargetPrimitiveId = value}
        onRelationModeChange={(value) => relationMode = value}
        onRelationScaleChange={(value) => relationScale = value}
        onRelationOffsetChange={(value) => relationOffset = value}
        onCancelRelation={resetRelationComposer}
        onSaveRelation={saveControlRelation}
        onPrimitiveLabelChange={(value) => primitiveLabel = value}
        onPrimitiveScopeChange={(value) => {
          primitiveScope = value;
          if (primitiveScope !== 'part') {
            primitivePartId = null;
          } else if (!primitivePartId) {
            primitivePartId = selectedPart?.partId || modelManifest?.parts?.[0]?.partId || null;
          }
        }}
        onPrimitivePartIdChange={(value) => primitivePartId = value}
        onPrimitiveAttachToViewChange={(value) => primitiveAttachToView = value}
        onTogglePrimitiveParameter={togglePrimitiveParameter}
        onUpdatePrimitiveDraft={updatePrimitiveDraft}
        onCancelPrimitive={resetPrimitiveComposer}
        onDeletePrimitive={deleteManualPrimitive}
        onSavePrimitive={saveManualPrimitive}
        onComposerLabelChange={(value) => composerViewLabel = value}
        onComposerScopeChange={(value) => {
          composerViewScope = value;
          if (composerViewScope !== 'part') {
            composerViewPartId = null;
          } else if (!composerViewPartId) {
            composerViewPartId = selectedPart?.partId || modelManifest?.parts?.[0]?.partId || null;
          }
        }}
        onComposerPartIdChange={(value) => composerViewPartId = value}
        onToggleComposerPrimitive={toggleComposerPrimitive}
        onCancelComposer={resetComposer}
        onSaveComposer={saveManualView}
        onDeleteManualAdvisory={deleteManualAdvisory}
        onDeleteControlRelation={deleteControlRelation}
        {isSectionExpanded}
        {toggleSection}
        {getRangeProps}
        {isManualPrimitive}
        onUpdateSemanticControl={updateSemanticControl}
        onEditPrimitiveComposer={openEditPrimitiveComposer}
        onPickSemanticControlImage={pickSemanticControlImage}
        onSetFocusedControl={setFocusedControl}
        onClearFocusedControl={clearFocusedControl}
      />
      {:else}
      <ParamPanelRawTab
        {filteredFields}
        {focusedFields}
        {remainingFields}
        {selectedPart}
        parameters={effectiveLocalParams}
        {highlightedParamKey}
        liveApply={$liveApply}
        getRangeProps={(field) => getRangeProps(field as RangeLikeField)}
        getCadTone={(field) => getCadHint(field).tone}
        onDraftValue={stageParamDraft}
        onUpdate={update}
        onPickImage={pickRawImage}
        onSetFocusedControl={setFocusedControl}
        onClearFocusedControl={clearFocusedControl}
      />
      {/if}
    {/if}
  </div>
</div>

<style>
  .param-panel {
    --cad-accent: var(--primary);
    --cad-axis-x: var(--cad-accent);
    --cad-axis-y: var(--cad-accent);
    --cad-axis-z: var(--cad-accent);
    --cad-axis-angle: var(--cad-accent);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 100%;
    box-sizing: border-box;
    overflow: hidden;
  }

  .param-panel-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    overflow-x: hidden;
    padding-bottom: 12px;
    scrollbar-gutter: stable;
  }

  .warning-stack {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .warning-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    text-transform: none;
    letter-spacing: normal;
    font-weight: 500;
  }

  .part-strip {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .part-strip-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .part-chip {
    padding: 4px 8px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text-dim);
    font-size: 0.64rem;
    font-weight: 700;
    cursor: pointer;
    max-width: 100%;
    overflow: hidden;
    text-overflow: clip;
    white-space: normal;
    overflow-wrap: anywhere;
    text-align: left;
  }

  .part-chip-active {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-200));
    color: var(--text);
  }

  .part-chip-readonly {
    opacity: 0.8;
  }

  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

</style>
