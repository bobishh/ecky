<script lang="ts">
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import '../externalShapes.css';
  import PuppetViewport from './PuppetViewport.svelte';
  import { movePivot, parentChoices, reparent } from './puppetTransforms';
  import { formatBackendError, showSafeSaveDialog as safeSaveDialog, importPuppetPart, previewPuppet, savePuppet, openPuppet, exportPuppet } from '../tauri/client';
  import type { PuppetDocument, PuppetPart } from '../tauri/contracts';

  let { active = true, captureRunId = '', currentModelPath = '' }: {
    active?: boolean; captureRunId?: string; currentModelPath?: string;
  } = $props();
  let document = $state<PuppetDocument>({ schemaVersion: 1, name: 'My puppet', durationSecs: 2, stepped: false, parts: [] });
  let selectedId = $state('');
  const selected = $derived(document.parts.find(p => p.id === selectedId) ?? null);
  let busy = $state(false), message = $state(''), error = $state(''), previewError = $state('');
  let previewReady = $state(false);
  let previewRefresh = $state(0);
  let previewPath = $state(''), previewVersion = $state(''), playing = $state(false), picking = $state(false);
  let metres = $state(false), zUp = $state(true);
  let saved = $state(false), replacementPending = $state(false);
  const changed = $derived(previewVersion !== JSON.stringify(document));
  const totalTriangles = $derived(document.parts.reduce((sum,p) => sum+p.triangleCount,0));
  const choices = $derived(selected ? parentChoices(document.parts, selected.id) : []);

  $effect(() => {
    if (!active) {
      playing = false;
      picking = false;
      previewReady = false;
    }
  });

  function edit(part: Partial<PuppetPart>) {
    if (!selected) return;
    document = { ...document, parts: document.parts.map(p => p.id === selectedId ? { ...p, ...part } : p) };
    saved = false; playing = false; picking = false; message = '';
  }
  function editNumber(key: 'scale' | 'amplitudeDeg' | 'phaseDeg', event: Event) {
    edit({ [key]: (event.target as HTMLInputElement).valueAsNumber });
  }
  function editVector(key: 'positionMm' | 'pivotMm' | 'rotationDeg', axis: number, event: Event) {
    if (!selected) return;
    const value = [...selected[key]] as [number, number, number];
    value[axis] = (event.target as HTMLInputElement).valueAsNumber;
    if (key === 'pivotMm' && value.every(Number.isFinite)) {
      document = movePivot(document, selected.id, value); saved = false; playing = false; picking = false;
    } else edit({ [key]: value });
  }
  async function action(work: () => Promise<void>) {
    if (busy) return;
    busy = true; error = ''; message = ''; playing = false; picking = false;
    try { await work(); } catch (cause) { error = formatBackendError(cause); } finally { busy = false; }
  }
  async function preview() {
    previewError = ''; playing = false; picking = false;
    const snapshot = $state.snapshot(document);
    const path = await previewPuppet(snapshot);
    previewPath = path; previewVersion = JSON.stringify(snapshot);
    previewRefresh += 1;
  }
  async function add(sourcePath: string | null, runId: string | null = null) {
    const part = await importPuppetPart(sourcePath, runId, runId ? true : metres, runId ? false : zUp);
    document = { ...document, parts: [...document.parts, part] }; selectedId = part.id; saved = false;
    await preview();
  }
  async function choosePart() {
    const path = await openDialog({ multiple: false, filters: [{ name: 'Puppet part', extensions: ['stl','obj'] }] });
    if (typeof path === 'string') await add(path);
  }
  async function choosePuppet() {
    replacementPending = false;
    const path = await openDialog({ multiple: false, filters: [{ name: 'Ecky puppet', extensions: ['eckypuppet'] }] });
    if (typeof path !== 'string') return;
    const next = await openPuppet(path);
    document = next; selectedId = next.parts[0]?.id ?? ''; saved = true;
    await preview(); message = `Opened ${next.name}`;
  }
  async function save(exporting: boolean) {
    const stem = document.name.replace(/[^\p{L}\p{N}_-]+/gu, '-').replace(/^-|-$/g,'') || 'puppet';
    const extension = exporting ? 'glb' : 'eckypuppet';
    const path = await safeSaveDialog(`${stem}.${extension}`, exporting ? 'Game asset' : 'Ecky puppet', [extension]);
    if (!path) return;
    if (exporting) { await exportPuppet($state.snapshot(document), path); message = `Exported ${path}`; }
    else { await savePuppet($state.snapshot(document), path); saved = true; message = `Saved ${path}`; }
  }
  function remove() {
    if (!selected) return;
    // Detach direct children without moving their neutral pose.
    for (const child of document.parts.filter(p => p.parentId === selectedId)) document = reparent(document, child.id, selected.parentId);
    document = { ...document, parts: document.parts.filter(p => p.id !== selectedId) };
    selectedId = document.parts[0]?.id ?? ''; saved = false; playing = false; picking = false;
    if (!document.parts.length) { previewPath = ''; previewVersion = ''; }
  }
</script>

{#if active}
  <div class="puppet-builder external-workspace" role="tabpanel" aria-label="Puppet">
    <div class="toolbar">
      <label class="name">Puppet name <input aria-label="Puppet name" bind:value={document.name} oninput={() => saved = false} disabled={busy} /></label>
      <button disabled={busy} onclick={() => { if (document.parts.length && !saved) replacementPending = true; else void action(choosePuppet); }}>Open puppet</button>
      <button disabled={busy || !document.parts.length} onclick={() => action(() => save(false))}>Save puppet</button>
      <button class="primary" disabled={busy || !document.parts.length} onclick={() => action(() => save(true))}>Export GLB</button>
    </div>
    {#if replacementPending}
      <div class="replace">Opening another puppet replaces this unsaved assembly.
        <button onclick={() => action(choosePuppet)}>Discard and open</button><button onclick={() => replacementPending = false}>Keep editing</button>
      </div>
    {/if}
    <div class="workspace">
      <div class="stage">
        {#if previewPath}
          <PuppetViewport path={previewPath} refreshToken={previewRefresh} {selected} {playing} {picking} onpick={(point) => {
            if (selected) { document = movePivot(document, selected.id, point); saved = false; picking = false; void action(preview); }
          }} onerror={(detail) => previewError = detail} onready={(value) => previewReady = value} />
        {:else}
          <div class="empty">
            <span class="eyebrow">Rigid puppet</span>
            <h2>Build a character from parts</h2>
            <p>Start with a body, then add a head and limbs. Each piece stays rigid and moves around its joint.</p>
            <ol><li>Add an STL, a textured OBJ or your current scan.</li><li>Choose a parent and place the joint on the part.</li><li>Set a swing, preview the motion, then export to your game.</li></ol>
          </div>
        {/if}
        {#if document.parts.length}<div class="motion">
          <button disabled={busy || !document.parts.length} onclick={() => action(preview)}>{changed && previewPath ? 'Update preview' : 'Preview puppet'}</button>
          <button disabled={busy || !previewPath || !previewReady || changed || Boolean(previewError)} onclick={() => { playing = !playing; picking = false; }}>{playing ? 'Stop motion' : 'Play motion'}</button>
          <label>Loop (s) <input aria-label="Motion duration" type="number" min="0.2" max="60" step="0.1" bind:value={document.durationSecs} oninput={() => {saved = false; playing = false;}} disabled={busy} /></label>
          <label><input type="checkbox" bind:checked={document.stepped} onchange={() => {saved = false; playing = false;}} disabled={busy} /> Stop-motion</label>
        </div>
        <p class="hint">{totalTriangles.toLocaleString()} triangles · Y up · dimensions in mm · GLB exports in metres</p>
        {#if totalTriangles > 300000}<p class="hint">This is a dense scan. Check performance in your game before adding many copies.</p>{/if}
        {/if}
        {#if previewError}<p class="error" role="alert">{previewError}</p>{/if}
      </div>
      <div class="properties">
        <section class="inspector-section" aria-label="Parts"><h3>Parts <span>{document.parts.length}</span></h3>
        <div class="imports">
          <button disabled={busy || document.parts.length >= 64} onclick={() => action(choosePart)}>Add part file</button>
          <button disabled={busy || !captureRunId || document.parts.length >= 64} onclick={() => action(() => add(null, captureRunId))}>Add current scan</button>
          {#if currentModelPath}<button disabled={busy || document.parts.length >= 64} onclick={() => action(async () => {
            const oldMetres = metres, oldUp = zUp; metres = false; zUp = true;
            try { await add(currentModelPath); } finally { metres = oldMetres; zUp = oldUp; }
          })}>Add current model</button>{/if}
        </div>
        <details class="import-settings"><summary>File settings · {metres ? 'metres' : 'mm'} · {zUp ? 'Z' : 'Y'} up</summary><div class="units">
          <label>File units <select aria-label="Import units" bind:value={metres} disabled={busy}><option value={false}>Millimetres</option><option value={true}>Metres</option></select></label>
          <label>File up axis <select aria-label="Import up axis" bind:value={zUp} disabled={busy}><option value={true}>Z (CAD / STL)</option><option value={false}>Y (Object Capture)</option></select></label>
        </div>
        <p class="hint">Current scan keeps the original colour texture. To use CAD crop edits, import the cropped STL instead.</p></details>
        <div class="parts" aria-label="Puppet parts">
          {#each document.parts as part (part.id)}
            <button class:active={part.id === selectedId} disabled={busy} onclick={() => { selectedId = part.id; picking = false; playing = false; }}>{part.name}<small>{part.textured ? 'Textured' : 'Plain'} · {part.triangleCount.toLocaleString()} triangles</small></button>
          {/each}
        </div>
        </section>
        {#if selected}
          <section class="inspector-section" aria-label="Mesh">
            <h3>Mesh</h3>
            <label>
              Smoothing limit (mm)
              <input type="range" min="0" max="2" step="0.05" value={selected.smoothingMm ?? 0}
                disabled={busy} oninput={(event) => edit({ smoothingMm: event.currentTarget.valueAsNumber || undefined })} />
            </label>
            <p class="hint">{selected.smoothingMm ?? 0} mm maximum movement. Set to 0 for the original mesh.</p>
          </section>
          <section class="inspector-section" aria-label="Selected part"><h3>Joint</h3>
          <label>Part name <input aria-label="Part name" value={selected.name} oninput={(event) => edit({name:event.currentTarget.value})} disabled={busy} /></label>
          <label>Parent part <select aria-label="Parent part" value={selected.parentId ?? ''} disabled={busy} onchange={(event) => {
            document = reparent(document, selected!.id, event.currentTarget.value || null); saved = false; playing = false; picking = false;
          }}><option value="">World</option>{#each choices as part}<option value={part.id}>{part.name}</option>{/each}</select></label>
          <button disabled={busy || !previewPath || !previewReady || changed || Boolean(previewError)} onclick={() => { picking = !picking; playing = false; }}>{picking ? 'Cancel joint pick' : 'Pick joint on part'}</button>
          <details class="placement"><summary>Position, rotation and size</summary><div class="placement-fields">
          {#each [{key:'positionMm',label:'Joint position',unit:'mm, relative to parent'},{key:'pivotMm',label:'Joint point',unit:'mm, in imported part'},{key:'rotationDeg',label:'Rest rotation',unit:'degrees'}] as field}
            <fieldset disabled={busy}><legend>{field.label} <small>{field.unit}</small></legend><div class="vector">
              {#each ['X','Y','Z'] as axis, i}<label>{axis}<input aria-label={`${field.label} ${axis}`} type="number" step="0.1" value={selected[field.key as 'positionMm'|'pivotMm'|'rotationDeg'][i]} oninput={(event) => editVector(field.key as 'positionMm'|'pivotMm'|'rotationDeg',i,event)} /></label>{/each}
            </div></fieldset>
          {/each}

          <label>Part size multiplier <input aria-label="Part scale" type="number" min="0.0001" max="10000" step="0.1" value={selected.scale} disabled={busy} oninput={(event) => editNumber('scale',event)} /></label>
          </div></details>
          </section>
          <section class="inspector-section" aria-label="Part motion"><h3>Motion</h3>
          <div class="joint-motion">
            <label>Swing axis <select aria-label="Swing axis" value={selected.axis} disabled={busy} onchange={(event) => edit({axis:event.currentTarget.value})}>{#each ['x','y','z'] as axis}<option value={axis}>{axis.toUpperCase()}</option>{/each}</select></label>
            <label>Angle (°) <input aria-label="Swing angle" type="number" min="0" max="180" value={selected.amplitudeDeg} disabled={busy} oninput={(event) => editNumber('amplitudeDeg',event)} /></label>
            <label>Phase (°) <input aria-label="Swing phase" type="number" step="15" value={selected.phaseDeg} disabled={busy} oninput={(event) => editNumber('phaseDeg',event)} /></label>
          </div>
          <p class="hint">Set angle above zero to animate. Use opposite phases (0° / 180°) for alternating legs.</p>
          </section>
          <button class="remove" disabled={busy} onclick={remove}>Remove selected part</button>
        {/if}
      </div>
    </div>
    <div class="feedback" aria-live="polite">
      {#if error}<span class="error" role="alert">{error}</span>{/if}
      <span role="status">{busy ? 'Working…' : message || (document.parts.length ? (saved ? 'Puppet saved.' : 'Save puppet to keep the parts and joint settings for later.') : 'STL supports shape; OBJ can retain scan colours and textures.')}</span>
    </div>
  </div>
{/if}
<style>
  .puppet-builder { display: flex; flex: 1; flex-direction: column; gap: 10px; padding: 12px; min-width: 0; min-height: 0; color: var(--text); font-size: 13px; background: var(--bg-100); }
  .toolbar,.imports,.motion,.units,.joint-motion { display: flex; gap: 8px; align-items: end; flex-wrap: wrap; }
  .toolbar { flex: none; padding-bottom: 12px; border-bottom: 1px solid var(--bg-300); }
  .toolbar .name { flex: 1; min-width: 150px; }
  label { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  button,input,select { font: inherit; color: var(--text); background: var(--bg-200); border: 1px solid var(--bg-400); border-radius: 0; padding: 6px; min-width: 0; }
  button { cursor: pointer; } button:hover:not(:disabled),button.active { border-color: var(--primary); } button:disabled { opacity: .45; cursor: default; }
  .primary { border-color: var(--secondary); background: var(--secondary); color: var(--bg-100); }
  .workspace { display: grid; grid-template-columns: minmax(0, 1fr) minmax(260px, 34%); gap: 12px; flex: 1; min-height: 0; }
  .stage { display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: auto; gap: 8px; }
  .stage :global(.viewport) { flex: 1; }
  .empty { display: flex; flex-direction: column; justify-content: center; align-items: flex-start; gap: 18px; flex: 1; min-height: 280px; padding: clamp(20px, 4cqi, 44px); color: var(--text-dim); }
  .properties { display: flex; flex-direction: column; gap: 0; overflow-y: auto; min-height: 0; min-width: 0; border-left: 1px solid var(--bg-300); padding: 0 0 0 16px; }
  .inspector-section { display: flex; flex-direction: column; gap: 12px; flex: none; padding: 16px 0; border-bottom: 1px solid var(--bg-300); }
  .inspector-section:first-child { padding-top: 0; }
  .inspector-section h3 { display: flex; align-items: center; justify-content: space-between; }
  .inspector-section h3 span { color: var(--text-dim); font-family: var(--font-mono); }
  .placement-fields { display: flex; flex-direction: column; gap: 12px; }
  .placement, .import-settings { border-top: 1px solid var(--bg-300); }
  .import-settings .hint { margin-top: 10px; }
  .eyebrow { color: var(--secondary); letter-spacing: .08em; text-transform: uppercase; font-size: 11px; }
  .empty p { margin: 0; max-width: 380px; }
  .empty ol { padding-left: 18px; margin: 0; font-size: 12px; line-height: 1.7; }
  .empty li + li { margin-top: 8px; }
  .parts { display: flex; flex-direction: column; gap: 4px; max-height: 140px; overflow-y: auto; flex-shrink: 0; }
  .parts button { display: flex; justify-content: space-between; gap: 10px; text-align: left; }.parts small { color: var(--text-dim); font-size: 11px; }
  fieldset { margin: 0; padding: 6px; border: 1px solid var(--bg-300); } legend small { color: var(--text-dim); font-size: 10px; }
  .vector { display: grid; grid-template-columns: repeat(3,minmax(0,1fr)); gap: 6px; } .vector input { width: 100%; box-sizing: border-box; }
  .motion input[type=number] { width: 65px; }.motion label:has(input[type=checkbox]) { flex-direction: row; align-items: center; padding-bottom: 6px; }
  .joint-motion { display: grid; grid-template-columns: repeat(3,minmax(0,1fr)); }
  .hint { margin: 0; color: var(--text-dim); font-size: 11px; line-height: 1.5; }.feedback { min-height: 30px; max-height: 100px; overflow-y: auto; display: flex; flex-direction: column; overflow-wrap: anywhere; }
  .remove { margin-top: 16px; flex: none; }
  .error,.remove { color: var(--red); }.error { overflow-wrap: anywhere; }.replace { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
  @container (max-width: 580px) { .workspace { display: flex; flex-direction: column; overflow-y: auto; }.stage { flex: none; min-height: 360px; }.properties { overflow: visible; flex: none; border-left: 0; border-top: 1px solid var(--bg-300); padding: 16px 0 0; } }
</style>
