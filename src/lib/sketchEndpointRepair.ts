import type { SketchDocument, SketchPrimitive } from './tauri/contracts';

export type SketchEndpointRepairResult = {
  document: SketchDocument;
  evidence: SketchEndpointRepairEvidence[];
};

export type SketchEndpointRepairEvidence = {
  primitiveId: string;
  detail: string;
};

const DEFAULT_ENDPOINT_TOLERANCE_MM = 1;

export function repairSketchDocumentEndpointGaps(
  document: SketchDocument,
  toleranceMm: number = DEFAULT_ENDPOINT_TOLERANCE_MM,
): SketchEndpointRepairResult {
  const repairedDocument = cloneSketchDocument(document);
  const evidence: SketchEndpointRepairEvidence[] = [];

  if (!Number.isFinite(toleranceMm) || toleranceMm <= 0) {
    return { document: repairedDocument, evidence };
  }

  for (const sketch of repairedDocument.sketches ?? []) {
    for (const primitive of sketch.primitives ?? []) {
      const repair = repairPrimitiveEndpointGap(primitive, toleranceMm);
      if (!repair) continue;
      evidence.push({
        primitiveId: primitive.primitiveId,
        detail: `sketch '${sketch.sketchId}' primitive '${primitive.primitiveId}' closed endpoint gap ${formatMm(repair.distance)}.`,
      });
    }
  }

  return { document: repairedDocument, evidence };
}

function repairPrimitiveEndpointGap(
  primitive: SketchPrimitive,
  toleranceMm: number,
): { distance: number } | null {
  if (primitive.kind !== 'polyline') return null;
  const points = primitive.points;
  if (!points || points.length < 3) return null;
  if (points.some((point) => !isValidPoint(point))) return null;

  const first = points[0];
  const last = points[points.length - 1];
  const distance = Math.hypot(first[0] - last[0], first[1] - last[1]);
  if (distance > toleranceMm) return null;
  if (distance === 0 && primitive.closed === true) return null;

  primitive.closed = true;
  points[points.length - 1] = [first[0], first[1]];
  return { distance };
}

function isValidPoint(point: unknown): point is [number, number] {
  return Array.isArray(point) && point.length === 2 && Number.isFinite(point[0]) && Number.isFinite(point[1]);
}

function formatMm(value: number): string {
  return `${Number(value.toFixed(4))}mm`;
}

function cloneSketchDocument(document: SketchDocument): SketchDocument {
  return JSON.parse(JSON.stringify(document)) as SketchDocument;
}
