import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import ViewportWorkspace from '../../src/lib/workbench/ViewportWorkspace.svelte';

function actionEl(container: HTMLElement, action: string): HTMLElement | null {
  return container.querySelector(`[data-viewport-action="${action}"]`);
}

describe('ViewportWorkspace action strip', () => {
  it('renders fork, export, and code actions when export is available', () => {
    const { container } = render(ViewportWorkspace, {
      props: {
        showExport: true,
        canExport: true,
        busy: false,
        hasSketchPreview: false,
      },
    });

    expect(actionEl(container, 'fork')).not.toBeNull();
    expect(actionEl(container, 'export')).not.toBeNull();
    expect(actionEl(container, 'code')).not.toBeNull();
  });

  it('hides the export action when no artifact bundle is available', () => {
    const { container } = render(ViewportWorkspace, {
      props: { showExport: false, canExport: true, busy: false },
    });

    // export is gated on the artifact bundle existing
    expect(actionEl(container, 'export')).toBeNull();
    // fork + code are independent of export availability
    expect(actionEl(container, 'fork')).not.toBeNull();
    expect(actionEl(container, 'code')).not.toBeNull();
  });

  it('enables fork and export when idle, exportable, and not sketching', () => {
    const { container } = render(ViewportWorkspace, {
      props: { showExport: true, canExport: true, busy: false, hasSketchPreview: false },
    });

    expect(actionEl(container, 'fork')?.hasAttribute('disabled')).toBe(false);
    expect(actionEl(container, 'export')?.hasAttribute('disabled')).toBe(false);
  });

  it('disables fork while the viewport busy mask is up', () => {
    const { container } = render(ViewportWorkspace, {
      props: { showExport: true, canExport: true, busy: true },
    });

    expect(actionEl(container, 'fork')?.hasAttribute('disabled')).toBe(true);
  });

  it('disables export while the viewport busy mask is up', () => {
    const { container } = render(ViewportWorkspace, {
      props: { showExport: true, canExport: true, busy: true },
    });

    expect(actionEl(container, 'export')?.hasAttribute('disabled')).toBe(true);
  });

  it('disables export when the model is not exportable', () => {
    const { container } = render(ViewportWorkspace, {
      props: { showExport: true, canExport: false, busy: false, hasSketchPreview: false },
    });

    expect(actionEl(container, 'export')?.hasAttribute('disabled')).toBe(true);
    // fork is independent of exportability
    expect(actionEl(container, 'fork')?.hasAttribute('disabled')).toBe(false);
  });

  it('disables export while a sketch preview is active', () => {
    const { container } = render(ViewportWorkspace, {
      props: { showExport: true, canExport: true, busy: false, hasSketchPreview: true },
    });

    expect(actionEl(container, 'export')?.hasAttribute('disabled')).toBe(true);
  });

  it('disables code while the viewport busy mask is up', () => {
    const { container } = render(ViewportWorkspace, {
      props: { showExport: true, canExport: true, busy: true },
    });

    expect(actionEl(container, 'code')?.hasAttribute('disabled')).toBe(true);
  });

  it('fires onFork, onExport, and onCode when the actions are clicked', async () => {
    const onFork = vi.fn();
    const onExport = vi.fn();
    const onCode = vi.fn();
    const { container } = render(ViewportWorkspace, {
      props: {
        showExport: true,
        canExport: true,
        busy: false,
        hasSketchPreview: false,
        onFork,
        onExport,
        onCode,
      },
    });

    actionEl(container, 'fork')?.click();
    actionEl(container, 'export')?.click();
    actionEl(container, 'code')?.click();

    expect(onFork).toHaveBeenCalledTimes(1);
    expect(onExport).toHaveBeenCalledTimes(1);
    expect(onCode).toHaveBeenCalledTimes(1);
  });
});
