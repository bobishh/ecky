import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';
import LibraryPanel from '../../src/lib/LibraryPanel.svelte';

const { libraryPanelIntent } = vi.hoisted(() => ({ libraryPanelIntent: vi.fn() }));

vi.mock('../../src/lib/tauri/client', () => ({
  libraryPanelIntent,
  formatBackendError: (error: unknown) => String(error),
}));

function component(overrides: Record<string, unknown> = {}) {
  return {
    name: 'Rail clip',
    oneLiner: 'Clips onto a 20 mm support rail.',
    paramKeys: ['rail_width', 'clip_depth'],
    tags: ['fixture'],
    ports: [],
    componentId: 'component-123',
    revisionDigest: 'abc123def456',
    threadId: 'thread-uuid-123',
    threadTitle: 'Hydrant cap project',
    origin: 'local',
    ...overrides,
  };
}

describe('LibraryPanel component cards', () => {
  beforeEach(() => {
    libraryPanelIntent.mockReset();
    libraryPanelIntent.mockResolvedValue({
      kind: 'componentPackages',
      packageHeaders: [],
      components: [component()],
      indexingDiagnostics: [],
    });
  });

  it('shows the authored description, thread title, and separate revision and parameter metadata', async () => {
    const { findByText, getByText, getByRole } = render(LibraryPanel, {
      props: { onApplyComponentImport: vi.fn() },
    });

    await findByText('Clips onto a 20 mm support rail.');
    expect(getByRole('heading', { name: 'Rail clip' })).toBeTruthy();
    expect(getByText('Hydrant cap project')).toBeTruthy();
    expect(getByText('abc123def456')).toBeTruthy();
    expect(getByText('rail_width / clip_depth')).toBeTruthy();
    expect(getByText('ID thread-uuid-123')).toBeTruthy();
  });

  it('shows an honest missing-description and provenance fallback without promoting thread ID to a title', async () => {
    libraryPanelIntent.mockResolvedValue({
      kind: 'componentPackages',
      packageHeaders: [],
      components: [component({ oneLiner: '', threadId: 'orphan-source-id', threadTitle: null })],
      indexingDiagnostics: [],
    });
    const { findByText, getByText, getByRole, queryByText } = render(LibraryPanel, {
      props: { onApplyComponentImport: vi.fn() },
    });

    await waitFor(() => expect(getByRole('heading', { name: 'Rail clip' })).toBeTruthy());
    expect(await findByText('No description available.')).toBeTruthy();
    expect(getByText('SOURCE THREAD UNAVAILABLE')).toBeTruthy();
    expect(getByText('ID orphan-source-id')).toBeTruthy();
    expect(queryByText('orphan-source-id', { exact: true })).toBeNull();
  });

  it('labels built-in component provenance as the built-in library', async () => {
    libraryPanelIntent.mockResolvedValue({
      kind: 'componentPackages',
      packageHeaders: [],
      components: [component({ origin: 'builtin', threadId: null, threadTitle: null })],
      indexingDiagnostics: [],
    });
    const { findByText, getByText, queryByText } = render(LibraryPanel, {
      props: { onApplyComponentImport: vi.fn() },
    });

    await findByText('Rail clip');
    expect(getByText('Built-in library')).toBeTruthy();
    expect(queryByText('SOURCE THREAD UNAVAILABLE')).toBeNull();
  });
});
