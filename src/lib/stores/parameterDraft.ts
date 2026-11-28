import { writable } from 'svelte/store';
import type { DesignParams } from '../types/domain';

export type ParameterDraftState = {
  versionKey: string | null;
  baseParams: DesignParams;
  draftParams: DesignParams | null;
};

export function paramsEqual(left: DesignParams, right: DesignParams): boolean {
  const leftKeys = Object.keys(left).sort();
  const rightKeys = Object.keys(right).sort();
  if (leftKeys.length !== rightKeys.length) return false;
  for (let i = 0; i < leftKeys.length; i += 1) {
    const key = leftKeys[i];
    if (key !== rightKeys[i]) return false;
    if (left[key] !== right[key]) return false;
  }
  return true;
}

export function effectiveDraftParams(state: ParameterDraftState): DesignParams {
  return state.draftParams ?? state.baseParams;
}

export function hasActiveParameterDraft(state: ParameterDraftState): boolean {
  return state.draftParams !== null && !paramsEqual(state.draftParams, state.baseParams);
}

export function createParameterDraftStore() {
  const store = writable<ParameterDraftState>({
    versionKey: null,
    baseParams: {},
    draftParams: null,
  });

  return {
    subscribe: store.subscribe,

    syncBase(versionKey: string | null, baseParams: DesignParams) {
      const normalizedBase = { ...baseParams };
      store.update((current) => {
        if (current.versionKey !== versionKey) {
          return {
            versionKey,
            baseParams: normalizedBase,
            draftParams: null,
          };
        }
        if (paramsEqual(current.baseParams, normalizedBase)) {
          return current;
        }
        const nextDraft =
          current.draftParams && !paramsEqual(current.draftParams, normalizedBase)
            ? current.draftParams
            : null;
        return {
          versionKey,
          baseParams: normalizedBase,
          draftParams: nextDraft,
        };
      });
    },

    setDraft(nextParams: DesignParams | null) {
      store.update((current) => {
        if (!nextParams) {
          return current.draftParams === null ? current : { ...current, draftParams: null };
        }
        const candidate = { ...nextParams };
        if (paramsEqual(candidate, current.baseParams)) {
          return current.draftParams === null ? current : { ...current, draftParams: null };
        }
        if (current.draftParams && paramsEqual(current.draftParams, candidate)) {
          return current;
        }
        return {
          ...current,
          draftParams: candidate,
        };
      });
    },

    patchDraft(patch: DesignParams) {
      if (Object.keys(patch).length === 0) return;
      store.update((current) => {
        const merged = { ...(current.draftParams ?? current.baseParams), ...patch };
        if (paramsEqual(merged, current.baseParams)) {
          return current.draftParams === null ? current : { ...current, draftParams: null };
        }
        if (current.draftParams && paramsEqual(current.draftParams, merged)) {
          return current;
        }
        return {
          ...current,
          draftParams: merged,
        };
      });
    },

    clearDraft() {
      store.update((current) =>
        current.draftParams === null ? current : { ...current, draftParams: null },
      );
    },

    reset() {
      store.set({ versionKey: null, baseParams: {}, draftParams: null });
    },
  };
}

export const parameterDraft = createParameterDraftStore();
