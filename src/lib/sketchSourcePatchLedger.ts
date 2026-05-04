export type SketchSourcePatchAction =
  | 'CLEAN UP'
  | 'REPAIR IMPORT'
  | 'AUTO SNAP'
  | 'DERIVE BREP'
  | 'TOPOLOGY REDRAW';

export type SketchSourcePatchEntry = {
  patchId: string;
  action: SketchSourcePatchAction;
  primitiveId: string;
  detail: string;
};

export type SketchSourcePatchInput = {
  action: SketchSourcePatchAction;
  primitiveId: string;
  detail: string;
};

export function appendSketchSourcePatch(
  entries: SketchSourcePatchEntry[],
  input: SketchSourcePatchInput,
): SketchSourcePatchEntry[] {
  const sequence = entries.length + 1;
  return [
    ...entries,
    {
      patchId: `source-patch-${sequence}`,
      action: input.action,
      primitiveId: input.primitiveId,
      detail: input.detail,
    },
  ];
}

export function compactRepairDetail(detail: string): string {
  return detail.replace(/^REPAIR AVAILABLE\s*\/\s*/i, '').trim();
}
