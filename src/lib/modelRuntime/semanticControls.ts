import type {
  Advisory,
  AdvisoryCondition,
  AdvisorySeverity,
  ControlPrimitive,
  ControlPrimitiveKind,
  ControlRelation,
  ControlView,
  ControlViewScope,
  ControlViewSection,
  ControlViewSource,
  DesignParams,
  ModelManifest,
  ParamValue,
  PartBinding,
  PrimitiveBinding,
  UiField,
  UiSpec,
} from '../types/domain';

export type MaterializedSemanticControl = {
  primitiveId: string;
  label: string;
  kind: ControlPrimitiveKind;
  source: ControlViewSource;
  editable: boolean;
  partIds: string[];
  order: number;
  rawField: UiField | null;
  bindings: PrimitiveBinding[];
  value: ParamValue;
};

export type MaterializedSemanticSection = {
  sectionId: string;
  label: string;
  collapsed: boolean;
  controls: MaterializedSemanticControl[];
};

export type MaterializedSemanticView = {
  viewId: string;
  label: string;
  scope: ControlViewScope;
  partIds: string[];
  isDefault: boolean;
  source: ControlViewSource;
  status: 'none' | 'pending' | 'accepted' | 'rejected';
  order: number;
  sections: MaterializedSemanticSection[];
  advisories: Advisory[];
};

const PRIMARY_CONTROL_WORDS = [
  'size',
  'diameter',
  'radius',
  'width',
  'height',
  'depth',
  'length',
  'thickness',
  'count',
  'clearance',
  'angle',
  'offset',
  'mesh',
  'logo',
  'connector',
  'hose',
  'spout',
  'handle',
  'lid',
  'cap',
];

const ADVANCED_CONTROL_WORDS = [
  'resolution',
  'pattern',
  'sharpness',
  'frequency',
  'mix',
  'theta',
  'fade',
  'sample',
  'seed',
  'noise',
  'twist',
  'amplitude',
  'detail',
  'smoothing',
];

const ROLE_TITLES: Record<string, string> = {
  connector: 'Connector',
  lid: 'Lid',
  handle: 'Handle',
  body: 'Body',
  base: 'Base',
  ornament: 'Detail',
  unknown: 'Part',
};

function slugify(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

function humanize(value: string): string {
  return value
    .split(/[_\-.]+/)
    .filter(Boolean)
    .map((token) => token.charAt(0).toUpperCase() + token.slice(1))
    .join(' ');
}

function tokenize(value: string): string[] {
  return value
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter(Boolean);
}

function includesToken(tokens: string[], candidates: string[]): boolean {
  return candidates.some((candidate) => tokens.includes(candidate));
}

function primitiveKindFromField(field: UiField): ControlPrimitiveKind {
  if (field.type === 'checkbox') return 'toggle';
  if (field.type === 'select') return 'choice';
  if (field.type === 'image') return 'choice'; // Maps roughly to string value for now
  return 'number';
}

function inferPresentation(field: UiField, label: string): 'primary' | 'advanced' {
  const tokens = tokenize(`${field.key} ${label}`);
  if (includesToken(tokens, ADVANCED_CONTROL_WORDS)) return 'advanced';
  if (field.type === 'checkbox' || field.type === 'select' || field.type === 'image') return 'primary';
  if (includesToken(tokens, PRIMARY_CONTROL_WORDS)) return 'primary';
  return 'advanced';
}

function inferPrimitiveLabel(field: UiField, parts: PartBinding[]): string {
  const base = field.label?.trim() || humanize(field.key);
  if (!parts.length) return base;
  const part = parts[0];
  const role = ROLE_TITLES[part.semanticRole || 'unknown'] || ROLE_TITLES.unknown;
  const baseLower = base.toLowerCase();
  if (baseLower.includes(role.toLowerCase())) return base;
  const tokens = tokenize(`${field.key} ${base}`);
  if (includesToken(tokens, ['connector', 'hose', 'spout', 'lid', 'cap', 'handle', 'body', 'base'])) {
    return base;
  }
  return `${role} ${base}`;
}

function findFieldByKey(uiSpec: UiSpec, key: string): UiField | null {
  return (uiSpec.fields || []).find((field) => field.key === key) ?? null;
}

function inferPartIdsForKey(manifest: ModelManifest, key: string): string[] {
  const partIds = new Set<string>();

  for (const group of manifest.parameterGroups || []) {
    if (!(group.parameterKeys || []).includes(key)) continue;
    for (const partId of group.partIds || []) {
      partIds.add(partId);
    }
  }

  for (const part of manifest.parts || []) {
    if ((part.parameterKeys || []).includes(key)) {
      partIds.add(part.partId);
    }
  }

  return [...partIds];
}

function normalizeBindingsForPrimitive(
  primitive: ControlPrimitive,
  uiSpec: UiSpec,
): PrimitiveBinding[] {
  const valid = (primitive.bindings || []).filter((binding) => findFieldByKey(uiSpec, binding.parameterKey));
  return valid.length > 0 ? valid : [];
}

function bindingSignature(bindings: PrimitiveBinding[]): string {
  return bindings
    .map((binding) => binding.parameterKey)
    .sort()
    .join('|');
}

function clampNumber(value: number, binding: PrimitiveBinding): number {
  let next = value;
  if (typeof binding.min === 'number') next = Math.max(binding.min, next);
  if (typeof binding.max === 'number') next = Math.min(binding.max, next);
  return next;
}

function readPrimitiveValue(
  primitive: ControlPrimitive,
  uiSpec: UiSpec,
  params: DesignParams,
): ParamValue {
  const binding = (primitive.bindings || [])[0];
  if (!binding) return null;
  const field = findFieldByKey(uiSpec, binding.parameterKey);
  const rawValue = params[binding.parameterKey];

  if (!field) return rawValue ?? null;
  if (field.type === 'checkbox') return Boolean(rawValue);
  if (field.type === 'select' || field.type === 'image') return rawValue ?? null;

  const numeric = Number(rawValue);
  if (!Number.isFinite(numeric)) return rawValue ?? null;
  const scale = Number(binding.scale) || 1;
  return (numeric - (binding.offset || 0)) / scale;
}

function defaultPrimitiveForField(
  manifest: ModelManifest,
  field: UiField,
  order: number,
): ControlPrimitive {
  const partIds = inferPartIdsForKey(manifest, field.key);
  const parts = (manifest.parts || []).filter((part) => partIds.includes(part.partId));

  return {
    primitiveId: `primitive-${slugify(field.key)}`,
    label: inferPrimitiveLabel(field, parts),
    kind: primitiveKindFromField(field),
    source: 'generated',
    partIds,
    bindings: [
      {
        parameterKey: field.key,
        scale: 1,
        offset: 0,
        min: null,
        max: null,
      },
    ],
    editable: !field.frozen,
    order,
  };
}

function mergePrimitive(
  fallback: ControlPrimitive,
  existing: ControlPrimitive | null,
  uiSpec: UiSpec,
  validPartIds: Set<string>,
): ControlPrimitive {
  if (!existing) return fallback;
  const normalizedBindings = normalizeBindingsForPrimitive(existing, uiSpec);
  const retainedPartIds = [...new Set((existing.partIds || []).filter((partId) => validPartIds.has(partId)))];
  return {
    primitiveId: existing.primitiveId || fallback.primitiveId,
    label: existing.label || fallback.label,
    kind: existing.kind || fallback.kind,
    source: existing.source || fallback.source,
    partIds: retainedPartIds.length > 0 ? retainedPartIds : fallback.partIds,
    bindings: normalizedBindings.length > 0 ? normalizedBindings : fallback.bindings,
    editable: existing.editable ?? fallback.editable,
    order: typeof existing.order === 'number' ? existing.order : fallback.order,
  };
}

function buildGeneratedSections(
  primitiveIds: string[],
  primitivesById: Map<string, ControlPrimitive>,
  uiSpec: UiSpec,
): ControlViewSection[] {
  const primaryIds: string[] = [];
  const advancedIds: string[] = [];

  for (const primitiveId of primitiveIds) {
    const primitive = primitivesById.get(primitiveId);
    if (!primitive) continue;
    const field = findFieldByKey(uiSpec, primitive.bindings?.[0]?.parameterKey || '');
    const presentation = field ? inferPresentation(field, primitive.label) : 'advanced';
    if (presentation === 'primary') {
      primaryIds.push(primitiveId);
    } else {
      advancedIds.push(primitiveId);
    }
  }

  const sections: ControlViewSection[] = [];
  if (primaryIds.length > 0) {
    sections.push({
      sectionId: 'main',
      label: 'Main',
      primitiveIds: primaryIds,
      collapsed: false,
    });
  }
  if (advancedIds.length > 0) {
    sections.push({
      sectionId: 'advanced',
      label: 'Advanced',
      primitiveIds: advancedIds,
      collapsed: true,
    });
  }
  return sections;
}

function validateView(
  view: ControlView,
  primitiveIds: Set<string>,
  partIds: Set<string>,
): boolean {
  if (!view.viewId || !view.label) return false;
  if ((view.primitiveIds || []).some((primitiveId) => !primitiveIds.has(primitiveId))) return false;
  if ((view.partIds || []).some((partId) => !partIds.has(partId))) return false;
  if ((view.sections || []).some((section) =>
    (section.primitiveIds || []).some((primitiveId) => !primitiveIds.has(primitiveId)))) {
    return false;
  }
  return true;
}

function sortByOrder<T extends { order?: number; label: string }>(left: T, right: T): number {
  return (left.order ?? 0) - (right.order ?? 0) || left.label.localeCompare(right.label);
}

function buildGeneratedViews(
  manifest: ModelManifest,
  primitives: ControlPrimitive[],
  uiSpec: UiSpec,
): ControlView[] {
  const primitivesById = new Map(primitives.map((primitive) => [primitive.primitiveId, primitive]));
  const views: ControlView[] = [];

  const globalPrimitiveIds = primitives
    .filter((primitive) => primitive.editable)
    .sort(sortByOrder)
    .map((primitive) => primitive.primitiveId);

  if (globalPrimitiveIds.length > 0) {
    views.push({
      viewId: 'view-model',
      label: 'Model',
      scope: 'global',
      partIds: [],
      primitiveIds: globalPrimitiveIds,
      sections: buildGeneratedSections(globalPrimitiveIds, primitivesById, uiSpec),
      default: true,
      source: 'generated',
      status: 'accepted',
      order: 0,
    });
  }

  for (const [index, part] of (manifest.parts || []).entries()) {
    const primitiveIds = primitives
      .filter((primitive) => (primitive.partIds || []).includes(part.partId) && primitive.editable)
      .sort(sortByOrder)
      .map((primitive) => primitive.primitiveId);
    if (primitiveIds.length === 0) continue;

    const roleTitle = ROLE_TITLES[part.semanticRole || 'unknown'] || ROLE_TITLES.unknown;
    views.push({
      viewId: `view-${slugify(part.partId)}`,
      label: part.semanticRole && part.semanticRole !== 'unknown' ? roleTitle : part.label,
      scope: 'part',
      partIds: [part.partId],
      primitiveIds,
      sections: buildGeneratedSections(primitiveIds, primitivesById, uiSpec),
      default: false,
      source: 'generated',
      status: 'accepted',
      order: index + 1,
    });
  }

  return views;
}

function viewPriority(view: ControlView, selectedPartId: string | null): number {
  if (selectedPartId) {
    if (view.scope === 'part' && (view.partIds || []).includes(selectedPartId)) {
      if (view.source === 'generated') return 0;
      if (view.source === 'manual') return 1;
      if (view.source === 'inherited') return 2;
      if (view.source === 'llm') return 3;
      return 4;
    }
    if (view.scope === 'global') return 10;
    return 20;
  }

  if (view.scope === 'global' && view.default) return 0;
  if (view.scope === 'global') return 1;
  return 10;
}

function carryForwardViews(
  manifest: ModelManifest,
  currentViews: ControlView[],
  previousManifest: ModelManifest | null,
): ControlView[] {
  const primitiveIds = new Set((manifest.controlPrimitives || []).map((primitive) => primitive.primitiveId));
  const partIds = new Set((manifest.parts || []).map((part) => part.partId));
  const next = new Map<string, ControlView>();

  for (const view of currentViews) {
    if (validateView(view, primitiveIds, partIds)) {
      next.set(view.viewId, view);
    }
  }

  for (const inherited of previousManifest?.controlViews || []) {
    if (!validateView(inherited, primitiveIds, partIds)) continue;
    if (next.has(inherited.viewId)) continue;
    if (!(inherited.status === 'accepted' || inherited.source === 'manual')) continue;
    next.set(inherited.viewId, {
      ...inherited,
      source: inherited.source === 'manual' ? 'manual' : 'inherited',
      status: 'accepted',
    });
  }

  return [...next.values()].sort(sortByOrder);
}

function carryForwardPrimitives(
  manifest: ModelManifest,
  uiSpec: UiSpec,
  defaults: ControlPrimitive[],
  previousManifest: ModelManifest | null,
): ControlPrimitive[] {
  const validPartIds = new Set((manifest.parts || []).map((part) => part.partId));
  const currentBySignature = new Map<string, ControlPrimitive>();
  const currentById = new Map<string, ControlPrimitive>();

  for (const primitive of manifest.controlPrimitives || []) {
    const bindings = normalizeBindingsForPrimitive(primitive, uiSpec);
    if (bindings.length === 0) continue;
    currentBySignature.set(bindingSignature(bindings), { ...primitive, bindings });
    currentById.set(primitive.primitiveId, { ...primitive, bindings });
  }

  for (const primitive of previousManifest?.controlPrimitives || []) {
    const bindings = normalizeBindingsForPrimitive(primitive, uiSpec);
    if (bindings.length === 0) continue;
    const signature = bindingSignature(bindings);
    if (!currentBySignature.has(signature) && !currentById.has(primitive.primitiveId)) {
      currentBySignature.set(signature, { ...primitive, bindings });
      currentById.set(primitive.primitiveId, { ...primitive, bindings });
    }
  }

  return defaults
    .map((primitive) => {
      const signature = bindingSignature(primitive.bindings || []);
      const existing = currentBySignature.get(signature) || currentById.get(primitive.primitiveId) || null;
      return mergePrimitive(primitive, existing, uiSpec, validPartIds);
    })
    .sort(sortByOrder);
}

function buildGeneratedAdvisories(
  manifest: ModelManifest,
  uiSpec: UiSpec,
  params: DesignParams,
): Advisory[] {
  const advisories: Advisory[] = [];

  for (const primitive of manifest.controlPrimitives || []) {
    const binding = primitive.bindings?.[0];
    if (!binding) continue;
    const field = findFieldByKey(uiSpec, binding.parameterKey);
    if (!field || (field.type !== 'number' && field.type !== 'range')) continue;
    const value = Number(readPrimitiveValue(primitive, uiSpec, params));
    if (!Number.isFinite(value)) continue;

    const signature = `${primitive.label} ${binding.parameterKey}`.toLowerCase();
    if (signature.includes('thickness') && value < 1.2) {
      advisories.push({
        advisoryId: `advisory-${slugify(primitive.primitiveId)}-thin`,
        label: 'Thin wall',
        severity: 'warning',
        primitiveIds: [primitive.primitiveId],
        viewIds: [],
        message: 'Wall thickness is below the recommended print range.',
        condition: 'below',
        threshold: 1.2,
      });
    }
    if (signature.includes('clearance') && value < 0.6) {
      advisories.push({
        advisoryId: `advisory-${slugify(primitive.primitiveId)}-clearance`,
        label: 'Low clearance',
        severity: 'warning',
        primitiveIds: [primitive.primitiveId],
        viewIds: [],
        message: 'Clearance is below the recommended fit range.',
        condition: 'below',
        threshold: 0.6,
      });
    }
  }

  const connectorView = (manifest.controlViews || []).find((view) => view.label === 'Connector');
  if (connectorView && (connectorView.primitiveIds || []).length > 1) {
    advisories.push({
      advisoryId: 'advisory-connector-fit',
      label: 'Connector fit',
      severity: 'info',
      primitiveIds: connectorView.primitiveIds || [],
      viewIds: [connectorView.viewId],
      message: 'Connector changes may require matching hole and clearance adjustments.',
      condition: 'always',
      threshold: null,
    });
  }

  return advisories;
}

export function ensureSemanticManifest(
  manifest: ModelManifest | null,
  uiSpec: UiSpec | null | undefined,
  params: DesignParams,
  previousManifest: ModelManifest | null = null,
): ModelManifest | null {
  if (!manifest || !uiSpec) return manifest;
  const fields = uiSpec.fields || [];
  if (fields.length === 0) {
    return {
      ...manifest,
      controlPrimitives: manifest.controlPrimitives || [],
      controlViews: manifest.controlViews || [],
      advisories: manifest.advisories || [],
    };
  }

  const defaults = fields.map((field, index) => defaultPrimitiveForField(manifest, field, index));
  const controlPrimitives = carryForwardPrimitives(manifest, uiSpec, defaults, previousManifest);
  const generatedViews = buildGeneratedViews(
    {
      ...manifest,
      controlPrimitives,
    },
    controlPrimitives,
    uiSpec,
  );
  const controlViews = carryForwardViews(
    {
      ...manifest,
      controlPrimitives,
      controlViews: manifest.controlViews || [],
    },
    generatedViews,
    previousManifest,
  );
  const advisories = buildGeneratedAdvisories(
    {
      ...manifest,
      controlPrimitives,
      controlViews,
    },
    uiSpec,
    params,
  );

  return {
    ...manifest,
    controlPrimitives,
    controlViews,
    advisories,
  };
}

export function resolveActiveControlViewId(
  manifest: ModelManifest | null,
  selectedPartId: string | null,
  requestedViewId: string | null,
): string | null {
  const views = manifest?.controlViews || [];
  if (views.length === 0) return null;

  if (requestedViewId && views.some((view) => view.viewId === requestedViewId)) {
    const requested = views.find((view) => view.viewId === requestedViewId) ?? null;
    if (
      requested &&
      (!selectedPartId ||
        requested.scope === 'global' ||
        (requested.partIds || []).includes(selectedPartId))
    ) {
      return requested.viewId;
    }
  }

  return [...views]
    .sort((left, right) => {
      const priorityDelta = viewPriority(left, selectedPartId) - viewPriority(right, selectedPartId);
      if (priorityDelta !== 0) return priorityDelta;
      return sortByOrder(left, right);
    })[0]?.viewId ?? null;
}

function materializePrimitive(
  primitive: ControlPrimitive,
  uiSpec: UiSpec,
  params: DesignParams,
): MaterializedSemanticControl | null {
  const firstBinding = primitive.bindings?.[0];
  if (!firstBinding) return null;
  const rawField = findFieldByKey(uiSpec, firstBinding.parameterKey);
  if (!rawField) return null;

  return {
    primitiveId: primitive.primitiveId,
    label: primitive.label,
    kind: primitive.kind,
    source: primitive.source ?? 'generated',
    editable: primitive.editable,
    partIds: primitive.partIds || [],
    order: primitive.order || 0,
    rawField,
    bindings: primitive.bindings || [],
    value: readPrimitiveValue(primitive, uiSpec, params),
  };
}

export function materializeControlViews(
  manifest: ModelManifest | null,
  uiSpec: UiSpec | null | undefined,
  params: DesignParams,
): MaterializedSemanticView[] {
  if (!manifest || !uiSpec) return [];
  const primitivesById = new Map(
    (manifest.controlPrimitives || [])
      .map((primitive) => [primitive.primitiveId, materializePrimitive(primitive, uiSpec, params)] as const)
      .filter((entry): entry is readonly [string, MaterializedSemanticControl] => Boolean(entry[1])),
  );

  return (manifest.controlViews || [])
    .map((view) => {
      const sections = (view.sections || [])
        .map((section) => ({
          sectionId: section.sectionId,
          label: section.label,
          collapsed: Boolean(section.collapsed),
          controls: (section.primitiveIds || [])
            .map((primitiveId) => primitivesById.get(primitiveId) ?? null)
            .filter((control): control is MaterializedSemanticControl => Boolean(control))
            .sort((left, right) => left.order - right.order || left.label.localeCompare(right.label)),
        }))
        .filter((section) => section.controls.length > 0);

      const advisories = (manifest.advisories || []).filter((advisory) => {
        if ((advisory.viewIds || []).includes(view.viewId)) return true;
        return (advisory.primitiveIds || []).some((primitiveId) =>
          sections.some((section) => section.controls.some((control) => control.primitiveId === primitiveId)),
        );
      });

      return {
        viewId: view.viewId,
        label: view.label,
        scope: view.scope,
        partIds: view.partIds || [],
        isDefault: Boolean(view.default),
        source: view.source ?? 'generated',
        status: view.status ?? 'none',
        order: view.order ?? 0,
        sections,
        advisories,
      };
    })
    .filter((view) => view.sections.length > 0)
    .sort(sortByOrder);
}

export function buildPrimitivePatch(
  manifest: ModelManifest | null,
  primitiveId: string,
  nextValue: ParamValue,
  uiSpec: UiSpec | null | undefined,
): DesignParams {
  if (!manifest || !uiSpec) return {};
  const primitive = (manifest.controlPrimitives || []).find((entry) => entry.primitiveId === primitiveId);
  if (!primitive) return {};
  const patch: DesignParams = {};

  for (const binding of primitive.bindings || []) {
    const field = findFieldByKey(uiSpec, binding.parameterKey);
    if (!field) continue;

    if (field.type === 'checkbox') {
      patch[binding.parameterKey] = Boolean(nextValue);
      continue;
    }

    if (field.type === 'select' || field.type === 'image') {
      patch[binding.parameterKey] = nextValue;
      continue;
    }

    const numeric = Number(nextValue);
    if (!Number.isFinite(numeric)) continue;
    const raw = clampNumber(numeric * (binding.scale || 1) + (binding.offset || 0), binding);
    patch[binding.parameterKey] = raw;
  }

  return patch;
}

function applyRelationToValue(
  sourceValue: ParamValue,
  relation: ControlRelation,
): ParamValue {
  const numeric = Number(sourceValue);
  if (!Number.isFinite(numeric)) {
    return sourceValue;
  }
  switch (relation.mode) {
    case 'mirror':
      return numeric;
    case 'scale':
      return numeric * (relation.scale ?? 1);
    case 'offset':
      return numeric + (relation.offset ?? 0);
    default:
      return numeric;
  }
}

export function buildSemanticPatch(
  manifest: ModelManifest | null,
  primitiveId: string,
  nextValue: ParamValue,
  uiSpec: UiSpec | null | undefined,
): DesignParams {
  if (!manifest || !uiSpec) return {};
  const patch: DesignParams = {
    ...buildPrimitivePatch(manifest, primitiveId, nextValue, uiSpec),
  };
  const visited = new Set<string>([primitiveId]);
  const queue: Array<{ primitiveId: string; value: ParamValue }> = [{ primitiveId, value: nextValue }];

  while (queue.length > 0) {
    const current = queue.shift();
    if (!current) continue;
    const outgoing = (manifest.controlRelations || []).filter(
      (relation) => relation.enabled !== false && relation.sourcePrimitiveId === current.primitiveId,
    );
    for (const relation of outgoing) {
      if (visited.has(relation.targetPrimitiveId)) continue;
      const targetValue = applyRelationToValue(current.value, relation);
      Object.assign(patch, buildPrimitivePatch(manifest, relation.targetPrimitiveId, targetValue, uiSpec));
      visited.add(relation.targetPrimitiveId);
      queue.push({ primitiveId: relation.targetPrimitiveId, value: targetValue });
    }
  }

  return patch;
}
