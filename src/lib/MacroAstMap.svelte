<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import {
    formatBackendError,
    getAppErrorDiagnosticContext,
    macroAstSourceMap,
  } from './tauri/client';
  import MacroSourcePane from './MacroSourcePane.svelte';
  import ParamPanelControlField from './components/ParamPanelControlField.svelte';
  import { buildMacroAstMapProjection, findOwningPartId, spliceMacroSource } from './macroAstMap';
  import { buildMacroAstSceneLayout, PART_COLLAPSE_THRESHOLD } from './macroAstSceneLayout';
  import type { DesignParams, ModelManifest, ParamValue, ResolvedUiField, UiSpec } from './types/domain';

  type MacroSourcePaneState = {
    label: string;
    /** Full macro document the slice offsets are valid against (snapshot at open). */
    baseCode: string;
    scopeStart: number;
    scopeEnd: number;
    busy: boolean;
    error: string | null;
    /** Forces the CodeMirror doc to rebuild when the slice swaps. */
    revision: number;
  };

  const MACRO_PANE_UNSAVED_NOTICE =
    'Draft has unsaved edits — APPLY or CLOSE before editing another node.';

  type CadTone = 'neutral' | 'size' | 'x' | 'y' | 'z' | 'angle' | 'state' | 'mode';
  type CadHint = {
    tone: CadTone;
    tag: string;
    glyph: string;
    note: string;
  };

  type RangeLikeField = Extract<ResolvedUiField, { type: 'range' | 'number' }>;

  let {
    macroCode = '',
    modelManifest = null,
    uiSpec = null,
    parameters = {},
    fields = [],
    highlightedParamKey = null,
    liveApply = false,
    focusNodeId = null,
    onFocusNodeHandled,
    onApplyMacroCode = undefined,
    onDraftValue,
    onUpdate,
    onControlFocusChange,
  }: {
    macroCode?: string;
    modelManifest?: ModelManifest | null;
    uiSpec?: UiSpec | null;
    parameters?: DesignParams;
    fields?: ResolvedUiField[];
    highlightedParamKey?: string | null;
    liveApply?: boolean;
    focusNodeId?: string | null;
    onFocusNodeHandled?: () => void;
    onApplyMacroCode?: (code: string) => Promise<unknown>;
    onDraftValue?: (key: string, value: ParamValue) => void;
    onUpdate?: (key: string, value: ParamValue) => void;
    onControlFocusChange?: (primitiveId: string | null, parameterKey: string | null) => void;
  } = $props();

  let macroSceneViewportElement = $state<HTMLElement | null>(null);
  let macroSceneWidth = $state(1120);
  let macroViewportW = $state(1120);
  let macroViewportH = $state(560);
  let macroCamera = $state({ x: 0, y: 0, k: 1 });
  let macroCameraManual = $state(false);
  const MACRO_ZOOM_MIN = 0.3;
  const MACRO_ZOOM_MAX = 1.6;
  const MACRO_ZOOM_FAR_TIER = 0.62;
  const MACRO_MINIMAP_W = 150;

  let macroCameraTweenFrame: number | null = null;
  let macroMinimapDragging = $state(false);
  let macroPan = $state<{ startX: number; startY: number; camX: number; camY: number } | null>(null);
  let macroSourceNodes = $state<Awaited<ReturnType<typeof macroAstSourceMap>> | null>(null);
  let macroSourcePane = $state<MacroSourcePaneState | null>(null);
  let macroSourcePaneDirty = $state(false);
  /** Dense-part expansion state: session-only, no persistence (design D5). */
  let expandedParts = $state(new Set<string>());

  $effect(() => {
    const code = macroCode;
    if (!code || !code.trim()) {
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
      parameters,
      sourceNodes: macroSourceNodes,
    }),
  );
  const macroFieldByKey = $derived.by(() => new Map(fields.map((field) => [field.key, field])));
  const macroScene = $derived.by(() =>
    buildMacroAstSceneLayout(macroAstMap, { width: macroSceneWidth, expandedPartIds: expandedParts }),
  );
  const macroSceneNodeByIdMap = $derived.by(() => new Map(macroScene.nodes.map((node) => [node.id, node])));
  const macroMinimapScale = $derived.by(() =>
    Math.min(MACRO_MINIMAP_W / macroScene.width, 110 / macroScene.height),
  );

  $effect(() => {
    const scene = macroScene;
    if (macroCameraManual) return;
    macroCameraFit(scene);
  });

  $effect(() => {
    if (!focusNodeId) return;
    const target = macroScene.nodes.find((node) => node.id === focusNodeId);
    if (!target) return;
    focusMacroSceneNode(focusNodeId);
    onFocusNodeHandled?.();
  });

  // A highlighted param owned by a collapsed dense part is otherwise invisible
  // (collapsed parts emit no param scene node / overlay control): expand the
  // owning part so the highlight is actually visible (design D5).
  $effect(() => {
    const key = highlightedParamKey;
    if (!key) return;
    const partId = findOwningPartId(macroAstMap.root, key);
    if (partId) expandMacroPart(partId);
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

  function toggleMacroPartExpanded(partId: string, event?: Event) {
    event?.stopPropagation();
    const next = new Set(expandedParts);
    if (next.has(partId)) {
      next.delete(partId);
    } else {
      next.add(partId);
    }
    expandedParts = next;
  }

  function expandMacroPart(partId: string) {
    if (expandedParts.has(partId)) return;
    const next = new Set(expandedParts);
    next.add(partId);
    expandedParts = next;
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

  /**
   * A collapsed dense part emits no param scene node for its fields, so a
   * focus flow targeting one of those fields must expand the owning part
   * first and defer to the next frame — after the scene re-layout has run —
   * before continuing (design D5 auto-expand).
   */
  function expandOwningPartThen(fieldKey: string, continueWith: () => void) {
    const partId = findOwningPartId(macroAstMap.root, fieldKey);
    if (partId && !expandedParts.has(partId)) {
      expandMacroPart(partId);
      requestAnimationFrame(() => requestAnimationFrame(continueWith));
      return;
    }
    continueWith();
  }

  function selectSceneFieldValue(fieldKey: string | undefined) {
    if (!fieldKey) return;
    expandOwningPartThen(fieldKey, () => selectSceneFieldValueAfterExpand(fieldKey));
  }

  function selectSceneFieldValueAfterExpand(fieldKey: string) {
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

  function activateMacroNode(node: {
    kind: string;
    fieldKey?: string;
    id: string;
    label: string;
    sourceRange?: { startByte: number; endByte: number };
  }) {
    if (node.kind === 'param') {
      focusSceneField(node.fieldKey);
    }
  }

  function editMacroNode(node: {
    kind: string;
    fieldKey?: string;
    id: string;
    label: string;
    sourceRange?: { startByte: number; endByte: number };
  }) {
    if (node.kind === 'param') {
      selectSceneFieldValue(node.fieldKey);
    } else if (node.kind === 'part' || node.kind === 'model' || node.kind === 'verify') {
      openMacroNodeEditor(node);
    }
  }

  function macroNodeKeyDown(
    event: KeyboardEvent,
    node: {
      kind: string;
      fieldKey?: string;
      id: string;
      label: string;
      sourceRange?: { startByte: number; endByte: number };
    },
  ) {
    const target = event.target as HTMLElement | null;
    if (
      target &&
      target !== event.currentTarget &&
      target.closest('input, button, textarea, select, [contenteditable="true"]')
    ) {
      return;
    }
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    editMacroNode(node);
  }

  function focusSceneField(fieldKey: string | undefined) {
    if (!fieldKey || !macroSceneViewportElement) return;
    expandOwningPartThen(fieldKey, () => focusSceneFieldAfterExpand(fieldKey));
  }

  function focusSceneFieldAfterExpand(fieldKey: string) {
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

  /**
   * System-driven jump (diagnostic retarget, focus requests): bypasses the
   * dirty guard — the draft was already rejected or superseded — and keeps
   * the pane error so the raw backend message stays visible at the new node.
   */
  function retargetMacroSourcePane(node: {
    label: string;
    sourceRange?: { startByte: number; endByte: number };
  }) {
    if (!node.sourceRange || !onApplyMacroCode) return;
    macroSourcePaneDirty = false;
    macroSourcePane = {
      label: node.label,
      baseCode: macroCode,
      scopeStart: node.sourceRange.startByte,
      scopeEnd: node.sourceRange.endByte,
      busy: false,
      error: macroSourcePane?.error ?? null,
      revision: (macroSourcePane?.revision ?? 0) + 1,
    };
  }

  function focusMacroSceneNode(nodeId: string | undefined) {
    if (!nodeId) return;
    const target = macroScene.nodes.find((node) => node.id === nodeId);
    if (!target) return;
    macroCameraFocusNode(target, () => {
      if (target.sourceRange && onApplyMacroCode) {
        retargetMacroSourcePane(target);
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

  /** Guards a slice swap: a dirty draft must be applied or closed first. */
  function macroPaneBlocksSwitch(): boolean {
    if (!macroSourcePane || !macroSourcePaneDirty) return false;
    macroSourcePane = { ...macroSourcePane, error: MACRO_PANE_UNSAVED_NOTICE };
    return true;
  }

  function openMacroNodeEditor(node: {
    id: string;
    label: string;
    sourceRange?: { startByte: number; endByte: number };
  }) {
    if (!node.sourceRange || !onApplyMacroCode) return;
    if (macroPaneBlocksSwitch()) return;
    macroSourcePaneDirty = false;
    macroSourcePane = {
      label: node.label,
      baseCode: macroCode,
      scopeStart: node.sourceRange.startByte,
      scopeEnd: node.sourceRange.endByte,
      busy: false,
      error: null,
      revision: (macroSourcePane?.revision ?? 0) + 1,
    };
  }

  function openMacroAddPart() {
    const modelRange = macroAstMap.root.sourceRange;
    if (!modelRange || !onApplyMacroCode) return;
    if (macroPaneBlocksSwitch()) return;
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
    macroSourcePaneDirty = false;
    macroSourcePane = {
      label: `new part part_${index}`,
      baseCode: draft,
      scopeStart,
      scopeEnd: scopeStart + template.length,
      busy: false,
      error: null,
      revision: (macroSourcePane?.revision ?? 0) + 1,
    };
  }

  function closeMacroSourcePane() {
    macroSourcePane = null;
    macroSourcePaneDirty = false;
  }

  async function applyMacroSourcePane(nextSlice: string) {
    const pane = macroSourcePane;
    if (!pane || !onApplyMacroCode) return;
    macroSourcePane = { ...pane, busy: true, error: null };
    const nextCode = spliceMacroSource(pane.baseCode, pane.scopeStart, pane.scopeEnd, nextSlice);
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
      closeMacroSourcePane();
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

  function parseOptionalNumber(raw: number | '' | undefined): number | undefined {
    if (raw === null || raw === undefined || raw === '') return undefined;
    const number = Number(raw);
    return Number.isFinite(number) ? number : undefined;
  }

  function asNumber(value: ParamValue | undefined, fallback = 0): number {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : fallback;
  }

  function getRangeProps(field: RangeLikeField) {
    const rawVal = Number(parameters[field.key]);
    const val = Number.isFinite(rawVal) ? rawVal : 0;
    let min = parseOptionalNumber(field.min) ?? 0;
    if (field.minFrom && parameters[field.minFrom] !== undefined) {
      min = asNumber(parameters[field.minFrom], min);
    }

    let max = parseOptionalNumber(field.max) ?? Math.max(200, val * 4);
    if (field.maxFrom && parameters[field.maxFrom] !== undefined) {
      max = asNumber(parameters[field.maxFrom], max);
    }

    if (max < min) max = min;
    if (max === min) max = min + 1;
    const stepCandidate = parseOptionalNumber(field.step) ?? (max - min > 50 ? 1 : 0.1);
    const step = Number.isFinite(stepCandidate) && stepCandidate > 0 ? stepCandidate : 1;
    return { min, max, step };
  }

  function getCadHint(field: ResolvedUiField): CadHint {
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
      return { tone: 'z', tag: 'Z', glyph: 'Z', note: 'vertical span' };
    }
    if (
      hasFragment('width') ||
      hasFragment('left') ||
      hasFragment('right') ||
      hasToken('width', 'x')
    ) {
      return { tone: 'x', tag: 'X', glyph: 'X', note: 'lateral span' };
    }
    if (
      hasFragment('depth') ||
      hasFragment('front') ||
      hasFragment('back') ||
      hasToken('depth', 'length', 'y')
    ) {
      return { tone: 'y', tag: 'Y', glyph: 'Y', note: 'fore-aft span' };
    }
    if (hasFragment('scale') || hasToken('scale', 'size', 'thickness')) {
      return { tone: 'size', tag: 'SIZE', glyph: '<>', note: 'overall size' };
    }
    return { tone: 'neutral', tag: 'CAD', glyph: '+', note: 'parameter' };
  }

  function firstSelectedPath(selection: string | string[] | null): string | null {
    if (typeof selection === 'string') return selection;
    if (Array.isArray(selection)) return selection[0] ?? null;
    return null;
  }

  function clearFocusedControl(event: MouseEvent | FocusEvent) {
    const current = event.currentTarget as HTMLElement | null;
    const related = (event as FocusEvent).relatedTarget as Node | null;
    if (current && related && current.contains(related)) return;
    onControlFocusChange?.(null, null);
  }
</script>

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
      role="application"
      aria-label="Macro AST map"
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
              role="button"
              tabindex="0"
              onclick={() => activateMacroNode(sceneNode)}
              ondblclick={() => editMacroNode(sceneNode)}
              onkeydown={(event) => macroNodeKeyDown(event, sceneNode)}
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

              {#if sceneNode.kind === 'part' && (sceneNode.paramCount ?? 0) > PART_COLLAPSE_THRESHOLD}
                <button
                  class="macro-ast-part-collapse-chip"
                  data-testid="macro-ast-part-collapse-chip"
                  onclick={(event) => toggleMacroPartExpanded(sceneNode.id, event)}
                >
                  {sceneNode.paramCount} PARAMS
                </button>
              {/if}

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
                      value={parameters[field.key]}
                      rangeProps={field.type === 'range' || field.type === 'number' ? getRangeProps(field) : null}
                      editable={!field.frozen}
                      frozen={field.frozen}
                      autoField={field._auto}
                      highlighted={highlightedParamKey === field.key}
                      cadTone={getCadHint(field).tone}
                      liveApply={liveApply}
                      compact={true}
                      onDraftValue={(nextValue) => onDraftValue?.(field.key, nextValue)}
                      onUpdate={(nextValue) => onUpdate?.(field.key, nextValue)}
                      onPickImage={async () => {
                        const file = await open({
                          multiple: false,
                          filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }]
                        });
                        const selected = firstSelectedPath(file);
                        if (selected) onUpdate?.(field.key, selected);
                      }}
                      onMouseEnter={() => onControlFocusChange?.(null, field.key)}
                      onMouseLeave={clearFocusedControl}
                      onFocusIn={() => onControlFocusChange?.(null, field.key)}
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
          code={macroSourcePane.baseCode.slice(macroSourcePane.scopeStart, macroSourcePane.scopeEnd)}
          scopeLabel={macroSourcePane.label}
          busy={macroSourcePane.busy}
          error={macroSourcePane.error}
          onApply={(nextSlice) => void applyMacroSourcePane(nextSlice)}
          onCancel={closeMacroSourcePane}
          onDirtyChange={(dirty) => (macroSourcePaneDirty = dirty)}
        />
      {/key}
    {/if}
  </div>
</div>

<style>
  .macro-ast-map-shell {
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow: hidden;
    min-height: 0;
  }

  .controls-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
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

  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
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
    transition: box-shadow 140ms ease, border-color 140ms ease;
  }

  .macro-ast-node:hover {
    border-color: color-mix(in srgb, var(--macro-variant-accent) 70%, var(--bg-300));
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--primary) 18%, transparent),
      0 0 14px color-mix(in srgb, var(--macro-variant-accent) 22%, transparent);
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

  .macro-ast-node-editable {
    cursor: text;
  }

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

  .macro-ast-part-collapse-chip {
    position: relative;
    z-index: 1;
    margin-top: 4px;
    align-self: flex-start;
    padding: 2px 8px;
    border: 1px solid color-mix(in srgb, var(--secondary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--secondary) 10%, var(--bg-200));
    color: color-mix(in srgb, var(--text-dim) 80%, var(--secondary));
    font-family: var(--font-mono);
    font-size: 0.58rem;
    font-weight: 800;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .macro-ast-part-collapse-chip:hover {
    border-color: var(--primary);
    color: var(--primary);
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
</style>
