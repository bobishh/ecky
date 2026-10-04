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
  cadTextFontPath: '',
  projectsRoot: '',
  freecadLibraryRoots: [],
  assets: [],
  microwave: {
    humId: null,
    dingId: null,
    muted: false,
  },
  voice: {
    sttLanguageCode: 'en-US',
  },
  mcp: {
    port: null,
    maxSessions: null,
    mode: 'passive',
    primaryAgentId: null,
    promptTimeoutSecs: 1800,
    eckyAstAuthoring: false,
    autoAgents: [],
  },
  femCompute: {
    quality: 'balanced',
    maximumWallTimeMinutes: 30,
    maximumMemoryMiB: 8192,
    threadCount: 0,
  },
  hasSeenOnboarding: false,
  connectionType: null,
  providerModels: { codex: '', agy: '' },
  jevClassifier: { enabled: false, apiKey: '' },
  defaultEngineKind: 'ecky',
  defaultSourceLanguage: 'ecky',
  defaultGeometryBackend: 'mesh',
  maxGenerationAttempts: 3,
  maxVerifyAttempts: 2,
});
// The app must not infer first-run state from the in-memory defaults while the
// canonical config is still loading (notably during a Vite/HMR remount).
export const configLoaded = writable(false);
export const availableModels = writable<string[]>([]);
export const isLoadingModels = writable<boolean>(false);
export const runtimeCapabilities = writable<RuntimeCapabilities | null>(null);
