<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { formatBackendError, startLivePaper, stopLivePaper } from './tauri/client';
  import { getLivePaper, livePaperStateLabel, type LivePaperSnapshot } from './livePaper';

  let { snapshot = $bindable<LivePaperSnapshot | null>(null), viewLabel, scopeKey, onStarted, onOpen, startDisabledReason = null }: {
    snapshot?: LivePaperSnapshot | null;
    viewLabel: 'front' | 'top' | 'side';
    scopeKey: string;
    onStarted: () => void;
    onOpen: () => void;
    startDisabledReason?: string | null;
  } = $props();

  let busy = $state(false);
  let error = $state('');
  let dialogElement: HTMLDialogElement;
  let visible = true;
  let waitingForFirstPreview = false;
  let matchesScope = $derived(snapshot?.scopeKey === scopeKey);
  let active = $derived(Boolean(snapshot && snapshot.state !== 'stopped'));
  let templateView = $derived(snapshot?.view ?? viewLabel);

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let receivedEvent = false;
    void listen<LivePaperSnapshot>('live-paper-state', (event) => {
      receivedEvent = true;
      if (!visible) return;
      const restoring = snapshot === null;
      snapshot = event.payload;
      if (restoring) onStarted();
      if (waitingForFirstPreview && snapshot.scopeKey === scopeKey && snapshot.cameraPreviewDataUrl) {
        waitingForFirstPreview = false;
        dialogElement.close();
      }
    }).then((cleanup) => {
      if (!visible) cleanup();
      else unlisten = cleanup;
      return getLivePaper();
    }).then((value) => {
      if (!visible || receivedEvent) return;
      snapshot = value;
      if (value) onStarted();
    }).catch((cause) => { if (visible) error = formatBackendError(cause); });
    return () => { visible = false; unlisten?.(); };
  });

  function openCamera(): void {
    waitingForFirstPreview = !snapshot?.cameraPreviewDataUrl;
    onOpen();
    dialogElement.showModal();
  }

  async function action(operation: () => Promise<LivePaperSnapshot>, started = false): Promise<void> {
    if (busy) return;
    busy = true;
    error = '';
    try {
      const next = await operation();
      if (!visible) return;
      snapshot = next;
      if (started) onStarted();
    } catch (cause) { if (visible) error = formatBackendError(cause); }
    finally { if (visible) busy = false; }
  }
</script>

<div class="camera-controls">
  <button class="action-button action-button--secondary camera-control" type="button" aria-label="Camera" title="Connect phone camera and print a view template" aria-haspopup="dialog" onclick={openCamera}>
    <svg aria-hidden="true" viewBox="0 0 20 16" width="14" height="12"><path d="M1 4h4l2-3h6l2 3h4v11H1zM13.5 9a3.5 3.5 0 1 0-7 0 3.5 3.5 0 0 0 7 0Z" /></svg>
    CAMERA
    <span class={['camera-indicator', active && matchesScope && 'camera-indicator--active']} aria-hidden="true"></span>
  </button>
  <dialog bind:this={dialogElement} class="camera-dialog" aria-labelledby="camera-dialog-title" onclose={() => waitingForFirstPreview = false}>
    <header class="camera-dialog__header">
      <h3 id="camera-dialog-title">Phone camera</h3>
      <button class="text-button" type="button" aria-label="Close phone camera settings" onclick={() => dialogElement.close()}>×</button>
    </header>
    <div class="camera-dialog__content">
      <div class="camera-state" aria-live="polite">
        <strong>{snapshot ? livePaperStateLabel(snapshot.state) : 'Connect a phone camera'}</strong>
        <span>{snapshot?.detail ?? 'Start capture, then scan the QR code with iPhone Safari on the same network.'}</span>
      </div>
      {#if scopeKey && snapshot && !matchesScope}
        <p class="scope-warning">{active ? 'Capture belongs to another model. Stop capture, then start live paper here.' : 'Saved paper belongs to another model. Start live paper here to capture.'}</p>
      {/if}
      {#if snapshot && active}
        <div class="pairing-details">
          <img class="pairing-qr" src={snapshot.phoneQrDataUrl} alt="Scan to open the phone camera" width="192" height="192" />
          <div class="pairing-instructions">
            <p>Scan with iPhone Safari. Allow camera access and keep the printed markers visible.</p>
            {#if snapshot.trustUrl}<a href={snapshot.trustUrl} target="_blank" rel="noreferrer">Trust local camera certificate</a>{/if}
            <a href={snapshot.phoneUrl} target="_blank" rel="noreferrer">Open phone camera</a>
            <label>Phone camera link<input aria-label="Phone camera link" readonly value={snapshot.phoneUrl} onfocus={(event) => event.currentTarget.select()} /></label>
          </div>
        </div>
      {/if}
      <section class="template-settings" aria-label="Paper template">
        <strong>{templateView.toUpperCase()} template</strong>
        <p>Choose FRONT, TOP, or SIDE in Sketch. Print its template at 100% and check the 50 mm segment. The camera stays connected when changing views.</p>
        {#if snapshot}
          <a class="action-button action-button--secondary" href={snapshot.templateDataUrl} download={`ecky-live-paper-${templateView}.pdf`}>Print {templateView.toUpperCase()} template</a>
        {/if}
      </section>
      <p id="camera-action-status" class={['camera-action-status', error && 'camera-action-status--error']} role="status">{error || startDisabledReason || (busy ? 'Updating camera session…' : '\u00a0')}</p>
      <footer class="camera-dialog__actions">
        <button class="action-button action-button--secondary" type="button" aria-describedby="camera-action-status" onclick={() => action(() => startLivePaper(scopeKey, viewLabel), true)} disabled={busy || active || Boolean(startDisabledReason)}>Start live paper</button>
        <button class="action-button action-button--secondary" type="button" onclick={() => action(() => stopLivePaper(snapshot!.sessionId))} disabled={busy || !active}>Stop live paper</button>
        <button class="action-button action-button--secondary" type="button" onclick={() => dialogElement.close()}>Done</button>
      </footer>
    </div>
  </dialog>
</div>

<style>
  .camera-controls { display: contents; }
  .camera-control { display: inline-flex; align-items: center; gap: 0.35rem; white-space: nowrap; }
  .camera-control svg { fill: none; stroke: currentColor; stroke-width: 1.4; }
  .camera-indicator { width: 0.3rem; height: 0.3rem; background: var(--profile-muted); opacity: 0.45; }
  .camera-indicator--active { background: var(--profile-primary); opacity: 1; }
  .camera-dialog { position: fixed; inset: 0; margin: auto; height: fit-content; width: min(38rem, calc(100vw - 2rem)); box-sizing: border-box; max-height: calc(100vh - 2rem); padding: 0; overflow: auto; border: 1px solid var(--profile-secondary); border-radius: 0; background: var(--profile-panel); color: var(--profile-text); }
  .camera-dialog::backdrop { background: rgb(0 0 0 / 72%); }
  .camera-dialog__header { display: flex; justify-content: space-between; align-items: center; padding: 0.65rem 0.85rem; border-bottom: 1px solid var(--profile-border); }
  h3 { margin: 0; color: var(--profile-secondary); font-size: 0.78rem; letter-spacing: 0.08em; text-transform: uppercase; }
  .camera-dialog__content { display: grid; gap: 0.8rem; padding: 0.9rem; }
  .camera-state { display: grid; gap: 0.3rem; min-height: 3.4rem; font-size: 0.75rem; }
  .camera-state strong, .scope-warning { color: var(--profile-secondary); }
  .camera-state span, p { margin: 0; color: var(--profile-muted); font-size: 0.72rem; line-height: 1.5; }
  .pairing-details { display: grid; grid-template-columns: 192px minmax(0, 1fr); align-items: start; gap: 1rem; }
  .pairing-qr { display: block; width: 192px; height: 192px; background: #fff; }
  .pairing-instructions, .template-settings { display: grid; gap: 0.6rem; }
  .template-settings { padding-top: 0.8rem; border-top: 1px solid var(--profile-border); }
  .template-settings strong { color: var(--profile-secondary); font-size: 0.75rem; text-transform: uppercase; }
  .template-settings a { justify-self: start; }
  a { color: var(--profile-secondary); font-size: 0.72rem; }
  label { display: grid; gap: 0.3rem; color: var(--profile-muted); font-size: 0.72rem; }
  input { box-sizing: border-box; width: 100%; min-height: 2rem; padding: 0.35rem 0.45rem; border: 1px solid var(--profile-border); border-radius: 0; background: var(--profile-bg); color: var(--profile-text); font: inherit; }
  .camera-action-status { min-height: 1.2rem; overflow-wrap: anywhere; }
  .camera-action-status--error { color: var(--red, #df9e86); }
  .camera-dialog__actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 0.5rem; }
  @media (max-width: 600px) { .pairing-details { grid-template-columns: 1fr; } }
</style>
