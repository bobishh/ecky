const WORKSPACE_CAPTURE_STORAGE_KEY = 'ecky:thread-workspace-capture:v1';
const NEW_THREAD_SCOPE_KEY = '__new__';

export type ThreadWorkspaceCapturePrefs = Record<string, boolean>;

export function workspaceCaptureScopeKey(threadId: string | null | undefined): string {
  const normalized = `${threadId ?? ''}`.trim();
  return normalized || NEW_THREAD_SCOPE_KEY;
}

export function readWorkspaceCapturePrefs(
  storage: Pick<Storage, 'getItem'> | null | undefined = globalThis.localStorage,
): ThreadWorkspaceCapturePrefs {
  if (!storage) return {};
  try {
    const raw = storage.getItem(WORKSPACE_CAPTURE_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

export function writeWorkspaceCapturePrefs(
  prefs: ThreadWorkspaceCapturePrefs,
  storage: Pick<Storage, 'setItem'> | null | undefined = globalThis.localStorage,
): void {
  if (!storage) return;
  try {
    storage.setItem(WORKSPACE_CAPTURE_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Ignore localStorage failures in restricted contexts.
  }
}

export function isWorkspaceCaptureEnabled(
  prefs: ThreadWorkspaceCapturePrefs,
  threadId: string | null | undefined,
): boolean {
  return prefs[workspaceCaptureScopeKey(threadId)] === true;
}

export function setWorkspaceCaptureEnabled(
  prefs: ThreadWorkspaceCapturePrefs,
  threadId: string | null | undefined,
  enabled: boolean,
): ThreadWorkspaceCapturePrefs {
  const key = workspaceCaptureScopeKey(threadId);
  if (enabled) {
    return { ...prefs, [key]: true };
  }
  const next = { ...prefs };
  delete next[key];
  return next;
}
