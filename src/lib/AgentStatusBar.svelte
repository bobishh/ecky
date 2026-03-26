<script lang="ts">
  import { phaseLabelForThreadAgentState } from './agents/state';
  import type { ThreadAgentState } from './tauri/client';

  let { state }: { state: ThreadAgentState | null } = $props();

  function agentLabel(s: ThreadAgentState): string {
    const base = s.agentLabel || 'Agent';
    return s.llmModelLabel ? `${base} · ${s.llmModelLabel}` : base;
  }

</script>

{#if state && state.connectionState !== 'none'}
  <div
    class="agent-status-bar"
    class:state-sleeping={state.connectionState === 'sleeping'}
    class:state-waking={state.connectionState === 'waking'}
    class:state-active={state.connectionState === 'active'}
    class:state-waiting={state.connectionState === 'waiting'}
    class:state-disconnected={state.connectionState === 'disconnected'}
    class:state-error={state.connectionState === 'error'}
  >
    {#if state.connectionState === 'sleeping'}
      <span class="dot dot-sleeping" aria-hidden="true">◌</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">{phaseLabelForThreadAgentState(state)}</span>
    {:else if state.connectionState === 'waking'}
      <span class="dot dot-waking" aria-hidden="true">◎</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">{phaseLabelForThreadAgentState(state)}</span>
    {:else if state.connectionState === 'active'}
      <span class="dot dot-active" aria-hidden="true">●</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">{phaseLabelForThreadAgentState(state)}</span>
    {:else if state.connectionState === 'waiting'}
      <span class="dot dot-waiting" aria-hidden="true">◌</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">{phaseLabelForThreadAgentState(state)}</span>
    {:else if state.connectionState === 'error'}
      <span class="dot dot-disconnected" aria-hidden="true">!</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">{phaseLabelForThreadAgentState(state)}</span>
    {:else}
      <span class="dot dot-disconnected" aria-hidden="true">✕</span>
      <span class="bar-label">{agentLabel(state)}</span>
      <span class="bar-phase">{phaseLabelForThreadAgentState(state)}</span>
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

  .state-sleeping .dot-sleeping,
  .state-waking .dot-waking {
    color: var(--secondary);
  }

  .state-sleeping .bar-label,
  .state-waking .bar-label {
    color: var(--secondary);
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

  .state-error .dot-disconnected,
  .state-error .bar-label,
  .state-error .bar-phase {
    color: var(--red, #ff6b6b);
  }

  @keyframes dot-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }
</style>
