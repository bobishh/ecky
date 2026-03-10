import { derived, writable } from 'svelte/store';

import {
  normalizeDesignOutput,
  normalizeDesignParams,
  normalizeUiSpec,
  type DesignOutput,
  type DesignParams,
  type UiSpec,
} from '../types/domain';

export interface WorkingCopyState {
  title: string;
  versionName: string;
  macroCode: string;
  uiSpec: UiSpec;
  params: DesignParams;
  dirty: boolean;
  sourceVersionId: string | null;
}

type WorkingCopyPatch = Partial<Omit<WorkingCopyState, 'dirty'>> & {
  dirty?: boolean;
};

function createInitialState(): WorkingCopyState {
  return {
    title: '',
    versionName: '',
    macroCode: '',
    uiSpec: { fields: [] },
    params: {},
    dirty: false,
    sourceVersionId: null,
  };
}

function createWorkingCopyStore() {
  const initialState = createInitialState();
  const { subscribe, set, update } = writable<WorkingCopyState>(initialState);

  return {
    subscribe,

    loadVersion(version: DesignOutput, messageId: string | null) {
      const normalized = normalizeDesignOutput(version);
      set({
        title: normalized.title,
        versionName: normalized.versionName,
        macroCode: normalized.macroCode,
        uiSpec: normalizeUiSpec(normalized.uiSpec),
        params: normalizeDesignParams(normalized.initialParams),
        dirty: false,
        sourceVersionId: messageId,
      });
    },

    patch(changes: WorkingCopyPatch) {
      update((state) => ({
        ...state,
        ...changes,
        uiSpec: changes.uiSpec ? normalizeUiSpec(changes.uiSpec) : state.uiSpec,
        params: changes.params ? normalizeDesignParams(changes.params) : state.params,
        dirty: changes.dirty ?? true,
      }));
    },

    updateParams(newParams: DesignParams) {
      update((state) => ({
        ...state,
        params: { ...state.params, ...normalizeDesignParams(newParams) },
        dirty: true,
      }));
    },

    reset() {
      set(createInitialState());
    },
  };
}

export const workingCopy = createWorkingCopyStore();
export const isDirty = derived(workingCopy, ($workingCopy) => $workingCopy.dirty);
