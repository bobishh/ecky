<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { formatBackendError } from './tauri/client';
  import {
    editLivePaperCoordinate, getLivePaper, livePaperCoordinates, livePaperStateLabel,
    type LivePaperPrimitive, type LivePaperSnapshot,
  } from './livePaper';

  let {
    snapshot = $bindable<LivePaperSnapshot | null>(null),
    selectedId = $bindable<string | null>(null),
    viewLabel,
    scopeKey,
    onStarted,
    onUseProfile,
    disabled = false,
  }: {
    snapshot?: LivePaperSnapshot | null;
    selectedId?: string | null;
    viewLabel: 'front' | 'top' | 'side';
    scopeKey: string;
    onStarted: () => void;
    onUseProfile: () => Promise<void>;
    disabled?: boolean;
  } = $props();

  let busy = $state(false);
  let error = $state('');
  let visible = true;
  let current = $derived(snapshot
    ? [...snapshot.accepted, ...snapshot.pending].find((item) => item.id === selectedId) ?? null
    : null);
  let needsConfirmation = $derived(Boolean(current && snapshot?.pending.some((item) => item.id === current.id)));
  let matchesScope = $derived(snapshot?.scopeKey === scopeKey);
  let active = $derived(Boolean(snapshot && snapshot.state !== 'stopped'));

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let receivedEvent = false;
    void listen<LivePaperSnapshot>('live-paper-state', (event) => {
      receivedEvent = true;
      if (visible) {
        const restoring = snapshot === null;
        snapshot = event.payload;
        if (restoring) onStarted();
      }
    }).then((cleanup) => {
      if (!visible) cleanup();
      else unlisten = cleanup;
      return getLivePaper();
    }).then((value) => {
      if (visible && !receivedEvent) {
        snapshot = value;
        if (value) onStarted();
      }
    }).catch((cause) => {
      if (visible) error = formatBackendError(cause);
    });
    return () => { visible = false; unlisten?.(); };
  });

  async function action(command: string, args?: Record<string, unknown>): Promise<void> {
    if (busy) return;
    busy = true;
    error = '';
    try {
      if (command === 'start_live_paper') {
        const next = await invoke<LivePaperSnapshot>(command, { scopeKey, view: viewLabel });
        if (!visible) return;
        snapshot = next;
        selectedId = null;
        onStarted();
      } else {
        await invoke(command, { sessionId: snapshot?.sessionId, ...args });
        const next = await getLivePaper();
        if (visible) snapshot = next;
      }
    } catch (cause) {
      if (visible) error = formatBackendError(cause);
    } finally {
      if (visible) busy = false;
    }
  }

  function editCoordinate(primitive: LivePaperPrimitive, index: number, axis: 0 | 1, value: string): void {
    try {
      if (value.trim() === '') throw new Error('Coordinate must be a finite number.');
      void action('edit_live_paper', { primitive: editLivePaperCoordinate(primitive, index, axis, Number(value)) });
    } catch (cause) { error = formatBackendError(cause); }
  }

  function editRadius(primitive: Extract<LivePaperPrimitive, { kind: 'circle' }>, value: string): void {
    const radiusMm = Number(value);
    if (!Number.isFinite(radiusMm) || radiusMm <= 0) { error = 'Radius must be greater than zero.'; return; }
    void action('edit_live_paper', { primitive: { ...primitive, radiusMm } });
  }

  async function useProfile(): Promise<void> {
    busy = true;
    error = '';
    try { await onUseProfile(); }
    catch (cause) { if (visible) error = formatBackendError(cause); }
    finally { if (visible) busy = false; }
  }
</script>

<section class="live-paper-controls" aria-label="Live paper">
  <h3>Live paper · {(matchesScope ? snapshot!.view : viewLabel).toUpperCase()}</h3>
  <p>Print at 100%, check the scale segment, then open the camera link on iPhone Safari.</p>
  <div class="actions">
    <button type="button" onclick={() => action('start_live_paper')} disabled={busy || active || disabled}>Start live paper</button>
    <button type="button" onclick={() => action('stop_live_paper')} disabled={busy || !active}>Stop live paper</button>
  </div>
  <div class="connection" aria-live="polite" data-testid="live-paper-state">
    <strong>{snapshot ? livePaperStateLabel(snapshot.state) : 'Camera not connected'}</strong>
    <span>{snapshot?.detail ?? 'Live paper appears beneath the sketch grid.'}</span>
  </div>
  {#if snapshot}
    {#if scopeKey && !matchesScope}
      <p class="scope-warning">{active ? 'Capture belongs to another model. Stop capture, then start live paper here.' : 'Saved paper belongs to another model. Start live paper here to capture.'}</p>
    {/if}
    <div class="pairing-slot">
      {#if active}
        <img class="pairing-qr" src={snapshot.phoneQrDataUrl} alt="Scan to open the phone camera" width="192" height="192" />
        <div class="links">
          {#if snapshot.trustUrl}<a href={snapshot.trustUrl} target="_blank" rel="noreferrer">Trust local camera certificate</a>{/if}
          <a href={snapshot.phoneUrl} target="_blank" rel="noreferrer">Open phone camera</a>
        </div>
        <label class="phone-link">Phone camera link<input aria-label="Phone camera link" readonly value={snapshot.phoneUrl} onfocus={(event) => event.currentTarget.select()} /></label>
      {:else}
        <p>Capture stopped. Accepted geometry remains editable. Start live paper to connect a camera.</p>
      {/if}
    </div>
    <a href={snapshot.templateDataUrl} download="ecky-live-paper-template.pdf">Print paper template</a>
    <p>{snapshot.accepted.length} accepted · {snapshot.pending.length} need review</p>
    <p>Image-derived · check dimensions. Confirm joins, crossings and broken strokes before use.</p>
    <label>Geometry
      <select aria-label="Live paper primitive" disabled={!matchesScope} value={selectedId ?? ''} onchange={(event) => selectedId = event.currentTarget.value || null}>
        <option value="">Select geometry</option>
        {#each snapshot.accepted as primitive (primitive.id)}<option value={primitive.id}>{primitive.kind} · {primitive.id}</option>{/each}
        {#each snapshot.pending as primitive (primitive.id)}<option value={primitive.id}>Review · {primitive.kind} · {primitive.id}</option>{/each}
      </select>
    </label>
    {#if current}
      <div class="coordinates">
        {#each livePaperCoordinates(current) as point, index (index)}
          <label>{current.kind === 'circle' ? 'Center' : current.kind === 'point' ? 'Point' : `Vertex ${index + 1}`} X (mm)
            <input aria-label={`Live ${current.kind === 'circle' ? 'center' : `vertex ${index + 1}`} X (mm)`} type="number" step="0.1" value={point[0]} disabled={busy || !matchesScope} onchange={(event) => editCoordinate(current!, index, 0, event.currentTarget.value)} />
          </label>
          <label>Y (mm)<input aria-label={`Live ${current.kind === 'circle' ? 'center' : `vertex ${index + 1}`} Y (mm)`} type="number" step="0.1" value={point[1]} disabled={busy || !matchesScope} onchange={(event) => editCoordinate(current!, index, 1, event.currentTarget.value)} /></label>
        {/each}
        {#if current.kind === 'circle'}
          {@const circle = current}
          <label>Radius (mm)<input aria-label="Live radius (mm)" type="number" min="0.001" step="0.1" value={circle.radiusMm} disabled={busy || !matchesScope} onchange={(event) => editRadius(circle, event.currentTarget.value)} /></label>
        {/if}
      </div>
      <div class="actions">
        <button type="button" disabled={!needsConfirmation || busy || !matchesScope} onclick={() => action('confirm_live_paper', { primitiveId: current?.id })}>Confirm geometry</button>
        <button type="button" disabled={busy || !matchesScope} onclick={() => action('reject_live_paper', { primitiveId: current?.id })}>Remove geometry</button>
      </div>
    {/if}
    <button type="button" onclick={useProfile} disabled={busy || disabled || !matchesScope || snapshot.accepted.length === 0}>Use accepted profile</button>
    {#if snapshot.diagnostics.length}<ul>{#each snapshot.diagnostics as diagnostic, index (index)}<li>{diagnostic}</li>{/each}</ul>{/if}
  {/if}
  <p class="error" role="status">{error || '\u00a0'}</p>
</section>

<style>
  .live-paper-controls { display: grid; gap: 0.55rem; border-bottom: 1px solid var(--border, #38312b); padding-bottom: 1rem; }
  h3 { margin: 0; color: var(--secondary, #c7a382); font-size: 0.8rem; text-transform: uppercase; }
  p, ul { margin: 0; font-size: 0.75rem; line-height: 1.45; color: var(--text-muted, #a49c94); }
  ul { padding-left: 1.1rem; max-height: 8rem; overflow-y: auto; }
  .actions, .links { display: flex; gap: 0.4rem; flex-wrap: wrap; }
  button, input, select { box-sizing: border-box; min-width: 0; border: 1px solid var(--border, #514438); background: var(--bg-panel, #171716); color: var(--text, #e3d8cb); border-radius: 0; padding: 0.4rem; font: inherit; font-size: 0.75rem; }
  button { cursor: pointer; border-color: var(--primary, #ad8158); }
  button:disabled { opacity: 0.45; cursor: default; }
  label { display: grid; gap: 0.25rem; font-size: 0.7rem; color: var(--text-muted, #a49c94); }
  input, select { width: 100%; }
  .coordinates { display: grid; grid-template-columns: 1fr 1fr; gap: 0.4rem; max-height: 15rem; overflow-y: auto; }
  .connection { display: grid; gap: 0.25rem; min-height: 4.5rem; align-content: start; font-size: 0.75rem; }
  .connection strong { color: var(--secondary, #c7a382); }
  .connection span { color: var(--text-muted, #a49c94); overflow-wrap: anywhere; }
  a { color: var(--secondary, #c7a382); font-size: 0.75rem; }
  .pairing-slot { display: grid; align-content: start; gap: 0.55rem; min-height: 20rem; }
  .pairing-qr { display: block; width: 192px; height: 192px; max-width: 100%; background: #fff; }
  .scope-warning { color: var(--secondary, #d1b391); }
  .error { min-height: 2.2rem; color: #e6a087; overflow-wrap: anywhere; }
</style>
