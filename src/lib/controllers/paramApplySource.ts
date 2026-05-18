export type ParamApplySourceName = 'forced' | 'workingCopy';

export type ParamApplySourceInput = {
  forcedCode?: string | null;
  workingMacroCode?: string | null;
  panelVersionId?: string | null;
  sourceVersionId?: string | null;
  activeVersionId?: string | null;
};

export type ResolvedParamApplySource =
  | {
      ok: true;
      source: ParamApplySourceName;
      code: string;
      targetVersionId: string | null;
    }
  | {
      ok: false;
      reason: 'stale-panel-source-version-mismatch';
      panelVersionId: string;
      sourceVersionId: string;
    }
  | {
      ok: false;
      reason: 'missing-macro-code';
      targetVersionId: string | null;
    };

function presentId(value: string | null | undefined): string | null {
  return value || null;
}

function presentCode(value: string | null | undefined): string | null {
  return value ? value : null;
}

export function resolveParamApplySource(
  input: ParamApplySourceInput,
): ResolvedParamApplySource {
  const panelVersionId = presentId(input.panelVersionId);
  const sourceVersionId = presentId(input.sourceVersionId) || presentId(input.activeVersionId);
  const targetVersionId = sourceVersionId || panelVersionId;

  const forcedCode = presentCode(input.forcedCode);
  if (forcedCode) {
    return {
      ok: true,
      source: 'forced',
      code: forcedCode,
      targetVersionId,
    };
  }

  if (panelVersionId && sourceVersionId && panelVersionId !== sourceVersionId) {
    return {
      ok: false,
      reason: 'stale-panel-source-version-mismatch',
      panelVersionId,
      sourceVersionId,
    };
  }

  const workingMacroCode = presentCode(input.workingMacroCode);
  if (workingMacroCode) {
    return {
      ok: true,
      source: 'workingCopy',
      code: workingMacroCode,
      targetVersionId,
    };
  }

  return {
    ok: false,
    reason: 'missing-macro-code',
    targetVersionId,
  };
}
