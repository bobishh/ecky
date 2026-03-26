<script lang="ts">
  type DropdownPrimitive = string | number;
  type DropdownOptionObject = {
    id?: DropdownPrimitive;
    value?: DropdownPrimitive;
    name?: string;
    label?: string;
  };
  type DropdownOption = DropdownPrimitive | DropdownOptionObject;

  let {
    options = [],
    value = $bindable<DropdownPrimitive | null | undefined>(undefined),
    placeholder = "Select...",
    onchange,
    disabled = false,
  }: {
    options?: DropdownOption[];
    value?: DropdownPrimitive | null;
    placeholder?: string;
    onchange?: (value: DropdownPrimitive | undefined) => void;
    disabled?: boolean;
  } = $props();
  
  let isOpen = $state(false);
  let container: HTMLDivElement | undefined;

  function getOptionId(option: DropdownOption): DropdownPrimitive | undefined {
    return typeof option === 'object' ? (option.id ?? option.value) : option;
  }

  function getOptionLabel(option: DropdownOption): string {
    if (typeof option === 'object') {
      return String(option.name || option.label || option.id || option.value || '');
    }
    return String(option);
  }

  const selectedOption = $derived(
    options.find((option) => getOptionId(option) === value) ||
    options.find(o => o === value)
  );
  const displayLabel = $derived(
    selectedOption 
      ? getOptionLabel(selectedOption)
      : placeholder
  );

  function toggle() {
    if (disabled) return;
    isOpen = !isOpen;
  }

  function select(option: DropdownOption) {
    if (disabled) return;
    const id = getOptionId(option);
    value = id;
    isOpen = false;
    if (onchange) onchange(id);
  }

  // Close when clicking outside
  function handleOutsideClick(e: MouseEvent) {
    if (container && e.target instanceof Node && !container.contains(e.target)) {
      isOpen = false;
    }
  }
</script>

<svelte:window onclick={handleOutsideClick} />

<div bind:this={container} class="custom-select {isOpen ? 'is-open' : ''} {disabled ? 'is-disabled' : ''}">
  <button type="button" class="select-trigger" onclick={toggle} disabled={disabled}>
    <span class="select-label">{displayLabel}</span>
    <span class="select-arrow">{isOpen ? '▲' : '▼'}</span>
  </button>

  {#if isOpen}
    <div class="select-dropdown scrollable">
      {#each options as option}
        {@const id = getOptionId(option)}
        {@const label = getOptionLabel(option)}
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
    display: flex;
    flex-direction: column;
    min-width: 0;
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

  .custom-select.is-disabled .select-trigger {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .select-arrow {
    font-size: 0.5rem;
    color: var(--secondary);
    margin-left: 8px;
  }

  .select-dropdown {
    position: relative;
    width: 100%;
    margin-top: -1px;
    background: var(--bg-200);
    border: 1px solid var(--primary);
    box-shadow: 0 8px 16px rgba(0,0,0,0.5);
    z-index: 20;
    max-height: 240px;
    overflow-y: auto;
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

</style>
