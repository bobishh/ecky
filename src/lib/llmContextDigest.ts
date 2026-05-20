import type { DesignParams, ModelManifest, ParamValue, SourceLanguage, UiField, UiSpec } from './types/domain';

const MAX_DIGEST_PARTS = 6;
const MAX_DIGEST_FIELDS = 10;
const MAX_DIGEST_PARAMS = 12;

function compactText(value: string | null | undefined, maxChars = 72): string {
  const normalized = `${value ?? ''}`.trim().replace(/\s+/g, ' ');
  if (normalized.length <= maxChars) return normalized;
  return `${normalized.slice(0, Math.max(0, maxChars - 1))}…`;
}

function formatNumber(value: number): string {
  if (!Number.isFinite(value)) return 'NaN';
  const rounded = Math.round(value * 100) / 100;
  return Number.isInteger(rounded) ? `${rounded}` : rounded.toFixed(2).replace(/\.?0+$/, '');
}

function formatParamValue(value: ParamValue): string {
  if (typeof value === 'number') return formatNumber(value);
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (value == null) return 'null';
  return `"${compactText(String(value), 32)}"`;
}

function fieldKindLabel(field: UiField): string {
  switch (field.type) {
    case 'range':
    case 'number':
      return `${field.type}${field.min != null || field.max != null ? ` ${field.min ?? 'min'}..${field.max ?? 'max'}` : ''}`;
    case 'select':
      return `select (${field.options.length} options)`;
    case 'checkbox':
      return 'checkbox';
    case 'image':
      return 'image';
  }
}

function buildUiSpecDigest(uiSpec: UiSpec | null | undefined): string | null {
  const fields = uiSpec?.fields ?? [];
  if (!fields.length) return null;
  const lines = [
    `UI fields: ${fields.length}`,
    ...fields.slice(0, MAX_DIGEST_FIELDS).map((field) => {
      const label = compactText(field.label || field.key, 28);
      return `- ${field.key}: ${fieldKindLabel(field)}${label && label !== field.key ? ` (${label})` : ''}`;
    }),
  ];
  if (fields.length > MAX_DIGEST_FIELDS) {
    lines.push(`- … ${fields.length - MAX_DIGEST_FIELDS} more fields`);
  }
  return lines.join('\n');
}

function buildParamsDigest(params: DesignParams | null | undefined): string | null {
  const entries = Object.entries(params ?? {});
  if (!entries.length) return null;
  const lines = [
    `Current params: ${entries.length}`,
    ...entries
      .sort(([left], [right]) => left.localeCompare(right))
      .slice(0, MAX_DIGEST_PARAMS)
      .map(([key, value]) => `- ${key} = ${formatParamValue(value)}`),
  ];
  if (entries.length > MAX_DIGEST_PARAMS) {
    lines.push(`- … ${entries.length - MAX_DIGEST_PARAMS} more params`);
  }
  return lines.join('\n');
}

function formatPartSize(parts: NonNullable<ModelManifest['parts']>, index: number): string | null {
  const bounds = parts[index]?.bounds;
  if (!bounds) return null;
  const width = bounds.xMax - bounds.xMin;
  const depth = bounds.yMax - bounds.yMin;
  const height = bounds.zMax - bounds.zMin;
  if (![width, depth, height].every(Number.isFinite)) return null;
  return `${formatNumber(width)}×${formatNumber(depth)}×${formatNumber(height)} mm`;
}

function buildManifestDigest(manifest: ModelManifest | null | undefined): string | null {
  if (!manifest) return null;
  const parts = manifest.parts ?? [];
  const lines = [
    `Model parts: ${parts.length}`,
    ...parts.slice(0, MAX_DIGEST_PARTS).map((part, index) => {
      const size = formatPartSize(parts, index);
      const role = part.semanticRole ? ` role=${part.semanticRole}` : '';
      const kind = part.kind ? ` [${part.kind}]` : '';
      return `- ${compactText(part.label || part.partId, 40)}${kind}${role}${size ? ` size≈${size}` : ''}`;
    }),
  ];
  if (parts.length > MAX_DIGEST_PARTS) {
    lines.push(`- … ${parts.length - MAX_DIGEST_PARTS} more parts`);
  }
  const warnings = manifest.warnings?.length ?? 0;
  const advisories = manifest.advisories?.length ?? 0;
  const views = manifest.controlViews?.length ?? 0;
  if (warnings || advisories || views) {
    lines.push(`Manifest signals: warnings=${warnings}, advisories=${advisories}, views=${views}`);
  }
  return lines.join('\n');
}

function authoringSourceExtension(
  sourceLanguage: SourceLanguage | null | undefined,
  geometryBackend: ModelManifest['geometryBackend'] | null | undefined,
) : string | null {
  switch (sourceLanguage) {
    case 'build123d':
      return '.py';
    case 'legacyPython':
      return '.FCMacro';
    case 'ecky':
      return '.ecky';
    default:
      break;
  }

  return geometryBackend ? '.ecky' : null;
}

export function buildAuthoringDigest(input: {
  title?: string | null;
  versionName?: string | null;
  sourceLanguage?: SourceLanguage | null;
  uiSpec?: UiSpec | null;
  params?: DesignParams | null;
  modelManifest?: ModelManifest | null;
}): string {
  const blocks: string[] = [];
  const title = compactText(input.title ?? '', 64);
  const versionName = compactText(input.versionName ?? '', 32);
  const sourceLanguage = sourceLanguageLabel(input.sourceLanguage);
  const sourceLanguageSuffix = sourceLanguage ? ` (${sourceLanguage})` : '';
  if (title || versionName) {
    blocks.push(
      `CURRENT WORKING SNAPSHOT\n${title || 'Untitled'}${versionName ? ` [${versionName}]` : ''}${sourceLanguageSuffix}`,
    );
  }
  const sourceExtension = authoringSourceExtension(
    input.sourceLanguage,
    input.modelManifest?.geometryBackend,
  );
  if (sourceExtension) {
    const backend = input.modelManifest?.geometryBackend;
    const lines = [`sourceExtension=${sourceExtension}`];
    if (backend) lines.push(`geometryBackend=${backend}`);
    blocks.push(
      `AUTHORING HINTS\n${lines.join('\n')}`,
    );
  }
  const manifestDigest = buildManifestDigest(input.modelManifest);
  if (manifestDigest) blocks.push(manifestDigest);
  const uiSpecDigest = buildUiSpecDigest(input.uiSpec);
  if (uiSpecDigest) blocks.push(uiSpecDigest);
  const paramsDigest = buildParamsDigest(input.params);
  if (paramsDigest) blocks.push(paramsDigest);
  return blocks.join('\n\n');
}
function sourceLanguageLabel(sourceLanguage: SourceLanguage | null | undefined): string {
  if (sourceLanguage === 'ecky') return 'ecky';
  if (sourceLanguage === 'legacyPython') return 'freecad';
  return sourceLanguage ?? '';
}
