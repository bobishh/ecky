import { derived, writable } from 'svelte/store';

import {
  type EngineKind,
  type MacroDialect,
  type SourceLanguage,
  type GeometryBackend,
  normalizeDesignOutput,
  normalizeDesignParams,
  normalizePostProcessing,
  normalizeUiSpec,
  type DesignOutput,
  type DesignParams,
  type PostProcessingSpec,
  type UiSpec,
} from '../types/domain';

export interface WorkingCopyState {
  title: string;
  versionName: string;
  macroCode: string;
  macroDialect: MacroDialect;
  engineKind: EngineKind;
  sourceLanguage: SourceLanguage;
  geometryBackend: GeometryBackend;
  uiSpec: UiSpec;
  params: DesignParams;
  postProcessing: PostProcessingSpec | null;
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
    macroDialect: 'legacy',
    engineKind: 'freecad',
    sourceLanguage: 'legacyPython',
    geometryBackend: 'freecad',
    uiSpec: { fields: [] },
    params: {},
    postProcessing: null,
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
        macroDialect: normalized.macroDialect ?? 'legacy',
        engineKind: normalized.engineKind ?? 'freecad',
        sourceLanguage: normalized.sourceLanguage ?? 'legacyPython',
        geometryBackend: normalized.geometryBackend ?? 'freecad',
        uiSpec: normalizeUiSpec(normalized.uiSpec),
        params: normalizeDesignParams(normalized.initialParams),
        postProcessing: normalized.postProcessing ?? null,
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
        postProcessing:
          changes.postProcessing !== undefined
            ? normalizePostProcessing(changes.postProcessing)
            : state.postProcessing,
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
