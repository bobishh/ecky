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

<div class="modal-backdrop" role="presentation" onclick={onclose}>
  <div class="modal-window" role="dialog" aria-modal="true" aria-labelledby="modal-title" tabindex="-1" onclick={stopPropagation} onkeydown={stopPropagation}>
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
    top: 0;
    left: 0;
    width: 100vw;
    height: 100vh;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-window {
    background: var(--bg-100);
    border: 1px solid var(--bg-300);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    max-width: 90vw;
    max-height: 90vh;
    min-width: 400px;
  }

  .modal-header {
    padding: 8px 12px;
    background: var(--bg-200);
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
</style>
