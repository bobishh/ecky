import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import DocsSite from '../../src/lib/DocsSite.svelte';

describe('reference operation links', () => {
  it('mounts the section owning each signature before scrolling', async () => {
    HTMLElement.prototype.scrollIntoView = vi.fn();
    const { container, getByRole } = render(DocsSite);
    await fireEvent.click(container.querySelector('.docs-article a[href="#box"]')!);
    expect(getByRole('heading', { name: 'Primitive Signatures', exact: true })).toBeTruthy();
    expect(container.querySelector('h3#box')).toBeTruthy();
    expect(HTMLElement.prototype.scrollIntoView).toHaveBeenCalled();
    await fireEvent.click(getByRole('button', { name: 'Operation Index', exact: true }));
    await fireEvent.click(container.querySelector('.docs-article a[href="#translate"]')!);
    expect(getByRole('heading', { name: 'Boolean and Transform Signatures', exact: true })).toBeTruthy();
    expect(container.querySelector('h3#translate')).toBeTruthy();
  });

  it('reports a missing heading without switching to an unrelated section', async () => {
    const { container, getByRole } = render(DocsSite);
    const link = document.createElement('a');
    link.href = '#missing-heading';
    container.querySelector('.docs-article__body')!.append(link);
    await fireEvent.click(link);
    expect(getByRole('alert').textContent).toContain('#missing-heading');
    expect(getByRole('heading', { name: 'Operation Index', exact: true })).toBeTruthy();
  });
});
