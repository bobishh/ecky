import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ChaptersReader from '../../src/lib/ChaptersReader.svelte';

describe('shared illustrated book in the app', () => {
  it('opens complete current chapter source, including component declarations', async () => {
    const onOpenAttempt = vi.fn();
    const { getByRole, container } = render(ChaptersReader, { props: { onOpenAttempt } });
    expect(container.textContent).toContain('Worm Farm');
    expect(container.querySelectorAll('pre').length).toBe(1);
    await fireEvent.click(getByRole('button', { name: 'OPEN ATTEMPT IN CODE' }));
    expect(onOpenAttempt.mock.calls[0][0].code).toContain('(tag connected-rake)');
    await fireEvent.click(getByRole('button', { name: /Chapter 3:/ }));
    await fireEvent.click(getByRole('button', { name: 'OPEN ATTEMPT IN CODE' }));
    const code = onOpenAttempt.mock.calls[1][0].code;
    expect(code).toMatch(/^\(define-component fence_post/);
    expect(code).toContain('(model');
    expect(code).toContain('(fence_post :x');
  });

  it('disables source opening when host supplies no editor action', () => {
    const { getByRole } = render(ChaptersReader);
    expect(getByRole('button', { name: 'OPEN ATTEMPT IN CODE' }).hasAttribute('disabled')).toBe(true);
  });
});
