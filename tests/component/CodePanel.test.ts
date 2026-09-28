import { afterEach, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';
import CodePanel from '../../src/lib/CodePanel.svelte';

afterEach(() => vi.restoreAllMocks());

it('selects the referenced line when source is already loaded at mount', async () => {
  vi.spyOn(window, 'requestAnimationFrame').mockReturnValue(1);
  const { container } = render(CodePanel, {
    props: { code: '; first\n(param height 80mm)\n; last', sourceLanguage: 'ecky', highlightLine: 2 },
  });
  await waitFor(() => {
    expect(container.querySelector('.cm-activeLine')?.textContent).toBe('(param height 80mm)');
  });
});

it('keeps the referenced line selected when editor language changes', async () => {
  vi.spyOn(window, 'requestAnimationFrame').mockReturnValue(1);
  const { container, rerender } = render(CodePanel, {
    props: { code: '; first\n(param height 80mm)\n; last', sourceLanguage: 'ecky', highlightLine: 2 },
  });
  await rerender({ sourceLanguage: 'legacyPython' });
  await waitFor(() => {
    expect(container.querySelector('.cm-activeLine')?.textContent).toBe('(param height 80mm)');
  });
});
