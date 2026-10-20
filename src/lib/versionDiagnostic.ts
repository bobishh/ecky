const SECRET_ASSIGNMENT = /((?:["']?(?:api[_-]?key|access[_-]?token|refresh[_-]?token|authorization|password|secret)["']?)\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s"',;}]+)/gi;
const PRIVATE_JSON_PAYLOAD = /\{\s*["']?(?:raw[_ ]?body|response[_ ]?body|private[_ ]?payload|payload)["']?\s*:\s*[\s\S]*$/i;
const PRIVATE_PAYLOAD = /((?:["']?(?:raw[_ ]?body|response[_ ]?body|private[_ ]?payload|payload)["']?)\s*[:=]\s*)[\s\S]*$/i;

export function versionDiagnosticForDisplay(value: string | null | undefined): string | null {
  const diagnostic = value?.trim();
  if (!diagnostic) return null;
  return diagnostic
    .replace(/\bBearer\s+[^\s,;]+/gi, 'Bearer [REDACTED]')
    .replace(/\bsk-[A-Za-z0-9_-]{12,}\b/g, '[REDACTED]')
    .replace(SECRET_ASSIGNMENT, '$1[REDACTED]')
    .replace(PRIVATE_JSON_PAYLOAD, '[REDACTED]')
    .replace(PRIVATE_PAYLOAD, '$1[REDACTED]')
    .slice(0, 2000);
}
