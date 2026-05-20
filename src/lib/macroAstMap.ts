import type { DesignParams, ModelManifest, UiField, UiSpec } from './types/domain';

export type MacroAstMapNodeKind = 'model' | 'part' | 'port' | 'param';

export type MacroAstMapNode = {
  id: string;
  kind: MacroAstMapNodeKind;
  label: string;
  value: string | number | boolean | null;
  fieldKey?: string;
  syntaxVariant?: string;
  syntaxLabel?: string;
  children: MacroAstMapNode[];
};

export type MacroAstMapProjection = {
  root: MacroAstMapNode;
};

type MacroAstMapInput = {
  macroCode?: string;
  modelManifest?: ModelManifest | null;
  uiSpec?: UiSpec | null;
  parameters?: DesignParams;
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

export function buildMacroAstMapProjection(input: MacroAstMapInput): MacroAstMapProjection {
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

  const root: MacroAstMapNode = {
    id: 'macro-root',
    kind: 'model',
    label: 'Macro Root',
    value: null,
    syntaxVariant: 'model',
    syntaxLabel: 'MODEL',
    children: parts.map((part, partIndex) => {
      const partFields = part.parameterKeys.length > 0
        ? part.parameterKeys
            .map((key) => fieldByKey.get(key))
            .filter((field): field is UiField => Boolean(field))
        : fields;

      return {
        id: `part:${part.partId}`,
        kind: 'part',
        label: part.label || `Part ${partIndex + 1}`,
        value: null,
        syntaxVariant: normalizeSyntaxVariant(partById.get(part.partId)?.kind ?? 'part'),
        syntaxLabel: normalizeSyntaxVariant(partById.get(part.partId)?.kind ?? 'part').toUpperCase(),
        children: partFields.map((field) => {
          const value = input.parameters?.[field.key] ?? null;
          const fieldSyntaxVariant = normalizeSyntaxVariant(field.type);
          return {
            id: `part:${part.partId}/port:${field.key}`,
            kind: 'port',
            label: normalizeFieldLabel(field),
            value,
            fieldKey: field.key,
            syntaxVariant: fieldSyntaxVariant,
            syntaxLabel: 'PORT',
            children: [
              {
                id: `part:${part.partId}/param:${field.key}`,
                kind: 'param',
                label: `${normalizeFieldLabel(field)}: ${formatValue(value)}`,
                value,
                fieldKey: field.key,
                syntaxVariant: fieldSyntaxVariant,
                syntaxLabel: fieldSyntaxVariant.toUpperCase(),
                children: [],
              },
            ],
          };
        }),
      };
    }),
  };

  return { root };
}
