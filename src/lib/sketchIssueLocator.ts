import type {
  SketchDefinition,
  SketchDocument,
  SketchPrimitive,
  SketchPrimitiveTopology,
  SketchValidationIssue,
} from './tauri/contracts';

export type SketchIssueMatch = {
  sketch: SketchDefinition;
  primitive: SketchPrimitive | null;
};

export function findSketchIssueMatch(document: SketchDocument, issue: SketchValidationIssue): SketchIssueMatch | null {
  const sketches = candidateSketches(document, issue);
  for (const sketch of sketches) {
    const primitive = findPrimitiveForIssue(sketch, issue);
    if (primitive) return { sketch, primitive };
  }
  return null;
}

export function primitiveMatchesIssueTopology(
  primitive: SketchPrimitive,
  topology: SketchPrimitiveTopology | null | undefined,
): boolean {
  if (!topology) return false;
  const primitiveTopology = primitive.topology;
  if (!primitiveTopology) return false;

  if (topology.loopId && primitiveTopology.loopId === topology.loopId) {
    return true;
  }

  if (topology.edgeIds?.length) {
    const left = normalizedEdgeIds(primitiveTopology.edgeIds);
    const right = normalizedEdgeIds(topology.edgeIds);
    if (left.length > 0 && left.length === right.length && left.every((edgeId, index) => edgeId === right[index])) {
      return true;
    }
  }

  return false;
}

export function primitiveMatchesIssueEdgeId(
  primitive: SketchPrimitive,
  edgeId: string | null | undefined,
): boolean {
  const expected = edgeId?.trim();
  if (!expected) return false;
  return (primitive.topology?.edgeIds ?? []).some((candidate) => candidate.trim() === expected);
}

function candidateSketches(document: SketchDocument, issue: SketchValidationIssue): SketchDefinition[] {
  const sketches = document.sketches ?? [];
  const directSketch = findDirectIssueSketch(sketches, issue);
  if (directSketch) {
    return [directSketch];
  }

  const topologyMatches = sketches.filter((sketch) => (sketch.primitives ?? []).some((primitive) => primitiveMatchesIssueTopology(primitive, issue.topology)));
  if (topologyMatches.length > 0) return topologyMatches;

  const edgeMatches = sketches.filter((sketch) => (sketch.primitives ?? []).some((primitive) => primitiveMatchesIssueEdgeId(primitive, issue.edgeId)));
  if (edgeMatches.length > 0) return edgeMatches;

  return [];
}

function findPrimitiveForIssue(sketch: SketchDefinition, issue: SketchValidationIssue): SketchPrimitive | null {
  const primitives = sketch.primitives ?? [];

  const directPrimitive = findDirectIssuePrimitive(sketch, issue);
  if (directPrimitive) {
    return directPrimitive;
  }

  const topologyCandidates = primitives.filter((primitive) => primitiveMatchesIssueTopology(primitive, issue.topology));
  if (topologyCandidates.length > 0) return topologyCandidates[0];

  const edgeCandidates = primitives.filter((primitive) => primitiveMatchesIssueEdgeId(primitive, issue.edgeId));
  if (edgeCandidates.length === 1) return edgeCandidates[0];

  if (issue.primitiveId) {
    const byId = primitives.find((primitive) => primitive.primitiveId === issue.primitiveId);
    if (byId) return byId;
  }

  const role = issue.topology?.loopRole;
  if (role) {
    const roleCandidates = primitives.filter((primitive) => primitive.topology?.loopRole === role);
    if (roleCandidates.length === 1) return roleCandidates[0];
  }

  return null;
}

function normalizedEdgeIds(edgeIds: string[] | undefined): string[] {
  return [...(edgeIds ?? [])].map((edgeId) => edgeId.trim()).filter(Boolean).sort();
}

function findDirectIssueSketch(sketches: SketchDefinition[], issue: SketchValidationIssue): SketchDefinition | null {
  if (!issue.sketchId || !issue.primitiveId) return null;
  const directSketch = sketches.find((sketch) => sketch.sketchId === issue.sketchId);
  if (!directSketch) return null;
  return findDirectIssuePrimitive(directSketch, issue) ? directSketch : null;
}

function findDirectIssuePrimitive(sketch: SketchDefinition, issue: SketchValidationIssue): SketchPrimitive | null {
  if (issue.sketchId !== sketch.sketchId || !issue.primitiveId) return null;
  const directPrimitive = (sketch.primitives ?? []).find((primitive) => primitive.primitiveId === issue.primitiveId);
  if (!directPrimitive) return null;
  if (issue.topology) {
    return primitiveMatchesIssueTopology(directPrimitive, issue.topology) ? directPrimitive : null;
  }
  if (issue.edgeId) {
    return primitiveMatchesIssueEdgeId(directPrimitive, issue.edgeId) ? directPrimitive : null;
  }
  return directPrimitive;
}
