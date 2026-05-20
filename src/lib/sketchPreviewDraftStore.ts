export type SketchPreviewDraftScope = {
  scopeId: string | null;
};

export function createSketchPreviewDraftScopeId(): string {
  return crypto.randomUUID();
}

export function normalizeSketchPreviewDraftScopeId(scopeId: string | null | undefined): string | null {
  const normalized = scopeId?.trim() ?? '';
  return normalized.length > 0 ? normalized : null;
}

export function resolveSketchPreviewDraftScopeId(input: {
  scopeId?: string | null;
  draftScopeId?: string | null;
}): string | null {
  return normalizeSketchPreviewDraftScopeId(input.scopeId ?? input.draftScopeId ?? null);
}
