<script lang="ts">
  import { get } from 'svelte/store';
  import Dropdown from './Dropdown.svelte';
  import {
    formatBackendError,
    parseMacroParams,
    saveModelManifest,
    updateParameters,
    updateUiSpec,
  } from './tauri/client';
  import { buildImportedSyntheticDesign } from './modelRuntime/importedRuntime';
  import type {
    MaterializedSemanticControl,
    MaterializedSemanticView,
  } from './modelRuntime/semanticControls';
  import { persistLastSessionSnapshot } from './modelRuntime/sessionSnapshot';
  import { activeThreadId, history } from './stores/domainState';
  import { liveApply } from './stores/paramPanelState';
  import { session } from './stores/sessionStore';
  import type {
    CheckboxField,
    AdvisoryCondition,
    AdvisorySeverity,
    ControlPrimitive,
    ControlPrimitiveKind,
    ControlRelationMode,
    ControlView,
    ControlViewScope,
    DesignParams,
    EnrichmentProposal,
    EnrichmentStatus,
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
  type EditableUiField =
    | EditableRangeField
    | EditableNumberField
    | EditableSelectField
    | EditableCheckboxField;
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
  type PrimitiveBindingDraft = {
    parameterKey: string;
    scale: string;
    offset: string;
    min: string;
    max: string;
  };

  let {
    uiSpec = $bindable(null),
    parameters = {},
    modelManifest = null,
    controlViews = [],
    activeControlViewId = null,
    selectedPartId = null,
    onSelectControlView,
    onSelectPart,
    onSemanticChange,
    onchange,
    onspecchange,
    activeVersionId = null,
    messageId = null,
    macroCode = '',
  }: {
    uiSpec?: UiSpec | null;
    parameters?: DesignParams;
    modelManifest?: ModelManifest | null;
    controlViews?: MaterializedSemanticView[];
    activeControlViewId?: string | null;
    selectedPartId?: string | null;
    onSelectControlView?: (viewId: string | null) => void;
    onSelectPart?: (partId: string | null) => void;
    onSemanticChange?: (primitiveId: string, value: ParamValue) => Promise<void> | void;
    onchange?: (params: DesignParams) => Promise<void> | void;
    onspecchange?: (uiSpec: UiSpec, params: DesignParams) => void;
    activeVersionId?: string | null;
    messageId?: string | null;
    macroCode?: string;
  } = $props();

  let editing = $state(false);
  let editFields = $state<EditableUiField[]>([]);
  let localParams = $state<DesignParams>({});
  let hasPendingChanges = $derived(JSON.stringify(localParams) !== JSON.stringify(parameters));
  let saveValuesState = $state<'idle' | 'saving' | 'saved'>('idle');
  let macroParamKeys = $state<Set<string> | null>(null);
  let macroParseSeq = 0;
  let localSelectedPartId = $state<string | null>(null);
  let proposalMutationId = $state<string | null>(null);
  let activeTab = $state<'views' | 'raw'>('views');
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
  let lastIncomingParamsSignature = $state('');

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
    }
  }

  function asNumber(value: ParamValue | undefined, fallback = 0): number {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : fallback;
  }

  function getSelectValue(key: string): string | number | null {
    const value = localParams[key];
    return typeof value === 'string' || typeof value === 'number' ? value : null;
  }

  function getInputValue(event: Event): string {
    return (event.currentTarget as HTMLInputElement).value;
  }

  function getInputChecked(event: Event): boolean {
    return (event.currentTarget as HTMLInputElement).checked;
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
      lastVersionId = null;
      lastIncomingParamsSignature = incomingParamsSignature;
      editing = false;
      editFields = [];
      return;
    }

    // If we switched to a different version/thread, we must reset everything
    if (activeVersionId !== lastVersionId) {
      localParams = { ...parameters };
      lastVersionId = activeVersionId;
      lastIncomingParamsSignature = incomingParamsSignature;
      editing = false;
      editFields = [];
      return;
    }

    if (incomingParamsSignature !== lastIncomingParamsSignature && !editing) {
      localParams = { ...parameters };
      lastIncomingParamsSignature = incomingParamsSignature;
      return;
    }

    // Same version: keep local edits intact while user has pending changes or edits controls.
    // Otherwise, hard-sync to canonical persisted parameters (prunes stale keys).
    if (editing || hasPendingChanges) {
      return;
    }

    if (JSON.stringify(localParams) !== incomingParamsSignature) {
      localParams = { ...parameters };
    }
    lastIncomingParamsSignature = incomingParamsSignature;
  });

  // Merge: each key in localParams not covered by uiSpec.fields gets a generated "number" field
  const mergedFields = $derived.by(() => {
    const specFields = uiSpec?.fields || [];
    const keys = macroParamKeys;
    const filteredSpecFields = keys
      ? specFields.filter((field) => keys.has(field.key))
      : specFields;
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
    const merged = [...existingFields];
    const seenKeys = new Set(
      existingFields.map((field) => field.key.trim()).filter((key) => key.length > 0),
    );

    for (const parsedField of parsedFields) {
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
  let searchQuery = $state('');

  const filteredFields = $derived.by(() => {
    if (!searchQuery.trim()) return mergedFields;
    const query = searchQuery.toLowerCase();
    return mergedFields.filter(f => 
      f.key.toLowerCase().includes(query) || 
      (f.label && f.label.toLowerCase().includes(query))
    );
  });

  const filteredEditFields = $derived.by(() => {
    if (!searchQuery.trim()) return editFields;
    const query = searchQuery.toLowerCase();
    return editFields.filter(f => 
      f.key.toLowerCase().includes(query) || 
      (f.label && f.label.toLowerCase().includes(query))
    );
  });

  $effect(() => {
    if (editing) return;
    if (controlViews.length === 0) {
      hadSemanticViews = false;
      activeTab = 'raw';
      return;
    }
    if (!hadSemanticViews) {
      hadSemanticViews = true;
      activeTab = 'views';
    }
  });

  $effect(() => {
    localSelectedPartId = selectedPartId;
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

  const editablePartCount = $derived(
    modelManifest?.parts?.filter((part) => part.editable).length ?? 0,
  );

  const inspectOnlyPartCount = $derived(
    modelManifest?.parts?.filter((part) => !part.editable).length ?? 0,
  );

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

  const pendingProposalCount = $derived(
    enrichmentProposals.filter((proposal) => proposal.status === 'pending').length,
  );

  const selectedGroups = $derived.by<ParameterGroup[]>(() => {
    if (!localSelectedPartId || !modelManifest?.parameterGroups?.length) return [];
    const selectedId = localSelectedPartId;
    return modelManifest.parameterGroups.filter((group) =>
      (group.partIds || []).includes(selectedId),
    );
  });

  const selectedParameterKeys = $derived.by(() => {
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
    if (!activeSemanticView) return [];
    if (!searchQuery.trim()) return activeSemanticView.sections;
    const query = searchQuery.toLowerCase();
    return activeSemanticView.sections
      .map((section) => ({
        ...section,
        controls: section.controls.filter((control) => {
          const signature = `${control.label} ${control.rawField?.key ?? ''}`.toLowerCase();
          return signature.includes(query);
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
        partLabels: (primitive.partIds || [])
          .map((partId) => partsById.get(partId)?.label || partId)
          .filter(Boolean),
      }))
      .sort((left, right) => left.label.localeCompare(right.label));
  });

  const composerVisiblePrimitives = $derived.by(() => {
    if (composerViewScope !== 'part' || !composerViewPartId) {
      return primitiveCatalog;
    }
    return primitiveCatalog.filter(
      (primitive) =>
        primitive.partIds.length === 0 || primitive.partIds.includes(composerViewPartId as string),
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
      console.warn('ParamPanel: macroCode is empty, skipping readFromMacro');
      return;
    }
    reading = true;
    console.log('ParamPanel: invoking parse_macro_params...');
    
    try {
      const result = await parseMacroParams(macroCode);
      console.log('ParamPanel: parse_macro_params result:', result);
      const { fields, params } = result;

      if (fields && fields.length > 0) {
        editFields = mergeParsedEditFields(editFields, fields);
        localParams = { ...params, ...localParams };
        console.log('ParamPanel: Updated editFields with', fields.length, 'fields');
      } else {
        console.warn('ParamPanel: parse_macro_params returned no fields');
      }
    } catch (e: unknown) {
      console.error('ParamPanel: Failed to parse macro params:', e);
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

  function update(key: string, value: ParamValue) {
    let clampedValue = value;
    const field = mergedFields.find(f => f.key === key);
    if (field && (field.type === 'range' || field.type === 'number')) {
      if (typeof value !== 'number' || !Number.isFinite(value)) return;
      const props = getRangeProps(field);
      clampedValue = Math.max(props.min, Math.min(props.max, value));
    }

    const nextParams: DesignParams = { ...localParams, [key]: clampedValue };

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

    localParams = nextParams;
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
    return control.primitiveId.startsWith('primitive-manual-');
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
    console.log('ParamPanel: applyChanges clicked', { localParams, hasPendingChanges, live: $liveApply });
    if (onchange) {
      applying = true;
      session.setError(null);
      try {
        await onchange(localParams);
      } catch (e: unknown) {
        console.error('ParamPanel: onchange failed', e);
        session.setError(`Apply Failed: ${formatBackendError(e)}`);
      } finally {
        applying = false;
      }
    } else {
      console.warn('ParamPanel: onchange prop is missing!');
      session.setError('Apply Failed: parameter change handler is missing.');
    }
  }

  async function saveValues() {
    if (!activeVersionId) return;
    saveValuesState = 'saving';
    try {
      await updateParameters(activeVersionId, localParams);
      saveValuesState = 'saved';
      setTimeout(() => {
        if (saveValuesState === 'saved') saveValuesState = 'idle';
      }, 1500);
    } catch (e: unknown) {
      console.error('Failed to save defaults:', formatBackendError(e));
      saveValuesState = 'idle';
    }
  }

  function getRangeProps(field: RangeLikeField) {
    const rawVal = Number(localParams[field.key]);
    const val = Number.isFinite(rawVal) ? rawVal : 0;
    let min = parseOptionalNumber(field.min) ?? 0;
    if (field.minFrom && localParams[field.minFrom] !== undefined) {
      min = asNumber(localParams[field.minFrom], min);
    }

    let max = parseOptionalNumber(field.max) ?? Math.max(200, val * 4);
    if (field.maxFrom && localParams[field.maxFrom] !== undefined) {
      max = asNumber(localParams[field.maxFrom], max);
    }

    if (max < min) max = min;
    if (max === min) max = min + 1;
    const stepCandidate = parseOptionalNumber(field.step) ?? (max - min > 50 ? 1 : 0.1);
    const step = Number.isFinite(stepCandidate) && stepCandidate > 0 ? stepCandidate : 1;
    return { min, max, step };
  }

  function getAvailableTypes(field: EditableUiField | ResolvedUiField) {
    // If it's boolean in parameters, don't allow range/number?
    // User said "booleans, it can't be turned to range"
    const val = parameters[field.key];
    if (typeof val === 'boolean' || field.type === 'checkbox') {
      return ['checkbox'];
    }
    if (field.type === 'select') {
      return ['select'];
    }
    return ['range', 'number'];
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
  <div class="panel-toolbar">
    <div class="search-box">
      <input 
        type="text" 
        placeholder="Search controls..." 
        bind:value={searchQuery}
        class="search-input"
      />
      {#if searchQuery}
        <button class="clear-search" onclick={() => searchQuery = ''}>✕</button>
      {/if}
    </div>
  </div>

  <div class="panel-actions">
    {#if !editing}
      <div class="live-apply-group">
        <label class="live-toggle" title="Update geometry immediately on every change">
          <input class="ui-checkbox" type="checkbox" bind:checked={$liveApply} />
          <span>LIVE</span>
        </label>
        <button 
          class="btn btn-xs btn-primary apply-btn" 
          onclick={applyChanges} 
          disabled={$liveApply || applying}
        >
          {#if applying}
            APPLYING...
          {:else}
            APPLY
          {/if}
        </button>
        <button
          class="btn btn-xs btn-ghost"
          onclick={saveValues}
          disabled={!activeVersionId || saveValuesState === 'saving'}
          title={activeVersionId ? 'Persist current values as defaults for this version' : 'Generate first to persist defaults'}
        >
          {#if saveValuesState === 'saving'}
            SAVING...
          {:else if saveValuesState === 'saved'}
            SAVED
          {:else}
            SAVE VALUES
          {/if}
        </button>
      </div>
      <button class="btn btn-xs" onclick={startEditing} title="Edit controls">✏️ EDIT CONTROLS</button>
    {:else}
      <div class="edit-toolbar-left">
        <button class="btn btn-xs btn-primary" onclick={saveFields}>💾 SAVE</button>
        <button class="btn btn-xs btn-ghost" onclick={cancelEditing}>✕ CANCEL</button>
      </div>
      <button class="btn btn-xs btn-secondary" onclick={readFromMacro} title="Auto-detect parameters from macro code" disabled={reading}>
        {#if reading}
          ⏳ READING...
        {:else}
          🔍 READ FROM MACRO
        {/if}
      </button>
    {/if}
  </div>

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
      <div class="runtime-strip model-status-card">
        <div class="runtime-strip__identity">
          <div class="runtime-strip__title">{modelManifest.document.documentLabel || modelManifest.modelId}</div>
          {#if selectedPart}
            <div class="runtime-strip__context">{selectedPart.label}</div>
          {/if}
        </div>
        <div class="runtime-strip__badges">
          <span class="status-chip" class:status-chip-imported={modelManifest.sourceKind === 'importedFcstd'}>
            {modelManifest.sourceKind === 'importedFcstd' ? 'IMPORTED FCSTD' : 'GENERATED'}
          </span>
          <span class="status-chip status-chip-editable">
            {editablePartCount} editable
          </span>
          {#if inspectOnlyPartCount > 0}
            <span class="status-chip status-chip-readonly">
              {inspectOnlyPartCount} inspect-only
            </span>
          {/if}
          {#if pendingProposalCount > 0}
            <span class="status-chip status-chip-pending">
              {pendingProposalCount} proposal{pendingProposalCount === 1 ? '' : 's'} pending
            </span>
          {/if}
        </div>
      </div>
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
              onclick={() => selectPart(part.partId)}
              title={part.editable ? 'Select part controls' : 'Inspect-only part'}
            >
              {part.label}
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <div class="panel-mode-tabs">
      {#if controlViews.length > 0}
        <button
          class="panel-mode-tab"
          class:panel-mode-tab-active={activeTab === 'views'}
          onclick={() => activeTab = 'views'}
        >
          VIEWS
        </button>
      {/if}
      <button
        class="panel-mode-tab"
        class:panel-mode-tab-active={activeTab === 'raw'}
        onclick={() => activeTab = 'raw'}
      >
        RAW
      </button>
    </div>

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

    {#if activeTab === 'views' && controlViews.length > 0}
      <div class="part-strip">
        <div class="context-strip-head">
          <div class="section-label">CONTEXTS</div>
          <div class="context-strip-actions">
            <button class="btn btn-xs btn-ghost" onclick={openCreateViewComposer}>
              + VIEW
            </button>
            <button class="btn btn-xs btn-ghost" onclick={openPrimitiveComposer}>
              + KNOB
            </button>
            <button class="btn btn-xs btn-ghost" onclick={openAdvisoryComposer}>
              + RULE
            </button>
            <button class="btn btn-xs btn-ghost" onclick={openRelationComposer}>
              + LINK
            </button>
            {#if activeSemanticView?.source === 'manual'}
              <button class="btn btn-xs btn-ghost" onclick={() => openEditViewComposer(activeSemanticView)}>
                EDIT
              </button>
              <button class="btn btn-xs btn-ghost" onclick={() => deleteManualView(activeSemanticView.viewId)}>
                DELETE
              </button>
            {/if}
          </div>
        </div>
        <div class="part-strip-list">
          {#each controlViews as view}
            <button
              class="view-chip"
              class:view-chip-active={view.viewId === activeControlViewId}
              onclick={() => onSelectControlView?.(view.viewId)}
            >
              {view.label}
            </button>
          {/each}
        </div>
      </div>

      {#if advisoryComposerOpen}
        <div class="view-composer">
          <div class="controls-head">
            <div class="section-label">NEW RULE</div>
          </div>
          <div class="composer-grid">
            <div class="composer-field">
              <label class="composer-label" for="composer-advisory-label">RULE NAME</label>
              <input
                id="composer-advisory-label"
                class="input-mono composer-input"
                bind:value={advisoryLabel}
                placeholder="Connector Fit / Thin Wall / Clearance Check..."
              />
            </div>
            <div class="composer-field">
              <div class="composer-label">SEVERITY</div>
              <Dropdown
                options={[
                  { id: 'warning', name: 'Warning' },
                  { id: 'info', name: 'Info' },
                ]}
                value={advisorySeverity}
                onchange={(value) => advisorySeverity = value === 'info' ? 'info' : 'warning'}
              />
            </div>
            <div class="composer-field">
              <div class="composer-label">CONDITION</div>
              <Dropdown
                options={[
                  { id: 'always', name: 'Always' },
                  { id: 'below', name: 'Below threshold' },
                  { id: 'above', name: 'Above threshold' },
                ]}
                value={advisoryCondition}
                onchange={(value) => advisoryCondition = value === 'below' || value === 'above' ? value : 'always'}
              />
            </div>
            {#if advisoryCondition !== 'always'}
              <div class="composer-field">
                <label class="composer-label" for="composer-advisory-threshold">THRESHOLD</label>
                <input
                  id="composer-advisory-threshold"
                  class="input-mono composer-input"
                  type="number"
                  step="0.01"
                  bind:value={advisoryThreshold}
                  placeholder="1.2"
                />
              </div>
            {/if}
          </div>
          <div class="composer-field">
            <label class="composer-label" for="composer-advisory-message">MESSAGE</label>
            <input
              id="composer-advisory-message"
              class="input-mono composer-input"
              bind:value={advisoryMessage}
              placeholder="Connector changes may require matching clearance adjustments."
            />
          </div>
          <div class="composer-note">
            Attach this rule to one or more semantic controls in the active context.
          </div>
          <div class="composer-list">
            {#if advisoryCandidateControls.length > 0}
              {#each advisoryCandidateControls as control}
                <label class="primitive-picker">
                  <input
                    class="ui-checkbox"
                    type="checkbox"
                    checked={advisoryPrimitiveIds.includes(control.primitiveId)}
                    onchange={(event) => toggleAdvisoryPrimitive(control.primitiveId, getInputChecked(event))}
                  />
                  <div class="primitive-picker__body">
                    <div class="primitive-picker__label">{control.label}</div>
                    <div class="primitive-picker__meta">{control.rawField?.key || control.primitiveId}</div>
                  </div>
                </label>
              {/each}
            {:else}
              <div class="no-params">Open a context first to attach a rule.</div>
            {/if}
          </div>
          <div class="composer-actions">
            <button class="btn btn-xs btn-ghost" onclick={resetAdvisoryComposer}>CANCEL</button>
            <button class="btn btn-xs btn-primary" onclick={saveManualAdvisory} disabled={!advisoryCanSave}>
              CREATE RULE
            </button>
          </div>
        </div>
      {/if}

      {#if relationComposerOpen}
        <div class="view-composer">
          <div class="controls-head">
            <div class="section-label">NEW LINK</div>
          </div>
          <div class="composer-grid">
            <div class="composer-field">
              <div class="composer-label">SOURCE KNOB</div>
              <Dropdown
                options={advisoryCandidateControls.map((control) => ({ id: control.primitiveId, name: control.label }))}
                value={relationSourcePrimitiveId}
                onchange={(value) => relationSourcePrimitiveId = typeof value === 'string' ? value : null}
                placeholder="Choose source..."
              />
            </div>
            <div class="composer-field">
              <div class="composer-label">TARGET KNOB</div>
              <Dropdown
                options={advisoryCandidateControls.map((control) => ({ id: control.primitiveId, name: control.label }))}
                value={relationTargetPrimitiveId}
                onchange={(value) => relationTargetPrimitiveId = typeof value === 'string' ? value : null}
                placeholder="Choose target..."
              />
            </div>
            <div class="composer-field">
              <div class="composer-label">MODE</div>
              <Dropdown
                options={[
                  { id: 'mirror', name: 'Mirror value' },
                  { id: 'scale', name: 'Scale source' },
                  { id: 'offset', name: 'Offset source' },
                ]}
                value={relationMode}
                onchange={(value) =>
                  relationMode =
                    value === 'scale' || value === 'offset' ? value : 'mirror'
                }
              />
            </div>
            {#if relationMode === 'scale'}
              <div class="composer-field">
                <label class="composer-label" for="relation-scale">SCALE</label>
                <input
                  id="relation-scale"
                  class="input-mono composer-input"
                  type="number"
                  step="0.01"
                  bind:value={relationScale}
                />
              </div>
            {/if}
            {#if relationMode === 'offset'}
              <div class="composer-field">
                <label class="composer-label" for="relation-offset">OFFSET</label>
                <input
                  id="relation-offset"
                  class="input-mono composer-input"
                  type="number"
                  step="0.01"
                  bind:value={relationOffset}
                />
              </div>
            {/if}
          </div>
          <div class="composer-note">
            Linked knobs apply on semantic edits and persist with this version.
          </div>
          <div class="composer-actions">
            <button class="btn btn-xs btn-ghost" onclick={resetRelationComposer}>CANCEL</button>
            <button class="btn btn-xs btn-primary" onclick={saveControlRelation} disabled={!relationCanSave}>
              CREATE LINK
            </button>
          </div>
        </div>
      {/if}

      {#if primitiveComposerOpen}
        <div class="view-composer">
          <div class="controls-head">
            <div class="section-label">{primitiveComposerMode === 'edit' ? 'EDIT KNOB' : 'NEW KNOB'}</div>
          </div>
          <div class="composer-grid">
            <div class="composer-field">
              <label class="composer-label" for="composer-primitive-label">KNOB NAME</label>
              <input
                id="composer-primitive-label"
                class="input-mono composer-input"
                bind:value={primitiveLabel}
                placeholder="Connector Size / Hose Fit / Wall Thickness..."
              />
            </div>
            <div class="composer-field">
              <div class="composer-label">SCOPE</div>
              <Dropdown
                options={[
                  { id: 'global', name: 'Global' },
                  { id: 'part', name: 'Part' },
                ]}
                value={primitiveScope}
                onchange={(value) => {
                  primitiveScope = value === 'part' ? 'part' : 'global';
                  if (primitiveScope !== 'part') {
                    primitivePartId = null;
                  } else if (!primitivePartId) {
                    primitivePartId = selectedPart?.partId || modelManifest?.parts?.[0]?.partId || null;
                  }
                }}
              />
            </div>
            {#if primitiveScope === 'part'}
              <div class="composer-field">
                <div class="composer-label">PART</div>
                <Dropdown
                  options={(modelManifest?.parts || []).map((part) => ({ id: part.partId, name: part.label }))}
                  value={primitivePartId}
                  onchange={(value) => primitivePartId = typeof value === 'string' ? value : null}
                  placeholder="Choose part..."
                />
              </div>
            {/if}
          </div>
          <div class="composer-note">
            Pick one or more raw params to drive with a single semantic knob. Mixed field types are not allowed in one knob yet.
          </div>
          <label class="primitive-picker">
            <input
              class="ui-checkbox"
              type="checkbox"
              bind:checked={primitiveAttachToView}
            />
            <div class="primitive-picker__body">
              <div class="primitive-picker__label">Add to current context</div>
              <div class="primitive-picker__meta">
                {#if activeSemanticView}
                  {activeSemanticView.source === 'manual'
                    ? `Updates ${activeSemanticView.label}.`
                    : `Creates a custom context from ${activeSemanticView.label}.`}
                {:else}
                  Creates a custom context for this knob.
                {/if}
              </div>
            </div>
          </label>
          <div class="composer-list">
            {#if primitiveCandidateFields.length > 0}
              {#each primitiveCandidateFields as field}
                <label class="primitive-picker">
                  <input
                    class="ui-checkbox"
                    type="checkbox"
                    checked={primitiveParameterKeys.includes(field.key)}
                    onchange={(event) => togglePrimitiveParameter(field.key, getInputChecked(event))}
                  />
                  <div class="primitive-picker__body">
                    <div class="primitive-picker__label">{field.label}</div>
                    <div class="primitive-picker__meta">{field.key}</div>
                  </div>
                </label>
              {/each}
            {:else}
              <div class="no-params">No raw params are available for this scope.</div>
            {/if}
          </div>
          {#if selectedPrimitiveFields.length > 0 && primitiveKindPreview === 'number'}
            <div class="binding-editor">
              <div class="section-label">BINDINGS</div>
              {#each selectedPrimitiveFields as field}
                {@const draft = primitiveBindingDrafts[field.key]}
                <div class="binding-row">
                  <div class="binding-row__label">{field.label}</div>
                  <input
                    class="input-mono binding-input"
                    type="number"
                    step="0.01"
                    value={draft?.scale ?? '1'}
                    oninput={(event) => updatePrimitiveDraft(field.key, 'scale', getInputValue(event))}
                    placeholder="scale"
                  />
                  <input
                    class="input-mono binding-input"
                    type="number"
                    step="0.01"
                    value={draft?.offset ?? '0'}
                    oninput={(event) => updatePrimitiveDraft(field.key, 'offset', getInputValue(event))}
                    placeholder="offset"
                  />
                  <input
                    class="input-mono binding-input"
                    type="number"
                    step="0.01"
                    value={draft?.min ?? ''}
                    oninput={(event) => updatePrimitiveDraft(field.key, 'min', getInputValue(event))}
                    placeholder="min"
                  />
                  <input
                    class="input-mono binding-input"
                    type="number"
                    step="0.01"
                    value={draft?.max ?? ''}
                    oninput={(event) => updatePrimitiveDraft(field.key, 'max', getInputValue(event))}
                    placeholder="max"
                  />
                </div>
              {/each}
            </div>
          {/if}
          <div class="composer-note">
            {#if primitiveKindPreview}
              This knob will behave as a {primitiveKindPreview}.
            {:else if primitiveParameterKeys.length > 0}
              Select params of the same kind only.
            {:else}
              Choose the raw params this knob should control.
            {/if}
          </div>
          <div class="composer-actions">
            <button class="btn btn-xs btn-ghost" onclick={resetPrimitiveComposer}>CANCEL</button>
            {#if primitiveComposerMode === 'edit' && primitiveEditingId}
              <button class="btn btn-xs btn-ghost" onclick={() => primitiveEditingId && deleteManualPrimitive(primitiveEditingId)}>
                DELETE KNOB
              </button>
            {/if}
            <button class="btn btn-xs btn-primary" onclick={saveManualPrimitive} disabled={!primitiveCanSave}>
              {primitiveComposerMode === 'edit' ? 'SAVE KNOB' : 'CREATE KNOB'}
            </button>
          </div>
        </div>
      {/if}

      {#if composerOpen}
        <div class="view-composer">
          <div class="controls-head">
            <div class="section-label">{composerMode === 'edit' ? 'EDIT VIEW' : 'NEW VIEW'}</div>
          </div>
          <div class="composer-grid">
            <div class="composer-field">
              <label class="composer-label" for="composer-view-label">VIEW NAME</label>
              <input
                id="composer-view-label"
                class="input-mono composer-input"
                bind:value={composerViewLabel}
                placeholder="Connector / Fit / Printability..."
              />
            </div>
            <div class="composer-field">
              <div class="composer-label">SCOPE</div>
              <Dropdown
                options={[
                  { id: 'global', name: 'Global' },
                  { id: 'part', name: 'Part' },
                ]}
                value={composerViewScope}
                onchange={(value) => {
                  composerViewScope = value === 'part' ? 'part' : 'global';
                  if (composerViewScope !== 'part') {
                    composerViewPartId = null;
                  } else if (!composerViewPartId) {
                    composerViewPartId = selectedPart?.partId || modelManifest?.parts?.[0]?.partId || null;
                  }
                }}
              />
            </div>
            {#if composerViewScope === 'part'}
              <div class="composer-field">
                <div class="composer-label">PART</div>
                <Dropdown
                  options={(modelManifest?.parts || []).map((part) => ({ id: part.partId, name: part.label }))}
                  value={composerViewPartId}
                  onchange={(value) => composerViewPartId = typeof value === 'string' ? value : null}
                  placeholder="Choose part..."
                />
              </div>
            {/if}
          </div>
          <div class="composer-note">
            Build a reusable semantic context from existing meaningful controls.
          </div>
          <div class="composer-list">
            {#if composerVisiblePrimitives.length > 0}
              {#each composerVisiblePrimitives as primitive}
                <label class="primitive-picker">
                  <input
                    class="ui-checkbox"
                    type="checkbox"
                    checked={composerPrimitiveIds.includes(primitive.primitiveId)}
                    onchange={(event) => toggleComposerPrimitive(primitive.primitiveId, getInputChecked(event))}
                  />
                  <div class="primitive-picker__body">
                    <div class="primitive-picker__label">{primitive.label}</div>
                    {#if primitive.partLabels.length > 0}
                      <div class="primitive-picker__meta">{primitive.partLabels.join(', ')}</div>
                    {/if}
                  </div>
                </label>
              {/each}
            {:else}
              <div class="no-params">No primitives are available for this scope yet.</div>
            {/if}
          </div>
          <div class="composer-actions">
            <button class="btn btn-xs btn-ghost" onclick={resetComposer}>CANCEL</button>
            <button class="btn btn-xs btn-primary" onclick={saveManualView} disabled={!composerCanSave}>
              {composerMode === 'edit' ? 'SAVE VIEW' : 'CREATE VIEW'}
            </button>
          </div>
        </div>
      {/if}

      {#if activeSemanticView?.advisories?.length}
        <div class="warning-stack">
          {#each activeSemanticView.advisories as advisory}
            <div class="warning-chip" data-severity={advisory.severity}>
              <span>{advisory.label}: {advisory.message}</span>
              {#if advisory.advisoryId.startsWith('advisory-manual-')}
                <button
                  class="btn btn-xs btn-ghost warning-chip-action"
                  onclick={() => deleteManualAdvisory(advisory.advisoryId)}
                >
                  DELETE
                </button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

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
                  <div
                    class="param-field"
                    class:field-select={field.type === 'select'}
                    class:field-checkbox={field.type === 'checkbox'}
                  >
                    <div class="field-header">
                      <div class="field-title">
                        <label class="param-label" for={control.primitiveId}>
                          {control.label}
                        </label>
                      </div>
                      {#if isManualPrimitive(control)}
                        <button
                          class="btn btn-xs btn-ghost field-action-btn"
                          onclick={() => openEditPrimitiveComposer(control)}
                        >
                          EDIT
                        </button>
                      {/if}
                    </div>

                    <div class="field-control">
                      {#if field.type === 'range'}
                        {@const range = getRangeProps(field)}
                        <div class="range-group cad-range">
                          <input
                            id={control.primitiveId}
                            type="range"
                            min={range.min}
                            max={range.max}
                            step={range.step}
                            value={asNumber(control.value, range.min)}
                            oninput={(e) => updateSemanticControl(control, parseFloat(getInputValue(e)))}
                            disabled={!control.editable}
                          />
                          <span class="range-value cad-readout">{control.value}</span>
                        </div>
                      {:else if field.type === 'number'}
                        <input
                          id={control.primitiveId}
                          type="number"
                          class="input-mono param-input"
                          value={asNumber(control.value, 0)}
                          oninput={(e) => updateSemanticControl(control, parseFloat(getInputValue(e)))}
                          disabled={!control.editable}
                        />
                      {:else if field.type === 'select'}
                        <Dropdown
                          options={(field.options || []).map(option => ({ id: option.value, name: option.label }))}
                          value={control.value as string | number | null}
                          onchange={(val) => { if (val !== undefined) updateSemanticControl(control, val); }}
                          disabled={!control.editable}
                          placeholder="Select..."
                        />
                      {:else if field.type === 'checkbox'}
                        <label class="checkbox-wrapper" class:checkbox-wrapper-checked={Boolean(control.value)}>
                          <input
                            id={control.primitiveId}
                            class="ui-checkbox"
                            type="checkbox"
                            checked={Boolean(control.value)}
                            onchange={(e) => updateSemanticControl(control, getInputChecked(e))}
                            disabled={!control.editable}
                          />
                          <span class="checkbox-status">{control.value ? 'ON' : 'OFF'}</span>
                        </label>
                      {/if}
                    </div>
                  </div>
                {/if}
              {/each}
            </div>
          {/if}
        {/each}
      {:else}
        <div class="no-params">
          {selectedPart
            ? 'No semantic controls are mapped to this part yet. Open RAW for fallback.'
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
              {@const cadHint = getCadHint(field)}
              <div
                class="param-field param-field-focus"
                data-cad-tone={cadHint.tone}
                class:auto-field={field._auto}
                class:param-freezed={field.frozen}
                class:field-select={field.type === 'select'}
                class:field-checkbox={field.type === 'checkbox'}
              >
                <div class="field-header">
                  <div class="field-title">
                    <label class="param-label" for={field.key}>
                      {field.label}
                    </label>
                  </div>
                  {#if field.frozen}<span class="frozen-badge" title="FROZEN">❄️</span>{/if}
                </div>

                <div class="field-control">
                  {#if field.type === 'range'}
                    {@const range = getRangeProps(field)}
                    <div class="range-group cad-range">
                      <input
                        id={field.key}
                        type="range"
                        min={range.min}
                        max={range.max}
                        step={range.step}
                        value={asNumber(localParams[field.key], range.min)}
                        oninput={(e) => update(field.key, parseFloat(getInputValue(e)))}
                        disabled={field.frozen}
                      />
                      <span class="range-value cad-readout">{localParams[field.key]}</span>
                    </div>
                  {:else if field.type === 'number'}
                    <input
                      id={field.key}
                      type="number"
                      class="input-mono param-input"
                      value={asNumber(localParams[field.key], 0)}
                      oninput={(e) => update(field.key, parseFloat(getInputValue(e)))}
                      disabled={field.frozen}
                    />
                  {:else if field.type === 'select'}
                    <Dropdown
                      options={(field.options || []).map(option => ({ id: option.value, name: option.label }))}
                      value={getSelectValue(field.key)}
                      onchange={(val) => { if (val !== undefined) update(field.key, val); }}
                      disabled={field.frozen}
                      placeholder="Select..."
                    />
                  {:else if field.type === 'checkbox'}
                    <label class="checkbox-wrapper" class:checkbox-wrapper-checked={Boolean(localParams[field.key])}>
                      <input
                        id={field.key}
                        class="ui-checkbox"
                        type="checkbox"
                        checked={Boolean(localParams[field.key])}
                        onchange={(e) => update(field.key, getInputChecked(e))}
                        disabled={field.frozen}
                      />
                      <span class="checkbox-status">{localParams[field.key] ? 'ON' : 'OFF'}</span>
                    </label>
                  {/if}
                </div>
              </div>
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
          {@const cadHint = getCadHint(field)}
          <div
            class="param-field"
            data-cad-tone={cadHint.tone}
            class:auto-field={field._auto}
            class:param-freezed={field.frozen}
            class:field-select={field.type === 'select'}
            class:field-checkbox={field.type === 'checkbox'}
          >
            <div class="field-header">
              <div class="field-title">
                <label class="param-label" for={field.key}>
                  {field.label}
                </label>
              </div>
              {#if field.frozen}<span class="frozen-badge" title="FROZEN">❄️</span>{/if}
            </div>

            <div class="field-control">
              {#if field.type === 'range'}
                {@const range = getRangeProps(field)}
                <div class="range-group cad-range">
                  <input
                    id={field.key}
                    type="range"
                    min={range.min}
                    max={range.max}
                    step={range.step}
                    value={asNumber(localParams[field.key], range.min)}
                    oninput={(e) => update(field.key, parseFloat(getInputValue(e)))}
                    disabled={field.frozen}
                  />
                  <span class="range-value cad-readout">{localParams[field.key]}</span>
                </div>
              {:else if field.type === 'number'}
                <input
                  id={field.key}
                  type="number"
                  class="input-mono param-input"
                  value={asNumber(localParams[field.key], 0)}
                  oninput={(e) => update(field.key, parseFloat(getInputValue(e)))}
                  disabled={field.frozen}
                />
              {:else if field.type === 'select'}
                <Dropdown
                  options={(field.options || []).map(option => ({ id: option.value, name: option.label }))}
                  value={getSelectValue(field.key)}
                  onchange={(val) => { if (val !== undefined) update(field.key, val); }}
                  disabled={field.frozen}
                  placeholder="Select..."
                />
              {:else if field.type === 'checkbox'}
                <label class="checkbox-wrapper" class:checkbox-wrapper-checked={Boolean(localParams[field.key])}>
                  <input
                    id={field.key}
                    class="ui-checkbox"
                    type="checkbox"
                    checked={Boolean(localParams[field.key])}
                    onchange={(e) => update(field.key, getInputChecked(e))}
                    disabled={field.frozen}
                  />
                  <span class="checkbox-status">{localParams[field.key] ? 'ON' : 'OFF'}</span>
                </label>
              {/if}
            </div>
          </div>
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
    overflow: hidden;
  }

  .panel-toolbar {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-bottom: 1px solid var(--bg-300);
    padding-bottom: 8px;
    margin-bottom: 4px;
  }

  .search-box {
    position: relative;
    width: 100%;
  }

  .search-input {
    width: 100%;
    padding: 6px 28px 6px 10px;
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-size: 0.75rem;
    outline: none;
    transition: border-color 0.2s;
  }

  .search-input:focus {
    border-color: var(--primary);
  }

  .clear-search {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.8rem;
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

  .runtime-strip {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--primary) 24%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-200) 68%, var(--bg-100));
    overflow: hidden;
  }

  .runtime-strip__identity {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .runtime-strip__title {
    color: var(--text);
    font-size: 0.76rem;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .runtime-strip__context {
    color: var(--text-dim);
    font-size: 0.6rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .runtime-strip__badges,
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

  .status-chip,
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

  .status-chip-imported,
  .status-chip-pending,
  .proposal-status-pending {
    border-color: color-mix(in srgb, var(--primary) 45%, var(--bg-300));
    color: var(--primary);
  }

  .status-chip-editable,
  .proposal-status-accepted {
    border-color: color-mix(in srgb, var(--secondary) 45%, var(--bg-300));
    color: var(--secondary);
  }

  .status-chip-readonly,
  .proposal-status-rejected {
    border-color: color-mix(in srgb, var(--text-dim) 45%, var(--bg-300));
    color: var(--text-dim);
  }

  .warning-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    text-transform: none;
    letter-spacing: normal;
    font-weight: 500;
  }

  .warning-chip-action {
    flex-shrink: 0;
  }

  .warning-chip[data-severity='warning'] {
    border-color: color-mix(in srgb, var(--primary) 45%, var(--bg-300));
    color: var(--primary);
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
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .context-strip-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .panel-mode-tabs {
    display: flex;
    gap: 6px;
    overflow: hidden;
  }

  .panel-mode-tab {
    flex: 0 0 auto;
    padding: 5px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text-dim);
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    cursor: pointer;
  }

  .panel-mode-tab-active {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-200));
    color: var(--text);
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
    text-overflow: ellipsis;
    white-space: nowrap;
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
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
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

  .live-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 0.6rem;
    font-weight: bold;
    color: var(--text-dim);
    cursor: pointer;
    user-select: none;
    padding: 2px 6px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .live-toggle:has(input:checked) {
    color: var(--secondary);
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 10%, var(--bg-200));
  }

  .live-toggle input {
    display: none;
  }

  .apply-btn {
    min-width: 50px;
  }

  .param-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
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

  .param-field[data-cad-tone="x"],
  .param-field[data-cad-tone="size"] {
    --cad-tone-color: var(--cad-axis-x);
  }

  .param-field[data-cad-tone="y"] {
    --cad-tone-color: var(--cad-axis-y);
  }

  .param-field[data-cad-tone="z"] {
    --cad-tone-color: var(--cad-axis-z);
  }

  .param-field[data-cad-tone="angle"],
  .param-field[data-cad-tone="mode"],
  .param-field[data-cad-tone="state"] {
    --cad-tone-color: var(--cad-axis-angle);
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
    align-items: center;
    gap: 8px;
  }

  .field-title {
    display: flex;
    flex-direction: column;
    gap: 0;
    min-width: 0;
  }

  .param-label {
    font-size: 0.72rem;
    color: var(--primary);
    text-transform: uppercase;
    font-weight: bold;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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

  input[type="range"] {
    flex: 1;
    cursor: pointer;
    height: 6px;
    background:
      linear-gradient(
        90deg,
        color-mix(in srgb, var(--cad-tone-color) 38%, var(--bg-300)) 0%,
        color-mix(in srgb, var(--cad-tone-color) 18%, var(--bg-300)) 100%
      );
    border-radius: 0;
    appearance: none;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 35%, transparent);
  }

  input[type="range"]::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    background: var(--cad-tone-color);
    border: 1px solid color-mix(in srgb, #fff 18%, #000 82%);
    border-radius: 0;
    cursor: pointer;
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--cad-tone-color) 16%, transparent),
      0 0 12px color-mix(in srgb, var(--cad-tone-color) 34%, transparent);
  }

  input[type="range"]::-moz-range-thumb {
    width: 14px;
    height: 14px;
    background: var(--cad-tone-color);
    border: 1px solid color-mix(in srgb, #fff 18%, #000 82%);
    border-radius: 0;
    cursor: pointer;
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--cad-tone-color) 16%, transparent),
      0 0 12px color-mix(in srgb, var(--cad-tone-color) 34%, transparent);
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

  .ui-checkbox:checked + .checkbox-status {
    color: var(--cad-tone-color);
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
</style>
