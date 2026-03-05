<script>
  let { options = [], value = $bindable(), placeholder = "Select...", onchange } = $props();
  
  let isOpen = $state(false);
  let container;

  const selectedOption = $derived(options.find(o => o.id === value) || options.find(o => o === value));
  const displayLabel = $derived(
    selectedOption 
      ? (typeof selectedOption === 'object' ? (selectedOption.name || selectedOption.id) : selectedOption)
      : placeholder
  );

  function toggle() {
    isOpen = !isOpen;
  }

  function select(option) {
    const id = typeof option === 'object' ? option.id : option;
    value = id;
    isOpen = false;
    if (onchange) onchange(id);
  }

  // Close when clicking outside
  function handleOutsideClick(e) {
    if (container && !container.contains(e.target)) {
      isOpen = false;
    }
  }
</script>

<svelte:window onclick={handleOutsideClick} />

<div bind:this={container} class="custom-select {isOpen ? 'is-open' : ''}">
  <button type="button" class="select-trigger" onclick={toggle}>
    <span class="select-label">{displayLabel}</span>
    <span class="select-arrow">{isOpen ? '▲' : '▼'}</span>
  </button>

  {#if isOpen}
    <div class="select-dropdown scrollable">
      {#each options as option}
        {@const id = typeof option === 'object' ? option.id : option}
        {@const label = typeof option === 'object' ? (option.name || option.id) : option}
        <button 
          type="button" 
          class="select-option {value === id ? 'is-selected' : ''}"
          onclick={() => select(option)}
        >
          {label}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .custom-select {
    position: relative;
    width: 100%;
    font-family: var(--font-mono);
  }

  .select-trigger {
    width: 100%;
    padding: 8px 12px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    text-align: left;
    display: flex;
    justify-content: space-between;
    align-items: center;
    cursor: pointer;
    font-size: 0.8rem;
    min-height: 36px;
  }

  .custom-select.is-open .select-trigger {
    border-color: var(--primary);
    background: var(--bg-300);
  }

  .select-arrow {
    font-size: 0.5rem;
    color: var(--secondary);
    margin-left: 8px;
  }

  .select-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    background: var(--bg-200);
    border: 1px solid var(--primary);
    box-shadow: 0 8px 16px rgba(0,0,0,0.5);
    z-index: 1000;
    max-height: 300px;
    overflow-y: auto;
    margin-top: -1px;
  }

  .select-option {
    width: 100%;
    padding: 10px 12px;
    background: none;
    border: none;
    border-bottom: 1px solid var(--bg-300);
    color: var(--text);
    text-align: left;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    transition: all 0.1s;
  }

  .select-option:last-child {
    border-bottom: none;
  }

  .select-option:hover {
    background: var(--primary);
    color: #fff;
  }

  .select-option.is-selected {
    background: var(--bg-300);
    color: var(--secondary);
    border-left: 3px solid var(--secondary);
    padding-left: 9px;
  }

  .scrollable::-webkit-scrollbar {
    width: 4px;
  }
  .scrollable::-webkit-scrollbar-thumb {
    background: var(--bg-300);
  }
</style>
