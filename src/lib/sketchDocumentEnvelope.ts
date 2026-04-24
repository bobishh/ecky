import type { SketchDocument } from './tauri/contracts';

export type SketchDocumentEnvelopeResult = { document: SketchDocument } | { error: string };

export const ECKY_SKETCH_DOCUMENT_BASE64_MARKER = '; ecky-sketch-document-base64:';

const MISSING_MARKER_ERROR = 'Sketch document marker missing.';
const INVALID_BASE64_ERROR = 'Sketch document base64 is invalid.';
const INVALID_JSON_ERROR = 'Sketch document JSON is invalid.';
const MISSING_SKETCHES_ERROR = 'Sketch document has no sketches.';

export function parseSketchDocumentEnvelope(source: string): SketchDocumentEnvelopeResult {
  const markerIndex = source.indexOf(ECKY_SKETCH_DOCUMENT_BASE64_MARKER);
  if (markerIndex < 0) return { error: MISSING_MARKER_ERROR };

  const encoded = source.slice(markerIndex + ECKY_SKETCH_DOCUMENT_BASE64_MARKER.length).split(/\r?\n/, 1)[0].trim();
  if (!encoded) return { error: INVALID_BASE64_ERROR };

  const bytes = decodeBase64(encoded);
  if (!bytes) return { error: INVALID_BASE64_ERROR };

  let jsonText: string;
  try {
    jsonText = decodeUtf8(bytes);
  } catch {
    return { error: INVALID_JSON_ERROR };
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(jsonText);
  } catch {
    return { error: INVALID_JSON_ERROR };
  }

  if (!isObject(parsed)) {
    return { error: INVALID_JSON_ERROR };
  }

  const sketches = parsed.sketches;
  if (!Array.isArray(sketches) || sketches.length === 0) {
    return { error: MISSING_SKETCHES_ERROR };
  }

  return { document: parsed as SketchDocument };
}

function decodeBase64(input: string): Uint8Array | null {
  const normalized = input.replace(/\s+/g, '');
  if (!normalized || !isStrictBase64(normalized)) return null;

  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=');

  try {
    if (typeof atob === 'function') {
      const binary = atob(padded);
      const bytes = new Uint8Array(binary.length);
      for (let index = 0; index < binary.length; index += 1) {
        bytes[index] = binary.charCodeAt(index);
      }
      return bytes;
    }

    return Uint8Array.from(Buffer.from(padded, 'base64'));
  } catch {
    return null;
  }
}

function decodeUtf8(bytes: Uint8Array): string {
  if (typeof TextDecoder === 'function') {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  }

  return Buffer.from(bytes).toString('utf8');
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isStrictBase64(value: string): boolean {
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(value)) return false;
  return value.length % 4 !== 1;
}
