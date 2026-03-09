import { get, writable } from 'svelte/store';
import type { DesignOutput, DesignParams, UiSpec } from '../types/domain';

type ParamPanelStateSnapshot = {
  versionId: string | null;
  macroCode: string;
  uiSpec: UiSpec;
  params: DesignParams;
};

function emptyUiSpec(): UiSpec {
  return { fields: [] };
}

function normalizeUiSpec(uiSpec: unknown): UiSpec {
  if (!uiSpec || typeof uiSpec !== 'object') return emptyUiSpec();
  const typed = uiSpec as Partial<UiSpec>;
  const fields = Array.isArray(typed.fields) ? typed.fields : [];
  return { ...typed, fields } as UiSpec;
}

function normalizeParams(params: unknown): DesignParams {
  if (!params || typeof params !== 'object' || Array.isArray(params)) return {};
  return { ...(params as DesignParams) };
}

const initialState: ParamPanelStateSnapshot = {
  versionId: null,
  macroCode: '',
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
      macroCode?: string;
      uiSpec?: UiSpec;
      params?: DesignParams;
    }) {
      set({
        versionId: payload.versionId ?? null,
        macroCode: payload.macroCode ?? '',
        uiSpec: normalizeUiSpec(payload.uiSpec),
        params: normalizeParams(payload.params)
      });
    },

    hydrateFromVersion(design: DesignOutput | null | undefined, versionId: string | null) {
      set({
        versionId: versionId ?? null,
        macroCode: design?.macroCode ?? '',
        uiSpec: normalizeUiSpec(design?.uiSpec),
        params: normalizeParams(design?.initialParams)
      });
    },

    setVersionId(versionId: string | null) {
      update(s => ({ ...s, versionId }));
    },

    setMacroCode(macroCode: string) {
      update(s => ({ ...s, macroCode: macroCode ?? '' }));
    },

    setUiSpec(uiSpec: UiSpec) {
      update(s => ({ ...s, uiSpec: normalizeUiSpec(uiSpec) }));
    },

    setParams(params: DesignParams) {
      update(s => ({ ...s, params: normalizeParams(params) }));
    },

    patchParams(partialParams: DesignParams) {
      update(s => ({
        ...s,
        params: {
          ...s.params,
          ...normalizeParams(partialParams)
        }
      }));
    }
  };
}

export const paramPanelState = createParamPanelState();

export function getParamPanelSnapshot() {
  return get(paramPanelState);
}
