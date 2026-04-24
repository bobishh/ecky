import type { SketchDocument, SketchPrimitive } from './tauri/contracts';

export type SketchDocumentConstraintValidationResult =
  | { passed: true; evidence: string[] }
  | { passed: false; issues: string[] };

export type SketchDocumentConstraintRepairResult =
  | { document: SketchDocument; evidence: string[] }
  | { error: string };

const DIMENSION_TOLERANCE = 1e-6;

export function validateSketchDocumentConstraints(document: SketchDocument): SketchDocumentConstraintValidationResult {
  const issues: string[] = [];
  const evidence: string[] = [];

  for (const sketch of document.sketches ?? []) {
    const primitivesById = new Map((sketch.primitives ?? []).map((primitive) => [primitive.primitiveId, primitive]));

    for (const constraint of sketch.constraints ?? []) {
      if (constraint.kind !== 'dimension') continue;

      const expectedValue = constraint.value;
      if (typeof expectedValue !== 'number' || !Number.isFinite(expectedValue)) {
        issues.push(`sketch '${sketch.sketchId}' dimension constraint '${constraint.constraintId}' has missing or non-finite value.`);
        continue;
      }

      const dimension = constraintDimension(constraint.constraintId);
      if (!dimension) {
        issues.push(`sketch '${sketch.sketchId}' dimension constraint '${constraint.constraintId}' is neither width nor height.`);
        continue;
      }

      for (const targetId of constraint.targetIds ?? []) {
        const primitive = primitivesById.get(targetId);
        if (!primitive) {
          issues.push(`sketch '${sketch.sketchId}' dimension constraint '${constraint.constraintId}' targets missing primitive '${targetId}'.`);
          continue;
        }

        const measured = measurePrimitiveDimension(primitive, dimension);
        if (measured === null) {
          issues.push(`sketch '${sketch.sketchId}' primitive '${primitive.primitiveId}' has invalid or no points.`);
          continue;
        }

        if (Math.abs(expectedValue - measured) > DIMENSION_TOLERANCE) {
          issues.push(
            `sketch '${sketch.sketchId}' primitive '${primitive.primitiveId}' ${dimension} dimension expected ${formatMm(expectedValue)} but measured ${formatMm(measured)}.`,
          );
          continue;
        }

        evidence.push(
          `sketch '${sketch.sketchId}' primitive '${primitive.primitiveId}' ${dimension} dimension matched ${formatMm(measured)}.`,
        );
      }
    }
  }

  if (issues.length > 0) return { passed: false, issues };
  if (evidence.length === 0) return { passed: true, evidence: ['No dimension constraints.'] };
  return { passed: true, evidence };
}

export function repairSketchDocumentDimensionConstraints(document: SketchDocument): SketchDocumentConstraintRepairResult {
  const repairedDocument = cloneSketchDocument(document);
  const issues: string[] = [];
  const evidence: string[] = [];

  for (const sketch of repairedDocument.sketches ?? []) {
    const primitivesById = new Map((sketch.primitives ?? []).map((primitive) => [primitive.primitiveId, primitive]));

    for (const constraint of sketch.constraints ?? []) {
      if (constraint.kind !== 'dimension') continue;

      const currentValue = constraint.value;
      if (typeof currentValue !== 'number' || !Number.isFinite(currentValue)) {
        issues.push(`sketch '${sketch.sketchId}' dimension constraint '${constraint.constraintId}' has missing or non-finite value.`);
        continue;
      }

      const dimension = constraintDimension(constraint.constraintId);
      if (!dimension) {
        issues.push(`sketch '${sketch.sketchId}' dimension constraint '${constraint.constraintId}' is neither width nor height.`);
        continue;
      }

      const targetIds = constraint.targetIds ?? [];
      if (targetIds.length === 0) {
        issues.push(`sketch '${sketch.sketchId}' dimension constraint '${constraint.constraintId}' has no targets.`);
        continue;
      }

      for (const targetId of targetIds) {
        const primitive = primitivesById.get(targetId);
        if (!primitive) {
          issues.push(`sketch '${sketch.sketchId}' dimension constraint '${constraint.constraintId}' targets missing primitive '${targetId}'.`);
          continue;
        }

        const measured = measurePrimitiveDimension(primitive, dimension);
        if (measured === null) {
          issues.push(`sketch '${sketch.sketchId}' primitive '${primitive.primitiveId}' has invalid or no points.`);
          continue;
        }

        if (Math.abs(currentValue - measured) <= DIMENSION_TOLERANCE) continue;

        const repairedValue = formatConstraintValue(measured);
        constraint.value = repairedValue;
        evidence.push(
          `sketch '${sketch.sketchId}' primitive '${primitive.primitiveId}' ${dimension} dimension repaired ${formatMm(currentValue)} -> ${formatMm(repairedValue)}.`,
        );
      }
    }
  }

  if (issues.length > 0) return { error: issues.join(' ') };
  if (evidence.length === 0) return { error: 'No repairable dimension constraint mismatch.' };
  return { document: repairedDocument, evidence };
}

function constraintDimension(constraintId: string): 'width' | 'height' | null {
  if (constraintId.includes('width')) return 'width';
  if (constraintId.includes('height')) return 'height';
  return null;
}

function measurePrimitiveDimension(primitive: SketchPrimitive, dimension: 'width' | 'height'): number | null {
  const points = primitivePoints(primitive);
  if (!points) return null;

  const values = points.map(([x, y]) => (dimension === 'width' ? x : y));
  return Math.max(...values) - Math.min(...values);
}

function primitivePoints(primitive: SketchPrimitive): [number, number][] | null {
  const points = primitive.points ?? [];
  const logicalPoints = hasClosedDuplicate(points) ? points.slice(0, -1) : points;
  if (logicalPoints.length === 0) return null;
  if (logicalPoints.some(([x, y]) => !Number.isFinite(x) || !Number.isFinite(y))) return null;
  return logicalPoints;
}

function hasClosedDuplicate(points: [number, number][]): boolean {
  if (points.length < 2) return false;
  const first = points[0];
  const last = points[points.length - 1];
  return first[0] === last[0] && first[1] === last[1];
}

function formatMm(value: number): string {
  return `${Number(value.toFixed(6))}mm`;
}

function formatConstraintValue(value: number): number {
  return Number(value.toFixed(4));
}

function cloneSketchDocument(document: SketchDocument): SketchDocument {
  return JSON.parse(JSON.stringify(document)) as SketchDocument;
}
