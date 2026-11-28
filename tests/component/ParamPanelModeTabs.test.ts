import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import ParamPanelModeTabs from '../../src/lib/components/ParamPanelModeTabs.svelte';

// thread-source-binding §3.2 — the Params editor-open control is labelled
// `OPEN FILE` (matching the Project card action and the spec wording), and
// clicking it invokes the handler whose raw failure is routed to the visible
// Ecky error surface in App.svelte (reusing createSourceActions, whose raw
// error contract is proven in src/lib/sourceActions.test.ts).

describe('ParamPanelModeTabs', () => {
  it('labels the editor-open control OPEN FILE (not the stale FILE ↗)', () => {
    const { getByRole } = render(ParamPanelModeTabs, {
      props: { macroCode: '# mock macro', onOpenInEditor: vi.fn() },
    });

    const btn = getByRole('button', { name: 'OPEN FILE' });
    expect(btn).toBeTruthy();
    // The stale glyph label must be gone.
    expect(document.body.textContent ?? '').not.toContain('FILE ↗');
  });

  it('OPEN FILE invokes onOpenInEditor when clicked', async () => {
    const onOpenInEditor = vi.fn();
    const { getByRole } = render(ParamPanelModeTabs, {
      props: { macroCode: '# mock macro', onOpenInEditor },
    });

    await fireEvent.click(getByRole('button', { name: 'OPEN FILE' }));

    expect(onOpenInEditor).toHaveBeenCalledTimes(1);
  });

  it('hides the OPEN FILE control when there is no macro or no handler', () => {
    const { queryByRole } = render(ParamPanelModeTabs, {
      props: { macroCode: '', onOpenInEditor: undefined },
    });

    expect(queryByRole('button', { name: 'OPEN FILE' })).toBeNull();
  });

  it('shows ASSEMBLY tab only when assemblyEnabled and switches to assembly on click', async () => {
    const onActiveTabChange = vi.fn();
    const view = render(ParamPanelModeTabs, {
      props: { activeTab: 'params', assemblyEnabled: false, onActiveTabChange },
    });

    expect(view.queryByRole('button', { name: 'ASSEMBLY' })).toBeNull();

    await view.rerender({ activeTab: 'params', assemblyEnabled: true, onActiveTabChange });
    const assemblyBtn = view.getByRole('button', { name: 'ASSEMBLY' });
    expect(assemblyBtn.getAttribute('aria-pressed')).toBe('false');

    await fireEvent.click(assemblyBtn);
    expect(onActiveTabChange).toHaveBeenCalledWith('assembly');
  });
});
