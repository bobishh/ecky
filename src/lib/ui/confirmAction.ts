import { confirm as tauriConfirm } from '@tauri-apps/plugin-dialog';

type TauriWindow = Window & {
  __TAURI_INTERNALS__?: unknown;
};

function hasTauriRuntime(): boolean {
  return typeof window !== 'undefined' && typeof (window as TauriWindow).__TAURI_INTERNALS__ === 'object';
}

export async function confirmAction(message: string, title = 'Ecky CAD'): Promise<boolean> {
  if (typeof window === 'undefined') return true;
  if (hasTauriRuntime()) {
    const confirmed = await tauriConfirm(message, {
      title,
      kind: 'warning',
      okLabel: 'OK',
      cancelLabel: 'Cancel',
    });
    return confirmed === true;
  }
  return window.confirm(message) === true;
}
