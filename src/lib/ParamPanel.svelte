<script lang="ts">
  import { get } from 'svelte/store';
  import { tick } from 'svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import Dropdown from './Dropdown.svelte';
  import { uiHighlightStore } from './stores/uiHighlightStore';
  import { open } from '@tauri-apps/plugin-dialog';
  import {
    formatBackendError,
    getAppErrorDiagnosticContext,
    macroAstSourceMap,
    parseMacroParams,
    saveModelManifest,
    updateParameters,
    updateUiSpec,
  } from './tauri/client';
  import { buildImportedSyntheticDesign } from './modelRuntime/importedRuntime';
  import MacroSourcePane from './MacroSourcePane.svelte';
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
  import ParamPanelControlField from './components/ParamPanelControlField.svelte';
  import ParamPanelAdvisoryList from './components/ParamPanelAdvisoryList.svelte';
  import ParamPanelContextStrip from './components/ParamPanelContextStrip.svelte';
  import ParamPanelPrimitiveComposer from './components/ParamPanelPrimitiveComposer.svelte';
  import ParamPanelAdvisoryComposer from './components/ParamPanelAdvisoryComposer.svelte';
  import ParamPanelRelationComposer from './components/ParamPanelRelationComposer.svelte';
  import ParamPanelViewComposer from './components/ParamPanelViewComposer.svelte';
  import { buildMacroAstMapProjection } from './macroAstMap';
  import { buildMacroAstSceneLayout } from './macroAstSceneLayout';
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
    LithophaneSide,
    OverflowMode,
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
  let macroSceneViewportElement = $state<HTMLElement | null>(null);
  let macroSceneWidth = $state(1120);
  let macroViewportW = $state(1120);
  let macroViewportH = $state(560);
  // Scene camera: pan/zoom over the SVG+HTML layer, Figma-style.
  let macroCamera = $state({ x: 0, y: 0, k: 1 });
  let macroCameraManual = $state(false);
  const MACRO_ZOOM_MIN = 0.3;
  const MACRO_ZOOM_MAX = 1.6;
  const MACRO_ZOOM_FAR_TIER = 0.62;

  let macroCameraTweenFrame: number | null = null;

  function macroCameraAnimateTo(
    target: { x: number; y: number; k: number },
    durationMs = 280,
    onComplete?: () => void,
  ) {
    if (macroCameraTweenFrame !== null) cancelAnimationFrame(macroCameraTweenFrame);
    const from = { ...macroCamera };
    const startedAt = performance.now();
    const easeOutCubic = (t: number) => 1 - (1 - t) ** 3;
    const step = (now: number) => {
      const t = Math.min(1, (now - startedAt) / durationMs);
      const e = easeOutCubic(t);
      macroCamera = {
        x: from.x + (target.x - from.x) * e,
        y: from.y + (target.y - from.y) * e,
        k: from.k + (target.k - from.k) * e,
      };
      if (t < 1) {
        macroCameraTweenFrame = requestAnimationFrame(step);
      } else {
        macroCameraTweenFrame = null;
        onComplete?.();
      }
    };
    macroCameraTweenFrame = requestAnimationFrame(step);
  }

  function macroCameraFit(scene: { width: number; height: number }) {
    const pad = 24;
    const k = Math.min(
      1,
      Math.max(
        MACRO_ZOOM_MIN,
        Math.min((macroViewportW - pad) / scene.width, (macroViewportH - pad) / scene.height),
      ),
    );
    const target = {
      k,
      x: Math.max(0, (macroViewportW - scene.width * k) / 2),
      y: Math.max(0, (macroViewportH - scene.height * k) / 2),
    };
    if (macroCameraManual) {
      macroCameraAnimateTo(target);
    } else {
      macroCamera = target;
    }
  }

  function macroCameraZoomBy(factor: number, cx?: number, cy?: number, animate = false) {
    const { x, y, k } = macroCamera;
    const nextK = Math.min(MACRO_ZOOM_MAX, Math.max(MACRO_ZOOM_MIN, k * factor));
    const px = cx ?? macroViewportW / 2;
    const py = cy ?? macroViewportH / 2;
    const target = {
      k: nextK,
      x: px - ((px - x) / k) * nextK,
      y: py - ((py - y) / k) * nextK,
    };
    if (animate) {
      macroCameraAnimateTo(target, 180);
    } else {
      macroCamera = target;
    }
    macroCameraManual = true;
  }

  function macroViewportWheel(event: WheelEvent) {
    event.preventDefault();
    const rect = macroSceneViewportElement?.getBoundingClientRect();
    const cx = rect ? event.clientX - rect.left : undefined;
    const cy = rect ? event.clientY - rect.top : undefined;
    if (event.ctrlKey || event.metaKey) {
      macroCameraZoomBy(Math.exp(-event.deltaY * 0.01), cx, cy);
    } else {
      macroCamera = {
        ...macroCamera,
        x: macroCamera.x - event.deltaX,
        y: macroCamera.y - event.deltaY,
      };
      macroCameraManual = true;
    }
  }

  const MACRO_MINIMAP_W = 150;
  const macroMinimapScale = $derived.by(() =>
    Math.min(MACRO_MINIMAP_W / macroScene.width, 110 / macroScene.height),
  );
  let macroMinimapDragging = $state(false);

  function macroMinimapCenterAt(event: PointerEvent, animate: boolean) {
    const rect = (event.currentTarget as Element).getBoundingClientRect();
    const sceneX = (event.clientX - rect.left) / macroMinimapScale;
    const sceneY = (event.clientY - rect.top) / macroMinimapScale;
    const target = {
      k: macroCamera.k,
      x: macroViewportW / 2 - sceneX * macroCamera.k,
      y: macroViewportH / 2 - sceneY * macroCamera.k,
    };
    if (animate) {
      macroCameraAnimateTo(target, 200);
    } else {
      macroCamera = target;
    }
    macroCameraManual = true;
  }

  let macroPan = $state<{ startX: number; startY: number; camX: number; camY: number } | null>(null);

  function macroViewportPointerDown(event: PointerEvent) {
    const target = event.target as HTMLElement | null;
    if (target?.closest('.macro-ast-node, .macro-ast-node-editor, .macro-ast-insert-slot, .macro-ast-minimap')) return;
    macroPan = {
      startX: event.clientX,
      startY: event.clientY,
      camX: macroCamera.x,
      camY: macroCamera.y,
    };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function macroViewportPointerMove(event: PointerEvent) {
    if (!macroPan) return;
    macroCamera = {
      ...macroCamera,
      x: macroPan.camX + (event.clientX - macroPan.startX),
      y: macroPan.camY + (event.clientY - macroPan.startY),
    };
    macroCameraManual = true;
  }

  function macroViewportPointerUp() {
    macroPan = null;
  }

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

  function getSelectValue(key: string): string | number | null {
    const value = effectiveLocalParams[key];
    return typeof value === 'string' || typeof value === 'number' ? value : null;
  }

  function getInputValue(event: Event): string {
    return (event.currentTarget as HTMLInputElement).value;
  }

  function getInputChecked(event: Event): boolean {
    return (event.currentTarget as HTMLInputElement).checked;
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

  function macroCameraFocusNode(
    node: { x: number; y: number; w: number; h: number },
    onArrived?: () => void,
  ) {
    macroCameraAnimateTo(
      {
        k: 1,
        x: macroViewportW / 2 - (node.x + node.w / 2),
        y: macroViewportH / 2 - (node.y + node.h / 2),
      },
      280,
      onArrived,
    );
    macroCameraManual = true;
  }

  function focusSceneFieldControl(fieldKey: string) {
    requestAnimationFrame(() => {
      const field = macroSceneViewportElement?.querySelector(`[data-param-key="${fieldKey}"]`) as HTMLElement | null;
      const focusTarget = field?.querySelector<HTMLElement>(
        'input:not([type="hidden"]), button, textarea, select, [tabindex]:not([tabindex="-1"])',
      );
      focusTarget?.focus();
    });
  }

  function selectSceneFieldControlValue(fieldKey: string) {
    requestAnimationFrame(() => {
      const input = document.getElementById(`macro-${fieldKey}`);
      if (input instanceof HTMLInputElement) {
        input.focus();
        input.select();
      }
    });
  }

  function selectSceneFieldValue(fieldKey: string | undefined) {
    if (!fieldKey) return;
    // One gesture: dblclick a knob -> caret in the value with it selected,
    // flying the camera in first when zoomed out.
    if (macroCamera.k < MACRO_ZOOM_FAR_TIER) {
      const target = macroScene.nodes.find(
        (node) => node.kind === 'param' && node.fieldKey === fieldKey,
      );
      if (target) {
        macroCameraFocusNode(target, () => selectSceneFieldControlValue(fieldKey));
        return;
      }
    }
    selectSceneFieldControlValue(fieldKey);
  }

  function focusSceneField(fieldKey: string | undefined) {
    if (!fieldKey || !macroSceneViewportElement) return;
    // Zoomed out, controls are collapsed to chips: fly the camera to the
    // module first, then focus its live control once the flight lands.
    if (macroCamera.k < MACRO_ZOOM_FAR_TIER) {
      const target = macroScene.nodes.find(
        (node) => node.kind === 'param' && node.fieldKey === fieldKey,
      );
      if (target) {
        macroCameraFocusNode(target, () => focusSceneFieldControl(fieldKey));
        return;
      }
    }
    focusSceneFieldControl(fieldKey);
  }

  function focusMacroSceneNode(nodeId: string | undefined) {
    if (!nodeId) return;
    const target = macroScene.nodes.find((node) => node.id === nodeId);
    if (!target) return;
    macroCameraFocusNode(target, () => {
      if (target.sourceRange && onApplyMacroCode) {
        openMacroNodeEditor(target);
      }
    });
  }

  function focusDiagnosticMacroNode(error: unknown) {
    const diagnostic = getAppErrorDiagnosticContext(error);
    if (diagnostic?.partKey) {
      focusMacroSceneNode(`part:${diagnostic.partKey}`);
      return;
    }
    if (diagnostic?.stableNodeKey) {
      focusMacroSceneNode(diagnostic.stableNodeKey);
    }
  }

  $effect(() => {
    if (activeTab !== 'newParams' || !pendingMacroFocusNodeId) return;
    const target = macroScene.nodes.find((node) => node.id === pendingMacroFocusNodeId);
    if (!target) return;
    focusMacroSceneNode(pendingMacroFocusNodeId);
    pendingMacroFocusNodeId = null;
  });

  $effect(() => {
    const element = macroSceneViewportElement;
    if (!element) return;
    const syncWidth = () => {
      macroSceneWidth = Math.max(960, Math.floor(element.clientWidth));
    };
    syncWidth();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => syncWidth());
    observer.observe(element);
    return () => observer.disconnect();
  });

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

  let reading = $state(false);
  let applying = $state(false);
  let committing = $state(false);

  const filteredFields = $derived.by(() => {
    return filterFieldsBySearch(mergedFields, searchQuery);
  });

  const filteredEditFields = $derived.by(() => {
    return filterFieldsBySearch(editFields, searchQuery);
  });

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

  let macroSourceNodes = $state<Awaited<ReturnType<typeof macroAstSourceMap>> | null>(null);
  $effect(() => {
    const code = macroCode;
    if (activeTab !== 'newParams' || !code || !code.trim()) {
      macroSourceNodes = null;
      return;
    }
    let cancelled = false;
    macroAstSourceMap(code)
      .then((nodes) => {
        if (!cancelled) macroSourceNodes = nodes;
      })
      .catch(() => {
        if (!cancelled) macroSourceNodes = null;
      });
    return () => {
      cancelled = true;
    };
  });

  const macroAstMap = $derived.by(() =>
    buildMacroAstMapProjection({
      macroCode,
      modelManifest,
      uiSpec,
      parameters: effectiveLocalParams,
      sourceNodes: macroSourceNodes,
    }),
  );

  type MacroSourcePaneState = {
    label: string;
    code: string;
    scopeStart: number;
    scopeEnd: number;
    busy: boolean;
    error: string | null;
    /** Forces the CodeMirror doc to rebuild when a template insert opens. */
    revision: number;
  };
  let macroSourcePane = $state<MacroSourcePaneState | null>(null);

  function openMacroNodeEditor(node: {
    id: string;
    label: string;
    sourceRange?: { startByte: number; endByte: number };
  }) {
    if (!node.sourceRange || !onApplyMacroCode) return;
    if (macroSourcePane) {
      // Scope navigation: keep the pane (and any draft) and move the highlight.
      macroSourcePane = {
        ...macroSourcePane,
        label: node.label,
        scopeStart: node.sourceRange.startByte,
        scopeEnd: node.sourceRange.endByte,
      };
      return;
    }
    macroSourcePane = {
      label: node.label,
      code: macroCode,
      scopeStart: node.sourceRange.startByte,
      scopeEnd: node.sourceRange.endByte,
      busy: false,
      error: null,
      revision: 0,
    };
  }

  function openMacroAddPart() {
    const modelRange = macroAstMap.root.sourceRange;
    if (!modelRange || !onApplyMacroCode) return;
    const existing = new Set(
      (macroSourceNodes ?? [])
        .filter((node) => node.kind === 'part' || node.kind === 'feature')
        .map((node) => node.label),
    );
    let index = existing.size + 1;
    while (existing.has(`part_${index}`)) index += 1;
    const template = `(part part_${index} (box 10 10 10))`;
    const insertAt = modelRange.endByte - 1;
    const draft = `${macroCode.slice(0, insertAt)}\n  ${template}${macroCode.slice(insertAt)}`;
    const scopeStart = insertAt + 3;
    macroSourcePane = {
      label: `new part part_${index}`,
      code: draft,
      scopeStart,
      scopeEnd: scopeStart + template.length,
      busy: false,
      error: null,
      revision: (macroSourcePane?.revision ?? 0) + 1,
    };
  }

  async function applyMacroSourcePane(nextCode: string) {
    const pane = macroSourcePane;
    if (!pane || !onApplyMacroCode) return;
    macroSourcePane = { ...pane, busy: true, error: null };
    try {
      const outcome = await onApplyMacroCode(nextCode);
      if (outcome === null || outcome === false) {
        macroSourcePane = {
          ...pane,
          busy: false,
          error: 'Apply failed. See app status for the raw backend error.',
        };
        return;
      }
      macroSourcePane = null;
    } catch (applyError) {
      const formattedError = formatBackendError(applyError);
      macroSourcePane = {
        ...pane,
        busy: false,
        error: formattedError,
      };
      focusDiagnosticMacroNode(applyError);
    }
  }

  const macroFieldByKey = $derived.by(() => new Map(mergedFields.map((field) => [field.key, field])));
  const macroScene = $derived.by(() => buildMacroAstSceneLayout(macroAstMap, { width: macroSceneWidth }));
  $effect(() => {
    const scene = macroScene;
    if (activeTab !== 'newParams' || macroCameraManual) return;
    macroCameraFit(scene);
  });
  const macroSceneNodeByIdMap = $derived.by(() => new Map(macroScene.nodes.map((node) => [node.id, node])));

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

  function getMacroNodeShapePath(kind: string, syntaxVariant?: string | null): string {
    switch (kind) {
      case 'model':
        return 'M10,8 C22,1 38,1 48,5 C58,9 72,0 84,7 C95,13 98,26 95,36 C92,49 80,57 66,59 C49,61 32,56 21,58 C10,60 2,50 2,39 C2,27 1,15 10,8 Z';
      case 'part':
        return 'M8,10 C18,2 35,1 46,5 C56,9 68,2 80,7 C91,12 97,22 95,34 C93,46 86,57 72,59 C58,61 42,56 31,58 C19,60 7,55 4,43 C1,30 2,18 8,10 Z';
      case 'port':
        return 'M10,12 C18,6 29,4 40,6 C51,8 62,4 74,6 C84,8 92,15 94,27 C96,39 92,49 83,53 C73,58 60,55 48,56 C35,57 20,58 11,50 C2,42 2,21 10,12 Z';
      case 'param':
        switch (syntaxVariant) {
          case 'checkbox':
            return 'M8,14 C14,7 24,4 36,5 C50,6 63,2 77,6 C88,9 95,18 96,29 C97,41 91,50 80,55 C68,60 53,56 41,57 C27,58 13,58 7,48 C1,38 1,23 8,14 Z';
          case 'select':
            return 'M7,13 C13,6 24,3 36,5 C48,7 62,3 76,6 C88,9 95,18 95,30 C95,42 89,51 79,55 C66,60 52,57 40,58 C26,59 12,58 6,48 C0,38 1,21 7,13 Z';
          default:
            return 'M6,12 C12,5 24,4 35,5 C48,7 61,3 74,6 C86,8 94,17 95,28 C96,40 91,50 82,55 C70,61 55,57 43,58 C28,59 14,58 7,49 C1,39 0,21 6,12 Z';
        }
      default:
        return 'M8,10 C18,2 34,1 46,5 C58,9 71,1 83,7 C94,13 97,25 95,36 C93,48 84,58 71,59 C56,60 40,56 29,58 C17,60 6,54 3,42 C1,30 2,18 8,10 Z';
    }
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
      <div class="edit-list">
      {#each filteredEditFields as field}
        {@const i = editFields.indexOf(field)}
        <div class="edit-field" class:is-freezed={field.frozen}>
          <div class="edit-row">
            <input class="input-mono edit-input" placeholder="key" bind:value={field.key} />
            <input class="input-mono edit-input flex-2" placeholder="Label" bind:value={field.label} />
            <div class="edit-select-wrap">
              <Dropdown
                options={getAvailableTypes(field).map(t => ({ id: t, name: t }))}
                bind:value={field.type}
                placeholder="Field Type"
              />
            </div>
            <label class="freeze-toggle" title="Freeze value and move to bottom">
              <input class="ui-checkbox ui-checkbox-sm" type="checkbox" bind:checked={field.frozen} />
              <span>❄️</span>
            </label>
            <button class="btn btn-xs btn-ghost" onclick={() => removeField(i)}>✕</button>
          </div>
          {#if field.type === 'range' || field.type === 'number'}
            <div class="edit-row edit-bounds">
              <input class="input-mono edit-input-sm" type="number" placeholder="min" bind:value={field.min} />
              <input class="input-mono edit-input-sm" type="number" placeholder="max" bind:value={field.max} />
              <input class="input-mono edit-input-sm" type="number" placeholder="step" bind:value={field.step} />
              <input class="input-mono edit-input-sm flex-1" placeholder="min from (key)" bind:value={field.minFrom} />
              <input class="input-mono edit-input-sm flex-1" placeholder="max from (key)" bind:value={field.maxFrom} />
            </div>
          {/if}
          {#if field.type === 'select'}
            <div class="edit-select-options">
              <div class="edit-row edit-info">
                <span class="info-tag">OPTIONS: {field.options?.length || 0}</span>
                <button class="btn btn-xs btn-ghost" onclick={() => addSelectOption(i)}>+ ADD OPTION</button>
              </div>
              {#if (field.options?.length || 0) > 0}
                {#each field.options || [] as option, optionIndex}
                  <div class="edit-row edit-select-option-row">
                    <input
                      class="input-mono edit-input flex-1"
                      placeholder="Option label"
                      bind:value={option.label}
                    />
                    <input
                      class="input-mono edit-input flex-1"
                      placeholder="Option value"
                      bind:value={option.value}
                    />
                    <button class="btn btn-xs btn-ghost" onclick={() => removeSelectOption(i, optionIndex)}>✕</button>
                  </div>
                {/each}
              {:else}
                <div class="edit-row edit-info">
                  <span class="info-tag">No options yet. Add them manually.</span>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      <button class="btn btn-xs add-field-btn" onclick={addField}>+ ADD FIELD</button>
      </div>
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
      <div class="proposal-section">
        <div class="section-label">BINDING PROPOSALS</div>
        <div class="proposal-list">
          {#each enrichmentProposals as proposal}
            <div class="proposal-card" class:proposal-card-pending={proposal.status === 'pending'}>
              <div class="proposal-head">
                <div class="proposal-label-row">
                  <span class="proposal-label">{proposal.label}</span>
                  <span class="proposal-confidence">{Math.round(proposal.confidence * 100)}%</span>
                </div>
                <span class="proposal-status proposal-status-{proposal.status}">
                  {proposal.status.toUpperCase()}
                </span>
              </div>
              <div class="proposal-meta">
                PARTS: {labelPartIds(proposal.partIds)}
              </div>
              <div class="proposal-meta">
                PARAMS: {proposal.parameterKeys?.length ? proposal.parameterKeys.join(', ') : 'No parameter keys'}
              </div>
              <div class="proposal-meta">SOURCE: {proposal.provenance}</div>
              <div class="proposal-actions">
                <button
                  class="btn btn-xs btn-primary"
                  onclick={() => updateProposalStatus(proposal.proposalId, 'accepted')}
                  disabled={proposalMutationId !== null || proposal.status === 'accepted'}
                >
                  ACCEPT
                </button>
                <button
                  class="btn btn-xs btn-ghost"
                  onclick={() => updateProposalStatus(proposal.proposalId, 'rejected')}
                  disabled={proposalMutationId !== null || proposal.status === 'rejected'}
                >
                  REJECT
                </button>
                {#if proposal.status !== 'pending'}
                  <button
                    class="btn btn-xs btn-ghost"
                    onclick={() => updateProposalStatus(proposal.proposalId, 'pending')}
                    disabled={proposalMutationId !== null}
                  >
                    RESET
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if activeTab === 'newParams'}
      <div class="macro-ast-map-shell">
        <div class="controls-head">
          <div class="section-label">MACRO AST</div>
          <div class="context-strip-actions">
            {#if onApplyMacroCode && macroAstMap.root.sourceRange}
              <button class="btn btn-xs btn-ghost macro-ast-add-part" onclick={openMacroAddPart}>
                + PART
              </button>
            {/if}
            <button class="btn btn-xs btn-ghost" aria-label="Zoom out" onclick={() => macroCameraZoomBy(1 / 1.25, undefined, undefined, true)}>−</button>
            <button class="btn btn-xs btn-ghost macro-ast-fit" onclick={() => { macroCameraManual = false; macroCameraFit(macroScene); }}>
              FIT {Math.round(macroCamera.k * 100)}%
            </button>
            <button class="btn btn-xs btn-ghost" aria-label="Zoom in" onclick={() => macroCameraZoomBy(1.25, undefined, undefined, true)}>+</button>
            <div class="macro-ast-shell-meta">SOURCE BACKED / EDIT IN PLACE</div>
          </div>
        </div>

        <div class="macro-ast-split" class:macro-ast-split-open={Boolean(macroSourcePane)}>
        <div
          bind:this={macroSceneViewportElement}
          bind:clientWidth={macroViewportW}
          bind:clientHeight={macroViewportH}
          class="macro-ast-map-viewport macro-ast-scene"
          data-zoom-tier={macroCamera.k < MACRO_ZOOM_FAR_TIER ? 'far' : 'near'}
          onwheel={macroViewportWheel}
          onpointerdown={macroViewportPointerDown}
          onpointermove={macroViewportPointerMove}
          onpointerup={macroViewportPointerUp}
          onpointercancel={macroViewportPointerUp}
        >
          <div
            class="macro-ast-camera"
            style={`width:${macroScene.width}px; height:${macroScene.height}px; transform: translate(${macroCamera.x}px, ${macroCamera.y}px) scale(${macroCamera.k});`}
          >
          <svg
            class="macro-ast-scene__svg"
            viewBox={`0 0 ${macroScene.width} ${macroScene.height}`}
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            {#each macroScene.connectors as connector}
              <path class="macro-ast-connector" d={connector.path} data-connector-id={connector.id} />
            {/each}
          </svg>

          {#each macroScene.nodes as node}
            {@const sceneNode = macroSceneNodeByIdMap.get(node.id)}
            {#if sceneNode}
              <section
                class="macro-ast-node"
                class:macro-ast-node-root={sceneNode.kind === 'model'}
                class:macro-ast-node-editable={(sceneNode.kind === 'part' || sceneNode.kind === 'model' || sceneNode.kind === 'verify') && Boolean(sceneNode.sourceRange) && Boolean(onApplyMacroCode)}
                class:macro-ast-node-part={sceneNode.kind === 'part'}
                class:macro-ast-node-port={sceneNode.kind === 'port'}
                class:macro-ast-node-param={sceneNode.kind === 'param'}
                class:macro-ast-node-verify={sceneNode.kind === 'verify'}
                data-node-id={sceneNode.id}
                data-node-kind={sceneNode.kind}
                data-syntax-variant={sceneNode.syntaxVariant}
                onclick={() => sceneNode.kind === 'param' && focusSceneField(sceneNode.fieldKey)}
                ondblclick={() => {
                  if (sceneNode.kind === 'param') {
                    selectSceneFieldValue(sceneNode.fieldKey);
                  } else if (sceneNode.kind === 'part' || sceneNode.kind === 'model' || sceneNode.kind === 'verify') {
                    openMacroNodeEditor(sceneNode);
                  }
                }}
                style={`left:${sceneNode.x}px; top:${sceneNode.y}px; width:${sceneNode.w}px; height:${sceneNode.h}px;`}
              >
                <svg
                  class="macro-ast-node__shape"
                  viewBox={`0 0 ${sceneNode.w} ${sceneNode.h}`}
                  preserveAspectRatio="none"
                  aria-hidden="true"
                >
                  <path d={sceneNode.shapePath} />
                </svg>

                <div class="macro-ast-node__header">
                  <div class="macro-ast-node__label">{sceneNode.label.toLowerCase()}</div>
                  <span class="macro-ast-syntax-badge">{sceneNode.syntaxLabel}</span>
                </div>

                {#if (sceneNode.kind === 'model' || sceneNode.kind === 'part' || sceneNode.kind === 'verify') && sceneNode.sourceRange && onApplyMacroCode}
                  <div class="macro-ast-node__hint" aria-hidden="true">dblclick: source</div>
                {/if}
                {#if sceneNode.kind === 'param'}
                  <div class="macro-ast-value-chip" aria-hidden="true">{sceneNode.value ?? '—'}</div>
                {/if}

                {#if sceneNode.kind === 'param'}
                  {@const field = sceneNode.fieldKey ? macroFieldByKey.get(sceneNode.fieldKey) : null}
                  {#if field}
                    <div class="macro-ast-node__overlay">
                      <ParamPanelControlField
                        elementId={`macro-${field.key}`}
                        field={field}
                        value={effectiveLocalParams[field.key]}
                        rangeProps={field.type === 'range' || field.type === 'number' ? getRangeProps(field) : null}
                        editable={!field.frozen}
                        frozen={field.frozen}
                        autoField={field._auto}
                        highlighted={highlightedParamKey === field.key}
                        cadTone={getCadHint(field).tone}
                        liveApply={$liveApply}
                        compact={true}
                        onDraftValue={(nextValue) => stageParamDraft(field.key, nextValue)}
                        onUpdate={(nextValue) => update(field.key, nextValue)}
                        onPickImage={async () => {
                          const file = await open({
                            multiple: false,
                            filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }]
                          });
                          const selected = firstSelectedPath(file);
                          if (selected) update(field.key, selected);
                        }}
                        onMouseEnter={() => setFocusedControl(null, field.key)}
                        onMouseLeave={clearFocusedControl}
                        onFocusIn={() => setFocusedControl(null, field.key)}
                        onFocusOut={clearFocusedControl}
                      />
                    </div>
                    <span class="macro-ast-control-anchor" aria-hidden="true"></span>
                  {/if}
                {/if}
              </section>
            {/if}
          {/each}

          {#if onApplyMacroCode && macroAstMap.root.sourceRange}
            <div
              class="macro-ast-insert-slot"
              style={`left:${macroScene.insertSlot.x}px; top:${macroScene.insertSlot.y}px; width:${macroScene.insertSlot.w}px; min-height:${macroScene.insertSlot.h}px;`}
            >
              <button class="macro-ast-insert-trigger" onclick={openMacroAddPart}>
                + ADD PART
              </button>
            </div>
          {/if}

          </div>

          <svg
            class="macro-ast-minimap"
            data-testid="macro-ast-minimap"
            width={MACRO_MINIMAP_W}
            height={Math.max(56, Math.round(macroScene.height * macroMinimapScale))}
            onpointerdown={(event) => {
              macroMinimapDragging = true;
              (event.currentTarget as Element).setPointerCapture(event.pointerId);
              macroMinimapCenterAt(event, true);
            }}
            onpointermove={(event) => macroMinimapDragging && macroMinimapCenterAt(event, false)}
            onpointerup={() => (macroMinimapDragging = false)}
            onpointercancel={() => (macroMinimapDragging = false)}
            role="presentation"
          >
            {#each macroScene.nodes.filter((node) => node.kind === 'model' || node.kind === 'part' || node.kind === 'verify') as miniNode (miniNode.id)}
              <rect
                class="minimap-node"
                class:minimap-node-model={miniNode.kind === 'model'}
                x={miniNode.x * macroMinimapScale}
                y={miniNode.y * macroMinimapScale}
                width={Math.max(2, miniNode.w * macroMinimapScale)}
                height={Math.max(2, miniNode.h * macroMinimapScale)}
              />
            {/each}
            <rect
              class="minimap-view"
              x={(-macroCamera.x / macroCamera.k) * macroMinimapScale}
              y={(-macroCamera.y / macroCamera.k) * macroMinimapScale}
              width={(macroViewportW / macroCamera.k) * macroMinimapScale}
              height={(macroViewportH / macroCamera.k) * macroMinimapScale}
            />
          </svg>
        </div>

        {#if macroSourcePane}
          {#key macroSourcePane.revision}
            <MacroSourcePane
              code={macroSourcePane.code}
              scopeLabel={macroSourcePane.label}
              scopeStart={macroSourcePane.scopeStart}
              scopeEnd={macroSourcePane.scopeEnd}
              busy={macroSourcePane.busy}
              error={macroSourcePane.error}
              onApply={(nextCode) => void applyMacroSourcePane(nextCode)}
              onCancel={() => (macroSourcePane = null)}
            />
          {/key}
        {/if}
        </div>
      </div>
    {:else if activeTab === 'litho'}
      <div class="controls-head">
        <div class="section-label">LITHOPHANE ATTACHMENTS</div>
        <div class="context-strip-actions">
          <button class="btn btn-xs btn-ghost" onclick={addLithophane}>
            + PATCH
          </button>
          {#if selectedLithophaneAttachment}
            <button
              class="btn btn-xs btn-ghost"
              onclick={() => duplicateLithophane(selectedLithophaneAttachment)}
            >
              DUPLICATE
            </button>
            <button
              class="btn btn-xs btn-ghost"
              onclick={() => deleteLithophane(selectedLithophaneAttachment.id)}
            >
              DELETE
            </button>
          {/if}
        </div>
      </div>

      {#if lithophaneAttachments.length > 0}
        <div class="part-strip">
          <div class="part-strip-list">
            {#each lithophaneAttachments as attachment}
              <button
                class="view-chip"
                class:view-chip-active={attachment.id === selectedLithoId}
                onclick={() => selectedLithoId = attachment.id}
              >
                <span>{attachment.source.kind === 'file' && attachment.source.imagePath
                  ? attachment.source.imagePath.split(/[/\\]/).pop()
                  : attachment.id}</span>
                <span class="semantic-source-badge">{attachment.enabled === false ? 'OFF' : attachment.color?.mode === 'cmyk' ? 'CMYK' : 'MONO'}</span>
              </button>
            {/each}
          </div>
        </div>

        {#if selectedLithophaneAttachment}
          {@const activeLitho = selectedLithophaneAttachment}
          {@const planarOnlyColor = activeLitho.placement?.projection === 'planar'}
          <div class="view-composer">
            <div class="composer-grid">
              <label class="primitive-picker">
                <input
                  class="ui-checkbox"
                  type="checkbox"
                  checked={activeLitho.enabled !== false}
                  onchange={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      enabled: getInputChecked(event),
                    }), getInputChecked(event) ? 'Lithophane enabled.' : 'Lithophane disabled.')}
                />
                <div class="primitive-picker__body">
                  <div class="primitive-picker__label">Attachment enabled</div>
                  <div class="primitive-picker__meta">Disabled patches stay saved but skip render.</div>
                </div>
              </label>
              <div class="composer-field">
                <div class="composer-label">TARGET PART</div>
                <Dropdown
                  options={(modelManifest?.parts || []).map((part) => ({ id: part.partId, name: part.label }))}
                  value={activeLitho.targetPartId || null}
                  onchange={(value) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      targetPartId: typeof value === 'string' ? value : '',
                    }))}
                  placeholder="Choose part..."
                />
              </div>
              <div class="composer-field">
                <div class="composer-label">IMAGE</div>
                <div class="composer-inline-actions">
                  <button
                    class="btn param-btn composer-image-select"
                    onclick={async () => {
                      const file = await open({
                        multiple: false,
                        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }]
                      });
                      const selected = firstSelectedPath(file);
                      if (selected) setLithophaneImage(activeLitho.id, selected);
                    }}
                  >
                    {activeLitho.source.kind === 'file' && activeLitho.source.imagePath
                      ? activeLitho.source.imagePath.split(/[/\\]/).pop()
                      : 'Select Image...'}
                  </button>
                  {#if activeLitho.source.kind === 'file' && activeLitho.source.imagePath}
                    <button
                      class="btn btn-xs btn-ghost"
                      onclick={() => clearLithophaneImage(activeLitho.id)}
                    >
                      CLEAR
                    </button>
                  {/if}
                </div>
              </div>
            </div>

            {#if activeLitho.source.kind === 'file' && activeLitho.source.imagePath}
              <div class="litho-preview">
                <img
                  src={previewImageUrl(activeLitho.source.imagePath) ?? ''}
                  alt="Lithophane source"
                  class="litho-preview__image"
                />
              </div>
            {/if}

            <div class="composer-grid">
              <div class="composer-field">
                <div class="composer-label">SIDE</div>
                <Dropdown
                  options={[
                    { id: 'front', name: 'Front' },
                    { id: 'back', name: 'Back' },
                    { id: 'left', name: 'Left' },
                    { id: 'right', name: 'Right' },
                    { id: 'top', name: 'Top' },
                    { id: 'bottom', name: 'Bottom' },
                  ]}
                  value={activeLitho.placement?.side}
                  onchange={(value) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      placement: {
                        ...attachment.placement,
                        side: (typeof value === 'string' ? value : 'front') as LithophaneSide,
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <div class="composer-label">PROJECTION</div>
                <Dropdown
                  options={[
                    { id: 'auto', name: 'Auto' },
                    { id: 'planar', name: 'Planar' },
                    { id: 'cylindrical', name: 'Cylindrical' },
                    { id: 'spherical', name: 'Spherical' },
                  ]}
                  value={activeLitho.placement?.projection}
                  onchange={(value) =>
                    setLithophaneProjection(activeLitho.id, (typeof value === 'string' ? value : 'auto') as ProjectionType)}
                />
              </div>
              <div class="composer-field">
                <div class="composer-label">OVERFLOW</div>
                <Dropdown
                  options={[
                    { id: 'contain', name: 'Contain' },
                    { id: 'cover', name: 'Cover' },
                    { id: 'clamp', name: 'Clamp' },
                    { id: 'bleed', name: 'Bleed' },
                  ]}
                  value={activeLitho.placement?.overflowMode}
                  onchange={(value) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      placement: {
                        ...attachment.placement,
                        overflowMode: (typeof value === 'string' ? value : 'contain') as OverflowMode,
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <div class="composer-label">COLOR MODE</div>
                <Dropdown
                  options={[
                    { id: 'mono', name: 'Mono' },
                    ...(planarOnlyColor ? [{ id: 'cmyk', name: 'CMYK' }] : []),
                  ]}
                  value={planarOnlyColor ? activeLitho.color?.mode : 'mono'}
                  onchange={(value) => setLithophaneColorMode(activeLitho.id, (typeof value === 'string' ? value : 'mono') as 'mono' | 'cmyk')}
                />
              </div>
            </div>

            {#if !planarOnlyColor}
              <div class="composer-note">
                CMYK export is only available for planar flat patches. Switch projection to PLANAR to unlock it.
              </div>
            {/if}

            <div class="composer-grid">
              <div class="composer-field">
                <label class="composer-label" for={`litho-width-${activeLitho.id}`}>WIDTH (MM)</label>
                <input
                  id={`litho-width-${activeLitho.id}`}
                  class="input-mono composer-input"
                  type="number"
                  step="0.1"
                  value={activeLitho.placement?.widthMm ?? 0}
                  oninput={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      placement: {
                        ...attachment.placement,
                        widthMm: Number(getInputValue(event)) || 0,
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <label class="composer-label" for={`litho-height-${activeLitho.id}`}>HEIGHT (MM)</label>
                <input
                  id={`litho-height-${activeLitho.id}`}
                  class="input-mono composer-input"
                  type="number"
                  step="0.1"
                  value={activeLitho.placement?.heightMm ?? 0}
                  oninput={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      placement: {
                        ...attachment.placement,
                        heightMm: Number(getInputValue(event)) || 0,
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <label class="composer-label" for={`litho-offset-x-${activeLitho.id}`}>OFFSET X (MM)</label>
                <input
                  id={`litho-offset-x-${activeLitho.id}`}
                  class="input-mono composer-input"
                  type="number"
                  step="0.1"
                  value={activeLitho.placement?.offsetXMm ?? 0}
                  oninput={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      placement: {
                        ...attachment.placement,
                        offsetXMm: Number(getInputValue(event)) || 0,
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <label class="composer-label" for={`litho-offset-y-${activeLitho.id}`}>OFFSET Y (MM)</label>
                <input
                  id={`litho-offset-y-${activeLitho.id}`}
                  class="input-mono composer-input"
                  type="number"
                  step="0.1"
                  value={activeLitho.placement?.offsetYMm ?? 0}
                  oninput={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      placement: {
                        ...attachment.placement,
                        offsetYMm: Number(getInputValue(event)) || 0,
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <label class="composer-label" for={`litho-rotation-${activeLitho.id}`}>ROTATION</label>
                <input
                  id={`litho-rotation-${activeLitho.id}`}
                  class="input-mono composer-input"
                  type="number"
                  step="1"
                  value={activeLitho.placement?.rotationDeg ?? 0}
                  oninput={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      placement: {
                        ...attachment.placement,
                        rotationDeg: Number(getInputValue(event)) || 0,
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <label class="composer-label" for={`litho-bleed-${activeLitho.id}`}>BLEED (MM)</label>
                <input
                  id={`litho-bleed-${activeLitho.id}`}
                  class="input-mono composer-input"
                  type="number"
                  step="0.1"
                  value={activeLitho.placement?.bleedMarginMm ?? 0}
                  oninput={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      placement: {
                        ...attachment.placement,
                        bleedMarginMm: Math.max(0, Number(getInputValue(event)) || 0),
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <label class="composer-label" for={`litho-depth-${activeLitho.id}`}>DEPTH (MM)</label>
                <input
                  id={`litho-depth-${activeLitho.id}`}
                  class="input-mono composer-input"
                  type="number"
                  step="0.1"
                  value={activeLitho.relief?.depthMm ?? 2}
                  oninput={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      relief: {
                        ...attachment.relief,
                        depthMm: Math.max(0.1, Number(getInputValue(event)) || 2),
                      },
                    }))}
                />
              </div>
              <div class="composer-field">
                <label class="composer-label" for={`litho-channel-${activeLitho.id}`}>CHANNEL THICKNESS</label>
                <input
                  id={`litho-channel-${activeLitho.id}`}
                  class="input-mono composer-input"
                  type="number"
                  step="0.05"
                  value={activeLitho.color?.channelThicknessMm ?? 0.4}
                  oninput={(event) =>
                    patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                      ...attachment,
                      color: {
                        ...attachment.color,
                        channelThicknessMm: Math.max(0.05, Number(getInputValue(event)) || 0.4),
                      },
                    }))}
                />
              </div>
            </div>

            <label class="primitive-picker">
              <input
                class="ui-checkbox"
                type="checkbox"
                checked={activeLitho.relief?.invert ?? false}
                onchange={(event) =>
                  patchLithophaneAttachment(activeLitho.id, (attachment) => ({
                    ...attachment,
                    relief: {
                      ...attachment.relief,
                      invert: getInputChecked(event),
                    },
                  }), getInputChecked(event) ? 'Lithophane inversion enabled.' : 'Lithophane inversion disabled.')}
              />
              <div class="primitive-picker__body">
                <div class="primitive-picker__label">Invert relief</div>
                <div class="primitive-picker__meta">Bright pixels become shallow instead of deep.</div>
              </div>
            </label>

            {#if selectedLithophaneExportArtifacts.length > 0}
              <div class="warning-stack">
                {#each selectedLithophaneExportArtifacts as exportArtifact}
                  <div class="warning-chip">
                    <span>{exportArtifact.role.toUpperCase()}: {exportArtifact.label}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      {:else}
        <div class="no-params">
          Add a lithophane patch to attach an image to the current model. It will render on Apply.
        </div>
      {/if}
    {:else if activeTab === 'views'}
      <ParamPanelContextStrip
        {controlViews}
        {activeControlViewId}
        {activeSemanticView}
        onSelectControlView={onSelectControlView}
        onOpenCreateViewComposer={openCreateViewComposer}
        onOpenPrimitiveComposer={openPrimitiveComposer}
        onOpenAdvisoryComposer={openAdvisoryComposer}
        onOpenRelationComposer={openRelationComposer}
        onOpenEditViewComposer={openEditViewComposer}
        onDeleteManualView={deleteManualView}
        {shouldShowSemanticSource}
        {semanticSourceLabel}
      />

      {#if advisoryComposerOpen}
        <ParamPanelAdvisoryComposer
          label={advisoryLabel}
          message={advisoryMessage}
          severity={advisorySeverity}
          condition={advisoryCondition}
          threshold={advisoryThreshold}
          candidateControls={advisoryCandidateControls}
          selectedPrimitiveIds={advisoryPrimitiveIds}
          canSave={advisoryCanSave}
          onLabelChange={(value) => advisoryLabel = value}
          onMessageChange={(value) => advisoryMessage = value}
          onSeverityChange={(value) => advisorySeverity = value}
          onConditionChange={(value) => advisoryCondition = value}
          onThresholdChange={(value) => advisoryThreshold = value}
          onTogglePrimitive={toggleAdvisoryPrimitive}
          onCancel={resetAdvisoryComposer}
          onSave={saveManualAdvisory}
        />
      {/if}

      {#if relationComposerOpen}
        <ParamPanelRelationComposer
          controls={advisoryCandidateControls}
          sourcePrimitiveId={relationSourcePrimitiveId}
          targetPrimitiveId={relationTargetPrimitiveId}
          mode={relationMode}
          scale={relationScale}
          offset={relationOffset}
          canSave={relationCanSave}
          onSourceChange={(value) => relationSourcePrimitiveId = value}
          onTargetChange={(value) => relationTargetPrimitiveId = value}
          onModeChange={(value) => relationMode = value}
          onScaleChange={(value) => relationScale = value}
          onOffsetChange={(value) => relationOffset = value}
          onCancel={resetRelationComposer}
          onSave={saveControlRelation}
        />
      {/if}

      {#if primitiveComposerOpen}
        <ParamPanelPrimitiveComposer
          mode={primitiveComposerMode}
          editingId={primitiveEditingId}
          label={primitiveLabel}
          scope={primitiveScope}
          partId={primitivePartId}
          attachToView={primitiveAttachToView}
          activeSemanticView={activeSemanticView
            ? {
                label: activeSemanticView.label,
                source: activeSemanticView.source,
              }
            : null}
          modelParts={modelManifest?.parts || []}
          candidateFields={primitiveCandidateFields}
          selectedParameterKeys={primitiveParameterKeys}
          selectedFields={selectedPrimitiveFields}
          bindingDrafts={primitiveBindingDrafts}
          kindPreview={primitiveKindPreview}
          canSave={primitiveCanSave}
          onLabelChange={(value) => primitiveLabel = value}
          onScopeChange={(value) => {
            primitiveScope = value;
            if (primitiveScope !== 'part') {
              primitivePartId = null;
            } else if (!primitivePartId) {
              primitivePartId = selectedPart?.partId || modelManifest?.parts?.[0]?.partId || null;
            }
          }}
          onPartIdChange={(value) => primitivePartId = value}
          onAttachToViewChange={(value) => primitiveAttachToView = value}
          onToggleParameter={togglePrimitiveParameter}
          onUpdateDraft={updatePrimitiveDraft}
          onCancel={resetPrimitiveComposer}
          onDelete={deleteManualPrimitive}
          onSave={saveManualPrimitive}
        />
      {/if}

      {#if composerOpen}
        <ParamPanelViewComposer
          mode={composerMode}
          label={composerViewLabel}
          scope={composerViewScope}
          partId={composerViewPartId}
          modelParts={modelManifest?.parts || []}
          visiblePrimitives={composerVisiblePrimitives}
          selectedPrimitiveIds={composerPrimitiveIds}
          canSave={composerCanSave}
          onLabelChange={(value) => composerViewLabel = value}
          onScopeChange={(value) => {
            composerViewScope = value;
            if (composerViewScope !== 'part') {
              composerViewPartId = null;
            } else if (!composerViewPartId) {
              composerViewPartId = selectedPart?.partId || modelManifest?.parts?.[0]?.partId || null;
            }
          }}
          onPartIdChange={(value) => composerViewPartId = value}
          onTogglePrimitive={toggleComposerPrimitive}
          onCancel={resetComposer}
          onSave={saveManualView}
        />
      {/if}

      <ParamPanelAdvisoryList
        advisories={activeSemanticView?.advisories || []}
        onDeleteManualAdvisory={deleteManualAdvisory}
      />

      {#if activeViewRelations.length > 0}
        <div class="warning-stack">
          {#each activeViewRelations as relation}
            <div class="warning-chip">
              <span>
                LINK: {relation.sourceLabel} -> {relation.targetLabel}
                {#if relation.mode === 'scale'}
                  (x{relation.scale})
                {:else if relation.mode === 'offset'}
                  (+{relation.offset})
                {:else}
                  (mirror)
                {/if}
              </span>
              <button
                class="btn btn-xs btn-ghost warning-chip-action"
                onclick={() => deleteControlRelation(relation.relationId)}
              >
                DELETE
              </button>
            </div>
          {/each}
        </div>
      {/if}

      {#if filteredSemanticSections.length > 0}
        {#each filteredSemanticSections as section}
          <div class="controls-head">
            <div class="section-label">{section.label}</div>
            {#if section.controls.length > 0}
              <button
                class="btn btn-xs btn-ghost"
                onclick={() => toggleSection(section.sectionId, section.collapsed)}
              >
                {isSectionExpanded(section.sectionId, section.collapsed) ? 'HIDE' : 'SHOW'}
              </button>
            {/if}
          </div>

          {#if isSectionExpanded(section.sectionId, section.collapsed)}
            <div class="param-list">
              {#each section.controls as control}
                {@const field = control.rawField}
                {#if field}
                  <ParamPanelControlField
                    elementId={control.primitiveId}
                    field={field}
                    value={control.value}
                    rangeProps={field.type === 'range' || field.type === 'number' ? getRangeProps(field) : null}
                    editable={control.editable}
                    highlighted={highlightedParamKey === field.key}
                    liveApply={$liveApply}
                    semanticSource={control.source}
                    showSemanticSource={shouldShowSemanticSource(control.source)}
                    canEdit={isManualPrimitive(control)}
                    onUpdate={(nextValue) => updateSemanticControl(control, nextValue)}
                    onEdit={() => openEditPrimitiveComposer(control)}
                    onPickImage={async () => {
                      const file = await open({
                        multiple: false,
                        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }]
                      });
                      const selected = firstSelectedPath(file);
                      if (selected) updateSemanticControl(control, selected);
                    }}
                    onMouseEnter={() => setFocusedControl(control.primitiveId, field.key)}
                    onMouseLeave={clearFocusedControl}
                    onFocusIn={() => setFocusedControl(control.primitiveId, field.key)}
                    onFocusOut={clearFocusedControl}
                  />
                {/if}
              {/each}
            </div>
          {/if}
        {/each}
      {:else}
        <div class="no-params">
          {selectedPart
            ? 'No semantic controls are mapped to this part yet. Open RAW for fallback.'
            : isSelectMode && (modelManifest?.selectionTargets?.length ?? 0) > 1
              ? 'Multiple face targets found. Select one in viewport; fallback waits for explicit target.'
              : 'No semantic controls match your search.'}
        </div>
      {/if}
      {:else}
      {#if filteredFields.length > 0 && focusedFields.length > 0}
        <div class="focused-section">
          <div class="controls-head">
            <div class="section-label">{selectedPart ? `${selectedPart.label} RAW` : 'RAW PART'}</div>
          </div>
          <div class="param-list">
            {#each focusedFields as field}
              <ParamPanelControlField
                elementId={field.key}
                field={field}
                value={effectiveLocalParams[field.key]}
                rangeProps={field.type === 'range' || field.type === 'number' ? getRangeProps(field) : null}
                editable={!field.frozen}
                frozen={field.frozen}
                autoField={field._auto}
                focused={true}
                highlighted={highlightedParamKey === field.key}
                cadTone={getCadHint(field).tone}
                liveApply={$liveApply}
                onDraftValue={(nextValue) => stageParamDraft(field.key, nextValue)}
                onUpdate={(nextValue) => update(field.key, nextValue)}
                onPickImage={async () => {
                  const file = await open({
                    multiple: false,
                    filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }]
                  });
                  const selected = firstSelectedPath(file);
                  if (selected) update(field.key, selected);
                }}
                onMouseEnter={() => setFocusedControl(null, field.key)}
                onMouseLeave={clearFocusedControl}
                onFocusIn={() => setFocusedControl(null, field.key)}
                onFocusOut={clearFocusedControl}
              />
            {/each}
          </div>
        </div>
      {/if}

      {#if filteredFields.length > 0 && remainingFields.length > 0}
        {#if focusedFields.length > 0}
          <div class="controls-head controls-head-secondary">
            <div class="section-label">OTHER RAW</div>
          </div>
        {/if}
        <div class="param-list">
          {#each remainingFields as field}
          <ParamPanelControlField
            elementId={field.key}
            field={field}
            value={effectiveLocalParams[field.key]}
            rangeProps={field.type === 'range' || field.type === 'number' ? getRangeProps(field) : null}
            editable={!field.frozen}
            frozen={field.frozen}
            autoField={field._auto}
            highlighted={highlightedParamKey === field.key}
            cadTone={getCadHint(field).tone}
            liveApply={$liveApply}
            onDraftValue={(nextValue) => stageParamDraft(field.key, nextValue)}
            onUpdate={(nextValue) => update(field.key, nextValue)}
            onPickImage={async () => {
              const file = await open({
                multiple: false,
                filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }]
              });
              const selected = firstSelectedPath(file);
              if (selected) update(field.key, selected);
            }}
            onMouseEnter={() => setFocusedControl(null, field.key)}
            onMouseLeave={clearFocusedControl}
            onFocusIn={() => setFocusedControl(null, field.key)}
            onFocusOut={clearFocusedControl}
          />
          {/each}
        </div>
      {:else if filteredFields.length === 0}
        <div class="no-params">
          {selectedPart
            ? 'This part has no raw controls that match your search.'
            : 'No raw controls match your search.'}
        </div>
      {/if}
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

  .panel-toolbar {
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-bottom: 1px solid var(--bg-300);
    padding-bottom: 10px;
    margin-bottom: 4px;
  }

  .search-box {
    position: relative;
    width: 100%;
  }

  .search-input {
    width: 100%;
    min-height: 42px;
    padding: 10px 36px 10px 12px;
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.86rem;
    font-weight: 600;
    line-height: 1.2;
    outline: none;
    transition:
      border-color 0.2s,
      background-color 0.2s;
  }

  .search-input:focus {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--bg-100) 88%, var(--primary) 12%);
  }

  .panel-mode-tab-compact {
    white-space: nowrap;
  }

  .clear-search {
    position: absolute;
    right: 10px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.95rem;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .clear-search:hover {
    color: var(--text);
  }

  .panel-actions {
    display: flex;
    gap: 8px;
    justify-content: space-between;
    align-items: center;
  }

  .proposal-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    overflow: hidden;
  }

  .warning-stack,
  .proposal-actions,
  .proposal-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .proposal-head {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    align-items: flex-start;
  }

  .proposal-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .proposal-list {
    flex-direction: column;
  }

  .warning-chip,
  .proposal-status {
    padding: 3px 6px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text-dim);
    font-size: 0.58rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .warning-chip,
  .proposal-status-pending {
    border-color: color-mix(in srgb, var(--primary) 45%, var(--bg-300));
    color: var(--primary);
  }

  .warning-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    text-transform: none;
    letter-spacing: normal;
    font-weight: 500;
  }

  .proposal-status-accepted {
    border-color: color-mix(in srgb, var(--secondary) 45%, var(--bg-300));
    color: var(--secondary);
  }

  .proposal-status-rejected {
    border-color: color-mix(in srgb, var(--text-dim) 45%, var(--bg-300));
    color: var(--text-dim);
  }

  .warning-chip-action {
    flex-shrink: 0;
  }

  .proposal-card {
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
  }

  .proposal-card-pending {
    border-color: color-mix(in srgb, var(--primary) 35%, var(--bg-300));
  }

  .proposal-label-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .proposal-label {
    color: var(--text);
    font-size: 0.74rem;
    font-weight: 700;
  }

  .proposal-confidence,
  .proposal-meta {
    color: var(--text-dim);
    font-size: 0.64rem;
  }

  .part-strip {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .context-strip-head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }

  .context-strip-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .panel-mode-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    overflow: visible;
    align-items: stretch;
    min-width: 0;
  }

  .panel-mode-tab {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 100%;
    padding: 5px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text-dim);
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    line-height: 1.3;
    text-align: left;
    cursor: pointer;
  }

  .panel-mode-tab-active {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-200));
    color: var(--text);
  }

  .panel-code-btn {
    margin-left: auto;
    border-color: color-mix(in srgb, var(--secondary) 55%, var(--bg-300));
    color: var(--secondary);
  }

  .part-strip-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .view-composer {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-200) 88%, var(--secondary) 12%);
    overflow: hidden;
  }

  .composer-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 10px;
  }

  .litho-preview {
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    padding: 8px;
    overflow: hidden;
  }

  .litho-preview__image {
    display: block;
    width: 100%;
    max-height: 180px;
    object-fit: contain;
    border: 1px solid var(--primary);
    background: var(--bg-100);
  }

  .composer-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .composer-label {
    color: var(--text-dim);
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.08em;
  }

  .composer-input {
    width: 100%;
  }

  .composer-inline-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .composer-image-select {
    flex: 1 1 auto;
    min-width: 0;
  }

  .composer-note {
    color: var(--text-dim);
    font-size: 0.68rem;
    line-height: 1.4;
  }

  .binding-editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: hidden;
  }

  .binding-row {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) repeat(4, minmax(0, 0.7fr));
    gap: 8px;
    align-items: center;
  }

  .binding-row__label {
    color: var(--text);
    font-size: 0.7rem;
    font-weight: 700;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .binding-input {
    min-width: 0;
    padding: 6px 8px;
    font-size: 0.7rem;
  }

  .composer-list {
    display: grid;
    gap: 8px;
    max-height: 220px;
    overflow: auto;
    padding-right: 4px;
  }

  .primitive-picker {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    cursor: pointer;
  }

  .primitive-picker__body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .primitive-picker__label {
    color: var(--text);
    font-size: 0.78rem;
    font-weight: 700;
  }

  .primitive-picker__meta {
    color: var(--text-dim);
    font-size: 0.64rem;
    line-height: 1.35;
  }

  .composer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .part-chip,
  .view-chip {
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

  .view-chip {
    display: inline-flex;
    align-items: flex-start;
    flex-wrap: wrap;
    gap: 6px;
  }

  .part-chip-active,
  .view-chip-active {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-200));
    color: var(--text);
  }

  .part-chip-readonly {
    opacity: 0.8;
  }

  .selection-kicker,
  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .selection-name {
    font-size: 0.82rem;
    font-weight: 700;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .focused-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: visible;
  }

  .controls-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }

  .controls-head-secondary {
    margin-top: 2px;
  }

  .field-action-btn {
    flex-shrink: 0;
  }

  .live-apply-group {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .edit-toolbar-left {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .apply-btn {
    min-width: 50px;
  }

  .param-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 220px), 1fr));
    gap: 12px;
    overflow: visible;
  }

  .param-field {
    --cad-tone-color: var(--cad-accent);
    display: flex;
    flex-direction: column;
    gap: 4px;
    position: relative;
    padding: 6px;
    overflow: hidden;
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--bg-100) 76%, transparent) 0%,
        color-mix(in srgb, var(--bg-200) 88%, #000 12%) 100%
      );
    border: 1px solid color-mix(in srgb, var(--bg-300) 82%, #000 18%);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 28%, transparent);
    transition: all 0.2s;
  }

  .param-field.field-select {
    overflow: visible;
    z-index: 4;
  }

  .param-field.field-select:has(:global(.custom-select.is-open)) {
    z-index: 12;
  }

  .param-field-focus {
    border-color: color-mix(in srgb, var(--primary) 55%, var(--bg-300));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--cad-tone-color) 10%, var(--bg-100)) 0%,
        color-mix(in srgb, var(--primary) 12%, var(--bg-200)) 100%
      );
  }

  .param-field:hover {
    border-color: color-mix(in srgb, var(--cad-tone-color) 35%, var(--bg-300));
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--cad-tone-color) 8%, var(--bg-100)) 0%,
        color-mix(in srgb, var(--bg-200) 82%, #000 18%) 100%
      );
  }

  .field-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 8px;
  }

  .field-title {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
    min-width: 0;
    flex-wrap: wrap;
  }

  .semantic-source-badge {
    padding: 1px 5px;
    border: 1px solid color-mix(in srgb, var(--primary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-200));
    color: var(--primary);
    font-family: var(--font-mono);
    font-size: 0.52rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    flex-shrink: 0;
  }

  .param-label {
    font-size: 0.72rem;
    color: var(--primary);
    text-transform: uppercase;
    font-weight: bold;
    white-space: normal;
    overflow: hidden;
    text-overflow: clip;
    overflow-wrap: anywhere;
    line-height: 1.25;
    letter-spacing: 0.01em;
  }

  .frozen-badge {
    font-size: 0.6rem;
    cursor: help;
  }

  .range-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .cad-range {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: 7px;
  }

  .range-value {
    font-size: 0.75rem;
    color: var(--cad-tone-color);
    font-weight: bold;
    min-width: 36px;
    text-align: right;
  }

  .cad-readout {
    padding: 2px 6px;
    border: 1px solid color-mix(in srgb, var(--cad-tone-color) 46%, var(--bg-300));
    background: color-mix(in srgb, var(--cad-tone-color) 12%, var(--bg-100));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 25%, transparent);
  }

  .param-input {
    width: 100%;
    padding: 4px 6px;
    background: var(--bg-100);
    border: 1px solid color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.75rem;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 22%, transparent);
  }

  .param-input-compact {
    width: 86px;
    min-width: 86px;
  }

  .param-input:focus {
    outline: none;
    border-color: var(--primary);
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, #000 22%, transparent),
      0 0 0 1px color-mix(in srgb, var(--primary) 18%, transparent);
  }

  .field-control {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .checkbox-wrapper {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    min-height: 42px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--cad-tone-color) 28%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 82%, #000 18%);
    cursor: pointer;
  }

  .checkbox-wrapper-checked {
    background: color-mix(in srgb, var(--cad-tone-color) 12%, var(--bg-100));
  }

  .checkbox-status {
    font-size: 0.68rem;
    color: var(--primary);
    font-weight: bold;
    letter-spacing: 0.06em;
  }

  .ui-checkbox {
    -webkit-appearance: none;
    appearance: none;
    width: 18px;
    height: 18px;
    border: 1px solid color-mix(in srgb, var(--cad-tone-color) 36%, var(--bg-300));
    background: var(--bg-100);
    display: inline-grid;
    place-content: center;
    cursor: pointer;
    margin: 0;
  }

  .ui-checkbox::after {
    content: '';
    width: 10px;
    height: 10px;
    background: var(--cad-tone-color);
    transform: scale(0);
    transition: transform 0.12s ease-in-out;
  }

  .ui-checkbox:checked::after {
    transform: scale(1);
  }

  .auto-field {
    border-left: 0;
  }

  .param-field :global(.select-trigger) {
    background: var(--bg-100);
    border-color: color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    color: var(--text);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 22%, transparent);
  }

  .param-field :global(.custom-select.is-open .select-trigger) {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
  }

  .param-field :global(.select-arrow) {
    color: var(--primary);
  }

  .param-field :global(.select-dropdown) {
    background: var(--bg-100);
    border-color: var(--primary);
  }

  .param-field :global(.select-option:hover) {
    background: color-mix(in srgb, var(--primary) 16%, var(--bg-200));
    color: var(--text);
  }

  .param-field :global(.select-option.is-selected) {
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
    color: var(--primary);
    border-left: 0;
    padding-left: 12px;
  }

  .param-freezed {
    opacity: 0.5;
  }

  .macro-ast-map-shell {
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow: hidden;
    min-height: 0;
  }

  .macro-ast-shell-meta {
    font-size: 0.6rem;
    font-weight: 700;
    letter-spacing: 0.16em;
    color: var(--secondary);
    text-transform: uppercase;
  }

  .macro-ast-map-viewport {
    position: relative;
    overflow: hidden;
    height: clamp(420px, 58vh, 720px);
    border: 1px solid color-mix(in srgb, var(--secondary) 40%, var(--bg-300));
    background:
      radial-gradient(color-mix(in srgb, var(--secondary) 16%, transparent) 1px, transparent 1px),
      radial-gradient(circle at top right, color-mix(in srgb, var(--secondary) 10%, transparent), transparent 44%),
      linear-gradient(180deg, color-mix(in srgb, var(--bg-100) 92%, var(--secondary) 8%), var(--bg-100));
    background-size: 22px 22px, auto, auto;
    padding: 0;
    cursor: grab;
    touch-action: none;
  }

  .macro-ast-map-viewport:active {
    cursor: grabbing;
  }

  .macro-ast-camera {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
    will-change: transform;
  }

  /* Semantic zoom: zoomed out, controls give way to dense value chips. */
  .macro-ast-map-viewport[data-zoom-tier='far'] .macro-ast-node__overlay,
  .macro-ast-map-viewport[data-zoom-tier='far'] .macro-ast-node__hint {
    display: none;
  }

  .macro-ast-value-chip {
    display: none;
    position: relative;
    z-index: 1;
    margin-top: 2px;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    font-weight: 700;
    color: var(--primary);
    letter-spacing: 0.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .macro-ast-map-viewport[data-zoom-tier='far'] .macro-ast-value-chip {
    display: block;
  }

  .macro-ast-scene {
    position: relative;
    overflow: hidden;
    min-height: 0;
  }

  .macro-ast-scene__svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 0;
  }

  .macro-ast-connector {
    fill: none;
    stroke: color-mix(in srgb, var(--secondary) 48%, var(--primary) 10%);
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.68;
    filter: drop-shadow(0 0 8px color-mix(in srgb, var(--secondary) 22%, transparent));
  }

  .macro-ast-split {
    display: flex;
    gap: 8px;
    min-width: 0;
  }

  .macro-ast-split > .macro-ast-map-viewport {
    flex: 1;
    min-width: 0;
  }

  .macro-ast-split :global(.macro-source-pane) {
    width: 44%;
    min-width: 320px;
    height: clamp(420px, 58vh, 720px);
  }

  .macro-ast-insert-slot {
    position: absolute;
    z-index: 2;
    display: flex;
    flex-direction: column;
    border: 1px dashed color-mix(in srgb, var(--secondary) 45%, transparent);
    background: color-mix(in srgb, var(--bg-200) 40%, transparent);
    transition: border-color 140ms ease, background 140ms ease;
  }

  .macro-ast-insert-slot:hover {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--bg-200) 70%, var(--secondary) 8%);
  }

  .macro-ast-insert-slot-open {
    border-style: solid;
    border-color: var(--primary);
    background: color-mix(in srgb, var(--bg-primary, #0b0e13) 92%, transparent);
  }

  .macro-ast-insert-trigger {
    flex: 1;
    background: transparent;
    border: 0;
    color: color-mix(in srgb, var(--text-dim) 85%, var(--secondary));
    font-family: var(--font-mono);
    font-size: 0.66rem;
    font-weight: 800;
    letter-spacing: 0.16em;
    cursor: pointer;
  }

  .macro-ast-insert-trigger:hover {
    color: var(--primary);
  }

  .macro-ast-minimap {
    position: absolute;
    right: 10px;
    bottom: 10px;
    z-index: 6;
    border: 1px solid color-mix(in srgb, var(--secondary) 55%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-primary, #0b0e13) 86%, transparent);
    cursor: crosshair;
    display: block;
  }

  .macro-ast-minimap rect.minimap-node {
    fill: color-mix(in srgb, var(--secondary) 38%, var(--bg-300));
    stroke: none;
  }

  .macro-ast-minimap rect.minimap-node-model {
    fill: color-mix(in srgb, var(--primary) 45%, var(--bg-300));
  }

  .macro-ast-minimap rect.minimap-view {
    fill: none;
    stroke: var(--primary);
    stroke-width: 1.2;
  }

  .macro-ast-node {
    transition: box-shadow 140ms ease, border-color 140ms ease;
  }

  .macro-ast-node:hover {
    border-color: color-mix(in srgb, var(--macro-variant-accent) 70%, var(--bg-300));
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--primary) 18%, transparent),
      0 0 14px color-mix(in srgb, var(--macro-variant-accent) 22%, transparent);
  }

  .macro-ast-node-editor {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 12px;
    z-index: 5;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px;
    background: color-mix(in srgb, var(--bg-primary, #0b0e13) 92%, transparent);
    border: 1px solid var(--primary);
    overflow: hidden;
  }

  .macro-ast-node-editor__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .macro-ast-node-editor__input {
    width: 100%;
    min-height: 110px;
    resize: vertical;
    background: transparent;
    color: var(--text-primary, #e6e1d6);
    border: 1px solid color-mix(in srgb, var(--secondary) 55%, transparent);
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    line-height: 1.45;
    padding: 8px;
  }

  .macro-ast-node-editor__error {
    color: var(--error, #e06c5a);
    font-size: 11px;
    white-space: pre-wrap;
  }

  .macro-ast-node-editor__actions {
    display: flex;
    justify-content: flex-end;
  }

  .macro-ast-node {
    --macro-variant-accent: var(--secondary);
    position: absolute;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--macro-variant-accent) 30%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-200) 84%, var(--macro-variant-accent) 16%);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--primary) 12%, transparent);
    padding: 8px 10px;
    z-index: 1;
    display: flex;
    flex-direction: column;
  }

  .macro-ast-node__shape {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
    pointer-events: none;
    opacity: 0.88;
    filter: drop-shadow(0 0 10px color-mix(in srgb, var(--macro-variant-accent) 24%, transparent));
  }

  .macro-ast-node__shape path {
    fill: color-mix(in srgb, var(--macro-variant-accent) 10%, var(--bg-200));
    stroke: color-mix(in srgb, var(--macro-variant-accent) 70%, var(--bg-300));
    stroke-width: 1.1;
  }

  .macro-ast-node-root {
    --macro-variant-accent: var(--secondary);
    background: color-mix(in srgb, var(--bg-200) 76%, var(--secondary) 24%);
    border-color: color-mix(in srgb, var(--secondary) 55%, var(--bg-300));
  }

  .macro-ast-node-root {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    padding: 4px 12px;
  }

  .macro-ast-node-root .macro-ast-node__header {
    flex: 1;
  }

  .macro-ast-node-param {
    background: color-mix(in srgb, var(--bg-200) 92%, var(--secondary) 8%);
    cursor: text;
    padding: 5px 8px 5px 14px;
  }

  /* devtools-style: any editable scope region invites a text caret. */
  .macro-ast-node-editable {
    cursor: text;
  }

  /* Input port: a dot on the module's left edge instead of a nested block. */
  .macro-ast-node-param::before {
    content: '';
    position: absolute;
    left: -1px;
    top: calc(50% - 5px);
    width: 9px;
    height: 9px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--primary) 70%, var(--bg-300));
    background: color-mix(in srgb, var(--secondary) 45%, var(--bg-200));
    box-shadow: 0 0 8px color-mix(in srgb, var(--secondary) 30%, transparent);
    z-index: 2;
  }

  .macro-ast-node-param:focus-within {
    outline: 1px solid color-mix(in srgb, var(--primary) 55%, transparent);
    outline-offset: 1px;
  }

  .macro-ast-node[data-syntax-variant='number'],
  .macro-ast-node[data-syntax-variant='range'] {
    --macro-variant-accent: var(--cad-axis-x);
  }

  .macro-ast-node[data-syntax-variant='checkbox'] {
    --macro-variant-accent: var(--cad-axis-y);
  }

  .macro-ast-node[data-syntax-variant='select'] {
    --macro-variant-accent: var(--cad-axis-z);
  }

  .macro-ast-node[data-syntax-variant='image'] {
    --macro-variant-accent: var(--primary);
  }

  .macro-ast-node[data-syntax-variant='solid'],
  .macro-ast-node[data-syntax-variant='shell'],
  .macro-ast-node[data-syntax-variant='feature'],
  .macro-ast-node[data-syntax-variant='assembly'],
  .macro-ast-node[data-syntax-variant='group'] {
    --macro-variant-accent: var(--secondary);
  }

  .macro-ast-node__header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
    position: relative;
    z-index: 1;
  }

  .macro-ast-node__header-param {
    align-items: center;
  }

  .macro-ast-syntax-badge {
    flex-shrink: 0;
    padding: 1px 6px;
    border: 1px solid color-mix(in srgb, var(--macro-variant-accent) 42%, var(--bg-400));
    background: color-mix(in srgb, var(--macro-variant-accent) 14%, var(--bg-200));
    color: var(--macro-variant-accent);
    font-family: var(--font-mono);
    font-size: 0.52rem;
    font-weight: 800;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .macro-ast-node__label {
    font-size: 0.72rem;
    font-weight: 700;
    color: var(--text);
    letter-spacing: 0.04em;
  }

  .macro-ast-node__hint {
    margin-top: 2px;
    font-size: 0.52rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: color-mix(in srgb, var(--text-dim) 70%, transparent);
    text-transform: uppercase;
    position: relative;
    z-index: 1;
  }

  .macro-ast-node-root .macro-ast-node__hint {
    margin-top: 0;
  }

  .macro-ast-node__overlay {
    position: relative;
    z-index: 1;
    margin-top: 2px;
  }

  .macro-ast-control-anchor {
    position: absolute;
    right: 10px;
    top: calc(50% - 5px);
    width: 10px;
    height: 10px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--primary) 65%, var(--bg-300));
    background: color-mix(in srgb, var(--secondary) 40%, var(--primary) 35%);
    box-shadow: 0 0 10px color-mix(in srgb, var(--secondary) 35%, transparent);
    z-index: 2;
    pointer-events: none;
  }

  .macro-ast-node :global(.param-field) {
    position: relative;
    z-index: 1;
  }

  .no-params {
    font-size: 0.7rem;
    color: var(--text-dim);
    font-style: italic;
    padding: 20px;
    text-align: center;
  }

  /* Edit mode */
  .edit-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .edit-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
  }

  .edit-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .edit-input {
    flex: 1;
    padding: 4px 6px;
    background: var(--bg);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.7rem;
  }

  .edit-input:focus, .edit-input-sm:focus {
    border-color: var(--primary);
    outline: none;
  }

  .flex-2 { flex: 2; }

  .edit-select-wrap {
    width: 132px;
  }

  .edit-bounds {
    padding-left: 4px;
  }

  .edit-input-sm {
    width: 60px;
    padding: 3px 5px;
    background: var(--bg);
    border: 1px solid var(--bg-300);
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.65rem;
  }

  .freeze-toggle {
    display: flex;
    align-items: center;
    gap: 2px;
    cursor: pointer;
    font-size: 0.8rem;
    user-select: none;
  }

  .freeze-toggle input {
    margin: 0;
  }

  .edit-info {
    font-size: 0.6rem;
    color: var(--text-dim);
    padding-left: 4px;
    align-items: center;
    gap: 8px;
  }

  .edit-select-options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-left: 4px;
  }

  .edit-select-option-row {
    align-items: center;
  }

  .info-tag {
    background: var(--bg-300);
    padding: 1px 4px;
    border-radius: 2px;
  }

  .add-field-btn {
    align-self: flex-start;
  }

  .btn-xs {
    padding: 2px 6px;
    font-size: 0.6rem;
  }

  @keyframes highlightPulse {
    0% { background-color: transparent; }
    50% { background-color: var(--primary); color: var(--bg-100); }
    100% { background-color: transparent; }
  }
  .highlight-pulse {
    animation: highlightPulse 2s ease-in-out;
  }
</style>
