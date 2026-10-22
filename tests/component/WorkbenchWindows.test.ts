import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import WorkbenchWindows from '../../src/lib/workbench/WorkbenchWindows.svelte';
import type { WindowState } from '../../src/lib/stores/windowStore';

const hidden = (): WindowState => ({ visible: false, minimized: false, active: false, x: 0, y: 0, width: 400, height: 300, z: 1 });

describe('WorkbenchWindows', () => {
  it('renders dock actions and forwards the requested window id', async () => {
    const onToggleWindow = vi.fn();
    const onActivateWindow = vi.fn();
    const states = Object.fromEntries(['projects', 'library', 'params', 'dialogue', 'docs', 'settings', 'terminal', 'activity'].map((id) => [id, hidden()])) as any;
    const { getByRole } = render(WorkbenchWindows, {
      props: {
        currentView: 'workbench',
        windowStates: states,
        mountedWindows: {},
        drawMode: false,
        canDraw: true,
        onActivateWindow,
        onDrawToggle: vi.fn(),
        onCloseView: vi.fn(),
        onCloseWindow: vi.fn(),
      },
    });

    expect(getByRole('button', { name: /projects/i })).not.toBeNull();
    expect(getByRole('button', { name: /dialogue/i })).not.toBeNull();
    expect(getByRole('button', { name: /code inspector/i })).not.toBeNull();
    expect(getByRole('button', { name: /ecky ir docs/i })).not.toBeNull();
    expect(document.querySelector('[data-dock-id="analysis"]')).toBeNull();
    await fireEvent.click(getByRole('button', { name: /parameters/i }));
    expect(onActivateWindow).toHaveBeenCalledWith('params', 'open');
  });

  it('shows the draw pending state as disabled with the raw capability reason', () => {
    const states = Object.fromEntries(['projects', 'library', 'params', 'dialogue', 'docs', 'settings', 'terminal', 'activity'].map((id) => [id, hidden()])) as any;
    const { getByRole } = render(WorkbenchWindows, {
      props: {
        currentView: 'workbench',
        windowStates: states,
        mountedWindows: {},
        drawMode: false,
        canDraw: false,
        drawUnavailableReason: 'Vision backend unavailable: raw provider detail',
        onActivateWindow: vi.fn(),
        onDrawToggle: vi.fn(),
        onCloseView: vi.fn(),
        onCloseWindow: vi.fn(),
      },
    });

    const draw = getByRole('button', { name: /draw annotations/i }) as HTMLButtonElement;
    expect(draw.getAttribute('aria-disabled')).toBe('true');
    expect(draw.title).toBe('Vision backend unavailable: raw provider detail');
  });
});
