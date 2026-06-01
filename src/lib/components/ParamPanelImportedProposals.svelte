<script lang="ts">
  import type { EnrichmentProposal, EnrichmentStatus } from '../types/domain';

  let {
    proposals,
    mutationId = null,
    labelPartIds,
    onUpdateProposalStatus,
  }: {
    proposals: EnrichmentProposal[];
    mutationId?: string | null;
    labelPartIds: (partIds: string[] | undefined) => string;
    onUpdateProposalStatus: (proposalId: string, status: EnrichmentStatus) => void;
  } = $props();
</script>

{#if proposals.length > 0}
  <div class="proposal-section">
    <div class="section-label">BINDING PROPOSALS</div>
    <div class="proposal-list">
      {#each proposals as proposal}
        <div class="proposal-card" class:proposal-card-pending={proposal.status === 'pending'}>
          <div class="proposal-head">
            <div class="proposal-label-row">
              <span class="proposal-label">{proposal.label}</span>
              <span class="proposal-confidence">{Math.round(proposal.confidence * 100)}%</span>
            </div>
            <span class="proposal-status proposal-status-{proposal.status}">
              {proposal.status.toUpperCase()}
            </span>
          </div>
          <div class="proposal-meta">
            PARTS: {labelPartIds(proposal.partIds)}
          </div>
          <div class="proposal-meta">
            PARAMS: {proposal.parameterKeys?.length ? proposal.parameterKeys.join(', ') : 'No parameter keys'}
          </div>
          <div class="proposal-meta">SOURCE: {proposal.provenance}</div>
          <div class="proposal-actions">
            <button
              class="btn btn-xs btn-primary"
              onclick={() => onUpdateProposalStatus(proposal.proposalId, 'accepted')}
              disabled={mutationId !== null || proposal.status === 'accepted'}
            >
              ACCEPT
            </button>
            <button
              class="btn btn-xs btn-ghost"
              onclick={() => onUpdateProposalStatus(proposal.proposalId, 'rejected')}
              disabled={mutationId !== null || proposal.status === 'rejected'}
            >
              REJECT
            </button>
            {#if proposal.status !== 'pending'}
              <button
                class="btn btn-xs btn-ghost"
                onclick={() => onUpdateProposalStatus(proposal.proposalId, 'pending')}
                disabled={mutationId !== null}
              >
                RESET
              </button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .proposal-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .proposal-list {
    display: flex;
    flex-direction: column;
    flex-wrap: wrap;
    gap: 6px;
  }

  .proposal-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
  }

  .proposal-card-pending {
    border-color: color-mix(in srgb, var(--primary) 35%, var(--bg-300));
  }

  .proposal-head {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    align-items: flex-start;
  }

  .proposal-label-row,
  .proposal-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .proposal-label-row {
    align-items: center;
    gap: 8px;
  }

  .proposal-label {
    color: var(--text);
    font-size: 0.74rem;
    font-weight: 700;
  }

  .proposal-confidence,
  .proposal-meta {
    color: var(--text-dim);
    font-size: 0.64rem;
  }

  .proposal-status {
    padding: 3px 6px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text-dim);
    font-size: 0.58rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .proposal-status-pending {
    border-color: color-mix(in srgb, var(--primary) 45%, var(--bg-300));
    color: var(--primary);
  }

  .proposal-status-accepted {
    border-color: color-mix(in srgb, var(--secondary) 45%, var(--bg-300));
    color: var(--secondary);
  }

  .proposal-status-rejected {
    border-color: color-mix(in srgb, var(--text-dim) 45%, var(--bg-300));
    color: var(--text-dim);
  }

  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
</style>
