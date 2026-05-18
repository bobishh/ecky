import { get, writable, derived } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';
import { getThreadWindowLayout, saveThreadWindowLayout } from '../tauri/client';
import { triggerHighlight } from './uiHighlightStore';
import type { ThreadWindowLayout, ThreadWindowState } from '../tauri/contracts';
import { fitRectToViewport } from '../windowGeometry';

export type WindowId =
  | 'projects'
  | 'params'
  | 'dialogue'
  | 'docs'
  | 'settings'
  | 'terminal'
  | 'sketch'
  | 'activity';

export type WindowRegistryEntry = {
  title: string;
  defaultRect: { x: number; y: number; width: number; height: number };
  minSize: { width: number; height: number };
  mountPolicy: 'lazy' | 'keepAlive';
};

export const windowRegistry: Record<WindowId, WindowRegistryEntry> = {
  projects: {
    title: 'Projects',
    defaultRect: { x: 80, y: 80, width: 420, height: 500 },
    minSize: { width: 320, height: 300 },
    mountPolicy: 'lazy',
  },
  params: {
    title: 'Parameters',
    defaultRect: { x: 520, y: 80, width: 360, height: 480 },
    minSize: { width: 280, height: 250 },
    mountPolicy: 'lazy',
  },
  dialogue: {
    title: 'Dialogue',
    defaultRect: { x: 320, y: 560, width: 980, height: 260 },
    minSize: { width: 350, height: 260 },
    mountPolicy: 'keepAlive',
  },
  docs: {
    title: 'Ecky IR Docs',
    defaultRect: { x: 160, y: 90, width: 1120, height: 760 },
    minSize: { width: 760, height: 480 },
    mountPolicy: 'keepAlive',
  },
  settings: {
    title: 'Settings',
    defaultRect: { x: 160, y: 100, width: 600, height: 500 },
    minSize: { width: 400, height: 350 },
    mountPolicy: 'lazy',
  },
  terminal: {
    title: 'Agent Terminal',
    defaultRect: { x: 100, y: 200, width: 800, height: 600 },
    minSize: { width: 400, height: 300 },
    mountPolicy: 'keepAlive',
  },
  sketch: {
    title: 'Sketch Workspace',
    defaultRect: { x: 180, y: 120, width: 760, height: 520 },
    minSize: { width: 520, height: 360 },
    mountPolicy: 'lazy',
  },
  activity: {
    title: 'Session Activity',
    defaultRect: { x: 220, y: 140, width: 760, height: 560 },
    minSize: { width: 440, height: 320 },
    mountPolicy: 'keepAlive',
  },
};

export type WindowState = {
  visible: boolean;
  minimized: boolean;
  x: number;
  y: number;
  width: number;
  height: number;
  z: number;
};

type WindowStoreState = Record<WindowId, WindowState>;
type ThreadWindowCacheEntry = {
  state: WindowStoreState;
  revision: number;
  dirtyRevision: number | null;
  rememberLayout: boolean;
};

const ALL_WINDOW_IDS: WindowId[] = [
  'projects',
  'params',
  'dialogue',
  'docs',
  'settings',
  'terminal',
  'sketch',
  'activity',
];

function buildDefaults(): WindowStoreState {
  const state = {} as WindowStoreState;
  for (const id of ALL_WINDOW_IDS) {
    const reg = windowRegistry[id];
    state[id] = {
      visible: false,
      minimized: false,
      ...reg.defaultRect,
      z: 0,
    };
  }
  return state;
}

function clampRect(
  rect: { x: number; y: number; width: number; height: number },
  minSize: { width: number; height: number },
  viewport?: { width: number; height: number },
): { x: number; y: number; width: number; height: number } {
  const vw = viewport?.width ?? (typeof window !== 'undefined' ? window.innerWidth : 1920);
  const vh = viewport?.height ?? (typeof window !== 'undefined' ? window.innerHeight : 1080);
  return fitRectToViewport(rect, minSize, { width: vw, height: vh });
}

function mergeDbLayout(dbLayout: ThreadWindowLayout | null): WindowStoreState {
  const defaults = buildDefaults();
  if (!dbLayout) return defaults;

  for (const id of ALL_WINDOW_IDS) {
    const saved = dbLayout.windows[id];
    if (!saved) continue;
    const reg = windowRegistry[id];
    const clamped = clampRect(
      { x: saved.x, y: saved.y, width: saved.width, height: saved.height },
      reg.minSize,
    );
    defaults[id] = {
      visible: saved.visible,
      minimized: saved.minimized ?? false,
      ...clamped,
      z: saved.z,
    };
  }
  return defaults;
}

const store = writable<WindowStoreState>(buildDefaults());
export const windowLayoutRemembered = writable(true);
let readThreadWindowLayout = getThreadWindowLayout;
let writeThreadWindowLayout = saveThreadWindowLayout;

let nextZ = 1;
let boundThreadId: string | null = null;
let dirty = false;
let dirtyTimerId: ReturnType<typeof setInterval> | null = null;
let layoutLoadToken = 0;
const layoutCacheByThreadId = new Map<string, ThreadWindowCacheEntry>();

function currentState(): WindowStoreState {
  return get(store);
}

function cloneState(state: WindowStoreState): WindowStoreState {
  const next = {} as WindowStoreState;
  for (const id of ALL_WINDOW_IDS) {
    next[id] = { ...state[id] };
  }
  return next;
}

function ensureThreadCache(threadId: string): ThreadWindowCacheEntry {
  const existing = layoutCacheByThreadId.get(threadId);
  if (existing) return existing;
  const fresh: ThreadWindowCacheEntry = {
    state: cloneState(buildDefaults()),
    revision: 0,
    dirtyRevision: null,
    rememberLayout: true,
  };
  layoutCacheByThreadId.set(threadId, fresh);
  return fresh;
}

function cacheThreadState(
  threadId: string,
  state: WindowStoreState,
  bumpRevision: boolean,
  rememberLayout?: boolean,
) {
  const existing = ensureThreadCache(threadId);
  const revision = bumpRevision ? existing.revision + 1 : existing.revision;
  layoutCacheByThreadId.set(threadId, {
    state: cloneState(state),
    revision,
    dirtyRevision: bumpRevision ? revision : existing.dirtyRevision,
    rememberLayout: rememberLayout ?? existing.rememberLayout,
  });
}

function commitState(next: WindowStoreState) {
  store.set(next);
  if (boundThreadId) {
    const cache = layoutCacheByThreadId.get(boundThreadId);
    if (cache?.rememberLayout !== false) {
      cacheThreadState(boundThreadId, next, true, cache?.rememberLayout ?? true);
    }
  }
  if (windowLayoutRememberedValue()) {
    dirty = true;
    startDirtyTick();
  }
}

function windowLayoutRememberedValue(): boolean {
  return get(windowLayoutRemembered);
}

function toDbLayout(state: WindowStoreState): ThreadWindowLayout {
  const windows: Partial<Record<string, ThreadWindowState>> = {};
  for (const id of ALL_WINDOW_IDS) {
    const s = state[id];
    windows[id] = {
      visible: s.visible,
      minimized: s.minimized,
      x: s.x,
      y: s.y,
      width: s.width,
      height: s.height,
      z: s.z,
    };
  }
  return {
    schemaVersion: 1,
    rememberLayout: windowLayoutRememberedValue(),
    windows,
  };
}

async function flushLayout() {
  if (!dirty || !boundThreadId || !windowLayoutRememberedValue()) return;
  const threadId = boundThreadId;
  const layout = toDbLayout(currentState());
  const cachedRevision = layoutCacheByThreadId.get(threadId)?.dirtyRevision ?? null;
  try {
    await writeThreadWindowLayout(threadId, layout);
  } catch {
    // Persistence failure is non-fatal; layout will re-save next tick
    return;
  }

  if (boundThreadId !== threadId) return;
  const entry = layoutCacheByThreadId.get(threadId);
  if (!entry) return;
  if (cachedRevision !== null && entry.dirtyRevision === cachedRevision) {
    dirty = false;
    layoutCacheByThreadId.set(threadId, {
      ...entry,
      dirtyRevision: null,
    });
  }
}

function startDirtyTick() {
  if (dirtyTimerId != null) return;
  dirtyTimerId = setInterval(() => {
    flushLayout();
  }, 2000);
}

function stopDirtyTick() {
  if (dirtyTimerId != null) {
    clearInterval(dirtyTimerId);
    dirtyTimerId = null;
  }
}

function markDirty() {
  dirty = true;
  startDirtyTick();
}

export async function loadLayoutForThread(threadId: string) {
  // Hard flush previous thread layout before switching
  await flushLayout();

  const token = ++layoutLoadToken;
  boundThreadId = threadId;
  dirty = false;
  const cacheEntry = ensureThreadCache(threadId);
  const startRevision = cacheEntry.revision;
  windowLayoutRemembered.set(cacheEntry.rememberLayout);
  store.set(cloneState(cacheEntry.rememberLayout ? cacheEntry.state : buildDefaults()));
  let dbLayout: ThreadWindowLayout | null = null;
  try {
    dbLayout = await readThreadWindowLayout(threadId);
  } catch {
    // If load fails, fall back to defaults
  }
  // Ignore stale response if another thread switch happened during the async load
  if (token !== layoutLoadToken || boundThreadId !== threadId) return;
  const currentCache = layoutCacheByThreadId.get(threadId);
  if (!currentCache || currentCache.revision !== startRevision || currentCache.dirtyRevision !== null) return;
  const rememberLayout = dbLayout?.rememberLayout ?? true;
  windowLayoutRemembered.set(rememberLayout);
  const merged = rememberLayout ? mergeDbLayout(dbLayout) : buildDefaults();
  cacheThreadState(threadId, merged, false, rememberLayout);
  // Compute max z from loaded state
  nextZ = 1;
  for (const id of ALL_WINDOW_IDS) {
    if (merged[id].z >= nextZ) nextZ = merged[id].z + 1;
  }
  store.set(merged);
}

export function bringToFront(id: WindowId) {
  const next = cloneState(currentState());
  next[id] = { ...next[id], z: nextZ++ };
  commitState(next);
}

export function showWindow(id: WindowId) {
  const next = cloneState(currentState());
  const reg = windowRegistry[id];
  const clamped = clampRect(next[id], reg.minSize);
  next[id] = { ...next[id], ...clamped, visible: true, minimized: false, z: nextZ++ };
  commitState(next);
}

export function ensureWindowVisible(id: WindowId) {
  showWindow(id);
}

export function toggleWindow(id: WindowId) {
  const next = cloneState(currentState());
  if (next[id].visible) {
    next[id] = { ...next[id], visible: false };
  } else {
    const reg = windowRegistry[id];
    const clamped = clampRect(next[id], reg.minSize);
    next[id] = { ...next[id], ...clamped, visible: true, minimized: false, z: nextZ++ };
  }
  commitState(next);
}

export function closeWindow(id: WindowId) {
  const next = cloneState(currentState());
  next[id] = { ...next[id], visible: false };
  commitState(next);
}

export function updateRect(id: WindowId, rect: { x: number; y: number; width: number; height: number }) {
  const next = cloneState(currentState());
  const reg = windowRegistry[id];
  const clamped = clampRect(rect, reg.minSize);
  next[id] = { ...next[id], ...clamped };
  commitState(next);
}

export async function setThreadWindowLayoutRemembered(rememberLayout: boolean) {
  windowLayoutRemembered.set(rememberLayout);
  if (!boundThreadId) return;
  const threadId = boundThreadId;
  const current = currentState();
  const layout = {
    schemaVersion: 1,
    rememberLayout,
    windows: toDbLayout(current).windows,
  } satisfies ThreadWindowLayout;
  cacheThreadState(threadId, rememberLayout ? current : buildDefaults(), false, rememberLayout);
  if (rememberLayout) {
    dirty = true;
    startDirtyTick();
  } else {
    dirty = false;
    stopDirtyTick();
  }
  await writeThreadWindowLayout(threadId, layout);
}

export function hardFlush() {
  flushLayout();
}

export function teardown() {
  stopDirtyTick();
  void flushLayout();
  boundThreadId = null;
  layoutCacheByThreadId.clear();
  windowLayoutRemembered.set(true);
}

export function windowState(id: WindowId) {
  return derived(store, ($s) => $s[id]);
}

export const windowStore = store;

if (
  typeof window !== 'undefined' &&
  typeof (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ === 'object'
) {
  void listen('mcp://ui-dispatch', (event: { payload: { action: string; target: string; value?: any } }) => {
    const { action, target } = event.payload;
    if (action === 'openWindow') {
      const id = target as WindowId;
      if (ALL_WINDOW_IDS.includes(id)) {
        showWindow(id);
      }
    } else if (action === 'closeWindow') {
      const id = target as WindowId;
      if (ALL_WINDOW_IDS.includes(id)) {
        const next = cloneState(currentState());
        next[id] = { ...next[id], visible: false };
        commitState(next);
      }
    } else if (action === 'highlightParam') {
      triggerHighlight(target, 'highlightParam');
    }
  });
}

// Export for testing
export function _setWindowStoreTransportForTest(transport: {
  readThreadWindowLayout?: typeof getThreadWindowLayout;
  writeThreadWindowLayout?: typeof saveThreadWindowLayout;
}) {
  readThreadWindowLayout = transport.readThreadWindowLayout ?? getThreadWindowLayout;
  writeThreadWindowLayout = transport.writeThreadWindowLayout ?? saveThreadWindowLayout;
}

export function _resetWindowStoreForTest() {
  stopDirtyTick();
  nextZ = 1;
  boundThreadId = null;
  dirty = false;
  layoutLoadToken = 0;
  layoutCacheByThreadId.clear();
  store.set(buildDefaults());
  windowLayoutRemembered.set(true);
  readThreadWindowLayout = getThreadWindowLayout;
  writeThreadWindowLayout = saveThreadWindowLayout;
}

export { mergeDbLayout as _mergeDbLayout, clampRect as _clampRect, cloneState as _cloneState, ALL_WINDOW_IDS };
