import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { expect, it, vi } from 'vitest';
import CapturePanel from '../../src/lib/CapturePanel.svelte';
vi.mock('../../src/lib/Viewer.svelte', () => ({ default: () => {} }));
it('opens with an actionable start and no invented camera or session state', async () => {
  const start = vi.fn();
  const ui = render(CapturePanel, { onStartCapture: start });
  expect(ui.getByRole('heading', { name: 'Scan a real object' })).toBeTruthy();
  expect(ui.queryByText('PAIR PHONE')).toBeNull();
  expect(ui.queryByText('Camera permission pending')).toBeNull();
  expect(ui.queryByRole('button', { name: /cancel/i })).toBeNull();
  expect(ui.queryByRole('tab', { name: 'GUIDED BREP' })).toBeNull();
  await fireEvent.click(ui.getByRole('button', { name: 'Start capture' }));
  expect(start).toHaveBeenCalledOnce();
});
it('shows phone instructions only for a real session and keeps cancellation available', async () => {
  const cancel = vi.fn();
  const ui = render(CapturePanel, { sessionActive: true, sessionState: 'pairing', pairingUrl: 'https://192.0.2.10:44000/capture/abc', trustUrl: 'http://192.0.2.10:44001/trust', cameraStatus: 'Waiting for phone camera', onCancelCapture: cancel });
  expect(ui.getByRole('heading', { name: 'Connect your phone' })).toBeTruthy();
  expect(ui.getByRole('img', { name: 'Capture pairing QR code' })).toBeTruthy();
  expect(ui.queryByRole('button', { name: 'Start capture' })).toBeNull();
  await fireEvent.click(ui.getByRole('button', { name: 'Cancel capture' }));
  expect(cancel).toHaveBeenCalledOnce();
});
it('displays the backend failure and offers the recovery appropriate to its session', async () => {
  const start = vi.fn(), retry = vi.fn();
  const ui = render(CapturePanel, { sessionState: 'failed', cameraStatus: 'Object Capture is unavailable on this Mac.', onStartCapture: start, onRetryReconstruction: retry });
  expect(ui.getByRole('alert').textContent).toContain('Object Capture is unavailable');
  await fireEvent.click(ui.getByRole('button', { name: 'Start capture' }));
  expect(start).toHaveBeenCalledOnce();
  await ui.rerender({ sessionActive: true });
  await fireEvent.click(ui.getByRole('button', { name: 'Retry reconstruction' }));
  expect(retry).toHaveBeenCalledOnce();
});
it('prevents repeated start requests while the command is pending and reports rejection', async () => {
  let reject: (error: Error) => void = () => {};
  const start = vi.fn(() => new Promise<void>((_, fail) => { reject = fail; }));
  const ui = render(CapturePanel, { onStartCapture: start });
  await fireEvent.click(ui.getByRole('button', { name: 'Start capture' }));
  expect(ui.getByRole('button', { name: 'Starting…' })).toHaveProperty('disabled', true);
  reject(new Error('Capture service unavailable'));
  await waitFor(() => expect(ui.getByRole('alert').textContent).toContain('Capture service unavailable'));
  expect(ui.getByRole('button', { name: 'Start capture' })).toHaveProperty('disabled', false);
});
