import { get, writable } from 'svelte/store';
import {
  normalizeDesignParams,
  normalizeUiSpec,
  type DesignOutput,
  type DesignParams,
  type UiSpec,
} from '../types/domain';

type ParamPanelStateSnapshot = {
  versionId: string | null;
  uiSpec: UiSpec;
  params: DesignParams;
};

function emptyUiSpec(): UiSpec {
  return { fields: [] };
}

const initialState: ParamPanelStateSnapshot = {
  versionId: null,
  uiSpec: emptyUiSpec(),
  params: {}
};

function createParamPanelState() {
  const { subscribe, set, update } = writable<ParamPanelStateSnapshot>(initialState);

  return {
    subscribe,

    reset() {
      set(initialState);
    },

    hydrate(payload: {
      versionId?: string | null;
      uiSpec?: UiSpec;
      params?: DesignParams;
    }) {
      set({
        versionId: payload.versionId ?? null,
        uiSpec: normalizeUiSpec(payload.uiSpec),
        params: normalizeDesignParams(payload.params)
      });
    },

    hydrateFromVersion(design: DesignOutput | null | undefined, versionId: string | null) {
      set({
        versionId: versionId ?? null,
        uiSpec: normalizeUiSpec(design?.uiSpec),
        params: normalizeDesignParams(design?.initialParams)
      });
    },

    setVersionId(versionId: string | null) {
      update(s => ({ ...s, versionId }));
    },

    setUiSpec(uiSpec: UiSpec) {
      update(s => ({ ...s, uiSpec: normalizeUiSpec(uiSpec) }));
    },

    setParams(params: DesignParams) {
      update(s => ({ ...s, params: normalizeDesignParams(params) }));
    },

    patchParams(partialParams: DesignParams) {
      update(s => ({
        ...s,
        params: {
          ...s.params,
          ...normalizeDesignParams(partialParams)
        }
      }));
    }
  };
}

export const paramPanelState = createParamPanelState();
export const liveApply = writable(false);

function getParamPanelSnapshot() {
  return get(paramPanelState);
}
