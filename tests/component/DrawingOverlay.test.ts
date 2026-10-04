import { expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import DrawingOverlay from '../../src/lib/DrawingOverlay.svelte';

it('Given a captured drawing When another stroke is committed Then its revision changes for acknowledgement safety', async () => {
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} });
  const context = { clearRect: vi.fn(), beginPath: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), stroke: vi.fn() };
  const getContext = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(context as unknown as CanvasRenderingContext2D);
  const originalCapture = HTMLCanvasElement.prototype.setPointerCapture;
  HTMLCanvasElement.prototype.setPointerCapture = vi.fn();
  try {
    const { container, component } = render(DrawingOverlay, { props: { active: true } });
    const canvas = container.querySelector('canvas')!;
    const revision = () => (component as unknown as { getRevision(): number }).getRevision();
    const initial = revision();
    await fireEvent.pointerDown(canvas, { pointerId: 1, clientX: 10, clientY: 10 });
    await fireEvent.pointerMove(canvas, { pointerId: 1, clientX: 30, clientY: 30 });
    await fireEvent.pointerUp(canvas, { pointerId: 1 });
    const sentRevision = revision();
    expect(sentRevision).toBeGreaterThan(initial);
    await fireEvent.pointerDown(canvas, { pointerId: 2, clientX: 40, clientY: 40 });
    expect(revision()).toBeGreaterThan(sentRevision);
    await fireEvent.pointerMove(canvas, { pointerId: 2, clientX: 60, clientY: 60 });
    await fireEvent.pointerUp(canvas, { pointerId: 2 });
    expect(revision()).toBeGreaterThan(sentRevision);
  } finally {
    getContext.mockRestore(); HTMLCanvasElement.prototype.setPointerCapture = originalCapture; vi.unstubAllGlobals();
  }
});
