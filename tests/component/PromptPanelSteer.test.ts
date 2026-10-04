import { expect, it, vi } from 'vitest';
import { fireEvent, render, waitFor } from '@testing-library/svelte';
import PromptPanel from '../../src/lib/PromptPanel.svelte';

it('Given a reference image When steering fails Then the entire draft is retained after pending input clears', async () => {
  let rejectSteer!: (reason: Error) => void;
  const onSteerCodexTakeover = vi.fn(() => new Promise<void>((_, reject) => { rejectSteer = reject; }));
  const { container, getByRole, getByPlaceholderText } = render(PromptPanel, {
    props: {
      onGenerate: async () => {}, onShowCode: () => {}, onSteerCodexTakeover,
      activeThreadId: 'steer-thread',
      dialogueState: { mode: 'provider', providerId: 'codex', externalConversationId: 'codex-thread', label: 'Codex', supportsSteer: true, supportsStop: true },
      codexTakeover: {
        binding: { eckyThreadId: 'steer-thread', codexThreadId: 'codex-thread', cwd: '/workspace', label: 'test', bootstrapVersion: 1, createdAt: 1, updatedAt: 1 },
        runtime: { phase: 'active', activeTurnId: 'turn-1', error: null },
        messages: [], liveMessages: [], turnTraces: [], queue: [], nextCursor: null, backwardsCursor: null,
      },
    },
  });
  await fireEvent.drop(container.querySelector('.prompt-container')!, {
    dataTransfer: { files: [new File(['image'], 'shoulder.png', { type: 'image/png' })] },
  });
  await waitFor(() => expect(container.querySelector('.attachment-item')?.textContent).toContain('shoulder.png'));
  await fireEvent.input(container.querySelector('.att-explanation')!, { target: { value: 'Inspect this shoulder.' } });
  const input = getByPlaceholderText(/Type a question or design change/i) as HTMLTextAreaElement;
  await fireEvent.input(input, { target: { value: 'Check this first.' } });
  await fireEvent.click(getByRole('button', { name: 'STEER' }));
  expect(onSteerCodexTakeover).toHaveBeenCalledWith('Check this first.', [expect.objectContaining({
    name: 'shoulder.png', type: 'image', explanation: 'Inspect this shoulder.',
  })]);
  expect(input.value).toBe('');
  expect(container.querySelector('.attachment-item')).toBeNull();
  rejectSteer(new Error('Jev timeout: raw classifier response'));
  await waitFor(() => expect(input.value).toBe('Check this first.'));
  expect(container.querySelector('.attachment-item')?.textContent).toContain('shoulder.png');
  expect((container.querySelector('.att-explanation') as HTMLInputElement).value).toBe('Inspect this shoulder.');
});
