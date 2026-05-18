import type { DesignOutput, DesignParams } from '../types/domain';
import { normalizeDesignParams } from '../types/domain';

export function mergeDraftPreviewParams(
  previewParams: DesignParams | null | undefined,
  currentParams: DesignParams | null | undefined,
): DesignParams {
  const preview = normalizeDesignParams(previewParams);
  const current = normalizeDesignParams(currentParams);
  const merged: DesignParams = { ...preview };

  for (const key of Object.keys(preview)) {
    if (Object.prototype.hasOwnProperty.call(current, key)) {
      merged[key] = current[key];
    }
  }

  return merged;
}

export function resolveDraftPreviewDesign(input: {
  design: DesignOutput;
  previewThreadId: string;
  activeThreadId: string | null;
  currentParams: DesignParams | null | undefined;
}): DesignOutput {
  if (input.activeThreadId !== input.previewThreadId) {
    return input.design;
  }

  return {
    ...input.design,
    initialParams: mergeDraftPreviewParams(input.design.initialParams, input.currentParams),
  };
}
