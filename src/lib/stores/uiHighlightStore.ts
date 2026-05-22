import { writable } from 'svelte/store';

export type UiHighlight = {
  target: string;
  action: 'highlightParam' | 'focusMacroNode';
  timestamp: number;
};

export const uiHighlightStore = writable<UiHighlight | null>(null);

export function triggerHighlight(target: string, action: 'highlightParam' = 'highlightParam') {
  uiHighlightStore.set({ target, action, timestamp: Date.now() });
}

export function triggerMacroNodeFocus(target: string) {
  uiHighlightStore.set({ target, action: 'focusMacroNode', timestamp: Date.now() });
}
