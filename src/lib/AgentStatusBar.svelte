<script lang="ts">
  import type { ThreadAgentState } from './tauri/client';

  let { state }: { state: ThreadAgentState | null } = $props();

  function agentLabel(s: ThreadAgentState): string {
    const base = s.agentLabel || 'Agent';
    return s.llmModelLabel ? `${base} · ${s.llmModelLabel}` : base;
  }

  function phaseLabel(s: ThreadAgentState): string {
    if (s.statusText?.trim()) return s.statusText;
    switch (s.phase) {
      case 'rendering':         return 'rendering model...';
      case 'restoring_version': return 'restoring version...';
      case 'saving_version':    return 'saving version...';
      case 'patching_params':   return 'tuning parameters...';
      case 'patching_macro':    return 'editing macro...';
      case 'reading':           return 'reading thread...';
      case 'resolving':         return 'resolving...';
      case 'error':             return 'error';
      default:                  return '...';
    }
  }
</script>

{#if state && state.connectionState !== 'none'}
  <div
    class="agent-status-bar"
    class:state-active={state.connectionState === 'active'}
    class:state-waiting={state.connectionState === 'waiting'}
    class:state-disconnected={state.connectionState === 'disconnected'}
  >
    {#if state.connectionState === 'active'}
      <span class="dot dot-active" aria-hidden="true">●</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">{phaseLabel(state)}</span>
    {:else if state.connectionState === 'waiting'}
      <span class="dot dot-waiting" aria-hidden="true">◌</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">waiting for agent...</span>
    {:else}
      <span class="dot dot-disconnected" aria-hidden="true">✕</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">disconnected</span>
    {/if}
  </div>
{/if}

<style>
  .agent-status-bar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 28px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border-bottom: 1px solid var(--bg-300);
    font-size: 0.65rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    z-index: 10;
    pointer-events: none;
    overflow: hidden;
    white-space: nowrap;
  }

  .dot {
    font-size: 0.6rem;
    flex-shrink: 0;
  }

  .bar-label {
    color: var(--text);
    text-transform: uppercase;
    flex-shrink: 0;
  }

  .bar-phase {
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Active — pulsing dot */
  .state-active .dot-active {
    color: var(--primary);
    animation: dot-pulse 1.5s ease-in-out infinite;
  }

  .state-active .bar-label {
    color: var(--primary);
  }

  /* Waiting — dim */
  .state-waiting .dot-waiting {
    color: var(--text-dim);
  }

  .state-waiting .bar-label {
    color: var(--text-dim);
  }

  /* Disconnected — warning color */
  .state-disconnected .dot-disconnected {
    color: var(--warning, #f59e0b);
  }

  .state-disconnected .bar-label {
    color: var(--warning, #f59e0b);
  }

  .state-disconnected .bar-phase {
    color: var(--warning, #f59e0b);
  }

  @keyframes dot-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }
</style>
