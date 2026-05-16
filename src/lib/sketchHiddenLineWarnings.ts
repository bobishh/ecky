import type {
  BrepHiddenLineProjectionResponse,
  BrepHiddenLineWarning,
  SketchView,
} from './tauri/contracts';

export function brepHiddenLineWarningMessages(
  response: BrepHiddenLineProjectionResponse | null | undefined,
): string[] {
  const entries = response?.warningEntries ?? [];
  return entries.map(formatBrepHiddenLineWarning);
}

export function brepHiddenLineViewHasWarning(
  response: BrepHiddenLineProjectionResponse | null | undefined,
  view: SketchView,
): boolean {
  return (response?.warningEntries ?? []).some((warning) => warning.view === view);
}

export function formatBrepHiddenLineWarning(warning: BrepHiddenLineWarning): string {
  return `${warning.view.toUpperCase()} ${warning.message}`.trim();
}
