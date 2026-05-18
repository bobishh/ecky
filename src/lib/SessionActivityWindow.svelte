<script lang="ts">
  import type { SessionEvent, SessionEventArtifact, SessionEventDiff } from './sessionActivity';

  let {
    events = [],
    selectedEventId = null,
    onSelectEvent,
  }: {
    events?: SessionEvent[];
    selectedEventId?: string | null;
    onSelectEvent?: (id: string) => void;
  } = $props();

  const selectedEvent = $derived.by<SessionEvent | null>(() => {
    if (events.length === 0) return null;
    return (
      events.find((event) => event.id === selectedEventId) ??
      events[events.length - 1] ??
      null
    );
  });

  function actorLabel(event: SessionEvent): string {
    if (event.actor.kind === 'agent') return event.actor.label || event.actor.id;
    return event.actor.kind.toUpperCase();
  }

  function eventTime(event: SessionEvent): string {
    if (!event.timestamp) return '';
    const date = new Date(event.timestamp);
    if (Number.isNaN(date.getTime())) return '';
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function formatRaw(value: unknown): string {
    if (typeof value === 'string') return value;
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }

  function diffLabel(diff: SessionEventDiff): string {
    return diff.label ?? diff.path ?? diff.key ?? diff.kind.toUpperCase();
  }

  function artifactLabel(artifact: SessionEventArtifact): string {
    return artifact.label || artifact.kind.toUpperCase();
  }
</script>

<div class="activity-window">
  <div class="activity-list" data-testid="activity-event-list">
    {#if events.length === 0}
      <div class="activity-empty">NO SESSION EVENTS</div>
    {:else}
      {#each [...events].reverse() as event (event.id)}
        <button
          type="button"
          class="activity-event"
          class:activity-event--selected={event.id === selectedEvent?.id}
          data-severity={event.severity}
          onclick={() => onSelectEvent?.(event.id)}
        >
          <span class="activity-event__meta">
            <span>{eventTime(event)}</span>
            <span>{actorLabel(event)}</span>
          </span>
          <span class="activity-event__title">{event.title}</span>
          <span class="activity-event__summary">{event.summary}</span>
        </button>
      {/each}
    {/if}
  </div>

  <div class="activity-detail" data-testid="activity-event-detail">
    {#if selectedEvent}
      <div class="activity-detail__head">
        <div>
          <div class="activity-detail__eyebrow">{selectedEvent.kind.replace(/_/g, ' ')}</div>
          <h3>{selectedEvent.title}</h3>
        </div>
        <span class="activity-detail__severity" data-severity={selectedEvent.severity}>
          {selectedEvent.severity}
        </span>
      </div>

      <pre class="activity-summary">{selectedEvent.summary}</pre>

      {#if selectedEvent.diffs?.length}
        <section class="activity-section">
          <h4>DIFFS</h4>
          {#each selectedEvent.diffs as diff}
            <div class="activity-diff">
              <div class="activity-section__label">{diffLabel(diff)}</div>
              <div class="activity-diff__grid">
                <pre>{diff.before ?? ''}</pre>
                <pre>{diff.after ?? ''}</pre>
              </div>
            </div>
          {/each}
        </section>
      {/if}

      {#if selectedEvent.artifacts?.length}
        <section class="activity-section" data-testid="session-preview-detail">
          <h4>ARTIFACTS</h4>
          {#each selectedEvent.artifacts as artifact}
            <div class="activity-artifact">
              <div class="activity-section__label">{artifactLabel(artifact)}</div>
              {#if artifact.kind === 'preview_image' && artifact.value}
                <img src={artifact.value} alt={artifactLabel(artifact)} />
              {:else if artifact.href}
                <a href={artifact.href}>{artifact.value || artifact.href}</a>
              {:else}
                <pre>{artifact.value}</pre>
              {/if}
              {#if artifact.raw !== undefined}
                <pre class="activity-raw">{formatRaw(artifact.raw)}</pre>
              {/if}
            </div>
          {/each}
        </section>
      {/if}

      {#if selectedEvent.raw !== undefined}
        <section class="activity-section">
          <h4>RAW</h4>
          <pre class="activity-raw">{formatRaw(selectedEvent.raw)}</pre>
        </section>
      {/if}
    {:else}
      <div class="activity-empty">SELECT EVENT</div>
    {/if}
  </div>
</div>

<style>
  .activity-window {
    width: 100%;
    height: 100%;
    display: grid;
    grid-template-columns: minmax(190px, 0.38fr) minmax(0, 1fr);
    min-height: 0;
    overflow: hidden;
    background: color-mix(in srgb, var(--bg) 86%, transparent);
    color: var(--text);
    font-family: var(--font-mono);
  }

  .activity-list {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    border-right: 1px solid color-mix(in srgb, var(--primary) 38%, var(--bg-300));
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
  }

  .activity-event {
    width: 100%;
    display: grid;
    gap: 5px;
    padding: 10px 12px;
    border: 0;
    border-bottom: 1px solid var(--bg-300);
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .activity-event:hover,
  .activity-event--selected {
    background: color-mix(in srgb, var(--primary) 14%, var(--bg-200));
  }

  .activity-event__meta {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    color: var(--text-muted);
    font-size: 0.68rem;
  }

  .activity-event__title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 800;
    text-transform: uppercase;
  }

  .activity-event__summary {
    display: -webkit-box;
    overflow: hidden;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    color: var(--text-muted);
    font-size: 0.75rem;
    line-height: 1.35;
  }

  .activity-detail {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 16px;
  }

  .activity-detail__head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    align-items: flex-start;
    margin-bottom: 12px;
  }

  .activity-detail__eyebrow,
  .activity-detail__severity,
  .activity-section__label {
    color: var(--secondary);
    font-size: 0.7rem;
    text-transform: uppercase;
  }

  .activity-detail h3,
  .activity-section h4 {
    margin: 3px 0 0;
    font-size: 1rem;
    letter-spacing: 0;
  }

  .activity-summary,
  .activity-raw,
  .activity-diff pre,
  .activity-artifact pre {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    line-height: 1.45;
  }

  .activity-summary {
    padding: 12px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 76%, transparent);
  }

  .activity-section {
    margin-top: 16px;
    display: grid;
    gap: 10px;
  }

  .activity-diff,
  .activity-artifact {
    display: grid;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 66%, transparent);
    overflow: hidden;
  }

  .activity-diff__grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 8px;
  }

  .activity-diff pre,
  .activity-raw,
  .activity-artifact pre {
    overflow: auto;
    padding: 8px;
    background: var(--bg);
    border: 1px solid var(--bg-300);
  }

  .activity-artifact img {
    max-width: 100%;
    max-height: 260px;
    object-fit: contain;
    border: 1px solid var(--bg-300);
    background: var(--bg);
  }

  .activity-empty {
    padding: 18px;
    color: var(--text-muted);
    font-weight: 800;
    text-align: center;
  }
</style>
