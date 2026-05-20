const MANIFEST_VERIFY_TEMPLATE = [
  '  (verify',
  '    (tag body_shell)',
  '    (metric check (manifest has-step))',
  '    (expect check (= true)))',
].join('\n');

function buildClearanceVerifyTemplate(leftPartId: string, rightPartId: string): string {
  const tag = `${leftPartId}_${rightPartId}_gap`;
  return [
    '  (verify',
    `    (tag ${tag})`,
    `    (metric gap (clearance min-distance ${leftPartId} ${rightPartId}))`,
    '    (expect gap (>= 3)))',
  ].join('\n');
}

function extractTopLevelPartIds(code: string): string[] {
  const matches = [...code.matchAll(/^\s*\(part\s+([A-Za-z0-9_.-]+)/gm)];
  const ids: string[] = [];
  for (const match of matches) {
    const id = match[1]?.trim();
    if (!id || ids.includes(id)) continue;
    ids.push(id);
  }
  return ids;
}

export function hasVerifyClause(code: string): boolean {
  return /\(\s*verify\b/.test(code);
}

export function looksLikeEckyModelSource(code: string): boolean {
  return code.trimStart().startsWith('(model');
}

export function canInsertVerifyTemplate(code: string): boolean {
  return looksLikeEckyModelSource(code) && !hasVerifyClause(code);
}

export function insertVerifyTemplate(code: string): string {
  if (!canInsertVerifyTemplate(code)) return code;

  const trimmed = code.trimEnd();
  const closingIndex = trimmed.lastIndexOf(')');
  if (closingIndex === -1) return code;

  const before = trimmed.slice(0, closingIndex).replace(/\s+$/, '');
  const after = trimmed.slice(closingIndex);
  const partIds = extractTopLevelPartIds(code);
  const verifyTemplate =
    partIds.length >= 2
      ? buildClearanceVerifyTemplate(partIds[0], partIds[1])
      : MANIFEST_VERIFY_TEMPLATE;
  return `${before}\n${verifyTemplate}\n${after}\n`;
}
