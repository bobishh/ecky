<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Modal from './Modal.svelte';
  import type { AgentOrigin, Attachment, Message, UsageSummary } from './types/domain';

  type TauriBridgeWindow = Window & typeof globalThis & {
    __TAURI_INTERNALS__?: {
      metadata?: object;
    };
  };

  type CodeVersionMessage = Message & {
    output: NonNullable<Message['output']>;
  };

  type VersionMessage = Message & {
    output?: Message['output'];
    artifactBundle?: Message['artifactBundle'];
  };

  type DialogueState =
    | { mode: 'generate' }
    | { mode: 'mcp-idle' }
    | { mode: 'agent-reply'; requestId: string; agentLabel: string };

  type QueuedMessage = { id: string; text: string; status: 'queued' | 'delivered' };

  let {
    onGenerate,
    isGenerating = false,
    freecadMissing = false,
    dialogueState = { mode: 'generate' } as DialogueState,
    queuedMessages = [] as QueuedMessage[],
    messages = [],
    onShowCode,
    activeThreadId = null,
    activeVersionId = $bindable(null),
    onVersionChange,
    onDeleteVersion,
  }: {
    onGenerate: (prompt: string, attachments: Attachment[]) => Promise<unknown>;
    isGenerating?: boolean;
    freecadMissing?: boolean;
    dialogueState?: DialogueState;
    queuedMessages?: QueuedMessage[];
    messages?: Message[];
    onShowCode: (message: CodeVersionMessage) => void;
    activeThreadId?: string | null;
    activeVersionId?: string | null;
    onVersionChange?: (message: VersionMessage) => void;
    onDeleteVersion?: (messageId: string) => void;
  } = $props();

  const PROMPT_DRAFTS_STORAGE_KEY = 'ecky:prompt-drafts:v1';
  const NEW_THREAD_DRAFT_KEY = '__new__';
  const PROMPT_DRAFT_DEBOUNCE_MS = 400;

  let prompt = $state('');
  let attachments = $state<Attachment[]>([]);
  let isDragging = $state(false);
  let showDeleteConfirm = $state(false);
  let draftScopeKey = $state<string | null>(null);
  let draftPersistTimer: number | null = null;
  let pendingDraftWrite = $state<{ scopeKey: string; prompt: string } | null>(null);
  const hasImageAttachments = $derived(attachments.some((attachment) => attachment.type === 'image'));

  function currentDraftScopeKey(threadId: string | null | undefined) {
    const normalized = `${threadId ?? ''}`.trim();
    return normalized || NEW_THREAD_DRAFT_KEY;
  }

  function readPromptDrafts(): Record<string, string> {
    if (typeof localStorage === 'undefined') return {};
    try {
      const raw = localStorage.getItem(PROMPT_DRAFTS_STORAGE_KEY);
      if (!raw) return {};
      const parsed = JSON.parse(raw);
      return parsed && typeof parsed === 'object' ? parsed : {};
    } catch {
      return {};
    }
  }

  function writePromptDrafts(nextDrafts: Record<string, string>) {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(PROMPT_DRAFTS_STORAGE_KEY, JSON.stringify(nextDrafts));
    } catch (error) {
      console.warn('Failed to persist prompt drafts:', error);
    }
  }

  function persistPromptDraftNow(scopeKey: string, nextPrompt: string) {
    const drafts = readPromptDrafts();
    const trimmedValue = `${nextPrompt ?? ''}`;
    if (trimmedValue.trim()) {
      drafts[scopeKey] = trimmedValue;
    } else {
      delete drafts[scopeKey];
    }
    writePromptDrafts(drafts);
  }

  function flushPendingPromptDraft() {
    if (draftPersistTimer) {
      clearTimeout(draftPersistTimer);
      draftPersistTimer = null;
    }
    if (!pendingDraftWrite) return;
    persistPromptDraftNow(pendingDraftWrite.scopeKey, pendingDraftWrite.prompt);
    pendingDraftWrite = null;
  }

  function schedulePromptDraftPersist(scopeKey: string, nextPrompt: string) {
    pendingDraftWrite = { scopeKey, prompt: nextPrompt };
    if (draftPersistTimer) clearTimeout(draftPersistTimer);
    draftPersistTimer = window.setTimeout(() => {
      flushPendingPromptDraft();
    }, PROMPT_DRAFT_DEBOUNCE_MS);
  }

  function loadPromptDraft(scopeKey: string) {
    const drafts = readPromptDrafts();
    prompt = drafts[scopeKey] ?? '';
    draftScopeKey = scopeKey;
  }

  function handlePromptInput(e: Event) {
    prompt = (e.currentTarget as HTMLTextAreaElement).value;
    schedulePromptDraftPersist(currentDraftScopeKey(activeThreadId), prompt);
  }

  function isVersionMessage(message: Message): message is VersionMessage {
    return message.role === 'assistant' && !!(message.output || message.artifactBundle);
  }

  $effect(() => {
    const nextScopeKey = currentDraftScopeKey(activeThreadId);
    if (nextScopeKey === draftScopeKey) return;
    flushPendingPromptDraft();
    loadPromptDraft(nextScopeKey);
  });

  function versionTitle(message: VersionMessage | null | undefined) {
    if (!message) return 'this version';
    return (
      message.output?.title ||
      message.modelManifest?.document?.documentLabel ||
      message.modelManifest?.document?.documentName ||
      message.artifactBundle?.modelId ||
      'Imported Model'
    );
  }

  function formatAgentOrigin(origin: AgentOrigin | null | undefined) {
    if (!origin) return null;
    const host = origin.hostLabel?.trim() || origin.agentLabel?.trim() || 'Agent';
    const model = origin.llmModelLabel?.trim() || origin.llmModelId?.trim() || '';
    if (!model || model.toLowerCase() === host.toLowerCase()) {
      return host;
    }
    return `${host} · ${model}`;
  }

  function processPaths(paths: string[]) {
    const newAttachments = paths.map((path) => {
      const name = path.split(/[\/\\]/).pop() || path;
      const ext = (name.split('.').pop() || '').toLowerCase();
      return {
        path,
        name,
        explanation: '',
        type: ['png', 'jpg', 'jpeg'].includes(ext) ? 'image' : 'cad'
      };
    });
    attachments = [...attachments, ...newAttachments];
  }

  onMount(() => {
    let unlisten: (() => void) | null = null;
    const tauriBridge = typeof window !== 'undefined' ? (window as TauriBridgeWindow).__TAURI_INTERNALS__ : null;
    const hasTauriWindow = tauriBridge && typeof tauriBridge.metadata === 'object';
    if (!hasTauriWindow) {
      return () => {};
    }
    // 1. Native Tauri Drag & Drop (for absolute paths)
    try {
      getCurrentWindow()
        .onDragDropEvent((event) => {
          if (event.payload.type === 'enter' || event.payload.type === 'over') {
            isDragging = true;
          } else if (event.payload.type === 'drop') {
            isDragging = false;
            processPaths(event.payload.paths);
          } else if (event.payload.type === 'leave') {
            isDragging = false;
          }
        })
        .then((cleanup) => {
          unlisten = cleanup;
        })
        .catch((e: unknown) => {
          console.error('Failed to wire Tauri drag-drop listener:', e);
        });
    } catch (e: unknown) {
      console.warn('Tauri drag-drop bridge unavailable:', e);
    }

    return () => {
      flushPendingPromptDraft();
      unlisten?.();
    };
  });

  // 2. Web Drag & Drop Fallback (mainly for E2E testing in browser environments)
  function handleWebDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleWebDragLeave() {
    isDragging = false;
  }

  function handleWebDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    
    // In a real browser, we don't get absolute paths, but for E2E tests 
    // we can simulate the 'paths' if needed or just test the UI reaction.
    if (e.dataTransfer && e.dataTransfer.files.length > 0) {
      const files = Array.from(e.dataTransfer.files);
      const mockPaths = files.map((file) => file.name); // Fallback to names
      processPaths(mockPaths);
    }
  }

  // Extract versions (pairs of user prompt + assistant output)
  const versions = $derived(messages.filter(isVersionMessage));
  
  const currentVersionIndex = $derived(versions.findIndex(v => v.id === activeVersionId));
  const hasPrev = $derived(currentVersionIndex > 0);
  const hasNext = $derived(currentVersionIndex >= 0 && currentVersionIndex < versions.length - 1);

  let isSubmitting = $state(false);

  async function submit() {
    if (!isGenerating && !isSubmitting && (prompt.trim() || attachments.length > 0)) {
      isSubmitting = true;
      try {
        const currentPrompt = prompt;
        const currentAttachments = [...attachments];
        const scopeKey = currentDraftScopeKey(activeThreadId);
        
        prompt = '';
        attachments = [];
        pendingDraftWrite = null;
        if (draftPersistTimer) {
          clearTimeout(draftPersistTimer);
          draftPersistTimer = null;
        }
        persistPromptDraftNow(scopeKey, '');
        
        await onGenerate(currentPrompt, currentAttachments);
      } catch (error) {
        console.error('Failed to submit prompt:', error);
      } finally {
        isSubmitting = false;
      }
    }
  }

  async function retryFailedPrompt() {
    if (!failedPromptForRetry || isGenerating || isSubmitting) return;
    isSubmitting = true;
    try {
      await onGenerate(failedPromptForRetry, []);
    } catch (error) {
      console.error('Failed to retry prompt:', error);
    } finally {
      isSubmitting = false;
    }
  }

  async function addAttachment() {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          { name: 'Images, CAD & Macros', extensions: ['png', 'jpg', 'jpeg', 'stl', 'step', 'stp', 'py', 'fcmacro'] }
        ]
      });

      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        const newAttachments = paths.map((path) => {
          const name = path.split(/[\/\\]/).pop() || path;
          const ext = (name.split('.').pop() || '').toLowerCase();
          return {
            path,
            name,
            explanation: '',
            type: ['png', 'jpg', 'jpeg'].includes(ext) ? 'image' : 'cad'
          };
        });
        attachments = [...attachments, ...newAttachments];
      }
    } catch (e: unknown) {
      console.error('Failed to open file dialog:', e);
    }
  }

  function removeAttachment(index: number) {
    attachments = attachments.filter((_, i) => i !== index);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && e.metaKey) {
      submit();
    }
  }

  function goPrev() {
    if (hasPrev && onVersionChange) onVersionChange(versions[currentVersionIndex - 1]);
  }

  function goNext() {
    if (hasNext && onVersionChange) onVersionChange(versions[currentVersionIndex + 1]);
  }

  function executeDelete() {
    if (onDeleteVersion && currentVersion) {
      onDeleteVersion(currentVersion.id);
      showDeleteConfirm = false;
    }
  }

  function showCode(message: VersionMessage | null) {
    if (message?.output) {
      onShowCode(message as CodeVersionMessage);
    }
  }

  const currentVersion = $derived(currentVersionIndex >= 0 ? versions[currentVersionIndex] : null);
  const promptTrail = $derived.by(() => {
    if (!currentVersion) return [];
    const isLatest = currentVersion.id === versions[versions.length - 1]?.id;
    const base = isLatest ? messages : messages.filter(m => m.timestamp <= currentVersion.timestamp);
    // Filter out standalone error messages (failed attempts with no output) — they pollute the trail
    return base.filter(m => !(m.role === 'assistant' && m.status === 'error' && !m.output));
  });
  const currentUserMsg = $derived.by(() => {
    const userMsgs = promptTrail.filter(m => m.role === 'user');
    return userMsgs.length > 0 ? userMsgs[userMsgs.length - 1] : null;
  });

  let detailsOpen = $state(false);
  let trailListEl = $state<HTMLDivElement | null>(null);
  let copiedTrailMessageId = $state<string | null>(null);
  let copiedTrailTimer = $state<number | null>(null);

  // Auto-scroll to bottom when opened or messages change
  $effect(() => {
    if (detailsOpen && trailListEl) {
      // Use requestAnimationFrame or setTimeout to ensure DOM is updated
      requestAnimationFrame(() => {
        if (trailListEl) trailListEl.scrollTop = trailListEl.scrollHeight;
      });
    }
  });

  // Also watch for message changes while the panel is open
  $effect(() => {
    if (detailsOpen && messages.length && trailListEl) {
      trailListEl.scrollTop = trailListEl.scrollHeight;
    }
  });

  function formatDate(ts: number) {
    return new Date(ts * 1000).toLocaleString();
  }

  function trailText(msg: Message) {
    if (msg.role === 'assistant' && msg.output) {
      return `[${msg.output.interactionMode.toUpperCase()}] ${msg.output.title} (${msg.output.versionName})\n${msg.output.response || msg.content}`;
    }
    return msg.content;
  }

  function trailVisuals(msg: Message) {
    const visuals: Array<{ src: string; alt: string; label: string }> = [];
    if (msg.imageData) {
      visuals.push({
        src: msg.imageData,
        alt: msg.role === 'user' ? 'Viewport snapshot' : 'Message image',
        label: msg.role === 'user' ? 'VIEWPORT' : 'IMAGE',
      });
    }
    for (const image of msg.attachmentImages || []) {
      visuals.push({
        src: image,
        alt: 'Attached reference image',
        label: 'REFERENCE',
      });
    }
    return visuals;
  }

  async function copyTrailMessage(msg: Message) {
    const text = trailText(msg).trim();
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      copiedTrailMessageId = msg.id;
      if (copiedTrailTimer) clearTimeout(copiedTrailTimer);
      copiedTrailTimer = window.setTimeout(() => {
        copiedTrailMessageId = null;
      }, 1200);
    } catch (error) {
      console.error('Failed to copy dialogue preview text:', error);
    }
  }

  function formatTokenCount(count: number | null | undefined) {
    const value = typeof count === 'number' ? count : 0;
    if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
    if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
    return `${value}`;
  }

  function formatCost(cost: number | null | undefined) {
    if (typeof cost !== 'number' || !Number.isFinite(cost)) return '';
    if (cost >= 1) return `$${cost.toFixed(2)}`;
    if (cost >= 0.01) return `$${cost.toFixed(3)}`;
    return `$${cost.toFixed(4)}`;
  }

  function mergeUsageSummary(
    left: UsageSummary | null | undefined,
    right: UsageSummary | null | undefined,
  ): UsageSummary | null {
    if (!left && !right) return null;
    if (!left) return right ?? null;
    if (!right) return left;

    return {
      inputTokens: (left.inputTokens ?? 0) + (right.inputTokens ?? 0),
      outputTokens: (left.outputTokens ?? 0) + (right.outputTokens ?? 0),
      totalTokens: (left.totalTokens ?? 0) + (right.totalTokens ?? 0),
      cachedInputTokens: (left.cachedInputTokens ?? 0) + (right.cachedInputTokens ?? 0),
      reasoningTokens: (left.reasoningTokens ?? 0) + (right.reasoningTokens ?? 0),
      estimatedCostUsd:
        typeof left.estimatedCostUsd === 'number' || typeof right.estimatedCostUsd === 'number'
          ? (left.estimatedCostUsd ?? 0) + (right.estimatedCostUsd ?? 0)
          : null,
      segments: [...(left.segments || []), ...(right.segments || [])],
    };
  }

  function usageLabel(usage: UsageSummary | null | undefined) {
    if (!usage) return '';
    const bits = [`${formatTokenCount(usage.totalTokens)} TOK`];
    const cost = formatCost(usage.estimatedCostUsd);
    if (cost) bits.push(`EST ${cost}`);
    return bits.join(' · ');
  }

  function usageTitle(usage: UsageSummary | null | undefined) {
    if (!usage) return '';
    const lines = [
      `Input: ${usage.inputTokens}`,
      `Output: ${usage.outputTokens}`,
      `Total: ${usage.totalTokens}`,
    ];
    if (usage.cachedInputTokens) lines.push(`Cached input: ${usage.cachedInputTokens}`);
    if (usage.reasoningTokens) lines.push(`Reasoning: ${usage.reasoningTokens}`);
    if (typeof usage.estimatedCostUsd === 'number') {
      lines.push(`Estimated cost: ${formatCost(usage.estimatedCostUsd)}`);
    }
    for (const segment of usage.segments || []) {
      const parts = [
        segment.stage.toUpperCase(),
        `${segment.provider}/${segment.model}`,
        `in ${segment.inputTokens}`,
        `out ${segment.outputTokens}`,
      ];
      if (typeof segment.estimatedCostUsd === 'number') {
        parts.push(`est ${formatCost(segment.estimatedCostUsd)}`);
      }
      lines.push(parts.join(' · '));
    }
    return lines.join('\n');
  }

  const lastMessage = $derived(messages.length > 0 ? messages[messages.length - 1] : null);
  const threadUsage = $derived.by(() =>
    messages.reduce<UsageSummary | null>(
      (aggregate, message) => mergeUsageSummary(aggregate, message.usage),
      null,
    )
  );
  const threadUsageMessageCount = $derived(
    messages.reduce((count, message) => count + (message.usage ? 1 : 0), 0)
  );
  const failedPromptForRetry = $derived.by(() => {
    if (!lastMessage || lastMessage.role !== 'assistant' || lastMessage.status !== 'error') return null;
    for (let i = messages.length - 1; i >= 0; i--) {
      const msg = messages[i];
      if (msg.role === 'user' && msg.timestamp <= lastMessage.timestamp) {
        const text = `${msg.content ?? ''}`.trim();
        if (text) return text;
      }
    }
    return null;
  });
</script>

<div 
  class="prompt-container" 
  class:is-dragging={isDragging}
  ondragover={handleWebDragOver}
  ondragleave={handleWebDragLeave}
  ondrop={handleWebDrop}
  role="region"
  aria-label="Prompt panel"
>
  {#if isDragging}
    <div class="drag-overlay">
      <div class="drag-msg">DROP TO ATTACH REFERENCES</div>
    </div>
  {/if}
  {#if versions.length > 0}
    {#if showDeleteConfirm}
      <Modal title="Confirm Version Purge" onclose={() => showDeleteConfirm = false}>
          <div class="confirm-delete-body">
          <p>Are you sure you want to delete <strong>{versionTitle(currentVersion)}</strong>?</p>
          <p class="warning">This specific iteration will be removed from the thread's timeline.</p>
          <div class="confirm-actions">
            <button class="btn btn-secondary" onclick={() => showDeleteConfirm = false}>CANCEL</button>
            <button class="btn btn-danger" onclick={executeDelete}>DELETE VERSION</button>
          </div>
        </div>
      </Modal>
    {/if}
    <div class="version-nav">
      <div class="nav-controls">
        <button class="nav-btn" disabled={!hasPrev} onclick={goPrev}>&larr;</button>
        <button class="nav-btn" disabled={!hasNext} onclick={goNext}>&rarr;</button>
      </div>
      
        <div class="version-info">
          <div class="version-counter-group">
          <span class="version-counter">V {currentVersionIndex + 1} OF {versions.length}</span>
          {#if currentVersion && currentVersion.output?.versionName}
            <span class="version-name">{currentVersion.output.versionName}</span>
          {/if}
          {#if currentVersion?.usage}
            <span class="usage-chip" title={usageTitle(currentVersion.usage)}>
              VERSION {usageLabel(currentVersion.usage)}
            </span>
          {/if}
          {#if currentVersion?.agentOrigin}
            <span
              class="version-agent-badge"
              title={`Agent-authored version via ${formatAgentOrigin(currentVersion.agentOrigin)}`}
            >
              {formatAgentOrigin(currentVersion.agentOrigin)}
            </span>
          {/if}
        </div>
        {#if currentVersion}
          <div class="version-actions">
            {#if currentVersion.output}
              <button class="code-btn" onclick={() => showCode(currentVersion)} title="Inspect Python Code">📜 CODE</button>
            {/if}
            <button class="code-btn delete-btn" onclick={() => showDeleteConfirm = true} title="Delete Version">🗑️ DEL</button>
          </div>
        {/if}
      </div>
    </div>

    {#if lastMessage && lastMessage.status === 'error' && dialogueState.mode === 'generate'}
      <div class="error-msg-box">
        <div class="error-header">LLM GENERATION ERROR</div>
        <div class="error-content">{lastMessage.content}</div>
        {#if failedPromptForRetry}
          <div class="error-actions">
            <button class="btn btn-danger" onclick={retryFailedPrompt} disabled={isGenerating || isSubmitting}>
              {isSubmitting ? 'RETRYING...' : 'RETRY LAST FAILED REQUEST'}
            </button>
          </div>
        {/if}
      </div>
    {/if}

    {#if threadUsage}
      <div class="usage-strip" title={usageTitle(threadUsage)}>
        THREAD TOTAL {usageLabel(threadUsage)}
        {#if threadUsageMessageCount > 0}
          <span class="usage-request-count">{threadUsageMessageCount} REQ</span>
        {/if}
      </div>
    {/if}

    {#if currentUserMsg && currentVersion}
      <div class="version-details">
        <button
          class="version-details-toggle"
          type="button"
          aria-expanded={detailsOpen}
          onclick={() => (detailsOpen = !detailsOpen)}
        >
          <span>Dialogue History for {versionTitle(currentVersion)}</span>
          <span class="details-toggle-indicator">{detailsOpen ? '−' : '+'}</span>
        </button>
        {#if detailsOpen}
        <div class="details-content">
          {#if promptTrail.length > 0}
            <div class="trail-list" bind:this={trailListEl}>
              {#each promptTrail as msg, i}
                {@const visuals = trailVisuals(msg)}
                <div class="trail-item {msg.role === 'assistant' ? 'trail-assistant' : 'trail-user'}">
                    <div class="trail-header-row">
                      <div class="trail-meta">
                        <span class="trail-role">{msg.role === 'assistant' ? 'ECKY' : 'YOU'}</span>
                        {#if msg.role === 'assistant' && msg.agentOrigin}
                          <span class="trail-agent-origin">{formatAgentOrigin(msg.agentOrigin)}</span>
                        {/if}
                        <span class="trail-time">{formatDate(msg.timestamp)}</span>
                      </div>
                    <button class="trail-copy-btn" type="button" onclick={() => copyTrailMessage(msg)}>
                      {copiedTrailMessageId === msg.id ? 'COPIED' : 'COPY'}
                    </button>
                  </div>
                  <div class="trail-content">
                    {#if visuals.length > 0}
                      <div class="trail-visuals">
                        {#each visuals as visual, visualIndex (`${msg.id}-${visual.label}-${visualIndex}`)}
                          <div class="trail-image-wrapper">
                            <div class="trail-image-kicker">{visual.label}</div>
                            <img src={visual.src} alt={visual.alt} class="trail-image" />
                          </div>
                        {/each}
                      </div>
                    {/if}
                    {#if msg.role === 'assistant' && msg.output}
                      <i>[{msg.output.interactionMode.toUpperCase()}] {msg.output.title} ({msg.output.versionName})</i>
                      <br/>
                      {msg.output.response || msg.content}
                    {:else}
                      {msg.content}
                    {/if}
                    {#if msg.usage}
                      <div class="trail-usage" title={usageTitle(msg.usage)}>{usageLabel(msg.usage)}</div>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
        {/if}
      </div>
    {/if}
  {/if}

  {#if queuedMessages.length > 0}
    <div class="mcp-inbox">
      {#each queuedMessages as msg (msg.id)}
        <div class="inbox-msg inbox-msg--{msg.status}">
          <span class="inbox-text">{msg.text}</span>
          <span class="inbox-ticks" title={msg.status === 'delivered' ? 'Delivered to agent' : 'Queued'}>
            {msg.status === 'delivered' ? '✓✓' : '✓'}
          </span>
        </div>
      {/each}
    </div>
  {/if}

  <div class="input-area">
    {#if attachments.length > 0}
      <div class="attachments-list">
        {#if hasImageAttachments}
          <div class="attachment-hint">
            Images go to the intent check and the design model. Add a short note so the model knows what to notice in each reference.
          </div>
        {/if}
        {#each attachments as att, i}
          <div class="attachment-item">
            <div class="att-header">
              <span class="att-type">{att.type === 'image' ? '🖼️ IMG' : '📐 CAD'}</span>
              <span class="att-name">{att.name}</span>
              <button class="btn-remove" onclick={() => removeAttachment(i)}>✕</button>
            </div>
            <input 
              class="input-mono att-explanation" 
              placeholder={att.type === 'image' ? "What should the model notice here?" : "How should this reference be used?"}
              bind:value={att.explanation}
            />
          </div>
        {/each}
      </div>
    {/if}

    <textarea
      class="input-mono prompt-input"
      value={prompt}
      oninput={handlePromptInput}
      onkeydown={handleKeydown}
      placeholder="Type a question or design change... (Cmd+Enter to process)"
      spellcheck="false"
    ></textarea>
    {#if dialogueState.mode === 'mcp-idle' && queuedMessages.length === 0}
      <div class="mcp-mode-hint">
        Agent is not asking yet — type to queue a message
      </div>
    {/if}
    <div class="prompt-actions">
      <button class="btn btn-xs btn-ghost" onclick={addAttachment} title="Attach images or reference CAD files">
        📎 ATTACH REFERENCE
      </button>
      <button
        class="btn btn-primary"
        disabled={isGenerating || isSubmitting || (dialogueState.mode === 'generate' && freecadMissing) || (!prompt.trim() && attachments.length === 0)}
        onclick={submit}
        title={dialogueState.mode === 'generate' && freecadMissing ? 'FreeCAD not found — configure in Settings' : undefined}
      >
        {#if isSubmitting}
          SENDING...
        {:else if isGenerating}
          PROCESSING...
        {:else if dialogueState.mode === 'agent-reply'}
          SEND TO AGENT
        {:else if dialogueState.mode === 'mcp-idle'}
          QUEUE
        {:else}
          PROCESS
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .prompt-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    position: relative;
  }

  .drag-overlay {
    position: absolute;
    inset: 0;
    background: color-mix(in srgb, var(--primary) 15%, transparent);
    border: 3px dashed var(--primary);
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    backdrop-filter: blur(2px);
  }

  .drag-msg {
    background: var(--bg);
    color: var(--primary);
    padding: 12px 24px;
    font-family: var(--font-mono);
    font-weight: bold;
    border: 1px solid var(--primary);
    letter-spacing: 0.1em;
    box-shadow: 0 0 20px rgba(0,0,0,0.5);
  }

  .attachments-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 8px;
    max-height: 160px;
    overflow-y: auto;
    padding: 4px;
    background: var(--bg-100);
    border: 1px dashed var(--bg-300);
  }

  .attachment-item {
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 240px;
    flex-shrink: 0;
  }

  .attachment-hint {
    flex: 1 0 100%;
    color: var(--text-dim);
    font-size: 0.64rem;
    line-height: 1.4;
    padding: 2px 2px 6px;
  }

  .att-header {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.6rem;
    font-weight: bold;
  }

  .att-type {
    color: var(--secondary);
    background: rgba(0,0,0,0.2);
    padding: 1px 4px;
  }

  .att-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-dim);
  }

  .btn-remove {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.8rem;
  }

  .btn-remove:hover {
    color: var(--red);
  }

  .att-explanation {
    background: var(--bg);
    border: 1px solid var(--bg-400);
    color: var(--text);
    padding: 2px 4px;
    font-size: 0.65rem;
  }

  .version-nav {
    display: flex;
    justify-content: flex-start;
    align-items: center;
    padding: 8px 12px;
    background: var(--bg-100);
    border-bottom: 1px solid var(--bg-300);
    gap: 12px;
  }

  .nav-controls {
    display: flex;
    gap: 4px;
  }

  .nav-btn {
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    padding: 4px 8px;
    font-size: 0.7rem;
    cursor: pointer;
    font-family: var(--font-mono);
  }

  .nav-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .nav-btn:not(:disabled):hover {
    border-color: var(--primary);
    color: var(--primary);
  }

  .version-info {
    display: flex;
    align-items: center;
    gap: 16px;
    flex: 1;
    min-width: 0;
  }

  .version-counter-group {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .version-counter {
    font-size: 0.7rem;
    font-weight: bold;
    color: var(--secondary);
    font-family: var(--font-mono);
    white-space: nowrap;
  }

  .version-name {
    font-size: 0.65rem;
    color: var(--text-dim);
    text-transform: uppercase;
    font-weight: 500;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .usage-chip,
  .usage-strip,
  .trail-usage {
    font-family: var(--font-mono);
    font-size: 0.62rem;
    letter-spacing: 0.06em;
    color: var(--secondary);
  }

  .usage-chip {
    padding: 2px 6px;
    border: 1px solid color-mix(in srgb, var(--secondary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--secondary) 8%, var(--bg-200));
    white-space: nowrap;
  }

  .version-agent-badge,
  .trail-agent-origin {
    padding: 2px 6px;
    border: 1px solid color-mix(in srgb, var(--primary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-200));
    color: var(--primary);
    font-family: var(--font-mono);
    font-size: 0.6rem;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .usage-strip {
    padding: 6px 12px;
    border-bottom: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--secondary) 6%, var(--bg-100));
  }

  .usage-request-count {
    margin-left: 10px;
    color: var(--text-dim);
  }

  .version-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }

  .code-btn {
    background: var(--bg-300);
    border: 1px solid var(--bg-400);
    color: var(--text);
    font-size: 0.6rem;
    padding: 2px 6px;
    cursor: pointer;
    font-weight: bold;
  }

  .code-btn:hover {
    color: var(--primary);
  }

  .delete-btn:hover {
    color: var(--danger, #ff4444);
    border-color: var(--danger, #ff4444);
  }

  .version-details {
    padding: 8px 12px;
    background: var(--bg-100);
    border-bottom: 1px solid var(--bg-300);
    font-size: 0.75rem;
  }

  .version-details-toggle {
    width: 100%;
    padding: 0;
    border: none;
    background: transparent;
    display: flex;
    align-items: center;
    justify-content: space-between;
    cursor: pointer;
    color: var(--text-dim);
    font-weight: bold;
    text-align: left;
  }

  .version-details-toggle:hover {
    color: var(--text);
  }

  .details-toggle-indicator {
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.82rem;
    flex-shrink: 0;
    margin-left: 12px;
  }

  .details-content {
    margin-top: 8px;
    padding-left: 16px;
    border-left: 2px solid var(--bg-300);
    -webkit-user-select: text;
    user-select: text;
  }

  .trail-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 8px;
    max-height: 300px;
    overflow-y: auto;
    overflow-x: hidden;
    padding-right: 4px;
  }

  .trail-item {
    border: 1px solid var(--bg-300);
    padding: 6px 10px;
    max-width: min(800px, 90%);
    width: fit-content;
    min-width: 200px;
    cursor: text;
    -webkit-user-select: text;
    user-select: text;
  }

  .trail-item,
  .trail-item * {
    -webkit-user-select: text;
    user-select: text;
  }

  .trail-user {
    background: var(--bg-200);
    border-left: 2px solid var(--text-dim);
    align-self: flex-start;
  }

  .trail-assistant {
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
    border-left: 2px solid var(--primary);
    align-self: flex-start;
  }

  .trail-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 4px;
    font-size: 0.6rem;
  }

  .trail-meta {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .trail-role {
    font-weight: bold;
    color: var(--secondary);
  }

  .trail-time {
    color: var(--text-dim);
    font-family: var(--font-mono);
  }

  .trail-copy-btn {
    border: 1px solid var(--bg-400);
    background: color-mix(in srgb, var(--bg) 78%, transparent);
    color: var(--text-dim);
    padding: 2px 6px;
    font-size: 0.55rem;
    font-family: var(--font-mono);
    letter-spacing: 0.06em;
    cursor: pointer;
    flex-shrink: 0;
    -webkit-user-select: none;
    user-select: none;
  }

  .trail-copy-btn:hover {
    border-color: var(--primary);
    color: var(--primary);
  }

  .trail-content {
    font-size: 0.7rem;
    color: var(--text);
    white-space: pre-wrap;
    line-height: 1.4;
    -webkit-user-select: text;
    user-select: text;
  }

  .trail-usage {
    margin-top: 8px;
    opacity: 0.9;
  }

  .trail-visuals {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 8px;
  }

  .trail-image-wrapper {
    border: 1px solid var(--bg-400);
    width: min(320px, 100%);
    background: #000;
    overflow: hidden;
  }

  .trail-image-kicker {
    padding: 4px 6px;
    border-bottom: 1px solid var(--bg-400);
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.56rem;
    letter-spacing: 0.08em;
  }

  .trail-image {
    display: block;
    width: 100%;
    height: auto;
    max-height: 200px;
    object-fit: contain;
  }

  .input-area {
    flex-shrink: 0;
    padding: 12px;
    background: var(--bg-100);
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 120px;
  }

  .prompt-input {
    flex: 1;
    width: 100%;
    min-height: 80px;
    padding: 12px;
    background: var(--bg-200);
    border: 1px solid var(--bg-300);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.8rem;
    resize: none;
    outline: none;
  }

  .prompt-input:focus {
    border-color: var(--primary);
  }

  .prompt-actions {
    display: flex;
    justify-content: flex-end;
  }

  .mcp-mode-hint {
    padding: 6px 12px;
    font-size: 0.62rem;
    letter-spacing: 0.06em;
    color: var(--text-dim);
    text-align: center;
  }

  .mcp-inbox {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--bg-300);
    max-height: 140px;
    overflow-y: auto;
  }

  .inbox-msg {
    display: flex;
    align-items: flex-start;
    justify-content: flex-end;
    gap: 6px;
  }

  .inbox-text {
    font-size: 0.72rem;
    padding: 5px 10px;
    background: color-mix(in srgb, var(--primary) 15%, var(--bg-200));
    border: 1px solid color-mix(in srgb, var(--primary) 30%, var(--bg-300));
    color: var(--text);
    max-width: 85%;
    word-break: break-word;
    white-space: pre-wrap;
  }

  .inbox-ticks {
    font-size: 0.65rem;
    flex-shrink: 0;
    align-self: flex-end;
    padding-bottom: 2px;
  }

  .inbox-msg--queued .inbox-ticks {
    color: var(--text-dim);
  }

  .inbox-msg--delivered .inbox-ticks {
    color: var(--primary);
  }


  .btn-primary {
    padding: 8px 16px;
    font-weight: bold;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .error-msg-box {
    margin: 8px 12px;
    padding: 12px;
    background: rgba(220, 38, 38, 0.1);
    border: 1px solid var(--red);
    color: var(--red);
    font-size: 0.75rem;
    overflow: hidden;
  }

  .error-header {
    font-weight: bold;
    margin-bottom: 8px;
    font-size: 0.65rem;
    letter-spacing: 0.1em;
  }

  .error-content {
    font-family: var(--font-mono);
    white-space: pre-wrap;
    max-height: 200px;
    overflow-y: auto;
    word-break: break-all;
  }

  .error-actions {
    margin-top: 10px;
    display: flex;
    justify-content: flex-end;
  }

  /* Shared Confirmation Styles (matching HistoryPanel) */
  .confirm-delete-body {
    padding: 20px;
    font-size: 0.85rem;
    color: var(--text);
  }

  .confirm-delete-body p {
    margin-bottom: 12px;
  }

  .confirm-delete-body .warning {
    color: var(--red);
    font-weight: bold;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }

  .btn {
    padding: 6px 16px;
    font-size: 0.75rem;
    font-weight: bold;
    cursor: pointer;
    border: 1px solid transparent;
  }

  .btn-secondary {
    background: var(--bg-300);
    color: var(--text);
    border-color: var(--bg-400);
  }

  .btn-secondary:hover {
    background: var(--bg-400);
  }

  .btn-danger {
    background: var(--red);
    color: white;
  }

  .btn-danger:hover {
    background: color-mix(in srgb, var(--red) 80%, black);
  }
</style>
