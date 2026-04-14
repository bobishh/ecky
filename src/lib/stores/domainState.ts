import { writable } from 'svelte/store';
import type { AppConfig, RuntimeCapabilities, Thread } from '../types/domain';

// Session Context
export const historyStore = writable<Thread[]>([]);
export const activeThreadIdStore = writable<string | null>(null);
export const activeVersionId = writable<string | null>(null);

// Config & Models
export const config = writable<AppConfig>({
  engines: [],
  selectedEngineId: '',
  freecadCmd: '',
  assets: [],
  microwave: {
    humId: null,
    dingId: null,
    muted: false,
  },
  mcp: {
    port: null,
    maxSessions: null,
    mode: 'passive',
    primaryAgentId: null,
    promptTimeoutSecs: 1800,
    autoAgents: [],
  },
  hasSeenOnboarding: false,
  connectionType: null,
  defaultEngineKind: 'freecad',
  defaultSourceLanguage: 'legacyPython',
  defaultGeometryBackend: 'freecad',
  maxGenerationAttempts: 3,
  maxVerifyAttempts: 0,
});
export const availableModels = writable<string[]>([]);
export const isLoadingModels = writable<boolean>(false);
export const runtimeCapabilities = writable<RuntimeCapabilities | null>(null);
