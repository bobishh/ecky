const VERIFY_TEMPLATE = [
  '  (verify',
  '    (tag body_shell)',
  '    (metric check (manifest has-step))',
  '    (expect check (= true)))',
].join('\n');

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
  return `${before}\n${VERIFY_TEMPLATE}\n${after}\n`;
}

