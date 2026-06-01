<script lang="ts">
  import ParamPanelAdvisoryComposer from './ParamPanelAdvisoryComposer.svelte';
  import ParamPanelAdvisoryList from './ParamPanelAdvisoryList.svelte';
  import ParamPanelContextStrip from './ParamPanelContextStrip.svelte';
  import ParamPanelControlField from './ParamPanelControlField.svelte';
  import ParamPanelPrimitiveComposer from './ParamPanelPrimitiveComposer.svelte';
  import ParamPanelRelationComposer from './ParamPanelRelationComposer.svelte';
  import ParamPanelViewComposer from './ParamPanelViewComposer.svelte';
  import type { MaterializedSemanticControl, MaterializedSemanticView } from '../modelRuntime/semanticControls';
  import type {
    AdvisoryCondition,
    AdvisorySeverity,
    ControlPrimitiveKind,
    ControlRelationMode,
    ControlView,
    ControlViewScope,
    ControlViewSource,
    PartBinding,
    ParamValue,
    ResolvedUiField,
  } from '../types/domain';

  type PrimitiveBindingDraft = {
    parameterKey: string;
    scale: string;
    offset: string;
    min: string;
    max: string;
  };
  type RangeProps = { min: number; max: number; step: number };
  type ActiveViewRelation = {
    relationId: string;
    sourceLabel: string;
    targetLabel: string;
    mode: ControlRelationMode;
    scale: number;
    offset: number;
  };
  type SemanticSection = {
    sectionId: string;
    label: string;
    collapsed: boolean;
    controls: MaterializedSemanticControl[];
  };

  let {
    controlViews,
    activeControlViewId,
    activeSemanticView = null,
    advisoryComposerOpen,
    advisoryLabel,
    advisoryMessage,
    advisorySeverity,
    advisoryCondition,
    advisoryThreshold,
    advisoryCandidateControls,
    advisoryPrimitiveIds,
    advisoryCanSave,
    relationComposerOpen,
    relationSourcePrimitiveId,
    relationTargetPrimitiveId,
    relationMode,
    relationScale,
    relationOffset,
    relationCanSave,
    primitiveComposerOpen,
    primitiveComposerMode,
    primitiveEditingId,
    primitiveLabel,
    primitiveScope,
    primitivePartId,
    primitiveAttachToView,
    modelParts,
    primitiveCandidateFields,
    primitiveParameterKeys,
    selectedPrimitiveFields,
    primitiveBindingDrafts,
    primitiveKindPreview,
    primitiveCanSave,
    composerOpen,
    composerMode,
    composerViewLabel,
    composerViewScope,
    composerViewPartId,
    composerVisiblePrimitives,
    composerPrimitiveIds,
    composerCanSave,
    advisories,
    activeViewRelations,
    filteredSemanticSections,
    selectedPart,
    isSelectMode,
    selectionTargetCount,
    highlightedParamKey,
    liveApply,
    onSelectControlView,
    onOpenCreateViewComposer,
    onOpenPrimitiveComposer,
    onOpenAdvisoryComposer,
    onOpenRelationComposer,
    onOpenEditViewComposer,
    onDeleteManualView,
    shouldShowSemanticSource,
    semanticSourceLabel,
    onAdvisoryLabelChange,
    onAdvisoryMessageChange,
    onAdvisorySeverityChange,
    onAdvisoryConditionChange,
    onAdvisoryThresholdChange,
    onToggleAdvisoryPrimitive,
    onCancelAdvisory,
    onSaveAdvisory,
    onRelationSourceChange,
    onRelationTargetChange,
    onRelationModeChange,
    onRelationScaleChange,
    onRelationOffsetChange,
    onCancelRelation,
    onSaveRelation,
    onPrimitiveLabelChange,
    onPrimitiveScopeChange,
    onPrimitivePartIdChange,
    onPrimitiveAttachToViewChange,
    onTogglePrimitiveParameter,
    onUpdatePrimitiveDraft,
    onCancelPrimitive,
    onDeletePrimitive,
    onSavePrimitive,
    onComposerLabelChange,
    onComposerScopeChange,
    onComposerPartIdChange,
    onToggleComposerPrimitive,
    onCancelComposer,
    onSaveComposer,
    onDeleteManualAdvisory,
    onDeleteControlRelation,
    isSectionExpanded,
    toggleSection,
    getRangeProps,
    isManualPrimitive,
    onUpdateSemanticControl,
    onEditPrimitiveComposer,
    onPickSemanticControlImage,
    onSetFocusedControl,
    onClearFocusedControl,
  }: {
    controlViews: ControlView[];
    activeControlViewId: string | null;
    activeSemanticView?: MaterializedSemanticView | null;
    advisoryComposerOpen: boolean;
    advisoryLabel: string;
    advisoryMessage: string;
    advisorySeverity: AdvisorySeverity;
    advisoryCondition: AdvisoryCondition;
    advisoryThreshold: string;
    advisoryCandidateControls: MaterializedSemanticControl[];
    advisoryPrimitiveIds: string[];
    advisoryCanSave: boolean;
    relationComposerOpen: boolean;
    relationSourcePrimitiveId: string;
    relationTargetPrimitiveId: string;
    relationMode: ControlRelationMode;
    relationScale: string;
    relationOffset: string;
    relationCanSave: boolean;
    primitiveComposerOpen: boolean;
    primitiveComposerMode: 'create' | 'edit';
    primitiveEditingId: string | null;
    primitiveLabel: string;
    primitiveScope: 'global' | 'part';
    primitivePartId: string | null;
    primitiveAttachToView: boolean;
    modelParts: PartBinding[];
    primitiveCandidateFields: ResolvedUiField[];
    primitiveParameterKeys: string[];
    selectedPrimitiveFields: ResolvedUiField[];
    primitiveBindingDrafts: Record<string, PrimitiveBindingDraft>;
    primitiveKindPreview: ControlPrimitiveKind | null;
    primitiveCanSave: boolean;
    composerOpen: boolean;
    composerMode: 'create' | 'edit';
    composerViewLabel: string;
    composerViewScope: ControlViewScope;
    composerViewPartId: string | null;
    composerVisiblePrimitives: { primitiveId: string; label: string; partLabels: string[] }[];
    composerPrimitiveIds: string[];
    composerCanSave: boolean;
    advisories: MaterializedSemanticView['advisories'];
    activeViewRelations: ActiveViewRelation[];
    filteredSemanticSections: SemanticSection[];
    selectedPart: PartBinding | null;
    isSelectMode: boolean;
    selectionTargetCount: number;
    highlightedParamKey: string | null;
    liveApply: boolean;
    onSelectControlView?: (viewId: string | null) => void;
    onOpenCreateViewComposer?: () => void;
    onOpenPrimitiveComposer?: () => void;
    onOpenAdvisoryComposer?: () => void;
    onOpenRelationComposer?: () => void;
    onOpenEditViewComposer?: (view: MaterializedSemanticView) => void;
    onDeleteManualView?: (viewId: string) => void;
    shouldShowSemanticSource: (source: ControlViewSource | undefined) => boolean;
    semanticSourceLabel: (source: ControlViewSource | undefined) => string;
    onAdvisoryLabelChange?: (value: string) => void;
    onAdvisoryMessageChange?: (value: string) => void;
    onAdvisorySeverityChange?: (value: AdvisorySeverity) => void;
    onAdvisoryConditionChange?: (value: AdvisoryCondition) => void;
    onAdvisoryThresholdChange?: (value: string) => void;
    onToggleAdvisoryPrimitive?: (primitiveId: string, checked: boolean) => void;
    onCancelAdvisory?: () => void;
    onSaveAdvisory?: () => void;
    onRelationSourceChange?: (value: string) => void;
    onRelationTargetChange?: (value: string) => void;
    onRelationModeChange?: (value: ControlRelationMode) => void;
    onRelationScaleChange?: (value: string) => void;
    onRelationOffsetChange?: (value: string) => void;
    onCancelRelation?: () => void;
    onSaveRelation?: () => void;
    onPrimitiveLabelChange?: (value: string) => void;
    onPrimitiveScopeChange?: (value: 'global' | 'part') => void;
    onPrimitivePartIdChange?: (value: string | null) => void;
    onPrimitiveAttachToViewChange?: (value: boolean) => void;
    onTogglePrimitiveParameter?: (key: string, checked: boolean) => void;
    onUpdatePrimitiveDraft?: (key: string, field: 'scale' | 'offset' | 'min' | 'max', value: string) => void;
    onCancelPrimitive?: () => void;
    onDeletePrimitive?: (primitiveId: string) => void;
    onSavePrimitive?: () => void;
    onComposerLabelChange?: (value: string) => void;
    onComposerScopeChange?: (value: ControlViewScope) => void;
    onComposerPartIdChange?: (value: string | null) => void;
    onToggleComposerPrimitive?: (primitiveId: string, checked: boolean) => void;
    onCancelComposer?: () => void;
    onSaveComposer?: () => void;
    onDeleteManualAdvisory?: (advisoryId: string) => void;
    onDeleteControlRelation?: (relationId: string) => void;
    isSectionExpanded: (sectionId: string, collapsed: boolean) => boolean;
    toggleSection: (sectionId: string, collapsed: boolean) => void;
    getRangeProps: (field: ResolvedUiField) => RangeProps;
    isManualPrimitive: (control: MaterializedSemanticControl) => boolean;
    onUpdateSemanticControl?: (control: MaterializedSemanticControl, nextValue: ParamValue) => void;
    onEditPrimitiveComposer?: (control: MaterializedSemanticControl) => void;
    onPickSemanticControlImage?: (control: MaterializedSemanticControl) => Promise<void> | void;
    onSetFocusedControl?: (primitiveId: string, parameterKey: string) => void;
    onClearFocusedControl?: () => void;
  } = $props();
</script>

<ParamPanelContextStrip
  {controlViews}
  {activeControlViewId}
  {activeSemanticView}
  onSelectControlView={onSelectControlView}
  onOpenCreateViewComposer={onOpenCreateViewComposer}
  onOpenPrimitiveComposer={onOpenPrimitiveComposer}
  onOpenAdvisoryComposer={onOpenAdvisoryComposer}
  onOpenRelationComposer={onOpenRelationComposer}
  onOpenEditViewComposer={onOpenEditViewComposer}
  onDeleteManualView={onDeleteManualView}
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
    onLabelChange={onAdvisoryLabelChange}
    onMessageChange={onAdvisoryMessageChange}
    onSeverityChange={onAdvisorySeverityChange}
    onConditionChange={onAdvisoryConditionChange}
    onThresholdChange={onAdvisoryThresholdChange}
    onTogglePrimitive={onToggleAdvisoryPrimitive}
    onCancel={onCancelAdvisory}
    onSave={onSaveAdvisory}
  />
{/if}

<style>
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

  .warning-chip-action {
    flex-shrink: 0;
  }

  .controls-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }

  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .param-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 220px), 1fr));
    gap: 12px;
    overflow: visible;
  }

  .no-params {
    font-size: 0.7rem;
    color: var(--text-dim);
    font-style: italic;
    padding: 20px;
    text-align: center;
  }

  .btn-xs {
    padding: 2px 6px;
    font-size: 0.6rem;
  }
</style>

{#if relationComposerOpen}
  <ParamPanelRelationComposer
    controls={advisoryCandidateControls}
    sourcePrimitiveId={relationSourcePrimitiveId}
    targetPrimitiveId={relationTargetPrimitiveId}
    mode={relationMode}
    scale={relationScale}
    offset={relationOffset}
    canSave={relationCanSave}
    onSourceChange={onRelationSourceChange}
    onTargetChange={onRelationTargetChange}
    onModeChange={onRelationModeChange}
    onScaleChange={onRelationScaleChange}
    onOffsetChange={onRelationOffsetChange}
    onCancel={onCancelRelation}
    onSave={onSaveRelation}
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
    {modelParts}
    candidateFields={primitiveCandidateFields}
    selectedParameterKeys={primitiveParameterKeys}
    selectedFields={selectedPrimitiveFields}
    bindingDrafts={primitiveBindingDrafts}
    kindPreview={primitiveKindPreview}
    canSave={primitiveCanSave}
    onLabelChange={onPrimitiveLabelChange}
    onScopeChange={onPrimitiveScopeChange}
    onPartIdChange={onPrimitivePartIdChange}
    onAttachToViewChange={onPrimitiveAttachToViewChange}
    onToggleParameter={onTogglePrimitiveParameter}
    onUpdateDraft={onUpdatePrimitiveDraft}
    onCancel={onCancelPrimitive}
    onDelete={onDeletePrimitive}
    onSave={onSavePrimitive}
  />
{/if}

{#if composerOpen}
  <ParamPanelViewComposer
    mode={composerMode}
    label={composerViewLabel}
    scope={composerViewScope}
    partId={composerViewPartId}
    {modelParts}
    visiblePrimitives={composerVisiblePrimitives}
    selectedPrimitiveIds={composerPrimitiveIds}
    canSave={composerCanSave}
    onLabelChange={onComposerLabelChange}
    onScopeChange={onComposerScopeChange}
    onPartIdChange={onComposerPartIdChange}
    onTogglePrimitive={onToggleComposerPrimitive}
    onCancel={onCancelComposer}
    onSave={onSaveComposer}
  />
{/if}

<ParamPanelAdvisoryList {advisories} onDeleteManualAdvisory={onDeleteManualAdvisory} />

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
          onclick={() => onDeleteControlRelation?.(relation.relationId)}
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
              {field}
              value={control.value}
              rangeProps={field.type === 'range' || field.type === 'number' ? getRangeProps(field) : null}
              editable={control.editable}
              highlighted={highlightedParamKey === field.key}
              {liveApply}
              semanticSource={control.source}
              showSemanticSource={shouldShowSemanticSource(control.source)}
              canEdit={isManualPrimitive(control)}
              onUpdate={(nextValue) => onUpdateSemanticControl?.(control, nextValue)}
              onEdit={() => onEditPrimitiveComposer?.(control)}
              onPickImage={() => onPickSemanticControlImage?.(control)}
              onMouseEnter={() => onSetFocusedControl?.(control.primitiveId, field.key)}
              onMouseLeave={onClearFocusedControl}
              onFocusIn={() => onSetFocusedControl?.(control.primitiveId, field.key)}
              onFocusOut={onClearFocusedControl}
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
      : isSelectMode && selectionTargetCount > 1
        ? 'Multiple face targets found. Select one in viewport; fallback waits for explicit target.'
        : 'No semantic controls match your search.'}
  </div>
{/if}
