import { writable } from 'svelte/store';

// Session Context
export const history = writable([]);
export const activeThreadId = writable(null);
export const activeVersionId = writable(null);

// Config & Models
export const config = writable({ engines: [], selected_engine_id: '' });
export const availableModels = writable([]);
export const isLoadingModels = writable(false);
