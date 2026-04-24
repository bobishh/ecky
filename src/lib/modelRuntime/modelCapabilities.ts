export type ModelCapabilitySummary = {
  supportsVision: boolean;
  reason: string | null;
};

const NVIDIA_NIM_TEXT_ONLY_REASON =
  'Selected NVIDIA NIM model looks text-only. Image attachments, concept-preview reuse, and screenshot verification are unavailable.';

const NVIDIA_NIM_HOSTS = new Set(['integrate.api.nvidia.com']);

const VISION_MODEL_PATTERNS = [
  /\bmultimodal\b/i,
  /\bpixtral\b/i,
  /\bvision\b/i,
  /(?:^|[\/\s_-])vl(?:$|[\/\s_-])/i,
];

function normalizeValue(value: string | null | undefined): string {
  return typeof value === 'string' ? value.trim() : '';
}

function isNvidiaNimEndpoint(provider: string, baseUrl: string): boolean {
  if (normalizeValue(provider).toLowerCase() !== 'openai') return false;

  const normalizedBaseUrl = normalizeValue(baseUrl).toLowerCase();
  if (!normalizedBaseUrl) return false;

  try {
    return NVIDIA_NIM_HOSTS.has(new URL(normalizedBaseUrl).hostname);
  } catch {
    return normalizedBaseUrl.includes('integrate.api.nvidia.com');
  }
}

function modelLooksVisionCapable(model: string): boolean {
  const normalizedModel = normalizeValue(model).toLowerCase();
  if (!normalizedModel) return true;
  return VISION_MODEL_PATTERNS.some((pattern) => pattern.test(normalizedModel));
}

function inferNvidiaNimVisionSupport(
  provider: string,
  baseUrl: string,
  model: string,
): boolean | null {
  if (!isNvidiaNimEndpoint(provider, baseUrl)) return null;
  return modelLooksVisionCapable(model);
}

export function isVisionCapableModel(
  provider: string,
  baseUrl: string,
  model: string,
): boolean {
  return inferNvidiaNimVisionSupport(provider, baseUrl, model) ?? true;
}

export function visionUnavailableReason(
  provider: string,
  baseUrl: string,
  model: string,
): string | null {
  const supportsVision = inferNvidiaNimVisionSupport(provider, baseUrl, model);
  return supportsVision === false ? NVIDIA_NIM_TEXT_ONLY_REASON : null;
}

export function inferModelCapabilities(
  provider: string,
  baseUrl: string,
  model: string,
): ModelCapabilitySummary {
  const reason = visionUnavailableReason(provider, baseUrl, model);
  return {
    supportsVision: reason === null,
    reason,
  };
}
