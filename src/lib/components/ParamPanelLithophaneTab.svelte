<script lang="ts">
  import Dropdown from '../Dropdown.svelte';
  import type {
    ExportArtifact,
    LithophaneAttachment,
    LithophaneSide,
    ModelManifest,
    OverflowMode,
    ProjectionType,
  } from '../types/domain';

  let {
    modelManifest = null,
    attachments,
    selectedAttachment = null,
    selectedAttachmentId = null,
    exportArtifacts,
    previewImageUrl,
    onSelectAttachment,
    onAddAttachment,
    onDuplicateAttachment,
    onDeleteAttachment,
    onPatchAttachment,
    onPickImage,
    onClearImage,
    onSetProjection,
    onSetColorMode,
  }: {
    modelManifest?: ModelManifest | null;
    attachments: LithophaneAttachment[];
    selectedAttachment?: LithophaneAttachment | null;
    selectedAttachmentId?: string | null;
    exportArtifacts: ExportArtifact[];
    previewImageUrl: (path: string | null | undefined) => string | null;
    onSelectAttachment: (attachmentId: string) => void;
    onAddAttachment: () => void;
    onDuplicateAttachment: (attachment: LithophaneAttachment | null) => void;
    onDeleteAttachment: (attachmentId: string) => void;
    onPatchAttachment: (
      attachmentId: string,
      mutate: (attachment: LithophaneAttachment) => LithophaneAttachment,
      statusText?: string,
    ) => void;
    onPickImage: (attachmentId: string) => Promise<void> | void;
    onClearImage: (attachmentId: string) => void;
    onSetProjection: (attachmentId: string, projection: ProjectionType) => void;
    onSetColorMode: (attachmentId: string, mode: 'mono' | 'cmyk') => void;
  } = $props();

  function getInputValue(event: Event): string {
    return (event.currentTarget as HTMLInputElement).value;
  }

  function getInputChecked(event: Event): boolean {
    return (event.currentTarget as HTMLInputElement).checked;
  }
</script>

<div class="controls-head">
  <div class="section-label">LITHOPHANE ATTACHMENTS</div>
  <div class="context-strip-actions">
    <button class="btn btn-xs btn-ghost" onclick={onAddAttachment}>
      + PATCH
    </button>
    {#if selectedAttachment}
      <button
        class="btn btn-xs btn-ghost"
        onclick={() => onDuplicateAttachment(selectedAttachment)}
      >
        DUPLICATE
      </button>
      <button
        class="btn btn-xs btn-ghost"
        onclick={() => onDeleteAttachment(selectedAttachment.id)}
      >
        DELETE
      </button>
    {/if}
  </div>
</div>

{#if attachments.length > 0}
  <div class="part-strip">
    <div class="part-strip-list">
      {#each attachments as attachment}
        <button
          class="view-chip"
          class:view-chip-active={attachment.id === selectedAttachmentId}
          onclick={() => onSelectAttachment(attachment.id)}
        >
          <span>{attachment.source.kind === 'file' && attachment.source.imagePath
            ? attachment.source.imagePath.split(/[/\\]/).pop()
            : attachment.id}</span>
          <span class="semantic-source-badge">{attachment.enabled === false ? 'OFF' : attachment.color?.mode === 'cmyk' ? 'CMYK' : 'MONO'}</span>
        </button>
      {/each}
    </div>
  </div>

  {#if selectedAttachment}
    {@const activeLitho = selectedAttachment}
    {@const planarOnlyColor = activeLitho.placement?.projection === 'planar'}
    <div class="view-composer">
      <div class="composer-grid">
        <label class="primitive-picker">
          <input
            class="ui-checkbox"
            type="checkbox"
            checked={activeLitho.enabled !== false}
            onchange={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                enabled: getInputChecked(event),
              }), getInputChecked(event) ? 'Lithophane enabled.' : 'Lithophane disabled.')}
          />
          <div class="primitive-picker__body">
            <div class="primitive-picker__label">Attachment enabled</div>
            <div class="primitive-picker__meta">Disabled patches stay saved but skip render.</div>
          </div>
        </label>
        <div class="composer-field">
          <div class="composer-label">TARGET PART</div>
          <Dropdown
            options={(modelManifest?.parts || []).map((part) => ({ id: part.partId, name: part.label }))}
            value={activeLitho.targetPartId || null}
            onchange={(value) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                targetPartId: typeof value === 'string' ? value : '',
              }))}
            placeholder="Choose part..."
          />
        </div>
        <div class="composer-field">
          <div class="composer-label">IMAGE</div>
          <div class="composer-inline-actions">
            <button
              class="btn param-btn composer-image-select"
              onclick={() => onPickImage(activeLitho.id)}
            >
              {activeLitho.source.kind === 'file' && activeLitho.source.imagePath
                ? activeLitho.source.imagePath.split(/[/\\]/).pop()
                : 'Select Image...'}
            </button>
            {#if activeLitho.source.kind === 'file' && activeLitho.source.imagePath}
              <button
                class="btn btn-xs btn-ghost"
                onclick={() => onClearImage(activeLitho.id)}
              >
                CLEAR
              </button>
            {/if}
          </div>
        </div>
      </div>

      {#if activeLitho.source.kind === 'file' && activeLitho.source.imagePath}
        <div class="litho-preview">
          <img
            src={previewImageUrl(activeLitho.source.imagePath) ?? ''}
            alt="Lithophane source"
            class="litho-preview__image"
          />
        </div>
      {/if}

      <div class="composer-grid">
        <div class="composer-field">
          <div class="composer-label">SIDE</div>
          <Dropdown
            options={[
              { id: 'front', name: 'Front' },
              { id: 'back', name: 'Back' },
              { id: 'left', name: 'Left' },
              { id: 'right', name: 'Right' },
              { id: 'top', name: 'Top' },
              { id: 'bottom', name: 'Bottom' },
            ]}
            value={activeLitho.placement?.side}
            onchange={(value) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                placement: {
                  ...attachment.placement,
                  side: (typeof value === 'string' ? value : 'front') as LithophaneSide,
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <div class="composer-label">PROJECTION</div>
          <Dropdown
            options={[
              { id: 'auto', name: 'Auto' },
              { id: 'planar', name: 'Planar' },
              { id: 'cylindrical', name: 'Cylindrical' },
              { id: 'spherical', name: 'Spherical' },
            ]}
            value={activeLitho.placement?.projection}
            onchange={(value) =>
              onSetProjection(activeLitho.id, (typeof value === 'string' ? value : 'auto') as ProjectionType)}
          />
        </div>
        <div class="composer-field">
          <div class="composer-label">OVERFLOW</div>
          <Dropdown
            options={[
              { id: 'contain', name: 'Contain' },
              { id: 'cover', name: 'Cover' },
              { id: 'clamp', name: 'Clamp' },
              { id: 'bleed', name: 'Bleed' },
            ]}
            value={activeLitho.placement?.overflowMode}
            onchange={(value) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                placement: {
                  ...attachment.placement,
                  overflowMode: (typeof value === 'string' ? value : 'contain') as OverflowMode,
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <div class="composer-label">COLOR MODE</div>
          <Dropdown
            options={[
              { id: 'mono', name: 'Mono' },
              ...(planarOnlyColor ? [{ id: 'cmyk', name: 'CMYK' }] : []),
            ]}
            value={planarOnlyColor ? activeLitho.color?.mode : 'mono'}
            onchange={(value) => onSetColorMode(activeLitho.id, (typeof value === 'string' ? value : 'mono') as 'mono' | 'cmyk')}
          />
        </div>
      </div>

      {#if !planarOnlyColor}
        <div class="composer-note">
          CMYK export is only available for planar flat patches. Switch projection to PLANAR to unlock it.
        </div>
      {/if}

      <div class="composer-grid">
        <div class="composer-field">
          <label class="composer-label" for={`litho-width-${activeLitho.id}`}>WIDTH (MM)</label>
          <input
            id={`litho-width-${activeLitho.id}`}
            class="input-mono composer-input"
            type="number"
            step="0.1"
            value={activeLitho.placement?.widthMm ?? 0}
            oninput={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                placement: {
                  ...attachment.placement,
                  widthMm: Number(getInputValue(event)) || 0,
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <label class="composer-label" for={`litho-height-${activeLitho.id}`}>HEIGHT (MM)</label>
          <input
            id={`litho-height-${activeLitho.id}`}
            class="input-mono composer-input"
            type="number"
            step="0.1"
            value={activeLitho.placement?.heightMm ?? 0}
            oninput={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                placement: {
                  ...attachment.placement,
                  heightMm: Number(getInputValue(event)) || 0,
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <label class="composer-label" for={`litho-offset-x-${activeLitho.id}`}>OFFSET X (MM)</label>
          <input
            id={`litho-offset-x-${activeLitho.id}`}
            class="input-mono composer-input"
            type="number"
            step="0.1"
            value={activeLitho.placement?.offsetXMm ?? 0}
            oninput={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                placement: {
                  ...attachment.placement,
                  offsetXMm: Number(getInputValue(event)) || 0,
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <label class="composer-label" for={`litho-offset-y-${activeLitho.id}`}>OFFSET Y (MM)</label>
          <input
            id={`litho-offset-y-${activeLitho.id}`}
            class="input-mono composer-input"
            type="number"
            step="0.1"
            value={activeLitho.placement?.offsetYMm ?? 0}
            oninput={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                placement: {
                  ...attachment.placement,
                  offsetYMm: Number(getInputValue(event)) || 0,
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <label class="composer-label" for={`litho-rotation-${activeLitho.id}`}>ROTATION</label>
          <input
            id={`litho-rotation-${activeLitho.id}`}
            class="input-mono composer-input"
            type="number"
            step="1"
            value={activeLitho.placement?.rotationDeg ?? 0}
            oninput={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                placement: {
                  ...attachment.placement,
                  rotationDeg: Number(getInputValue(event)) || 0,
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <label class="composer-label" for={`litho-bleed-${activeLitho.id}`}>BLEED (MM)</label>
          <input
            id={`litho-bleed-${activeLitho.id}`}
            class="input-mono composer-input"
            type="number"
            step="0.1"
            value={activeLitho.placement?.bleedMarginMm ?? 0}
            oninput={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                placement: {
                  ...attachment.placement,
                  bleedMarginMm: Math.max(0, Number(getInputValue(event)) || 0),
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <label class="composer-label" for={`litho-depth-${activeLitho.id}`}>DEPTH (MM)</label>
          <input
            id={`litho-depth-${activeLitho.id}`}
            class="input-mono composer-input"
            type="number"
            step="0.1"
            value={activeLitho.relief?.depthMm ?? 2}
            oninput={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                relief: {
                  ...attachment.relief,
                  depthMm: Math.max(0.1, Number(getInputValue(event)) || 2),
                },
              }))}
          />
        </div>
        <div class="composer-field">
          <label class="composer-label" for={`litho-channel-${activeLitho.id}`}>CHANNEL THICKNESS</label>
          <input
            id={`litho-channel-${activeLitho.id}`}
            class="input-mono composer-input"
            type="number"
            step="0.05"
            value={activeLitho.color?.channelThicknessMm ?? 0.4}
            oninput={(event) =>
              onPatchAttachment(activeLitho.id, (attachment) => ({
                ...attachment,
                color: {
                  ...attachment.color,
                  channelThicknessMm: Math.max(0.05, Number(getInputValue(event)) || 0.4),
                },
              }))}
          />
        </div>
      </div>

      <label class="primitive-picker">
        <input
          class="ui-checkbox"
          type="checkbox"
          checked={activeLitho.relief?.invert ?? false}
          onchange={(event) =>
            onPatchAttachment(activeLitho.id, (attachment) => ({
              ...attachment,
              relief: {
                ...attachment.relief,
                invert: getInputChecked(event),
              },
            }), getInputChecked(event) ? 'Lithophane inversion enabled.' : 'Lithophane inversion disabled.')}
        />
        <div class="primitive-picker__body">
          <div class="primitive-picker__label">Invert relief</div>
          <div class="primitive-picker__meta">Bright pixels become shallow instead of deep.</div>
        </div>
      </label>

      {#if exportArtifacts.length > 0}
        <div class="warning-stack">
          {#each exportArtifacts as exportArtifact}
            <div class="warning-chip">
              <span>{exportArtifact.role.toUpperCase()}: {exportArtifact.label}</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
{:else}
  <div class="no-params">
    Add a lithophane patch to attach an image to the current model. It will render on Apply.
  </div>
{/if}

<style>
  .controls-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }

  .section-label {
    color: var(--secondary);
    font-size: 0.58rem;
    font-weight: bold;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .context-strip-actions,
  .part-strip-list,
  .warning-stack {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .part-strip {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .view-chip {
    display: inline-flex;
    align-items: flex-start;
    flex-wrap: wrap;
    gap: 6px;
    padding: 4px 8px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text-dim);
    font-size: 0.64rem;
    font-weight: 700;
    cursor: pointer;
    max-width: 100%;
    overflow: hidden;
    text-overflow: clip;
    white-space: normal;
    overflow-wrap: anywhere;
    text-align: left;
  }

  .view-chip-active {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-200));
    color: var(--text);
  }

  .semantic-source-badge {
    padding: 1px 5px;
    border: 1px solid color-mix(in srgb, var(--primary) 45%, var(--bg-400));
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-200));
    color: var(--primary);
    font-family: var(--font-mono);
    font-size: 0.52rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    flex-shrink: 0;
  }

  .view-composer {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-200) 88%, var(--secondary) 12%);
    overflow: hidden;
  }

  .composer-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 10px;
  }

  .composer-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .composer-label {
    color: var(--text-dim);
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.08em;
  }

  .composer-input {
    width: 100%;
  }

  .composer-inline-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .composer-image-select {
    flex: 1 1 auto;
    min-width: 0;
  }

  .composer-note {
    color: var(--text-dim);
    font-size: 0.68rem;
    line-height: 1.4;
  }

  .litho-preview {
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    padding: 8px;
    overflow: hidden;
  }

  .litho-preview__image {
    display: block;
    width: 100%;
    max-height: 180px;
    object-fit: contain;
    border: 1px solid var(--primary);
    background: var(--bg-100);
  }

  .primitive-picker {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    cursor: pointer;
  }

  .primitive-picker__body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .primitive-picker__label {
    color: var(--text);
    font-size: 0.78rem;
    font-weight: 700;
  }

  .primitive-picker__meta {
    color: var(--text-dim);
    font-size: 0.64rem;
    line-height: 1.35;
  }

  .ui-checkbox {
    -webkit-appearance: none;
    appearance: none;
    width: 18px;
    height: 18px;
    border: 1px solid color-mix(in srgb, var(--cad-tone-color, var(--primary)) 36%, var(--bg-300));
    background: var(--bg-100);
    display: inline-grid;
    place-content: center;
    cursor: pointer;
    margin: 0;
  }

  .ui-checkbox::after {
    content: '';
    width: 10px;
    height: 10px;
    background: var(--cad-tone-color, var(--primary));
    transform: scale(0);
    transition: transform 0.12s ease-in-out;
  }

  .ui-checkbox:checked::after {
    transform: scale(1);
  }

  .warning-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 3px 6px;
    border: 1px solid color-mix(in srgb, var(--primary) 45%, var(--bg-300));
    background: var(--bg-200);
    color: var(--primary);
    font-size: 0.58rem;
    font-weight: 500;
  }

  .no-params {
    font-size: 0.7rem;
    color: var(--text-dim);
    font-style: italic;
    padding: 20px;
    text-align: center;
  }
</style>
