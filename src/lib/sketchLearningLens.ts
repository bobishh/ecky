export type SketchLearningLens = {
  title: string;
  operationLabel: string;
  explanation: string;
  formula: string;
  domain: string;
};

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

function formatMillimeters(value: number): string {
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(2)));
}
