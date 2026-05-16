import type {
  SketchConstraint,
  SketchDocument,
  SketchPrimitive,
  SketchPrimitiveTopology,
  SketchSuggestionRequest,
  SketchView,
} from './tauri/contracts';
import type { SketchPoint, SketchStroke } from './sketchWorkspaceState';
import { parseSketchDocumentEnvelope } from './sketchDocumentEnvelope';
import { validateSketchDocumentConstraints } from './sketchConstraintValidation';

export const EMPTY_SKETCH_DOCUMENT_REPLAY_ERROR = 'Sketch document unavailable.';

export type SketchDocumentSourceInput = SketchDocument | SketchSuggestionRequest | null | undefined;

export type SketchDocumentParseResult = { document: SketchDocument } | { error: string };

export type SketchDocumentReplayResult = { strokes: SketchStroke[] } | { error: string };

const EMPTY_SKETCH_DOCUMENT_JSON_ERROR = 'Sketch document JSON is empty.';
const MISSING_SKETCH_DOCUMENT_JSON_ERROR = 'Sketch document JSON missing document/sketches.';

export function parseSketchDocumentSource(source: SketchDocumentSourceInput): SketchDocumentParseResult {
  if (!source) return { error: EMPTY_SKETCH_DOCUMENT_REPLAY_ERROR };

  if ('document' in source) {
    if (!source.document) return { error: EMPTY_SKETCH_DOCUMENT_REPLAY_ERROR };
    return { document: source.document };
  }

  return { document: source };
}

export function parseSketchDocumentJson(source: string): SketchDocumentParseResult {
  const trimmed = source.trim();
  if (!trimmed) return { error: EMPTY_SKETCH_DOCUMENT_JSON_ERROR };

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch (error) {
    const message = looksTruncatedJson(trimmed) ? 'Unexpected end of JSON input' : error instanceof Error ? error.message : String(error);
    return { error: `Sketch document JSON is invalid: ${message}` };
  }

  return parseSketchDocumentJsonValue(parsed);
}

export function parseSketchDocumentImportSource(source: string): SketchDocumentParseResult {
  const trimmed = source.trim();
  if (!trimmed) return { error: EMPTY_SKETCH_DOCUMENT_JSON_ERROR };

  if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
    return parseSketchDocumentJson(trimmed);
  }

  return parseSketchDocumentEnvelope(trimmed);
}

export function sketchDocumentJsonToStrokes(source: string): SketchDocumentReplayResult {
  const parsed = parseSketchDocumentJson(source);
  if ('error' in parsed) return parsed;

  return sketchDocumentToStrokes(parsed.document);
}

export function sketchDocumentToStrokes(source: SketchDocumentSourceInput): SketchDocumentReplayResult {
  const parsed = parseSketchDocumentSource(source);
  if ('error' in parsed) return parsed;

  const sketches = parsed.document.sketches;
  if (!sketches || sketches.length === 0) {
    return { error: 'Sketch document has no sketches.' };
  }

  const constraintValidation = validateSketchDocumentConstraints(parsed.document);
  if (!constraintValidation.passed) {
    return { error: constraintValidation.issues.join(' ') };
  }

  const strokes: SketchStroke[] = [];

  for (const sketch of sketches) {
    const primitives = sketch.primitives ?? [];

    for (const primitive of primitives) {
      const replay = primitiveToStroke(sketch.view, sketch.sketchId, primitive, sketch.constraints ?? []);
      if ('error' in replay) return replay;
      strokes.push(replay.stroke);
    }
  }

  return { strokes };
}

export function nextPrimitiveSequenceFromStrokes(strokes: SketchStroke[]): number {
  let highestSequence = 0;

  for (const stroke of strokes) {
    const sequence = primitiveSequenceFromPrimitiveId(stroke.primitiveId);
    if (sequence > highestSequence) highestSequence = sequence;
  }

  return highestSequence;
}

function primitiveToStroke(
  view: SketchView,
  sketchId: string,
  primitive: SketchPrimitive,
  constraints: SketchConstraint[],
): { stroke: SketchStroke } | { error: string } {
  if (primitive.kind === 'circle') {
    const center = primitive.points?.[0];
    if (!isValidPoint(center)) {
      return {
        error: `sketch '${sketchId}' primitive '${primitive.primitiveId}' has invalid point at index 0.`,
      };
    }
    if (typeof primitive.radius !== 'number' || !Number.isFinite(primitive.radius) || primitive.radius <= 0) {
      return {
        error: `sketch '${sketchId}' primitive '${primitive.primitiveId}' has invalid radius.`,
      };
    }
    return {
      stroke: {
        primitiveId: primitive.primitiveId,
        sketchId,
        view,
        kind: 'circle',
        points: [[center[0], center[1]]],
        closed: true,
        radius: primitive.radius,
        ...(primitive.topology ? { topology: copyTopology(primitive.topology) } : {}),
      },
    };
  }

  if (primitive.kind !== 'polyline') {
    return {
      error: `sketch '${sketchId}' primitive '${primitive.primitiveId}' has unsupported kind '${primitive.kind}'.`,
    };
  }

  if (primitive.closed !== true) {
    return {
      error: `sketch '${sketchId}' primitive '${primitive.primitiveId}' is not closed.`,
    };
  }

  const points = primitive.points;
  if (!points || points.length === 0) {
    return {
      error: `sketch '${sketchId}' primitive '${primitive.primitiveId}' has invalid points.`,
    };
  }

  const strokePoints: SketchPoint[] = [];
  for (let index = 0; index < points.length; index += 1) {
    const point = points[index];
    if (!isValidPoint(point)) {
      return {
        error: `sketch '${sketchId}' primitive '${primitive.primitiveId}' has invalid point at index ${index}.`,
      };
    }

    strokePoints.push([point[0], point[1]]);
  }

  return {
    stroke: {
      primitiveId: primitive.primitiveId,
      sketchId,
      view,
      kind: 'polyline',
      points: strokePoints,
      closed: true,
      ...(primitive.topology ? { topology: copyTopology(primitive.topology) } : {}),
      ...dimensionLocksForPrimitive(primitive.primitiveId, constraints),
    },
  };
}

function dimensionLocksForPrimitive(
  primitiveId: string,
  constraints: SketchConstraint[],
): Pick<SketchStroke, 'dimensionLocks'> {
  const dimensionLocks = constraints.reduce<NonNullable<SketchStroke['dimensionLocks']>>((locks, constraint) => {
    if (constraint.kind !== 'dimension') return locks;
    if (!(constraint.targetIds ?? []).includes(primitiveId)) return locks;
    if (constraint.constraintId.includes('width')) locks.width = true;
    if (constraint.constraintId.includes('height')) locks.height = true;
    return locks;
  }, {});

  return dimensionLocks.width || dimensionLocks.height ? { dimensionLocks } : {};
}

function isValidPoint(point: unknown): point is SketchPoint {
  if (!Array.isArray(point) || point.length !== 2) return false;
  return Number.isFinite(point[0]) && Number.isFinite(point[1]);
}

function primitiveSequenceFromPrimitiveId(primitiveId: string): number {
  const match = primitiveId.match(/(\d+)$/);
  if (!match) return 0;
  return Number(match[1]);
}

function copyTopology(topology: SketchPrimitiveTopology | null | undefined): SketchPrimitiveTopology | null | undefined {
  if (!topology) return topology;
  return {
    ...topology,
    edgeIds: topology.edgeIds ? [...topology.edgeIds] : undefined,
  };
}

function parseSketchDocumentJsonValue(value: unknown): SketchDocumentParseResult {
  if (!isObject(value)) {
    return { error: MISSING_SKETCH_DOCUMENT_JSON_ERROR };
  }

  if ('document' in value) {
    const document = value.document;
    if (!isObject(document)) {
      return { error: MISSING_SKETCH_DOCUMENT_JSON_ERROR };
    }
    return { document: document as SketchDocument };
  }

  if ('sketches' in value) {
    return { document: value as SketchDocument };
  }

  return { error: MISSING_SKETCH_DOCUMENT_JSON_ERROR };
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function looksTruncatedJson(source: string): boolean {
  const stack: string[] = [];
  let inString = false;
  let escaped = false;

  for (const char of source) {
    if (escaped) {
      escaped = false;
      continue;
    }

    if (char === '\\' && inString) {
      escaped = true;
      continue;
    }

    if (char === '"') {
      inString = !inString;
      continue;
    }

    if (inString) continue;

    if (char === '{') stack.push('}');
    if (char === '[') stack.push(']');
    if ((char === '}' || char === ']') && stack.at(-1) === char) stack.pop();
  }

  return inString || stack.length > 0;
}
