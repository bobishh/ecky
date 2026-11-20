import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, expect, it, vi } from 'vitest';
import PuppetBuilder from '../../src/lib/puppet/PuppetBuilder.svelte';

const api = vi.hoisted(() => ({ importPart: vi.fn(), preview: vi.fn(), save: vi.fn(), export: vi.fn(), open: vi.fn(), choose: vi.fn(), destination: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: api.choose }));
vi.mock('../../src/lib/tauri/client', () => ({
  formatBackendError: (error: Error) => error.message,
  importPuppetPart: api.importPart, previewPuppet: api.preview, savePuppet: api.save,
  exportPuppet: api.export, openPuppet: api.open, showSafeSaveDialog: api.destination,
}));
// The GLB loader is checked separately. These tests cover
// user interactions without requiring WebGL in jsdom.
vi.mock('../../src/lib/puppet/PuppetViewport.svelte', () => ({ default: () => {} }));
const part = (id: string) => ({ id, name: id, assetId: 'a'.repeat(64), parentId: null,
  positionMm: [0,0,0], pivotMm: [0,0,0], rotationDeg: [0,0,0], scale: 1,
  axis: 'y', amplitudeDeg: 0, phaseDeg: 0, triangleCount: 4, textured: false });
beforeEach(() => {
  vi.resetAllMocks();
  api.choose.mockResolvedValue('/parts/body.stl');
  api.destination.mockImplementation((filename: string) => Promise.resolve(`/exports/${filename}`));
  api.preview.mockResolvedValue('/preview/puppet.glb');
  api.importPart.mockResolvedValueOnce(part('Body')).mockResolvedValueOnce(part('Head'));
});
it('assembles parts, sets a parent and swing, then saves and exports the authored document', async () => {
  const ui = render(PuppetBuilder);
  await fireEvent.click(ui.getByRole('button', { name: 'Add part file' }));
  await waitFor(() => expect(ui.getByLabelText('Part name')).toHaveProperty('value', 'Body'));
  await waitFor(() => expect(ui.getByRole('button', { name: 'Add part file' })).toHaveProperty('disabled', false));
  await fireEvent.click(ui.getByRole('button', { name: 'Add part file' }));
  await waitFor(() => expect(ui.getByLabelText('Part name')).toHaveProperty('value', 'Head'));
  await fireEvent.input(ui.getByLabelText('Part name'), { target: { value: 'Unicorn head' } });
  await fireEvent.change(ui.getByLabelText('Parent part'), { target: { value: 'Body' } });
  await fireEvent.input(ui.getByLabelText('Swing angle'), { target: { value: '20' } });
  await fireEvent.click(ui.getByRole('button', { name: 'Save puppet' }));
  await waitFor(() => expect(ui.getByRole('status').textContent).toContain('Saved'));
  expect(api.save.mock.calls[0][0].parts[1]).toMatchObject({ name: 'Unicorn head', parentId: 'Body', amplitudeDeg: 20 });
  await fireEvent.click(ui.getByRole('button', { name: 'Export GLB' }));
  await waitFor(() => expect(api.export).toHaveBeenCalledOnce());
  expect(api.export.mock.calls[0][1]).toBe('/exports/My-puppet.glb');
  expect(ui.getByRole('status').textContent).toContain('Exported');
});
it('retains existing parts and surfaces a missing texture when another import fails', async () => {
  const ui = render(PuppetBuilder);
  await fireEvent.click(ui.getByRole('button', { name: 'Add part file' }));
  await waitFor(() => expect(ui.getByLabelText('Part name')).toHaveProperty('value', 'Body'));
  await waitFor(() => expect(ui.getByRole('button', { name: 'Add part file' })).toHaveProperty('disabled', false));
  api.importPart.mockReset().mockRejectedValue(new Error('Missing texture: head.png. Keep OBJ, MTL and images together.'));
  await fireEvent.click(ui.getByRole('button', { name: 'Add part file' }));
  await waitFor(() => expect(ui.getByRole('alert').textContent).toContain('Missing texture: head.png'));
  expect(ui.getByLabelText('Part name')).toHaveProperty('value', 'Body');
  expect(ui.getByRole('button', { name: 'Export GLB' })).toHaveProperty('disabled', false);
});
it('imports the original current scan and prevents duplicate submissions while it loads', async () => {
  let resolve: (value: ReturnType<typeof part>) => void = () => {};
  api.importPart.mockReset().mockImplementation(() => new Promise(done => { resolve = done; }));
  const ui = render(PuppetBuilder, { captureRunId: 'scan-123' });
  await fireEvent.click(ui.getByRole('button', { name: 'Add current scan' }));
  expect(api.importPart).toHaveBeenCalledWith(null, 'scan-123', true, false);
  expect(ui.getByRole('button', { name: 'Add current scan' })).toHaveProperty('disabled', true);
  resolve(part('Head'));
  await waitFor(() => expect(ui.getByLabelText('Part name')).toHaveProperty('value', 'Head'));
});
