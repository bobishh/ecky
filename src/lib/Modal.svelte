<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    onclose,
    children,
  }: {
    title: string;
    onclose: () => void;
    children: Snippet;
  } = $props();

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onclose();
    }
  }

  function stopPropagation(e: Event) {
    e.stopPropagation();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="modal-backdrop" role="presentation" data-window-drag-ignore onclick={onclose} onmousedown={stopPropagation} onpointerdown={stopPropagation}>
  <div class="modal-window" role="dialog" aria-modal="true" aria-labelledby="modal-title" tabindex="-1" data-window-drag-ignore onclick={stopPropagation} onmousedown={stopPropagation} onpointerdown={stopPropagation} onkeydown={stopPropagation}>
    <div class="modal-header">
      <h3 id="modal-title" class="modal-title">{title}</h3>
      <button class="modal-close" onclick={onclose}>&times;</button>
    </div>
    <div class="modal-body">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.86);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }

  .modal-window {
    background: #111827;
    border: 1px solid var(--bg-300);
    box-shadow: 0 14px 48px rgba(0, 0, 0, 0.72);
    display: flex;
    flex-direction: column;
    max-width: 90%;
    max-height: 90%;
    min-width: min(400px, 90%);
  }

  .modal-header {
    padding: 8px 12px;
    background: #172033;
    border-bottom: 1px solid var(--bg-300);
    display: flex;
    align-items: center;
    justify-content: space-between;
    user-select: none;
  }

  .modal-title {
    margin: 0;
    font-size: 0.7rem;
    font-weight: bold;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--secondary);
  }

  .modal-close {
    background: none;
    border: none;
    color: var(--text-dim);
    font-size: 1.2rem;
    cursor: pointer;
    line-height: 1;
    padding: 0 4px;
  }

  .modal-close:hover {
    color: var(--text);
  }

  .modal-body {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  :global(.modal-window .modal-actions),
  :global(.modal-window .confirm-actions) {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    margin-top: 8px;
  }

  :global(.modal-window .btn) {
    padding: 8px 16px;
    border: 1px solid var(--bg-400);
    background: var(--bg-200);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  :global(.modal-window .btn:hover:not(:disabled)) {
    border-color: var(--primary);
    color: var(--text);
  }

  :global(.modal-window .btn:disabled) {
    opacity: 0.55;
    cursor: default;
  }

  :global(.modal-window .btn-ghost),
  :global(.modal-window .btn-secondary) {
    background: color-mix(in srgb, var(--bg-200) 88%, black 12%);
    border-color: var(--bg-400);
    color: var(--text-dim);
  }

  :global(.modal-window .btn-ghost:hover:not(:disabled)),
  :global(.modal-window .btn-secondary:hover:not(:disabled)) {
    border-color: var(--secondary);
    color: var(--text);
  }

  :global(.modal-window .btn-primary) {
    border-color: var(--secondary);
    color: var(--secondary);
    background: transparent;
  }

  :global(.modal-window .btn-primary:hover:not(:disabled)) {
    background: color-mix(in srgb, var(--secondary) 18%, transparent);
  }

  :global(.modal-window .btn-danger) {
    border-color: var(--red);
    color: var(--red);
    background: color-mix(in srgb, var(--red) 10%, transparent);
  }

  :global(.modal-window .btn-danger:hover:not(:disabled)) {
    border-color: var(--red);
    background: color-mix(in srgb, var(--red) 18%, transparent);
    color: color-mix(in srgb, var(--red) 70%, white 30%);
  }
</style>
