export type SketchLearningLens = {
  title: string;
  operationLabel: string;
  explanation: string;
  formula: string;
  domain: string;
};

export type SketchLearningLensSourcePatch = {
  action: string;
  primitiveId: string;
  detail: string;
};

export function buildSketchLearningLens(
  amountMm: number,
  sourcePatchEntries: SketchLearningLensSourcePatch[] = [],
): SketchLearningLens {
  const topologyRedraw = [...sourcePatchEntries]
    .reverse()
    .find((entry) => entry.action === 'TOPOLOGY REDRAW');

  if (topologyRedraw) {
    return brepTopologyRedrawLearningLens(topologyRedraw);
  }

  const brepRepair = [...sourcePatchEntries]
    .reverse()
    .find((entry) => entry.action === 'AUTO SNAP' && entry.detail.toLowerCase().match(/brep auto (snap|contain)/));

  if (brepRepair) {
      return brepAutoRepairLearningLens(brepRepair);
  }

  return extrudeLearningLens(amountMm);
}

export function extrudeLearningLens(amountMm: number): SketchLearningLens {
  const amount = formatMillimeters(amountMm);

  return {
    title: 'LEARNING LENS / MATH LENS',
    operationLabel: `EXTRUDE ${amount}MM`,
    explanation: `A closed 2D profile becomes a solid by copying every profile point through ${amount}mm of depth.`,
    formula: '(x, y) -> (x, y, z)',
    domain: `0 <= z <= ${amount}`,
  };
}

function brepAutoRepairLearningLens(entry: SketchLearningLensSourcePatch): SketchLearningLens {
  const isContainment = entry.detail.toLowerCase().includes('brep auto contain');
  return {
    title: 'LEARNING LENS / MATH LENS',
    operationLabel: isContainment ? 'BREP AUTO CONTAIN' : 'BREP AUTO SNAP',
    explanation: isContainment
      ? `Accepted CAD repair expanded ${entry.primitiveId} source bounds to contain exact BRep hidden-line bounds, then reran exact validation.`
      : `Accepted CAD repair mapped ${entry.primitiveId} source bounds to exact BRep hidden-line bounds, then reran exact validation.`,
    formula: `x' = minBrepX + (x - minSketchX) * brepWidth / sketchWidth`,
    domain: entry.detail,
  };
}

function brepTopologyRedrawLearningLens(entry: SketchLearningLensSourcePatch): SketchLearningLens {
  return {
    title: 'LEARNING LENS / MATH LENS',
    operationLabel: 'BREP TOPOLOGY REDRAW',
    explanation: `Accepted CAD repair replaced ${entry.primitiveId} with a projection-derived loop from exact BRep hidden-line evidence. This is not original authoring history; it is an editable redraw seed.`,
    formula: 'BRep HLR loop -> Sketch polyline',
    domain: entry.detail,
  };
}

function formatMillimeters(value: number): string {
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(2)));
}
