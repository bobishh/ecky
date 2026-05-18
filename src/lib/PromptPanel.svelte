<script lang="ts">
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { readFile } from '@tauri-apps/plugin-fs';
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Modal from './Modal.svelte';
  import Viewer from './Viewer.svelte';
  import { appendTranscriptToPrompt, createPromptAudioRecorder, type PromptAudioRecorder } from './audio/pushToTalk';
  import { formatBackendError, transcribePromptAudio } from './tauri/client';
  import { resolveVersionLoupeRuntime } from './versionLoupeRuntime';
  import type {
    Attachment,
    Message,
    UsageSummary,
    ViewerAsset,
  } from './types/domain';
  import {
    activeVersionTimelineIndex,
    formatTimelineAgentOrigin,
    isVersionTimelineMessage,
    threadTimelineMessages,
    timelineVisuals,
    versionTimelineMessages,
    versionTimelineTitle,
    type TimelineVisual,
  } from './threadTimeline';
  import { modelEngineLabel } from './modelEngineLabel';
  import type { DialogueState } from './composables/dialogueState';

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

  type VisualLoupeState = {
    title: string;
    src: string;
    alt: string;
    caption: string;
  };

  type VersionLoupeState = {
    message: VersionMessage;
    loadError: string | null;
    loading: boolean;
    previewUrl: string | null;
    viewerAssets: ViewerAsset[];
  };

  let {
    onGenerate,
    isGenerating = false,
    generationUnavailableReason = null,
    imageAttachmentUnavailableReason = null,
    dialogueState = { mode: 'generate' } as DialogueState,
    messages = [],
    messagesLoading = false,
    messagesHasMore = false,
    messagesPageLoading = false,
    onLoadOlderMessages,
    onShowCode,
    activeThreadId = null,
    sendWorkspaceCapture = false,
    workspaceCaptureHint = null,
    sttLanguageCode = 'en-US',
    onToggleWorkspaceCapture,
    activeVersionId = $bindable(null),
    onVersionChange,
    onDeleteVersion,
    onRestoreVersion,
  }: {
    onGenerate: (prompt: string, attachments: Attachment[]) => Promise<unknown>;
    isGenerating?: boolean;
    generationUnavailableReason?: string | null;
    imageAttachmentUnavailableReason?: string | null;
    dialogueState?: DialogueState;
    messages?: Message[];
    messagesLoading?: boolean;
    messagesHasMore?: boolean;
    messagesPageLoading?: boolean;
    onLoadOlderMessages?: () => Promise<void> | void;
    onShowCode: (message: CodeVersionMessage) => void;
    activeThreadId?: string | null;
    sendWorkspaceCapture?: boolean;
    workspaceCaptureHint?: string | null;
    sttLanguageCode?: string;
    onToggleWorkspaceCapture?: (enabled: boolean) => void;
    activeVersionId?: string | null;
    onVersionChange?: (message: VersionMessage) => void;
    onDeleteVersion?: (messageId: string) => void;
    onRestoreVersion?: (messageId: string) => void;
  } = $props();

  const PROMPT_DRAFTS_STORAGE_KEY = 'ecky:prompt-drafts:v1';
  const NEW_THREAD_DRAFT_KEY = '__new__';
  const PROMPT_DRAFT_DEBOUNCE_MS = 400;
  type VoiceState = 'idle' | 'listening' | 'transcribing' | 'error';

  let prompt = $state('');
  let attachments = $state<Attachment[]>([]);
  let isDragging = $state(false);
  let versionToDelete = $state<VersionMessage | null>(null);
  let visualLoupe = $state<VisualLoupeState | null>(null);
  let versionLoupe = $state<VersionLoupeState | null>(null);
  let versionLoupeLoadSeq = 0;
  let draftScopeKey = $state<string | null>(null);
  let draftPersistTimer: number | null = null;
  let pendingDraftWrite = $state<{ scopeKey: string; prompt: string } | null>(null);
  let voiceRecorder = $state<PromptAudioRecorder | null>(null);
  let voiceState = $state<VoiceState>('idle');
  let voiceStatus = $state('');
  const voiceBusy = $derived(voiceState === 'listening' || voiceState === 'transcribing');
  const hasImageAttachments = $derived(attachments.some((attachment) => attachment.type === 'image'));
  const submitUnavailableReason = $derived.by<string | null>(() => {
    if (dialogueState.mode !== 'generate') return null;
    if (generationUnavailableReason) return generationUnavailableReason;
    if (hasImageAttachments && imageAttachmentUnavailableReason) return imageAttachmentUnavailableReason;
    return null;
  });

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

  async function startVoiceInput() {
    if (voiceBusy || isGenerating || isSubmitting) return;
    const recorder = createPromptAudioRecorder();
    voiceRecorder = recorder;
    voiceState = 'listening';
    voiceStatus = 'LISTENING';
    try {
      await recorder.start();
    } catch (error) {
      voiceRecorder = null;
      voiceState = 'error';
      voiceStatus = formatBackendError(error);
    }
  }

  async function finishVoiceInput() {
    if (voiceState !== 'listening' || !voiceRecorder) return;
    const recorder = voiceRecorder;
    voiceRecorder = null;
    voiceState = 'transcribing';
    voiceStatus = 'TRANSCRIBING';
    try {
      const capture = await recorder.stop();
      const transcript = await transcribePromptAudio({
        base64Data: capture.base64Data,
        mimeType: capture.mimeType,
        languageCode: sttLanguageCode.trim() || 'en-US',
      });
      prompt = appendTranscriptToPrompt(prompt, transcript.text);
      schedulePromptDraftPersist(currentDraftScopeKey(activeThreadId), prompt);
      voiceState = 'idle';
      voiceStatus = '';
    } catch (error) {
      voiceState = 'error';
      voiceStatus = formatBackendError(error);
    }
  }

  function cancelVoiceInput() {
    if (voiceState === 'listening') {
      voiceRecorder?.cancel();
    }
    voiceRecorder = null;
    if (voiceState === 'listening') {
      voiceState = 'idle';
      voiceStatus = '';
    }
  }

  function handleVoicePointerDown(event: PointerEvent) {
    event.preventDefault();
    (event.currentTarget as HTMLButtonElement).setPointerCapture?.(event.pointerId);
    void startVoiceInput();
  }

  function handleVoicePointerUp(event: PointerEvent) {
    event.preventDefault();
    const button = event.currentTarget as HTMLButtonElement;
    if (button.hasPointerCapture?.(event.pointerId)) {
      button.releasePointerCapture(event.pointerId);
    }
    void finishVoiceInput();
  }

  function handleVoiceKeydown(event: KeyboardEvent) {
    if ((event.key !== ' ' && event.key !== 'Enter') || event.repeat) return;
    event.preventDefault();
    void startVoiceInput();
  }

  function handleVoiceKeyup(event: KeyboardEvent) {
    if (event.key !== ' ' && event.key !== 'Enter') return;
    event.preventDefault();
    void finishVoiceInput();
  }

  function isVersionMessage(message: Message): message is VersionMessage {
    return isVersionTimelineMessage(message);
  }

  $effect(() => {
    const nextScopeKey = currentDraftScopeKey(activeThreadId);
    if (nextScopeKey === draftScopeKey) return;
    flushPendingPromptDraft();
    loadPromptDraft(nextScopeKey);
  });

  function imageMimeType(name: string): string {
    const ext = (name.split('.').pop() || '').toLowerCase();
    if (ext === 'jpg' || ext === 'jpeg') return 'image/jpeg';
    if (ext === 'webp') return 'image/webp';
    if (ext === 'svg') return 'image/svg+xml';
    return 'image/png';
  }

  function bytesToBase64(bytes: Uint8Array): string {
    let binary = '';
    const chunkSize = 0x8000;
    for (let i = 0; i < bytes.length; i += chunkSize) {
      const chunk = bytes.subarray(i, i + chunkSize);
      binary += String.fromCharCode(...chunk);
    }
    return btoa(binary);
  }

  async function inlineImageAttachmentFromPath(path: string): Promise<Attachment> {
    const name = path.split(/[\/\\]/).pop() || path;
    const bytes = await readFile(path);
    return {
      path: '',
      name,
      explanation: '',
      dataUrl: `data:${imageMimeType(name)};base64,${bytesToBase64(bytes)}`,
      type: 'image',
    };
  }

  async function processPaths(paths: string[]) {
    const newAttachments = await Promise.all(paths.map(async (path) => {
      const name = path.split(/[\/\\]/).pop() || path;
      const ext = (name.split('.').pop() || '').toLowerCase();
      if (['png', 'jpg', 'jpeg', 'webp', 'svg'].includes(ext)) {
        try {
          return await inlineImageAttachmentFromPath(path);
        } catch {
          return {
            path,
            name,
            explanation: '',
            type: 'image',
          };
        }
      }
      return {
        path,
        name,
        explanation: '',
        type: 'cad',
      };
    }));
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
            void processPaths(event.payload.paths);
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
      void processPaths(mockPaths);
    }
  }

  function toAssetUrl(path: string | null | undefined): string {
    if (!path) return '';
    try {
      return convertFileSrc(path);
    } catch {
      return path;
    }
  }

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
          { name: 'Images, CAD & Macros', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg', 'stl', 'step', 'stp', 'py', 'fcmacro'] }
        ]
      });

      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        await processPaths(paths);
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

  function executeDelete() {
    if (onDeleteVersion && versionToDelete) {
      onDeleteVersion(versionToDelete.id);
      versionToDelete = null;
    }
  }

  function restoreVersionToCarousel(messageId: string) {
    onRestoreVersion?.(messageId);
  }

  function showCode(message: VersionMessage) {
    if (message.output) onShowCode(message as CodeVersionMessage);
  }

  async function openVersionLoupe(message: VersionMessage) {
    const loadSeq = ++versionLoupeLoadSeq;
    versionLoupe = {
      message,
      loadError: null,
      loading: true,
      previewUrl: null,
      viewerAssets: [],
    };
    try {
      const runtime = await resolveVersionLoupeRuntime(message, activeThreadId, toAssetUrl);
      if (!versionLoupe || loadSeq !== versionLoupeLoadSeq || versionLoupe.message.id !== message.id) {
        return;
      }
      versionLoupe = {
        ...versionLoupe,
        loading: false,
        previewUrl: runtime.previewUrl,
        viewerAssets: runtime.viewerAssets,
      };
    } catch (error) {
      if (!versionLoupe || loadSeq !== versionLoupeLoadSeq || versionLoupe.message.id !== message.id) {
        return;
      }
      versionLoupe = {
        ...versionLoupe,
        loading: false,
        loadError: formatBackendError(error),
      };
    }
  }

  function setVersionLoupeLoadError(message: string) {
    if (!versionLoupe) return;
    versionLoupe = { ...versionLoupe, loading: false, loadError: message };
  }

  function openVisualLoupe(message: Message, visual: TimelineVisual) {
    visualLoupe = {
      title: visual.label,
      src: visual.src,
      alt: visual.alt,
      caption: message.content.trim(),
    };
  }

  const timelineMessages = $derived(threadTimelineMessages(messages));
  const versionMessages = $derived(versionTimelineMessages(messages));
  const activeVersionIndex = $derived(
    activeVersionTimelineIndex(versionMessages, activeVersionId),
  );
  const activeVersion = $derived(
    activeVersionIndex >= 0 ? (versionMessages[activeVersionIndex] as VersionMessage) : null,
  );
  const hasQueuedTimelineMessage = $derived(
    timelineMessages.some((message) => message.role === 'user' && message.status === 'pending'),
  );
  let trailListEl = $state<HTMLDivElement | null>(null);
  let copiedTrailMessageId = $state<string | null>(null);
  let copiedTrailTimer = $state<number | null>(null);

  $effect(() => {
    if (!trailListEl || !timelineMessages.length) return;
    requestAnimationFrame(() => {
      if (trailListEl) trailListEl.scrollTop = trailListEl.scrollHeight;
    });
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

  function isCopiedTrailMessage(msg: Message) {
    return copiedTrailMessageId === msg.id;
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

  const lastMessage = $derived(
    timelineMessages.length > 0 ? timelineMessages[timelineMessages.length - 1] : null
  );
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
    for (let i = timelineMessages.length - 1; i >= 0; i--) {
      const msg = timelineMessages[i];
      if (msg.role === 'user' && msg.timestamp <= lastMessage.timestamp) {
        const text = `${msg.content ?? ''}`.trim();
        if (text) return text;
      }
    }
    return null;
  });

  function messageStatusLabel(message: Message) {
    if (message.status === 'discarded' && isVersionMessage(message)) {
      return 'OFF CAROUSEL';
    }
    switch (message.status) {
      case 'pending':
        return message.role === 'user' ? 'QUEUED' : 'PENDING';
      case 'working':
        return 'WORKING';
      case 'error':
        return 'ERROR';
      default:
        return '';
    }
  }

  function messageRoleLabel(message: Message, isVersion: boolean) {
    if (isVersion) {
      if ((message as VersionMessage).output?.interactionMode === 'question') return 'TUNE';
      return 'VERSION';
    }
    return message.role === 'assistant' ? 'ECKY' : 'YOU';
  }

  function openRelativeVersion(direction: -1 | 1) {
    if (!versionMessages.length || activeVersionIndex < 0) return;
    const nextIndex = activeVersionIndex + direction;
    if (nextIndex < 0 || nextIndex >= versionMessages.length) return;
    const target = versionMessages[nextIndex];
    if (isVersionMessage(target)) {
      onVersionChange?.(target);
    }
  }

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
  {#if versionToDelete}
    <Modal title="Remove From Carousel" onclose={() => (versionToDelete = null)}>
      <div class="confirm-delete-body">
        <p>Remove <strong>{versionTimelineTitle(versionToDelete)}</strong> from the version carousel?</p>
        <p class="warning">It stays in thread history and can be returned to the carousel later.</p>
        <div class="confirm-actions">
          <button class="btn btn-ghost" onclick={() => (versionToDelete = null)}>CANCEL</button>
          <button class="btn btn-danger" onclick={executeDelete}>REMOVE</button>
        </div>
      </div>
    </Modal>
  {/if}

  {#if visualLoupe}
    <Modal title={`${visualLoupe.title} Loupe`} onclose={() => (visualLoupe = null)}>
      <div class="visual-loupe">
        <div class="visual-loupe__image-frame">
          <img class="visual-loupe__image" src={visualLoupe.src} alt={visualLoupe.alt} />
        </div>
        {#if visualLoupe.caption}
          <div class="visual-loupe__caption">{visualLoupe.caption}</div>
        {/if}
      </div>
    </Modal>
  {/if}

  {#if versionLoupe}
    <Modal title="Version Preview" onclose={() => (versionLoupe = null)}>
      <div class="version-loupe">
        <div class="version-loupe__meta">
          <div class="version-loupe__title">{versionTimelineTitle(versionLoupe.message)}</div>
          {#if versionLoupe.message.output?.versionName}
            <div class="version-loupe__subtitle">{versionLoupe.message.output.versionName}</div>
          {/if}
        </div>
        {#if versionLoupe.loading}
          <div class="version-loupe__empty">LOADING PREVIEW...</div>
        {:else if versionLoupe.previewUrl}
          <div class="version-loupe__viewer">
            <Viewer
              modelKey={versionLoupe.message.artifactBundle?.modelId ?? versionLoupe.message.id}
              stlUrl={versionLoupe.previewUrl}
              viewerAssets={versionLoupe.viewerAssets}
              manifestParts={versionLoupe.message.modelManifest?.parts ?? []}
              showContextOverlay={false}
              onModelLoadError={setVersionLoupeLoadError}
            />
          </div>
          {#if versionLoupe.loadError}
            <div class="version-loupe__error" role="alert">{versionLoupe.loadError}</div>
          {/if}
        {:else if versionLoupe.loadError}
          <div class="version-loupe__error" role="alert">{versionLoupe.loadError}</div>
        {:else}
          <div class="version-loupe__empty">NO RUNTIME ARTIFACT</div>
        {/if}
      </div>
    </Modal>
  {/if}

  {#if threadUsage}
    <div class="usage-strip" title={usageTitle(threadUsage)}>
      THREAD TOTAL {usageLabel(threadUsage)}
      {#if threadUsageMessageCount > 0}
        <span class="usage-request-count">{threadUsageMessageCount} REQ</span>
      {/if}
    </div>
  {/if}

  {#if versionMessages.length > 0}
    <div class="version-nav">
      <div class="version-counter-group">
        <button
          class="nav-btn"
          type="button"
          onclick={() => openRelativeVersion(-1)}
          disabled={activeVersionIndex <= 0}
        >
          ◀
        </button>
        <div class="version-counter">
          V {Math.max(activeVersionIndex + 1, 1)} OF {versionMessages.length}
        </div>
        <button
          class="nav-btn"
          type="button"
          onclick={() => openRelativeVersion(1)}
          disabled={activeVersionIndex < 0 || activeVersionIndex >= versionMessages.length - 1}
        >
          ▶
        </button>
      </div>
      <div class="version-info">
        <div class="version-title">{versionTimelineTitle(activeVersion)}</div>
        {#if activeVersion?.output?.versionName}
          <div class="version-subtitle">{activeVersion.output.versionName}</div>
        {/if}
        {#if activeVersion}
          <div class="version-engine">{modelEngineLabel(activeVersion)}</div>
        {/if}
      </div>
      {#if activeVersion}
        <div class="version-nav__actions">
          <button class="trail-copy-btn delete-btn" type="button" title="Remove from carousel" onclick={() => (versionToDelete = activeVersion)}>
            DELETE
          </button>
        </div>
      {/if}
    </div>
  {/if}

  <div class="trail-list" bind:this={trailListEl}>
    {#if messagesHasMore}
      <button
        class="load-older-btn"
        type="button"
        disabled={messagesPageLoading}
        onclick={() => onLoadOlderMessages?.()}
      >
        {messagesPageLoading ? 'LOADING OLDER...' : 'LOAD OLDER'}
      </button>
    {/if}
    {#if messagesLoading}
      <div class="thread-loading">
        <div class="thread-loading-bar"></div>
        <span>LOADING THREAD MESSAGES...</span>
      </div>
    {/if}
    {#each timelineMessages as msg (msg.id)}
      {@const visuals = timelineVisuals(msg, toAssetUrl)}
      {@const isVersion = isVersionMessage(msg)}
      {@const isTuneVersion = isVersion && msg.output?.interactionMode === 'question'}
      {@const isActiveVersion = isVersion && activeVersion?.id === msg.id}
      {@const isDiscardedVersion = isVersion && msg.status === 'discarded'}
      {@const statusLabel = messageStatusLabel(msg)}
      <div
        class="trail-item {msg.role === 'assistant' ? 'trail-assistant' : 'trail-user'} {isActiveVersion ? 'trail-active-version' : ''} {isDiscardedVersion ? 'trail-discarded-version' : ''} {isTuneVersion ? 'trail-tune-version' : ''} {msg.status === 'error' ? 'trail-error' : ''}"
      >
        <div class="trail-header-row">
          <div class="trail-meta">
            <span class="trail-role">
              {messageRoleLabel(msg, isVersion)}
            </span>
            {#if msg.role === 'assistant' && msg.agentOrigin}
              <span class="trail-agent-origin">{formatTimelineAgentOrigin(msg.agentOrigin)}</span>
            {/if}
            {#if isVersion && msg.output?.versionName}
              <span class="version-name">{msg.output.versionName}</span>
            {/if}
            {#if statusLabel}
              <span class="trail-status trail-status--{msg.status}">{statusLabel}</span>
            {/if}
            <span class="trail-time">{formatDate(msg.timestamp)}</span>
          </div>
          <div class="trail-header-actions">
            {#if isVersion}
              {#if !isTuneVersion}
                {#if isDiscardedVersion}
                  <button class="trail-copy-btn" type="button" onclick={() => restoreVersionToCarousel(msg.id)}>
                    RETURN
                  </button>
                {:else}
                  <button
                    class="trail-copy-btn"
                    type="button"
                    onclick={() => onVersionChange?.(msg)}
                  >
                    {isActiveVersion ? 'CURRENT' : 'SET CURRENT'}
                  </button>
                  <button class="trail-copy-btn" type="button" onclick={() => openVersionLoupe(msg)}>
                    VIEW
                  </button>
                {/if}
              {/if}
              {#if msg.output}
                <button class="trail-copy-btn" type="button" onclick={() => showCode(msg)}>
                  CODE
                </button>
              {/if}
              {#if !isDiscardedVersion}
                <button class="trail-copy-btn delete-btn" type="button" title="Remove from carousel" onclick={() => (versionToDelete = msg)}>
                  DELETE
                </button>
              {/if}
            {:else}
              <button class="trail-copy-btn" type="button" onclick={() => copyTrailMessage(msg)}>
                {isCopiedTrailMessage(msg) ? 'COPIED' : 'COPY'}
              </button>
            {/if}
          </div>
        </div>
        <div class="trail-content">
          {#if visuals.length > 0}
            <div class="trail-visuals">
              {#each visuals as visual, visualIndex (`${msg.id}-${visual.label}-${visualIndex}`)}
                <div class="trail-image-wrapper">
                  <div class="trail-image-kicker">{visual.label}</div>
                  <button
                    class="trail-image-button"
                    type="button"
                    aria-label={`Open ${visual.label.toLowerCase()} preview`}
                    onclick={() => openVisualLoupe(msg, visual)}
                  >
                    <img src={visual.src} alt={visual.alt} class="trail-image" />
                  </button>
                </div>
              {/each}
            </div>
          {/if}
          {#if isVersion && msg.output}
            <div class="trail-version-title">
              [{msg.output.interactionMode.toUpperCase()}] {versionTimelineTitle(msg)}
            </div>
            {msg.output.response || msg.content}
          {:else}
            {msg.content}
          {/if}
          {#if msg.id === lastMessage?.id && msg.role === 'assistant' && msg.status === 'error' && dialogueState.mode === 'generate' && failedPromptForRetry}
            <div class="error-actions">
              <button class="btn btn-danger" onclick={retryFailedPrompt} disabled={isGenerating || isSubmitting}>
                {isSubmitting ? 'RETRYING...' : 'RETRY LAST FAILED REQUEST'}
              </button>
            </div>
          {/if}
          {#if msg.usage}
            <div class="trail-usage" title={usageTitle(msg.usage)}>{usageLabel(msg.usage)}</div>
          {/if}
        </div>
      </div>
    {/each}
  </div>

  <div class="input-area">
    {#if attachments.length > 0}
      <div class="attachments-list">
        {#if hasImageAttachments}
          <div class="attachment-hint" class:attachment-hint--warning={Boolean(imageAttachmentUnavailableReason)}>
            {#if imageAttachmentUnavailableReason}
              {imageAttachmentUnavailableReason}
            {:else}
              Images go to the intent check and the design model. Add a short note so the model knows what to notice in each reference.
            {/if}
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
    <div class="prompt-actions">
      <div class="prompt-actions__left">
        {#if dialogueState.mode !== 'generate'}
          <label class="workspace-capture-toggle">
            <input
              type="checkbox"
              checked={sendWorkspaceCapture}
              onchange={(event) =>
                onToggleWorkspaceCapture?.((event.currentTarget as HTMLInputElement).checked)}
            />
            <span>SEND WORKSPACE IF NEEDED</span>
          </label>
        {/if}
      </div>
      <div class="prompt-actions__right">
        <button
          class="btn btn-xs btn-ghost voice-btn"
          class:voice-btn--active={voiceState === 'listening'}
          class:voice-btn--busy={voiceState === 'transcribing'}
          aria-label="Start voice input"
          title="Hold to record voice input"
          disabled={isGenerating || isSubmitting || voiceState === 'transcribing'}
          onpointerdown={handleVoicePointerDown}
          onpointerup={handleVoicePointerUp}
          onpointercancel={cancelVoiceInput}
          onkeydown={handleVoiceKeydown}
          onkeyup={handleVoiceKeyup}
        >
          {#if voiceState === 'listening'}
            ⏹ LISTENING
          {:else if voiceState === 'transcribing'}
            … TRANSCRIBING
          {:else}
            🎙 VOICE
          {/if}
        </button>
        <button class="btn btn-xs btn-ghost" onclick={addAttachment} title="Attach images or reference CAD files">
          📎 ATTACH REFERENCE
        </button>
        <button
          class="btn btn-primary"
          disabled={isGenerating || isSubmitting || Boolean(submitUnavailableReason) || (!prompt.trim() && attachments.length === 0)}
          onclick={submit}
          title={submitUnavailableReason ?? undefined}
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
    {#if voiceStatus}
      <div class="voice-status" class:voice-status--error={voiceState === 'error'}>{voiceStatus}</div>
    {/if}
    {#if workspaceCaptureHint}
      <div class="workspace-capture-hint">{workspaceCaptureHint}</div>
    {/if}
  </div>
</div>

<style>
  .prompt-container {
    display: flex;
    flex-direction: column;
    flex: 1;
    height: 100%;
    min-height: 0;
    background: var(--bg);
    position: relative;
    overflow: hidden;
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
    margin-bottom: 4px;
    max-height: 120px;
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

  .trail-visual-actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }

  .trail-action-btn {
    background: color-mix(in srgb, var(--bg-200) 88%, transparent);
    border: 1px solid var(--bg-300);
    color: var(--secondary);
    cursor: pointer;
    padding: 4px 8px;
    font-family: var(--font-mono);
    font-size: 0.64rem;
    letter-spacing: 0.08em;
  }

  .trail-action-btn:hover:not(:disabled) {
    border-color: var(--primary);
    color: var(--primary);
  }

  .trail-action-btn:disabled {
    cursor: default;
    color: var(--text-dim);
    opacity: 0.65;
  }

  .attachment-hint {
    flex: 1 0 100%;
    color: var(--text-dim);
    font-size: 0.64rem;
    line-height: 1.4;
    padding: 2px 2px 6px;
  }

  .attachment-hint--warning {
    color: var(--secondary);
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
    flex-shrink: 0;
    padding: 8px 12px;
    background: var(--bg-100);
    border-bottom: 1px solid var(--bg-300);
    gap: 12px;
  }

  .version-engine,
  .version-engine-badge {
    font-family: var(--font-mono);
    font-size: 0.62rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
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

  .version-engine,
  .version-engine-badge {
    padding: 2px 6px;
    border: 1px solid color-mix(in srgb, var(--secondary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--secondary) 10%, var(--bg-200));
    color: var(--secondary);
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
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 8px 12px;
  }

  .thread-loading,
  .load-older-btn {
    align-self: stretch;
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 0.64rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 7px 10px;
    overflow: hidden;
  }

  .thread-loading {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .thread-loading-bar {
    width: 28px;
    height: 2px;
    background: var(--primary);
    animation: thread-loading-pulse 1s infinite ease-in-out;
  }

  .load-older-btn {
    cursor: pointer;
    color: var(--secondary);
  }

  .load-older-btn:disabled {
    cursor: default;
    opacity: 0.65;
  }

  @keyframes thread-loading-pulse {
    0%, 100% { opacity: 0.35; transform: scaleX(0.45); }
    50% { opacity: 1; transform: scaleX(1); }
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

  .trail-active-version {
    border-color: var(--primary);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--primary) 35%, transparent);
  }

  .trail-discarded-version {
    background: color-mix(in srgb, var(--bg-200) 92%, transparent);
    border-left-color: var(--bg-400);
    border-color: color-mix(in srgb, var(--bg-400) 85%, transparent);
    opacity: 0.78;
  }

  .trail-discarded-version .trail-content,
  .trail-discarded-version .trail-time,
  .trail-discarded-version .version-name,
  .trail-discarded-version .version-engine-badge {
    color: var(--text-dim);
  }

  .trail-discarded-version .trail-version-title,
  .trail-discarded-version .trail-role {
    color: color-mix(in srgb, var(--text-dim) 88%, var(--secondary));
  }

  .trail-tune-version {
    border-color: color-mix(in srgb, var(--bg-300) 70%, transparent);
    background: color-mix(in srgb, var(--bg-100) 60%, transparent);
    opacity: 0.82;
  }

  .trail-tune-version .trail-role {
    color: var(--text-dim);
    font-size: 0.65rem;
  }

  .trail-tune-version .trail-version-title,
  .trail-tune-version .trail-content {
    font-size: 0.8em;
    color: var(--text-dim);
  }

  .trail-error {
    border-left-color: var(--danger);
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

  .trail-status {
    border: 1px solid var(--bg-400);
    padding: 1px 5px;
    font-size: 0.52rem;
    letter-spacing: 0.06em;
    color: var(--text-dim);
  }

  .trail-status--pending {
    border-color: var(--secondary);
    color: var(--secondary);
  }

  .trail-status--working {
    border-color: var(--primary);
    color: var(--primary);
  }

  .trail-status--error {
    border-color: var(--danger);
    color: var(--danger);
  }

  .trail-time {
    color: var(--text-dim);
    font-family: var(--font-mono);
  }

  .trail-header-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
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

  .trail-version-title {
    margin-bottom: 6px;
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.62rem;
    letter-spacing: 0.04em;
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .trail-image {
    display: block;
    width: 100%;
    height: auto;
    max-height: 200px;
    object-fit: contain;
  }

  .trail-image-button {
    display: block;
    width: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: zoom-in;
  }

  .visual-loupe {
    min-width: min(900px, 86vw);
    max-width: 86vw;
    max-height: 78vh;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    overflow: hidden;
  }

  .visual-loupe__image-frame {
    flex: 1;
    min-height: 0;
    border: 1px solid var(--bg-400);
    background: #000;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .visual-loupe__image {
    display: block;
    max-width: 100%;
    max-height: 70vh;
    object-fit: contain;
  }

  .visual-loupe__caption {
    flex: 0 0 auto;
    color: var(--text-dim);
    font-size: 0.72rem;
    line-height: 1.4;
    max-height: 5rem;
    overflow: auto;
    border: 1px solid var(--bg-400);
    background: var(--bg-100);
    padding: 8px;
  }

  .version-loupe {
    width: min(980px, 86vw);
    height: min(720px, 78vh);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    gap: 8px;
    padding: 10px;
    overflow: hidden;
  }

  .version-loupe__meta {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
    font-family: var(--font-mono);
    text-transform: uppercase;
    overflow: hidden;
  }

  .version-loupe__title {
    min-width: 0;
    color: var(--secondary);
    font-size: 0.72rem;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .version-loupe__subtitle {
    flex: 0 0 auto;
    color: var(--text-dim);
    font-size: 0.62rem;
  }

  .version-loupe__viewer {
    min-width: 0;
    min-height: 0;
    border: 1px solid var(--bg-400);
    background: #05070d;
    overflow: hidden;
  }

  .version-loupe__empty,
  .version-loupe__error {
    border: 1px solid var(--bg-400);
    background: var(--bg-100);
    color: var(--text-dim);
    padding: 8px;
    font-family: var(--font-mono);
    font-size: 0.66rem;
    overflow: auto;
  }

  .version-loupe__error {
    border-color: var(--danger);
    color: var(--danger);
  }

  .input-area {
    flex-shrink: 0;
    border-top: 1px solid var(--bg-300);
    padding: 6px 8px 8px;
    background: var(--bg-100);
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    overflow: hidden;
    position: sticky;
    bottom: 0;
    z-index: 1;
  }

  .prompt-input {
    width: 100%;
    min-height: 52px;
    max-height: 80px;
    padding: 8px 10px;
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
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: nowrap;
    min-height: 34px;
  }

  .prompt-actions__left,
  .prompt-actions__right {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .prompt-actions__right {
    margin-left: auto;
  }

  .voice-btn {
    min-width: 86px;
    justify-content: center;
    white-space: nowrap;
    user-select: none;
  }

  .voice-btn--active {
    color: var(--bg-100);
    background: var(--secondary);
    border-color: var(--secondary);
  }

  .voice-btn--busy {
    color: var(--primary);
    border-color: var(--primary);
  }

  .voice-status {
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.58rem;
    line-height: 1.35;
    max-height: 42px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: pre-wrap;
  }

  .voice-status--error {
    color: var(--red);
  }

  .workspace-capture-toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.64rem;
    letter-spacing: 0.08em;
    white-space: nowrap;
  }

  .workspace-capture-toggle input {
    width: 14px;
    height: 14px;
    accent-color: var(--primary);
  }

  .workspace-capture-hint {
    color: var(--text-dim);
    font-size: 0.58rem;
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .btn-primary {
    padding: 8px 16px;
    font-weight: bold;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    white-space: nowrap;
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

</style>
