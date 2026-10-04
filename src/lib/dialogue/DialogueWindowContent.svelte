<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import PromptPanel from '../PromptPanel.svelte';

  type PromptPanelProps = ComponentProps<typeof PromptPanel>;

  let {
    activeThreadId,
    promptProps,
    activeVersionId = $bindable(null),
  }: {
    activeThreadId: string | null;
    promptProps: PromptPanelProps;
    activeVersionId?: string | null;
  } = $props();
</script>

<div class="dialogue-content">
  {#key activeThreadId ?? 'new-thread'}
    <PromptPanel {...promptProps} bind:activeVersionId />
  {/key}
</div>

<style>
  .dialogue-content { flex: 1; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
</style>
