import type {
  SketchDefinition,
  SketchDocument,
  SketchPrimitive,
  SketchSuggestionRequest,
} from './tauri/contracts';
import { constraintsForStroke } from './sketchWorkspaceState';
import type { SketchPoint, SketchStroke } from './sketchWorkspaceState';

export const WORKSPACE_SKETCH_DOCUMENT_ID = 'workspace-sketch-document';
export const EMPTY_SKETCH_SUGGESTION_ERROR = 'Close profile before suggestions.';

export type SketchSuggestionRequestResult = SketchSuggestionRequest | { error: string };

export function buildSketchSuggestionRequest(strokes: SketchStroke[]): SketchSuggestionRequestResult {
  const document = buildSketchSuggestionDocument(strokes);
  if (!document) return { error: EMPTY_SKETCH_SUGGESTION_ERROR };

  return { document };
}

export function buildSketchSuggestionDocument(strokes: SketchStroke[]): SketchDocument | null {
  const sketches = new Map<string, SketchDefinition>();
  let activeSketchId: string | null = null;

  for (const stroke of strokes) {
    if (!stroke.closed) continue;

    const sketchId = `sketch-${stroke.view}`;
    if (!activeSketchId) activeSketchId = sketchId;

    let sketch = sketches.get(sketchId);
    if (!sketch) {
      sketch = {
        sketchId,
        view: stroke.view,
        primitives: [],
        constraints: [],
      };
      sketches.set(sketchId, sketch);
    }

    sketch.primitives?.push(strokeToPrimitive(stroke));
    sketch.constraints?.push(...constraintsForStroke(stroke));
  }

  if (sketches.size === 0) return null;

  return {
    documentId: WORKSPACE_SKETCH_DOCUMENT_ID,
    sketches: [...sketches.values()],
    activeSketchId,
    units: 'mm',
    metadata: { source: 'workspace' },
  };
}

function strokeToPrimitive(stroke: SketchStroke): SketchPrimitive {
  return {
    primitiveId: stroke.primitiveId,
    kind: 'polyline',
    points: stroke.points.map(copyPoint),
    closed: true,
  };
}

function copyPoint(point: SketchPoint): SketchPoint {
  return [point[0], point[1]];
}
