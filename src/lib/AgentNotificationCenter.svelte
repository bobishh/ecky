<script lang="ts">
  import { agentNotificationsStore } from './stores/agentNotifications';
  import { localNotificationActionsStore } from './stores/localNotificationActions';
  import { historyStore } from './stores/domainState';
  import { workingCopy } from './stores/workingCopy';
  import { collapseProjectFolderRenderCards } from './notificationAggregation';
  import { longTasksStore, type LongTask } from './stores/longTasks';
  import { cancelFemStudy, formatBackendError } from './tauri/client';

  type DisplayCard = {
    eventId: string;
    activityEventId: string | null;
    threadId: string | null;
    actorKind: string;
    actorLabel: string;
    summary: string;
    detail: string | null;
    severity: string;
    state: string;
    requiresAttention: boolean;
    local: boolean;
    activityKind: string | null;
    actions: Array<{ label: string; onclick: () => void }>;
  };

  let {
    activeThreadId = null,
    onOpenThreadDialogue = null,
    onOpenActivityEvent = null,
  }: {
    activeThreadId?: string | null;
    onOpenThreadDialogue?: ((threadId: string | null) => Promise<void> | void) | null;
    onOpenActivityEvent?: ((eventId: string, threadId: string | null, local: boolean) => void) | null;
  } = $props();

  let politeAnnouncement = $state('');
  let assertiveAnnouncement = $state('');
  let announcedEventIds: string[] = [];
  let copiedEventId = $state<string | null>(null);
  let copiedResetTimer = $state<number | null>(null);
  let expandedTaskId = $state<string | null>(null);
  let taskActionError = $state<string | null>(null);

  function threadLabel(threadId: string | null): string {
    if (!threadId) return 'ECKY APP';
    const historyTitle = $historyStore.find((thread) => thread.id === threadId)?.title?.trim();
    if (historyTitle) return historyTitle;
    if (threadId === activeThreadId) return $workingCopy.title?.trim() || 'CURRENT PROJECT';
    return 'BACKGROUND PROJECT';
  }

  function actorLabel(card: { actorKind: string; actorLabel: string }): string {
    if (card.actorKind === 'system') return 'RELAY';
    const label = card.actorLabel.trim();
    if (!label || /^(folder[-_ ]?sync|system|ecky)$/i.test(label)) return 'ECKY';
    return label;
  }

  function elapsedLabel(elapsedMs: number): string {
    const seconds = Math.floor(elapsedMs / 1000);
    const minutes = Math.floor(seconds / 60);
    return `${minutes.toString().padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`;
  }

  function progressLabel(task: LongTask): string | null {
    if (!task.progress) return null;
    return `${task.progress.current} / ${task.progress.total}`;
  }

  function toggleTask(taskId: string) {
    expandedTaskId = expandedTaskId === taskId ? null : taskId;
    taskActionError = null;
  }

  async function cancelTask(task: LongTask, event: MouseEvent) {
    event.stopPropagation();
    if (!task.jobId) return;
    taskActionError = null;
    try {
      await cancelFemStudy(task.jobId);
    } catch (error) {
      taskActionError = formatBackendError(error);
    }
  }

  function stateLabel(state: string): string {
    return state.toUpperCase();
  }

  function copyPayload(card: DisplayCard): string {
    const lines = [
      `thread: ${card.threadId ?? 'threadless'}`,
      `actor: ${actorLabel(card)}`,
      `state: ${stateLabel(card.state)}`,
      `summary: ${card.summary}`,
    ];
    if (card.detail) lines.push(`detail: ${card.detail}`);
    return lines.join('\n');
  }

  async function copyCard(card: DisplayCard, event: MouseEvent) {
    event.stopPropagation();
    try {
      await navigator.clipboard.writeText(copyPayload(card));
      copiedEventId = card.eventId;
      if (copiedResetTimer) clearTimeout(copiedResetTimer);
      copiedResetTimer = window.setTimeout(() => {
        copiedEventId = null;
      }, 1200);
    } catch {
      copiedEventId = null;
    }
  }

  function dismissCard(card: DisplayCard, event: MouseEvent) {
    event.stopPropagation();
    removeCard(card);
  }

  function removeCard(card: DisplayCard) {
    if (card.local) localNotificationActionsStore.set(null);
    else agentNotificationsStore.dismiss(card.eventId);
  }

  function traceKind(raw: unknown): string | null {
    let payload = raw;
    if (typeof payload === 'string') {
      try {
        payload = JSON.parse(payload);
      } catch {
        return null;
      }
    }
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return null;
    const kind = (payload as Record<string, unknown>).kind;
    return typeof kind === 'string' ? kind : null;
  }

  function cardDestination(card: DisplayCard): 'activity' | 'dialogue' {
    if (card.severity === 'error' || card.state === 'failed') return 'activity';
    if (card.local && card.activityEventId) return 'activity';
    if (card.local) return 'dialogue';
    if (card.requiresAttention || card.severity === 'question') return 'dialogue';
    if (card.activityKind === 'request_user_prompt' || card.activityKind === 'final_reply_save') {
      return 'dialogue';
    }
    return 'activity';
  }

  function openCard(card: DisplayCard) {
    if (cardDestination(card) === 'activity') {
      removeCard(card);
      onOpenActivityEvent?.(card.activityEventId ?? card.eventId, card.threadId, card.local);
      return;
    }
    removeCard(card);
    void onOpenThreadDialogue?.(card.threadId);
  }

  function cardAriaLabel(card: DisplayCard): string {
    const destination = cardDestination(card) === 'activity'
      ? 'activity details'
      : 'project dialogue';
    return `Open ${destination} for ${threadLabel(card.threadId)}`;
  }

  function runLocalAction(action: { onclick: () => void }, event: MouseEvent) {
    event.stopPropagation();
    action.onclick();
  }

  function setHover(cardId: string | null) {
    agentNotificationsStore.setHoveredEventId(cardId);
  }

  function setFocus(cardId: string | null) {
    agentNotificationsStore.setFocusedEventId(cardId);
  }

  function handleCardKeydown(card: DisplayCard, event: KeyboardEvent) {
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    openCard(card);
  }

  $effect(() => {
    const cards = visibleCards;
    for (const card of cards) {
      if (announcedEventIds.includes(card.eventId)) continue;
      announcedEventIds = [...announcedEventIds, card.eventId];
      const message = `${threadLabel(card.threadId)} ${actorLabel(card)} ${card.state} ${card.summary}`;
      if (card.severity === 'error' || card.severity === 'question') {
        assertiveAnnouncement = message;
      } else {
        politeAnnouncement = message;
      }
    }
  });

  $effect(() => {
    void activeThreadId;
  });

  const visibleCards = $derived.by<DisplayCard[]>(() => {
    const activityCards = $agentNotificationsStore.visibleCards.map((card) => ({
      eventId: card.eventId,
      activityEventId: card.eventId,
      threadId: card.threadId,
      actorKind: card.actorKind,
      actorLabel: card.actorLabel,
      summary: card.summary,
      detail: card.detail,
      severity: card.severity,
      state: card.state,
      requiresAttention: card.requiresAttention,
      local: false,
      activityKind: traceKind(card.sourceEvents.at(-1)?.raw),
      actions: [],
    }));
    const local = $localNotificationActionsStore;
    if (!local) return activityCards;
    const collapsedActivityCards = collapseProjectFolderRenderCards(activityCards, local.threadId);
    if (local.activityEventId && collapsedActivityCards.some((card) => card.eventId === local.activityEventId)) {
      return collapsedActivityCards;
    }
    const localDetail = local.detail?.replace(/\s+/g, ' ').trim() ?? '';
    const duplicatedByActivity = Boolean(localDetail) && collapsedActivityCards.some((card) => {
      const summary = card.summary.replace(/\s+/g, ' ').trim();
      const detail = card.detail?.replace(/\s+/g, ' ').trim() ?? '';
      return summary === localDetail || detail === localDetail;
    });
    if (duplicatedByActivity) return collapsedActivityCards;
    return [...collapsedActivityCards, {
      ...local,
      activityEventId: local.activityEventId ?? null,
      actorKind: 'agent',
      local: true,
      activityKind: null,
    }];
  });

  $effect(() => {
    return () => {
      if (copiedResetTimer) clearTimeout(copiedResetTimer);
    };
  });
</script>

<section class="agent-notification-center" aria-label="Agent notifications">
  <div class="agent-notification-live" role="status" aria-live="polite" aria-atomic="true">
    {politeAnnouncement}
  </div>
  <div class="agent-notification-live" role="alert" aria-live="assertive" aria-atomic="true">
    {assertiveAnnouncement}
  </div>

  {#if $longTasksStore.length > 0}
    <div class="long-task-stack" aria-label="Running long tasks">
      {#each $longTasksStore as task (task.taskId)}
        <div
          class="long-task-bubble"
          class:long-task-bubble--expanded={expandedTaskId === task.taskId}
          data-testid="long-task-bubble"
          data-task-id={task.taskId}
        >
          <button
            class="long-task-bubble__head"
            aria-expanded={expandedTaskId === task.taskId}
            onclick={() => toggleTask(task.taskId)}
          >
            <span class="long-task-bubble__pulse" aria-hidden="true"></span>
            <span class="long-task-bubble__summary">{task.summary}</span>
            <span class="long-task-bubble__stage">{task.stage}</span>
            <span class="long-task-bubble__elapsed">{elapsedLabel(task.elapsedMs)}</span>
          </button>
          {#if task.progress}
            <div class="long-task-bubble__track" aria-label={`Progress ${progressLabel(task)}`}>
              <span style={`width: ${Math.min(100, Math.max(0, task.progress.current / task.progress.total * 100))}%`}></span>
            </div>
          {/if}
          {#if expandedTaskId === task.taskId}
            <div class="long-task-bubble__details" data-testid="long-task-details">
              <div><span>THREAD</span>{threadLabel(task.threadId)}</div>
              <div><span>STAGE</span>{task.stage}</div>
              {#if progressLabel(task)}<div><span>PROGRESS</span>{progressLabel(task)}</div>{/if}
              {#if task.detail}<div><span>DETAIL</span>{task.detail}</div>{/if}
              {#if task.cancellable && task.jobId}
                <button class="long-task-bubble__cancel" aria-label="Cancel FEM job" onclick={(event) => cancelTask(task, event)}>CANCEL</button>
              {/if}
              {#if taskActionError}<div class="long-task-bubble__error">{taskActionError}</div>{/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <div class="agent-notification-stack" aria-live="off">
    {#each visibleCards as card (card.eventId)}
      <div
        class="agent-card"
        class:agent-card--active={card.threadId === activeThreadId}
        class:agent-card--muted={card.threadId !== activeThreadId}
        class:agent-card--success={card.severity === 'success'}
        class:agent-card--warning={card.severity === 'warning'}
        class:agent-card--error={card.severity === 'error' || (card.state === 'failed' && card.severity !== 'success' && card.severity !== 'warning')}
        class:agent-card--resolved={card.state === 'resolved'}
        tabindex="0"
        role="button"
        aria-label={cardAriaLabel(card)}
        data-event-id={card.eventId}
        data-thread-id={card.threadId ?? undefined}
        data-state={card.state}
        data-severity={card.severity}
        onmouseenter={() => setHover(card.eventId)}
        onmouseleave={() => setHover(null)}
        onfocusin={() => setFocus(card.eventId)}
        onfocusout={(event) => {
          if (!event.currentTarget || !(event.currentTarget instanceof HTMLElement)) return;
          if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
          setFocus(null);
        }}
        onclick={() => openCard(card)}
        onkeydown={(event) => handleCardKeydown(card, event)}
        >
        <div class="agent-card__header">
          <span class="agent-card__thread">{threadLabel(card.threadId)}</span>
          <span class="agent-card__severity" data-severity={card.state === 'resolved' ? 'resolved' : card.severity}>
            {card.state === 'resolved' ? 'RESOLVED' : card.severity}
          </span>
          {#if actorLabel(card) !== 'ECKY'}
            <span class="agent-card__actor">via {actorLabel(card)}</span>
          {/if}
        </div>
        <div class="agent-card__summary">{card.summary}</div>
        {#if card.detail}
          <div class="agent-card__detail">{card.detail}</div>
        {/if}
        <div class="agent-card__footer">
          {#if card.local}
            {#each card.actions as action (action.label)}
              <button class="agent-card__action" onclick={(event) => runLocalAction(action, event)}>
                {action.label}
              </button>
            {/each}
          {/if}
          <button class="agent-card__action" onclick={(event) => copyCard(card, event)}>
            {copiedEventId === card.eventId ? 'COPIED' : 'COPY'}
          </button>
          <button class="agent-card__action" onclick={(event) => dismissCard(card, event)}>
            DISMISS
          </button>
        </div>
      </div>
    {/each}
  </div>
</section>

<style>
  .agent-notification-center {
    position: absolute;
    left: 126px;
    top: 6px;
    z-index: 1;
    width: min(380px, max(248px, calc(100vw - 558px)));
    max-height: min(70vh, 620px);
    overflow: hidden;
    pointer-events: none;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .agent-notification-live {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .agent-notification-stack {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: hidden;
  }

  .long-task-stack {
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
    pointer-events: auto;
  }

  .long-task-bubble {
    width: 100%;
    overflow: hidden;
    border: 1px solid var(--primary);
    background: color-mix(in srgb, var(--bg-100) 94%, transparent);
    color: var(--text);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--primary) 18%, transparent), var(--shadow);
    font-family: var(--font-mono);
    cursor: pointer;
  }

  .long-task-bubble:focus-visible {
    outline: 2px solid var(--secondary);
    outline-offset: 2px;
  }

  .long-task-bubble__head {
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 8px;
    min-height: 34px;
    padding: 6px 9px;
    overflow: hidden;
    width: 100%;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .long-task-bubble__pulse {
    width: 6px;
    height: 6px;
    background: var(--secondary);
    box-shadow: 0 0 8px var(--secondary);
  }

  .long-task-bubble__summary {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.65rem;
    font-weight: 700;
  }

  .long-task-bubble__stage,
  .long-task-bubble__elapsed {
    color: var(--primary);
    font-size: 0.58rem;
    letter-spacing: 0.06em;
  }

  .long-task-bubble__track {
    height: 2px;
    overflow: hidden;
    background: var(--bg-300);
  }

  .long-task-bubble__track span {
    display: block;
    height: 100%;
    background: var(--secondary);
  }

  .long-task-bubble__details {
    display: grid;
    gap: 6px;
    padding: 9px;
    border-top: 1px solid var(--bg-300);
    overflow: hidden;
    font-size: 0.62rem;
  }

  .long-task-bubble__details > div {
    display: grid;
    grid-template-columns: 70px minmax(0, 1fr);
    gap: 8px;
    overflow: hidden;
  }

  .long-task-bubble__details span {
    color: var(--text-dim);
  }

  .long-task-bubble__cancel {
    justify-self: end;
    border: 1px solid var(--red);
    background: transparent;
    color: var(--red);
    padding: 4px 8px;
    font: inherit;
    cursor: pointer;
  }

  .long-task-bubble__error {
    color: var(--red);
  }

  @media (max-width: 960px) {
    .agent-notification-center {
      left: 14px;
      top: 126px;
      width: min(calc(100vw - 28px), 320px);
      max-height: min(62vh, 520px);
    }
  }

  .agent-card {
    --agent-card-tone: color-mix(in srgb, var(--primary) 42%, var(--bg-300));
    position: relative;
    pointer-events: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
    min-height: 74px;
    border: 2px solid var(--agent-card-tone);
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    color: var(--text);
    padding: 12px 104px 12px 14px;
    border-radius: 0;
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--bg-300) 85%, transparent), var(--shadow);
    backdrop-filter: blur(9px);
    text-align: left;
    cursor: pointer;
    width: 100%;
    font-family: var(--font-mono);
  }

  .agent-card--active {
    box-shadow:
      inset 4px 0 0 color-mix(in srgb, var(--primary) 70%, var(--bg-300)),
      0 0 0 2px color-mix(in srgb, var(--bg-300) 85%, transparent),
      var(--shadow);
  }

  .agent-card--muted {
    opacity: 0.72;
    color: var(--text-dim);
  }

  .agent-card--error {
    --agent-card-tone: color-mix(in srgb, var(--red) 72%, var(--bg-300));
  }

  .agent-card--warning {
    --agent-card-tone: color-mix(in srgb, var(--secondary) 72%, var(--bg-300));
  }

  .agent-card--success {
    --agent-card-tone: color-mix(in srgb, var(--green) 72%, var(--bg-300));
  }

  .agent-card--resolved {
    opacity: 0.88;
  }

  .agent-card:first-child::before {
    content: '';
    position: absolute;
    left: -12px;
    top: 26px;
    width: 12px;
    height: 20px;
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    border-left: 2px solid var(--agent-card-tone);
    border-top: 2px solid var(--agent-card-tone);
    border-bottom: 2px solid var(--agent-card-tone);
  }

  .agent-card:focus-visible {
    outline: 2px solid var(--secondary);
    outline-offset: 2px;
  }

  .agent-card__header {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    align-items: center;
    margin-bottom: 2px;
    font-size: 0.58rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .agent-card__thread,
  .agent-card__actor,
  .agent-card__severity {
    padding: 2px 6px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 72%, transparent);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .agent-card__thread {
    color: var(--secondary);
    border-color: color-mix(in srgb, var(--secondary) 54%, var(--bg-300));
    max-width: 190px;
  }

  .agent-card__actor {
    color: var(--text-dim);
  }

  .agent-card__severity {
    color: var(--primary);
  }

  .agent-card__severity[data-severity='warning'] { color: var(--secondary); }
  .agent-card__severity[data-severity='error'] { color: var(--red); }
  .agent-card__severity[data-severity='resolved'] { color: var(--primary); }

  .agent-card__summary {
    font-size: 0.74rem;
    line-height: 1.42;
    font-weight: 400;
    max-height: 12em;
    overflow: auto;
    overflow-wrap: anywhere;
  }

  .agent-card__detail {
    font-size: 0.68rem;
    line-height: 1.38;
    color: var(--text-dim);
    max-height: 9em;
    overflow: auto;
    overflow-wrap: anywhere;
  }

  .agent-card__footer {
    position: absolute;
    top: 8px;
    right: 10px;
    display: flex;
    justify-content: flex-end;
    gap: 5px;
  }

  .agent-card__action {
    pointer-events: auto;
    min-width: 38px;
    height: 20px;
    border: 2px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg) 78%, transparent);
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.54rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    padding: 0 5px;
    cursor: pointer;
  }

  .agent-card__action:hover {
    border-color: var(--primary);
    color: var(--primary);
  }

  .agent-card__action:focus-visible {
    outline: 2px solid var(--secondary);
    outline-offset: 2px;
  }
</style>
