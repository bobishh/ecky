<script>
  import Modal from './Modal.svelte';
  import ConfigPanel from './ConfigPanel.svelte';
  let { config = $bindable(), availableModels, isLoadingModels, onsave, onclose } = $props();

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
