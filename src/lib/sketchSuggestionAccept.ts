import type {
  SketchDefinition,
  SketchDraftRequest,
  SketchFeatureSuggestion,
  SketchPrimitive,
} from './tauri/contracts';

export type SketchSuggestionAcceptResult = SketchDraftRequest | { error: string };

export function buildDraftRequestFromSuggestion(
  document: { sketches?: SketchDefinition[] },
  suggestion: SketchFeatureSuggestion,
): SketchSuggestionAcceptResult {
  const sketch = document.sketches?.find((candidate) => candidate.sketchId === suggestion.sketchId);
  if (!sketch) {
    return {
      error: `suggestion '${suggestion.suggestionId}' references missing sketch '${suggestion.sketchId}'.`,
    };
  }

  const primitiveResult = selectedPrimitives(sketch, suggestion);
  if ('error' in primitiveResult) return primitiveResult;

  return {
    partId: suggestion.partId,
    sketch: copySketch(sketch, primitiveResult.primitives),
    operation: suggestion.operation,
    amount: suggestion.amount,
    symmetric: suggestion.symmetric,
  };
}

function selectedPrimitives(
  sketch: SketchDefinition,
  suggestion: SketchFeatureSuggestion,
): { primitives: SketchPrimitive[] } | { error: string } {
  if (suggestion.primitiveId == null) {
    return { primitives: (sketch.primitives ?? []).map(copyPrimitive) };
  }

  const primitive = sketch.primitives?.find((candidate) => candidate.primitiveId === suggestion.primitiveId);
  if (!primitive) {
    return {
      error: `suggestion '${suggestion.suggestionId}' references missing primitive '${suggestion.primitiveId}'.`,
    };
  }

  return { primitives: [copyPrimitive(primitive)] };
}

function copyPrimitive(primitive: SketchPrimitive): SketchPrimitive {
  const copy: SketchPrimitive = {
    ...primitive,
  };

  if (primitive.points) copy.points = primitive.points.map((point) => [point[0], point[1]]);

  return copy;
}

function copySketch(sketch: SketchDefinition, primitives: SketchPrimitive[]): SketchDefinition {
  const copy: SketchDefinition = {
    ...sketch,
    primitives,
  };

  const primitiveIds = new Set(primitives.map((primitive) => primitive.primitiveId));
  if (sketch.constraints) {
    copy.constraints = sketch.constraints
      .filter((constraint) => !constraint.targetIds?.length || constraint.targetIds.every((targetId) => primitiveIds.has(targetId)))
      .map((constraint) => ({
        ...constraint,
        targetIds: constraint.targetIds ? [...constraint.targetIds] : constraint.targetIds,
      }));
  }

  return copy;
}
