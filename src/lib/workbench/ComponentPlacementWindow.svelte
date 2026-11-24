<script lang="ts">
  import Window from '../Window.svelte';

  let {
    label,
    coordinates,
    preparing,
    applying,
    canApply,
    error,
    onCoordinateChange,
    onApply,
    onCancel,
  }: {
    label: string;
    coordinates: [number | null, number | null, number | null];
    preparing: boolean;
    applying: boolean;
    canApply: boolean;
    error: string | null;
    onCoordinateChange: (index: 0 | 1 | 2, value: number | null) => void;
    onApply: () => void;
    onCancel: () => void;
  } = $props();

  const axes = [[0, 'X'], [1, 'Y'], [2, 'Z']] as const;
  const width = Math.min(324, window.innerWidth - 24);
</script>

<Window
  title="Place component"
  x={Math.max(12, window.innerWidth - width - 24)}
  y={58}
  {width}
  height={238}
  minWidth={260}
  minHeight={220}
  z={1000}
  closeDisabled={applying}
  onclose={onCancel}
>
  <form onsubmit={(event) => { event.preventDefault(); onApply(); }}>
    <strong class="component-name">{label}</strong>
    <div class="coordinates">
      {#each axes as [index, axis] (axis)}
        <label>
          {axis} (mm)
          <input
            type="number"
            step="any"
            bind:value={() => coordinates[index] ?? undefined, (value) =>
              onCoordinateChange(index, typeof value === 'number' && Number.isFinite(value) ? value : null)}
            disabled={applying}
          />
        </label>
      {/each}
    </div>
    <div class="placement-status" role="status" aria-live="polite">
      {#if error}
        <span class="placement-error">{error}</span>
      {:else if preparing}
        Preparing component preview…
      {:else if applying}
        Rendering the new version…
      {:else}
        Click the XY plane or drag the arrows.
      {/if}
    </div>
    <div class="actions">
      <button type="button" onclick={onCancel} disabled={applying}>Cancel</button>
      <button type="submit" class="primary" disabled={!canApply}>{applying ? 'Applying…' : 'Apply'}</button>
    </div>
  </form>
</Window>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    height: 100%;
    min-height: 0;
  }

  .component-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .coordinates {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }

  label {
    display: grid;
    gap: 4px;
    font-size: 0.72rem;
    color: var(--text-dim);
  }

  input, button {
    min-width: 0;
    padding: 5px 7px;
    border: 1px solid var(--bg-400);
    border-radius: 0;
    background: var(--bg-200);
    color: var(--text);
  }

  input:focus-visible, button:focus-visible {
    outline: 1px solid var(--primary);
    outline-offset: 2px;
  }

  .placement-status {
    flex: 1;
    min-height: 32px;
    overflow: auto;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    color: var(--text-dim);
    font-size: 0.75rem;
  }

  .placement-error {
    color: var(--danger);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  button {
    cursor: pointer;
    min-width: 58px;
  }

  button.primary {
    border-color: var(--primary);
    color: var(--primary);
  }

  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
</style>
