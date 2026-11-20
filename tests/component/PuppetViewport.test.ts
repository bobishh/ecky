import { render, waitFor } from '@testing-library/svelte';
import { expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import PuppetViewport from '../../src/lib/puppet/PuppetViewport.svelte';
const state = vi.hoisted(() => ({ frame: null as (() => void) | null, render: vi.fn(), load: vi.fn(), dispose: vi.fn(), failNext: false }));
vi.mock('three', async importOriginal => {
  const actual = await importOriginal<typeof import('three')>();
  return { ...actual, WebGLRenderer: class {
    domElement = document.createElement('canvas');
    setPixelRatio() {} setSize() {}
    setAnimationLoop(frame: (() => void) | null) { state.frame = frame; }
    render = state.render; dispose = state.dispose;
  } };
});
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc: (path: string) => path }));
vi.mock('three/examples/jsm/loaders/GLTFLoader.js', async importOriginal => {
  const { GLTFLoader } = await importOriginal<typeof import('three/examples/jsm/loaders/GLTFLoader.js')>();
  return { GLTFLoader: class extends GLTFLoader {
    load(_url: string, onLoad: (asset: any) => void, _progress: unknown, onError: (error: Error) => void) {
      state.load();
      if (state.failNext) {
        state.failNext = false;
        onError(new Error('Preview file temporarily unavailable'));
        return;
      }
      const bytes = readFileSync('e2e/fixtures/puppet.glb');
      const buffer = new ArrayBuffer(bytes.length);
      new Uint8Array(buffer).set(bytes);
      this.parse(buffer, '', onLoad, error => { throw error; });
    }
  } };
});
it('loads the exported hierarchy, retries a failed preview at the same path and disposes the renderer', async () => {
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} });
  const ready = vi.fn(), error = vi.fn();
  const part = { id: 'part2', name: 'Head', assetId: 'a'.repeat(64), parentId: 'part1', positionMm: [0,30,0], pivotMm: [0,0,0], rotationDeg: [0,0,0], scale: 0.5, axis: 'y', amplitudeDeg: 20, phaseDeg: 0, triangleCount: 4, textured: false };
  const ui = render(PuppetViewport, { path: '/test.glb', selected: part, playing: false, picking: false, onready: ready, onerror: error, onpick: vi.fn() } as any);
  await waitFor(() => expect(ready).toHaveBeenLastCalledWith(true));
  expect(error).not.toHaveBeenCalled();
  expect(state.load).toHaveBeenCalledOnce();
  state.frame?.();
  const scene = state.render.mock.calls.at(-1)![0];
  const head = scene.getObjectByName('joint_part2');
  expect(head.parent.name).toBe('joint_part1');
  await ui.rerender({ playing: true }); state.frame?.();
  expect(state.load).toHaveBeenCalledOnce();
  ready.mockClear();
  await ui.rerender({ path: '/updated.glb' });
  await waitFor(() => expect(ready).toHaveBeenLastCalledWith(true));
  expect(state.load).toHaveBeenCalledTimes(2);
  ready.mockClear();
  state.failNext = true;
  await ui.rerender({ path: '/retry.glb' });
  await waitFor(() => expect(error).toHaveBeenLastCalledWith(expect.stringContaining('temporarily unavailable')));
  expect(ready).toHaveBeenLastCalledWith(false);
  await ui.rerender({ refreshToken: 1 });
  await waitFor(() => expect(ready).toHaveBeenLastCalledWith(true));
  expect(state.load).toHaveBeenCalledTimes(4);
  await ui.unmount(); expect(state.frame).toBeNull(); expect(state.dispose).toHaveBeenCalledOnce();
  vi.unstubAllGlobals();
});
