<script lang="ts">
  import Modal from './Modal.svelte';
  import ConfigPanel from './ConfigPanel.svelte';

  import type { AppConfig } from './types/domain';

  let {
    config = $bindable(),
    availableModels = [],
    isLoadingModels = false,
    onsave,
    onclose,
  }: {
    config: AppConfig;
    availableModels?: string[];
    isLoadingModels?: boolean;
    onsave: () => Promise<void> | void;
    onclose: () => void;
  } = $props();

  async function handleSave() {
    await onsave();
    onclose();
  }
</script>

<Modal title="CONFIGURATION" {onclose}>
  <div class="config-modal-content">
    <ConfigPanel 
      bind:config 
      {availableModels} 
      {isLoadingModels} 
      onsave={handleSave} 
    />
  </div>
</Modal>

<style>
  .config-modal-content {
    width: 600px;
    height: 60vh;
    padding: 20px;
    background: var(--bg);
    overflow-y: auto;
  }
</style>
