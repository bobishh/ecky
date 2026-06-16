import type { Engine } from '../tauri/contracts';
import type { VisionCapability } from '../tauri/contracts';

export type ModelCapabilitySummary = {
  supportsVision: boolean;
  reason: string | null;
};

const TEXT_ONLY_REASON =
  'Selected model is text-only. Image attachments, concept-preview reuse, screenshot capture, and drawing annotations are unavailable.';

/**
 * Model-id patterns that indicate a vision-capable model regardless of host.
 * Matched against the lower-cased model id.
 */
const VISION_POSITIVE_PATTERNS = [
  /\bmultimodal\b/i,
  /\bmulti-modal\b/i,
  /\bpixtral\b/i,
  /\bvision\b/i,
  /(?:^|[\/\s_-])vl(?:$|[\/\s_-])/i,
  // OpenAI vision-capable families
  /^gpt-4o/i,
  /^gpt-4\.1/i,
  /^gpt-4-turbo/i,
  /^chatgpt-4o/i,
  // Anthropic
  /\bclaude/i,
  // Google Gemini
  /\bgemini\b/i,
  // Z.AI vision naming: glm-4v, glm-5v-turbo, glm-ovs, etc.
  /(?:^|[0-9])v(?:[-_]|$)/i,
];

/**
 * Model-id patterns that indicate a text-only model. These win when a positive
 * vision pattern does NOT match, and are useful for the common text-only catalogs
 * (instruct models, coders, deepseek non-vl, glm without a vision marker).
 */
const TEXT_POSITIVE_PATTERNS = [
  // bare "instruct" without a vision qualifier
  /\binstruct\b/i,
  /\bcoder\b/i,
  /\bdeepseek\b(?!.*vl)/i,
  // GLM text variants: glm-4.x, glm-5.x without a trailing vision "v"
  /\bglm-[0-9]/i,
];

function normalizeValue(value: string | null | undefined): string {
  return typeof value === 'string' ? value.trim() : '';
}

/**
 * Name-pattern inference. Returns `true` for vision-capable, `false` for text-only,
 * or `null` when unknown (caller decides the optimistic default).
 *
 * Hostname-agnostic: only the model id drives the decision.
 */
export function inferVisionByName(model: string): boolean | null {
  const normalizedModel = normalizeValue(model).toLowerCase();
  if (!normalizedModel) return null;

  if (VISION_POSITIVE_PATTERNS.some((p) => p.test(normalizedModel))) return true;
  if (TEXT_POSITIVE_PATTERNS.some((p) => p.test(normalizedModel))) return false;

  // GLM vision variants like glm-5v-turbo already matched above; a bare glm-4/glm-5
  // matched the text pattern. Anything else is unknown.
  return null;
}

export function overrideForEngine(engine: {
  visionOverrides?: Partial<Record<string, VisionCapability>> | null;
}, model: string): VisionCapability | null {
  const overrides = engine?.visionOverrides;
  if (!overrides) return null;
  const key = normalizeValue(model);
  if (!key) return null;
  return overrides[key] ?? null;
}

/**
 * Resolve the effective vision capability for a given engine + model.
 *
 * Precedence:
 *   1. `visionOverrides[model]` (user-set or auto-disabled) — authoritative.
 *   2. Name-pattern inference.
 *   3. Optimistic default: vision-capable (so first-party OpenAI/Gemini work).
 */
export function resolveEngineVision(
  engine: Pick<Engine, 'visionOverrides' | 'provider' | 'baseUrl' | 'model'> | null | undefined,
  model?: string,
): boolean {
  if (!engine) return true;
  const targetModel = normalizeValue(model ?? engine.model ?? '');
  const override = overrideForEngine(engine, targetModel);
  if (override === 'vision') return true;
  if (override === 'textOnly') return false;

  const inferred = inferVisionByName(targetModel);
  if (inferred === null) return true; // optimistic
  return inferred;
}

export function resolveEngineCapabilitySummary(
  engine: Pick<Engine, 'visionOverrides' | 'provider' | 'baseUrl' | 'model'> | null | undefined,
  model?: string,
): ModelCapabilitySummary {
  const supportsVision = resolveEngineVision(engine, model);
  return {
    supportsVision,
    reason: supportsVision ? null : TEXT_ONLY_REASON,
  };
}

// ── Legacy call-surface (provider, baseUrl, model) ─────────────────────────
// Kept for call sites that still resolve from raw fields without an engine
// object. Prefer `resolveEngineVision` / `resolveEngineCapabilitySummary`.

export function isVisionCapableModel(
  provider: string,
  baseUrl: string,
  model: string,
): boolean {
  const inferred = inferVisionByName(model);
  if (inferred === null) return true;
  return inferred;
}

export function visionUnavailableReason(
  provider: string,
  baseUrl: string,
  model: string,
): string | null {
  return isVisionCapableModel(provider, baseUrl, model) ? null : TEXT_ONLY_REASON;
}

export function inferModelCapabilities(
  provider: string,
  baseUrl: string,
  model: string,
): ModelCapabilitySummary {
  return {
    supportsVision: isVisionCapableModel(provider, baseUrl, model),
    reason: visionUnavailableReason(provider, baseUrl, model),
  };
}
