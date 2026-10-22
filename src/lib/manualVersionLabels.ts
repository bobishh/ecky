export function manualVersionLabel(
  requested: string | null | undefined,
  now = new Date(),
): string {
  const trimmed = requested?.trim();
  if (trimmed && !/^(?:v[\s-]*manual|manual edit)$/i.test(trimmed)) return trimmed;

  const timestamp = now.toISOString().slice(0, 19).replace('T', ' ');
  return `Manual edit ${timestamp} UTC`;
}

export function manualModelTitle(
  workingCopyTitle: string | null | undefined,
  threadTitle: string | null | undefined,
  versionTitle: string | null | undefined,
): string {
  return [workingCopyTitle, threadTitle, versionTitle]
    .map((value) => value?.trim())
    .find((value): value is string => value !== undefined && value.length > 0 && !/^manual edit$/i.test(value)) ?? 'Untitled model';
}
