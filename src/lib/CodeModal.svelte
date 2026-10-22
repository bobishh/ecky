<script lang="ts">
  import Window from './Window.svelte';
  import CodePanel from './CodePanel.svelte';
  import MacroDiffPanel from './MacroDiffPanel.svelte';
  import CodeSourceActions from './components/CodeSourceActions.svelte';
  import {
    seedCodeModalDraftField,
    shouldReseedCodeModalDraftFields,
  } from './codeModalDraftFields';
  import { manualVersionLabel } from './manualVersionLabels';
  import { composeMacroDiffPanelModel } from './macroDiffPanel';
  import type { SessionCodeDiffView } from './sessionActivity';
  import {
    canInsertVerifyTemplate,
    hasVerifyClause,
    insertVerifyTemplate,
    looksLikeEckyModelSource,
  } from './verifyTemplate';
  import { formatBackendError } from './tauri/client';
  import { looksLikeFreecadComponentSource } from './modelRuntime/freecadComponentSource';
  import type { CodeModalSourceAuthority } from './codeModalSource';

  /** Parse a line number (1-based) from an error message or object. */
  function parseErrorLine(error: unknown): number | null {
    // Check direct fields on AppError-shaped objects
    const startLine = (error as { startLine?: number | null } | null)?.startLine;
    const ctxStart = (error as { diagnosticContext?: { startLine?: number | null } | null } | null)
      ?.diagnosticContext?.startLine;
    const direct = startLine ?? ctxStart;
    if (typeof direct === 'number' && direct > 0) return direct;
    // Fallback: scan formatted text for 'line N'
    const text = formatBackendError(error);
    const match = text.match(/line\s+(\d+)/i);
    return match ? parseInt(match[1], 10) : null;
  }

  function authoringErrorDetails(error: unknown): { layer: string; fix: string } | null {
    const authored = error as {
      layer?: 'surface' | 'coreIr' | 'backend' | null;
      fix?: { hint?: string | null; suggestions?: string[] | null } | null;
    } | null;
    const layer =
      authored?.layer === 'surface'
        ? 'SURFACE'
        : authored?.layer === 'coreIr'
          ? 'CORE IR'
          : authored?.layer === 'backend'
            ? 'BACKEND'
            : '';
    const hint = `${authored?.fix?.hint ?? ''}`.trim();
    const suggestions = (authored?.fix?.suggestions ?? []).map((value) => `${value}`.trim()).filter(Boolean);
    const fix = hint && suggestions.length
      ? `${hint} Try: ${suggestions.join(', ')}`
      : hint || (suggestions.length ? `Try: ${suggestions.join(', ')}` : '');
    return layer || fix ? { layer, fix } : null;
  }

  type CodeModalApplyPayload = {
    code: string;
    title: string;
    versionName: string;
  };

  type CodeModalMode = 'version' | 'foreign-evidence' | 'sketch-preview' | 'docs-snippet';

  let {
    code = $bindable(''),
    evidence = '',
    mode = 'version',
    sourceLanguage = null,
    macroDiffView = null,
    title,
    draftScopeKey = '',
    defaultTitle = '',
    defaultVersionName = '',
    sourceThreadId = null,
    sourceMessageId = null,
    sourceAuthority = 'bound',
    highlightLine = null,
    z = 0,
    hidden = false,
    focused = true,
    onclose,
    onApplyVersion,
    onTranslateToEcky,
  }: {
    code?: string;
    evidence?: string;
    mode?: CodeModalMode;
    sourceLanguage?: string | null;
    macroDiffView?: SessionCodeDiffView | null;
    title: string;
    draftScopeKey?: string;
    defaultTitle?: string;
    defaultVersionName?: string;
    sourceThreadId?: string | null;
    sourceMessageId?: string | null;
    sourceAuthority?: CodeModalSourceAuthority;
    highlightLine?: number | null;
    z?: number;
    hidden?: boolean;
    focused?: boolean;
    onclose: () => void;
    onApplyVersion?: (payload: CodeModalApplyPayload) => Promise<void> | void;
    onTranslateToEcky?: (source: string) => Promise<void>;
  } = $props();

  let x = $state(60);
  let y = $state(40);
  let width = $state(960);
  let height = $state(620);

  let copyState = $state<'idle' | 'copied'>('idle');
  let verifyState = $state<'idle' | 'inserted' | 'exists'>('idle');
  let commitState = $state<'idle' | 'applying' | 'translating'>('idle');
  let commitError = $state('');
  let commitAuthoringError = $state<{ layer: string; fix: string } | null>(null);
  let errorLine = $state<number | null>(null);
  let draftTitle = $state('');
  let draftVersionName = $state('');
  let seededDraftScopeKey = $state('');
  let foreignTab = $state<'summary' | 'component'>('summary');
  const canMutateVersion = $derived(
    mode === 'version' || (mode === 'foreign-evidence' && foreignTab === 'component'),
  );
  const displayedCode = $derived(
    mode === 'foreign-evidence' && foreignTab === 'summary' ? evidence : code,
  );
  const componentSourceReady = $derived(
    mode !== 'foreign-evidence' || looksLikeFreecadComponentSource(code),
  );
  const effectiveDraftScopeKey = $derived(
    draftScopeKey || `${mode}:${title}:${defaultTitle}:${defaultVersionName}:${sourceLanguage ?? ''}`,
  );
  const macroDiffModel = $derived.by(() =>
    canMutateVersion && macroDiffView ? composeMacroDiffPanelModel(macroDiffView) : null,
  );

  $effect(() => {
    if (!shouldReseedCodeModalDraftFields(seededDraftScopeKey, effectiveDraftScopeKey, commitState)) return;
    draftTitle = seedCodeModalDraftField(defaultTitle, title);
    draftVersionName = manualVersionLabel(defaultVersionName);
    seededDraftScopeKey = effectiveDraftScopeKey;
    commitError = '';
    commitAuthoringError = null;
    errorLine = null;
    verifyState = 'idle';
    foreignTab = 'summary';
  });



  async function copyCode() {
    try {
      await navigator.clipboard.writeText(displayedCode);
      copyState = 'copied';
      setTimeout(() => copyState = 'idle', 2000);
    } catch (e: unknown) {
      console.error('Failed to copy code:', e);
    }
  }

  function applyPayload(): CodeModalApplyPayload {
    return {
      code,
      title: draftTitle.trim() || defaultTitle || title || 'Untitled model',
      versionName: manualVersionLabel(draftVersionName || defaultVersionName),
    };
  }

  async function handleApply() {
    if (!onApplyVersion || commitState !== 'idle') return;
    commitState = 'applying';
    commitError = '';
    commitAuthoringError = null;
    errorLine = null;
    try {
      await onApplyVersion(applyPayload());
    } catch (e: unknown) {
      console.error('Failed to apply code:', e);
      commitError = formatBackendError(e);
      commitAuthoringError = authoringErrorDetails(e);
      errorLine = parseErrorLine(e);
    } finally {
      commitState = 'idle';
    }
  }

  async function handleTranslateToEcky() {
    if (!onTranslateToEcky || commitState !== 'idle' || !code.trim()) return;
    commitState = 'translating';
    commitError = '';
    commitAuthoringError = null;
    errorLine = null;
    try {
      await onTranslateToEcky(code);
    } catch (e: unknown) {
      console.error('Failed to translate CAD source:', e);
      commitError = formatBackendError(e);
      commitAuthoringError = authoringErrorDetails(e);
      errorLine = parseErrorLine(e);
    } finally {
      commitState = 'idle';
    }
  }

  function handleCodeChange(nextCode: string) {
    code = nextCode;
    verifyState = 'idle';
  }

  function handleInsertVerify() {
    if (canInsertVerifyTemplate(code)) {
      code = insertVerifyTemplate(code);
      verifyState = 'inserted';
      return;
    }
    if (hasVerifyClause(code)) {
      verifyState = 'exists';
    }
  }
</script>

<Window 
  windowId="code"
  title={`MACRO INSPECTOR: ${title}`} 
  {onclose} 
  {z}
  {hidden}
  {focused}
  bind:x 
  bind:y 
  bind:width 
  bind:height
>
  <div class="code-modal-content">
    {#if mode === 'foreign-evidence'}
      <div class="foreign-tabs" role="tablist" aria-label="Imported CAD code views">
        <button
          class:active={foreignTab === 'summary'}
          type="button"
          role="tab"
          aria-selected={foreignTab === 'summary'}
          onclick={() => foreignTab = 'summary'}
        >SUMMARY</button>
        <button
          class:active={foreignTab === 'component'}
          type="button"
          role="tab"
          aria-selected={foreignTab === 'component'}
          onclick={() => foreignTab = 'component'}
        >COMPONENT</button>
      </div>
    {/if}
    {#if canMutateVersion}
      <div class="code-modal-topbar">
        <div class="commit-fields">
          <label class="commit-field commit-field-title">
            <span class="commit-field__label">Title</span>
            <input
              class="commit-input"
              bind:value={draftTitle}
              placeholder="Title"
              aria-label="Version title"
              disabled={commitState !== 'idle'}
            />
          </label>
          <label class="commit-field commit-field-version">
            <span class="commit-field__label">Version</span>
            <input
              class="commit-input commit-input-version"
              bind:value={draftVersionName}
              placeholder="Version"
              aria-label="Version name"
              disabled={commitState !== 'idle'}
            />
          </label>
        </div>
        <div class="commit-actions">
          <button
            class="btn btn-primary"
            onclick={handleApply}
            disabled={!onApplyVersion || !componentSourceReady || commitState !== 'idle'}
            title="Apply changes; this immediately creates a history version"
          >
            {#if commitState === 'applying'}
              APPLYING...
            {:else}
              APPLY
            {/if}
          </button>
        </div>
      </div>
    {/if}
    <div class="code-editor-area">
      {#if sourceAuthority === 'draft' && mode === 'version'}
        <div class="draft-source-notice" data-testid="code-draft-source-notice">
          RENDERED VIEWPORT SOURCE — APPLY CREATES THE NEXT VERSION IMMEDIATELY.
        </div>
      {/if}
      {#if mode === 'foreign-evidence' && foreignTab === 'summary'}
        <div class="foreign-evidence-notice">
          IMPORTED CAD EVIDENCE — READ ONLY. COMPONENT PARAMETERS APPLY THROUGH THE FREECAD RUNTIME.
        </div>
      {:else if mode === 'foreign-evidence'}
        <div class="foreign-evidence-notice component-source-notice">
          FREECAD-COMPONENT — SOURCE IDENTITY AND BINDINGS. EDIT PARAMETERS; APPLY USES THE IMPORTED COMPONENT RUNTIME.
        </div>
      {/if}
      <CodePanel
        code={displayedCode}
        sourceLanguage={mode === 'foreign-evidence' && foreignTab === 'component' ? 'ecky' : sourceLanguage}
        readOnly={mode === 'foreign-evidence' && foreignTab === 'summary'}
        highlightLine={errorLine ?? highlightLine}
        onchange={handleCodeChange}
      />
    </div>
    <MacroDiffPanel model={macroDiffModel} />
    <div class="code-modal-footer">
      <div class="footer-left">
        <button class="btn btn-secondary" onclick={copyCode}>
          {copyState === 'copied' ? 'COPIED!' : 'COPY CODE'}
        </button>
        {#if (canMutateVersion || mode === 'foreign-evidence') && sourceThreadId}
          <CodeSourceActions
            threadId={sourceThreadId}
            messageId={sourceMessageId}
            importedCad={mode === 'foreign-evidence'}
            baseSource={sourceAuthority === 'draft'}
          />
        {/if}
        {#if canMutateVersion && looksLikeEckyModelSource(code)}
          <button
            class="btn btn-secondary"
            onclick={handleInsertVerify}
            disabled={hasVerifyClause(code)}
            title={hasVerifyClause(code) ? 'This source already contains a verify clause.' : 'Append a top-level verify template to this Ecky model.'}
          >
            {#if verifyState === 'inserted'}
              VERIFY INSERTED
            {:else if hasVerifyClause(code)}
              VERIFY EXISTS
            {:else}
              INSERT VERIFY
            {/if}
          </button>
        {/if}
        {#if canMutateVersion && code.trim() && !looksLikeEckyModelSource(code)}
          <button
            class="btn btn-primary"
            onclick={handleTranslateToEcky}
            disabled={!onTranslateToEcky || commitState !== 'idle'}
            title="Send this foreign CAD source through the normal Ecky authoring and verification pipeline"
          >
            {commitState === 'translating' ? 'TRANSLATING...' : 'TRANSLATE TO ECKY'}
          </button>
        {/if}
        {#if commitError}
          <div class="commit-error" title={commitError}>
            <span>{commitError}</span>
            {#if commitAuthoringError?.layer || commitAuthoringError?.fix}
              <span class="commit-error-details">
                {#if commitAuthoringError?.layer}
                  <span class="commit-error-layer">{commitAuthoringError.layer}</span>
                {/if}
                {#if commitAuthoringError?.fix}
                  <span>{commitAuthoringError.fix}</span>
                {/if}
              </span>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  </div>
</Window>

<style>
  .code-modal-content {
    width: 100%;
    height: 100%;
    background: var(--bg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .code-modal-topbar {
    flex: 0 0 auto;
    padding: 10px 12px;
    background: color-mix(in srgb, var(--bg-100) 92%, var(--primary) 8%);
    border-bottom: 1px solid var(--bg-300);
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 12px;
    overflow: hidden;
  }

  .code-editor-area {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .draft-source-notice {
    flex: 0 0 auto;
    padding: 7px 10px;
    border-bottom: 1px solid var(--secondary);
    color: var(--secondary);
    background: color-mix(in srgb, var(--bg-100) 90%, var(--secondary) 10%);
    font-family: var(--font-mono);
    font-size: 0.66rem;
    letter-spacing: 0.04em;
    overflow: hidden;
  }

  .foreign-evidence-notice {
    flex: 0 0 auto;
    padding: 8px 12px;
    border-bottom: 1px solid var(--secondary);
    background: color-mix(in srgb, var(--bg-100) 88%, var(--secondary) 12%);
    color: var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.64rem;
    font-weight: 700;
    letter-spacing: 0.05em;
    overflow: hidden;
  }

  .foreign-tabs {
    display: flex;
    flex: 0 0 auto;
    gap: 8px;
    padding: 8px 12px 0;
    overflow: hidden;
  }

  .foreign-tabs button {
    padding: 7px 12px;
    border: 1px solid var(--border);
    border-radius: 0;
    background: var(--surface-raised);
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-weight: 700;
  }

  .foreign-tabs button.active {
    border-color: var(--secondary);
    color: var(--secondary);
  }

  .component-source-notice {
    color: var(--secondary);
  }

  .code-modal-footer {
    padding: 12px;
    background: var(--bg-100);
    border-top: 1px solid var(--bg-300);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    overflow: hidden;
  }

  .footer-left {
    display: flex;
    gap: 8px;
    align-items: center;
    min-width: 0;
  }

  .commit-error {
    max-width: 480px;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--red) 72%, var(--bg-300));
    background: color-mix(in srgb, var(--red) 14%, var(--bg-100));
    color: var(--text);
    font-size: 0.72rem;
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .commit-error-details {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 5px;
    color: var(--text-dim);
  }

  .commit-error-layer {
    border: 1px solid color-mix(in srgb, var(--secondary) 68%, var(--bg-300));
    color: var(--secondary);
    padding: 1px 5px;
    font-size: 0.58rem;
    font-weight: 700;
    letter-spacing: 0.08em;
  }

  .commit-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: flex-end;
    min-width: 0;
    flex: 0 0 auto;
  }

  .commit-fields {
    flex: 1 1 auto;
    display: grid;
    grid-template-columns: minmax(240px, 1fr) 140px;
    gap: 10px;
    min-width: 0;
  }

  .commit-field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  .commit-field__label {
    color: var(--text-dim);
    font-size: 0.58rem;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .commit-input {
    min-width: 0;
    width: 100%;
    height: 34px;
    border: 1px solid var(--bg-300);
    background: var(--bg);
    color: var(--text);
    padding: 0 10px;
    font-size: 0.72rem;
    font-family: inherit;
  }

  .commit-input-version {
    width: 100%;
  }
</style>
