import { writable, derived, get } from 'svelte/store';

/**
 * @typedef {Object} WorkingCopy
 * @property {string} title
 * @property {string} versionName
 * @property {string} macroCode
 * @property {Object} uiSpec
 * @property {Object} params
 * @property {boolean} dirty
 * @property {string|null} sourceVersionId
 */

function createWorkingCopyStore() {
  const initialState = {
    title: '',
    versionName: '',
    macroCode: '',
    uiSpec: { fields: [] },
    params: {},
    dirty: false,
    sourceVersionId: null
  };

  const { subscribe, set, update } = writable(initialState);

  return {
    subscribe,
    /**
     * Updates the working copy from a persisted version.
     * Marks it as not dirty.
     */
    loadVersion: (version, messageId) => {
      set({
        title: version.title || 'Untitled Design',
        versionName: version.versionName || 'Working Copy',
        macroCode: version.macroCode || '',
        uiSpec: version.uiSpec || { fields: [] },
        params: version.initialParams || {},
        dirty: false,
        sourceVersionId: messageId
      });
    },
    /**
     * Merges partial updates into the working copy.
     * Marks it as dirty.
     */
    patch: (changes) => {
      update(state => ({
        ...state,
        ...changes,
        dirty: true
      }));
    },
    /**
     * Specifically for parameter updates.
     */
    updateParams: (newParams) => {
      update(state => ({
        ...state,
        params: { ...state.params, ...newParams },
        dirty: true
      }));
    },
    reset: () => set(initialState)
  };
}

export const workingCopy = createWorkingCopyStore();

// Helper derived stores for UI
export const isDirty = derived(workingCopy, $wc => $wc.dirty);
