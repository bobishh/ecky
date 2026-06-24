import type { DesignParams, ModelManifest, UiField, UiSpec } from './types/domain';

export type MacroAstMapNodeKind = 'model' | 'part' | 'port' | 'param' | 'verify';

export type MacroAstSourceRange = { startByte: number; endByte: number };

export type MacroAstMapNode = {
  id: string;
  kind: MacroAstMapNodeKind;
  label: string;
  value: string | number | boolean | null;
  fieldKey?: string;
  syntaxVariant?: string;
  syntaxLabel?: string;
  /** Exact byte range of this node in the macro source, when known. */
  sourceRange?: MacroAstSourceRange;
  children: MacroAstMapNode[];
};

/** Shape of `macro_ast_source_map` results (backend command). */
export type MacroAstSourceMapEntry = {
  id: string;
  kind: string;
  label: string;
  startByte: number;
  endByte: number;
};

export type MacroAstMapProjection = {
  root: MacroAstMapNode;
};

type MacroAstMapInput = {
  macroCode?: string;
  modelManifest?: ModelManifest | null;
  uiSpec?: UiSpec | null;
  parameters?: DesignParams;
  sourceNodes?: MacroAstSourceMapEntry[] | null;
};

type MacroAstPart = {
  partId: string;
  label: string;
  parameterKeys?: string[] | null;
};

function normalizeFieldLabel(field: UiField): string {
  return `${field.label ?? field.key ?? ''}`.trim() || field.key;
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined || value === '') return 'Unset';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return `${value}`;
  try {
    return JSON.stringify(value);
  } catch {
    return `${value}`;
  }
}

function synthesizeParts(fields: UiField[]): MacroAstPart[] {
  return [
    {
      partId: 'macro-part',
      label: 'Parameter Region',
      parameterKeys: fields.map((field) => field.key),
    },
  ];
}

function normalizeSyntaxVariant(value: string | null | undefined): string {
  const normalized = `${value ?? ''}`.trim().toLowerCase();
  return normalized.replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'unknown';
}

function sourceRangeFor(
  entries: Map<string, MacroAstSourceMapEntry>,
  ...ids: string[]
): MacroAstSourceRange | undefined {
  for (const id of ids) {
    const entry = entries.get(id);
    if (entry) return { startByte: entry.startByte, endByte: entry.endByte };
  }
  return undefined;
}

function paramNode(
  idPrefix: string,
  field: UiField,
  value: string | number | boolean | null,
): MacroAstMapNode {
  const fieldSyntaxVariant = normalizeSyntaxVariant(field.type);
  return {
    id: `${idPrefix}/param:${field.key}`,
    kind: 'param',
    label: normalizeFieldLabel(field),
    value,
    fieldKey: field.key,
    syntaxVariant: fieldSyntaxVariant,
    syntaxLabel: fieldSyntaxVariant.toUpperCase(),
    children: [],
  };
}

/**
 * Splices an edited slice back into a base document.
 *
 * `start`/`end` are clamped to `[0, base.length]`; an inverted range
 * (`start > end`) collapses to a zero-width point at the clamped `start` so
 * the result is a pure insertion instead of a throw or a reordering.
 */
/**
 * Locates the id of the part node (direct child of `root`) whose param
 * children include `fieldKey`. Used by focus flows to decide which part must
 * be auto-expanded before a param control can receive focus.
 */
export function findOwningPartId(root: MacroAstMapNode, fieldKey: string | undefined | null): string | null {
  if (!fieldKey) return null;
  for (const part of root.children ?? []) {
    if (part.children?.some((param) => param.fieldKey === fieldKey)) return part.id;
  }
  return null;
}

export function spliceMacroSource(base: string, start: number, end: number, slice: string): string {
  const length = base.length;
  const clampedStart = Math.max(0, Math.min(start, length));
  const clampedEnd = Math.max(clampedStart, Math.min(end, length));
  return base.slice(0, clampedStart) + slice + base.slice(clampedEnd);
}

export function buildMacroAstMapProjection(input: MacroAstMapInput): MacroAstMapProjection {
  const sourceEntries = new Map(
    (input.sourceNodes ?? []).map((entry) => [entry.id, entry]),
  );
  const fields = Array.isArray(input.uiSpec?.fields)
    ? input.uiSpec.fields.filter((field): field is UiField => Boolean(field))
    : [];
  const fieldByKey = new Map(fields.map((field) => [field.key, field]));
  const manifestParts = input.modelManifest?.parts || [];
  const partById = new Map(manifestParts.map((part) => [part.partId, part]));
  const parts = (input.modelManifest?.parts?.length ? input.modelManifest.parts : synthesizeParts(fields)).map(
    (part) => ({
      partId: part.partId,
      label: `${part.label ?? part.partId}`.trim() || part.partId,
      parameterKeys: Array.isArray(part.parameterKeys) ? [...part.parameterKeys] : [],
    }),
  );

  // A field belongs to a part only when exactly one part claims it. Fields
  // claimed by several parts (or by none) are model-level knobs: rendered
  // once in a shared group instead of duplicated under every part.
  const claimCounts = new Map<string, number>();
  for (const part of parts) {
    for (const key of part.parameterKeys) {
      if (!fieldByKey.has(key)) continue;
      claimCounts.set(key, (claimCounts.get(key) ?? 0) + 1);
    }
  }
  const valueOf = (field: UiField) =>
    (input.parameters?.[field.key] ?? null) as string | number | boolean | null;

  const partNodes: MacroAstMapNode[] = parts.map((part, partIndex) => {
    const ownFields = part.parameterKeys
      .filter((key) => claimCounts.get(key) === 1)
      .map((key) => fieldByKey.get(key))
      .filter((field): field is UiField => Boolean(field));
    return {
      id: `part:${part.partId}`,
      kind: 'part',
      label: part.label || `Part ${partIndex + 1}`,
      value: null,
      sourceRange: sourceRangeFor(
        sourceEntries,
        `part:${part.partId}`,
        `feature:${part.partId}`,
      ),
      syntaxVariant: normalizeSyntaxVariant(partById.get(part.partId)?.kind ?? 'part'),
      syntaxLabel: normalizeSyntaxVariant(partById.get(part.partId)?.kind ?? 'part').toUpperCase(),
      children: ownFields.map((field) => paramNode(`part:${part.partId}`, field, valueOf(field))),
    };
  });

  const sharedFields = fields.filter((field) => (claimCounts.get(field.key) ?? 0) !== 1);
  const sharedGroup: MacroAstMapNode[] = sharedFields.length
    ? [
        {
          id: 'shared-params',
          kind: 'part',
          label: 'Model Params',
          value: null,
          syntaxVariant: 'shared',
          syntaxLabel: 'SHARED',
          children: sharedFields.map((field) => paramNode('shared', field, valueOf(field))),
        },
      ]
    : [];
  const verifyNodes: MacroAstMapNode[] = (input.sourceNodes ?? [])
    .filter((entry) => entry.kind === 'verify')
    .map((entry, index) => {
      const tag = `${entry.label || ''}`.trim();
      return {
        // Key by tag so an authored verify chip (`verify:<tag>`) focuses this
        // node; fall back to the source-map id when the clause has no tag.
        id: tag ? `verify:${tag}` : entry.id || `verify:${index}`,
        kind: 'verify' as const,
        label: tag || `verify ${index + 1}`,
        value: null,
        syntaxVariant: 'verify',
        syntaxLabel: 'VERIFY',
        sourceRange: { startByte: entry.startByte, endByte: entry.endByte },
        children: [],
      } satisfies MacroAstMapNode;
    });

  const root: MacroAstMapNode = {
    id: 'macro-root',
    kind: 'model',
    label: 'Macro Root',
    value: null,
    syntaxVariant: 'model',
    syntaxLabel: 'MODEL',
    sourceRange: sourceRangeFor(sourceEntries, 'model'),
    children: [...verifyNodes, ...sharedGroup, ...partNodes],
  };

  return { root };
}
