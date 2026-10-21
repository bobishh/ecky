<script lang="ts">
  // Viewport actions and their prop-driven disabled states.
  // State and handlers belong to the caller.
  //
  // Disabled-state logic mirrors App.svelte's inline action strip verbatim:
  //   fork   -> disabled while the viewport busy mask is up
  //   export -> visible only when an artifact bundle exists (showExport);
  //             disabled when !canExport || busy || hasSketchPreview
  //   code   -> disabled while the viewport busy mask is up
  let {
    onFork,
    onExport,
    onCode,
    busy = false,
    showExport = false,
    canExport = false,
    hasSketchPreview = false,
    showCode = true,
  }: {
    onFork?: () => void;
    onExport?: () => void;
    onCode?: () => void;
    busy?: boolean;
    showExport?: boolean;
    canExport?: boolean;
    hasSketchPreview?: boolean;
    showCode?: boolean;
  } = $props();

  const exportDisabled = $derived(!canExport || busy || hasSketchPreview);
  const exportTitle = $derived(
    hasSketchPreview
      ? 'Sketch preview is diagnostic only. Accepted CAD export needs exact BRep/STEP validation.'
      : 'Open export options',
  );
</script>

<div class="viewport-action-strip">
  <button
    type="button"
    class="btn btn-xs btn-secondary"
    data-viewport-action="fork"
    aria-label="FORK"
    title="Fork this design into a new project"
    disabled={busy}
    onclick={onFork}
  >
    🍴 FORK
  </button>

  {#if showExport}
    <button
      type="button"
      class="btn btn-xs btn-primary"
      data-viewport-action="export"
      aria-label="EXPORT"
      title={exportTitle}
      disabled={exportDisabled}
      onclick={onExport}
    >
      💾 EXPORT
    </button>
  {/if}

  {#if showCode}
    <button
      type="button"
      class="btn btn-xs btn-secondary"
      data-viewport-action="code"
      aria-label="CODE"
      title="Code inspector"
      disabled={busy}
      onclick={onCode}
    >
      📋 CODE
    </button>
  {/if}
</div>

<style>
  /* Layout boundary: keep the action strip from bleeding (AGENTS.md mandate). */
  .viewport-action-strip {
    display: flex;
    gap: 4px;
    align-items: center;
    overflow: hidden;
  }
</style>
