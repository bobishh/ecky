<script lang="ts">
  import type { Snippet } from 'svelte';
  import Window from '../Window.svelte';
  import type { WindowId, WindowState } from '../stores/windowStore';
  import DockIcon from './DockIcon.svelte';
  import {
    dockControls,
    moveDockFocus,
    reduceDockState,
    resolveLauncherAction,
    type DockControl,
    type DockControlId,
    type DockLauncherAction,
    type DockNavigationKey,
  } from './dock';

  type ShellId = 'code' | 'projects' | 'library' | 'capture' | 'analysis' | 'params' | 'settings' | 'activity' | 'dialogue' | 'docs' | 'terminal' | 'sketch';
  type TerminalDock = { agentLabel: string; attentionRequired: boolean };

  let {
    currentView,
    windowStates,
    mountedWindows,
    highlightTarget = null,
    drawMode,
    canDraw,
    drawUnavailableReason = null,
    terminalDock = null,
    overlayActionsEl = $bindable(null),
    onActivateWindow,
    onDrawToggle,
    onCloseView,
    onCloseWindow,
    projectsContent,
    libraryContent,
    sketchContent,
    paramsContent,
    captureContent,
    analysisContent,
    settingsContent,
    activityContent,
    dialogueContent,
    docsContent,
    terminalContent,
  }: {
    currentView: string;
    windowStates: Record<ShellId, WindowState>;
    mountedWindows: Partial<Record<WindowId, boolean>>;
    highlightTarget?: string | null;
    drawMode: boolean;
    canDraw: boolean;
    drawUnavailableReason?: string | null;
    terminalDock?: TerminalDock | null;
    overlayActionsEl?: HTMLElement | null;
    onActivateWindow: (id: WindowId, action: DockLauncherAction) => void;
    onDrawToggle: () => void;
    onCloseView: () => void;
    onCloseWindow: (id: WindowId) => void;
    projectsContent?: Snippet;
    libraryContent?: Snippet;
    sketchContent?: Snippet;
    paramsContent?: Snippet;
    captureContent?: Snippet;
    analysisContent?: Snippet;
    settingsContent?: Snippet;
    activityContent?: Snippet;
    dialogueContent?: Snippet;
    docsContent?: Snippet;
    terminalContent?: Snippet;
  } = $props();

  const shell = (id: ShellId) =>
    windowStates[id] ?? {
      visible: false,
      active: false,
      hasUnsavedChanges: false,
      pinned: false,
      minimized: false,
      focused: false,
      zIndex: 0,
    };
  const renderedControls = $derived(dockControls(Boolean(terminalDock)));
  const persistentControls = $derived(renderedControls.filter((control) => control.group === 'persistent'));
  const utilityControls = $derived(renderedControls.filter((control) => control.group === 'utility'));
  let rovingId = $state<DockControlId>('projects');
  let tooltipId = $state<DockControlId | null>(null);
  let tooltipLeft = $state(0);
  let tooltipTop = $state(0);
  const tooltipControl = $derived(renderedControls.find((control) => control.id === tooltipId) ?? null);

  $effect(() => {
    if (!renderedControls.some(({ id }) => id === rovingId)) {
      rovingId = renderedControls[0]?.id ?? 'projects';
    }
    if (tooltipId && !renderedControls.some(({ id }) => id === tooltipId)) {
      tooltipId = null;
    }
  });

  function isDisabled(id: DockControlId): boolean {
    return id === 'draw' && !canDraw;
  }

  function isPressed(id: DockControlId): boolean {
    if (id === 'draw') return drawMode;
    if (id === 'code') return shell('code').visible;
    if (id === 'sketch') return shell('sketch').visible;
    if (id === 'terminal') return shell('terminal').visible;
    return shell(id).visible;
  }

  function controlState(id: DockControlId): string {
    return reduceDockState({
      visible: isPressed(id),
      focused: id !== 'draw' && shell(id).active,
      activeMode: id === 'draw' && drawMode,
      disabled: isDisabled(id),
      attention: id === 'terminal' && Boolean(terminalDock?.attentionRequired),
    });
  }

  function descriptionFor(id: DockControlId): string | undefined {
    if (id === 'draw' && !canDraw) {
      return drawUnavailableReason ?? 'Drawing unavailable for this model';
    }
    if (id === 'terminal' && terminalDock?.attentionRequired) {
      return `${terminalDock.agentLabel} needs terminal input`;
    }
    return undefined;
  }

  function titleFor(control: DockControl): string {
    if (control.id === 'draw') {
      return descriptionFor('draw') ?? (drawMode ? 'Exit Draw Mode' : 'Draw Annotations');
    }
    if (control.id === 'terminal' && terminalDock) {
      return descriptionFor('terminal') ?? `Open ${terminalDock.agentLabel} terminal`;
    }
    return control.accessibleName;
  }

  function tooltipText(control: DockControl): string {
    return descriptionFor(control.id) ?? control.accessibleName;
  }

  function showTooltip(control: DockControl) {
    tooltipId = control.id;
    requestAnimationFrame(() => {
      const controlRect = overlayActionsEl
        ?.querySelector<HTMLElement>(`[data-dock-id="${control.id}"]`)
        ?.getBoundingClientRect();
      if (!controlRect) return;
      const proposedLeft = controlRect.left + controlRect.width / 2;
      tooltipLeft = proposedLeft;
      tooltipTop = controlRect.top - 8;
      requestAnimationFrame(() => {
        const tooltip = document.getElementById(`dock-tooltip-${control.id}`);
        if (!tooltip) return;
        const halfWidth = tooltip.getBoundingClientRect().width / 2;
        tooltipLeft = Math.max(halfWidth + 8, Math.min(window.innerWidth - halfWidth - 8, proposedLeft));
      });
    });
  }

  function hideTooltip(id: DockControlId) {
    if (tooltipId === id) tooltipId = null;
  }

  function activate(control: DockControl) {
    if (isDisabled(control.id)) return;
    if (control.id === 'draw') {
      onDrawToggle();
      return;
    }
    onActivateWindow(control.id, resolveLauncherAction({
      visible: isPressed(control.id),
      focused: shell(control.id).active,
    }));
  }

  function handleDockKeydown(event: KeyboardEvent, id: DockControlId) {
    if (event.key === 'Escape') {
      hideTooltip(id);
      event.preventDefault();
      return;
    }
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const nextId = moveDockFocus(
      renderedControls.map((control) => control.id),
      id,
      event.key as DockNavigationKey,
    );
    if (!nextId) return;
    rovingId = nextId;
    requestAnimationFrame(() => {
      overlayActionsEl?.querySelector<HTMLElement>(`[data-dock-id="${nextId}"]`)?.focus();
    });
  }
</script>

<div
  class="app-overlay-actions"
  class:app-overlay-actions--dock={currentView === 'workbench'}
  data-testid="workbench-bottom-dock"
  role={currentView === 'workbench' ? 'toolbar' : undefined}
  aria-label={currentView === 'workbench' ? 'Workbench tools' : undefined}
  aria-orientation={currentView === 'workbench' ? 'horizontal' : undefined}
  bind:this={overlayActionsEl}
>
  {#if currentView === 'workbench'}
    <div class="dock-group dock-group--primary" role="group" aria-label="Workspace windows">
      {#each persistentControls as control (control.id)}
        <button
          class="dock-btn"
          class:dock-btn--active={isPressed(control.id)}
          class:dock-btn--disabled={isDisabled(control.id)}
          class:onboarding-highlight={highlightTarget === control.id}
          type="button"
          data-onboarding-target={control.id}
          data-dock-id={control.id}
          data-state={controlState(control.id)}
          aria-label={control.accessibleName}
          aria-describedby={descriptionFor(control.id) ? `dock-description-${control.id}` : undefined}
          aria-pressed={isPressed(control.id)}
          aria-disabled={isDisabled(control.id) ? 'true' : undefined}
          tabindex={rovingId === control.id ? 0 : -1}
          title={titleFor(control)}
          onclick={() => activate(control)}
          onfocus={() => { rovingId = control.id; showTooltip(control); }}
          onblur={() => hideTooltip(control.id)}
          onpointerenter={() => showTooltip(control)}
          onpointerleave={() => hideTooltip(control.id)}
          onkeydown={(event) => handleDockKeydown(event, control.id)}
        >
          <DockIcon icon={control.iconId} />
          <span class="dock-label" aria-hidden="true">{control.shortLabel}</span>
          {#if descriptionFor(control.id)}
            <span class="dock-description" id={`dock-description-${control.id}`}>{descriptionFor(control.id)}</span>
          {/if}
        </button>
      {/each}
    </div>
    <div class="dock-separator" role="separator" aria-orientation="vertical"></div>
    <div class="dock-group dock-group--utility" role="group" aria-label="Modes and utilities">
      {#each utilityControls as control (control.id)}
        <button
          class="dock-btn"
          class:dock-btn--active={isPressed(control.id)}
          class:dock-btn--disabled={isDisabled(control.id)}
          class:terminal-overlay-btn-attention={control.id === 'terminal' && Boolean(terminalDock?.attentionRequired)}
          type="button"
          data-dock-id={control.id}
          data-state={controlState(control.id)}
          aria-label={control.accessibleName}
          aria-describedby={descriptionFor(control.id) ? `dock-description-${control.id}` : undefined}
          aria-pressed={isPressed(control.id)}
          aria-disabled={isDisabled(control.id) ? 'true' : undefined}
          tabindex={rovingId === control.id ? 0 : -1}
          title={titleFor(control)}
          onclick={() => activate(control)}
          onfocus={() => { rovingId = control.id; showTooltip(control); }}
          onblur={() => hideTooltip(control.id)}
          onpointerenter={() => showTooltip(control)}
          onpointerleave={() => hideTooltip(control.id)}
          onkeydown={(event) => handleDockKeydown(event, control.id)}
        >
          <DockIcon icon={control.iconId} />
          <span class="dock-label" aria-hidden="true">{control.shortLabel}</span>
          {#if descriptionFor(control.id)}
            <span class="dock-description" id={`dock-description-${control.id}`}>{descriptionFor(control.id)}</span>
          {/if}
        </button>
      {/each}
    </div>
  {:else}
    <button class="settings-overlay-btn" onclick={onCloseView} title="Close">×</button>
  {/if}
</div>

{#if currentView === 'workbench'}
  <div class="dock-tooltip-layer" data-testid="dock-tooltip-layer">
    {#if tooltipControl}
      <div
        class="dock-tooltip"
        id={`dock-tooltip-${tooltipControl.id}`}
        role="tooltip"
        style={`left: ${tooltipLeft}px; top: ${tooltipTop}px;`}
      >
        {tooltipText(tooltipControl)}
      </div>
    {/if}
  </div>
{/if}

{#if shell('projects').visible}<Window windowId="projects" {...shell('projects')} minWidth={320} minHeight={300} title="Projects" focused={shell('projects').active} hidden={!shell('projects').visible} highlighted={highlightTarget === 'projects'} onclose={() => onCloseWindow('projects')}>{#if projectsContent}{@render projectsContent()}{/if}</Window>{/if}
{#if shell('library').visible}<Window windowId="library" {...shell('library')} minWidth={320} minHeight={320} title="Library" focused={shell('library').active} hidden={!shell('library').visible} onclose={() => onCloseWindow('library')}>{#if libraryContent}{@render libraryContent()}{/if}</Window>{/if}
{#if mountedWindows.sketch}<Window windowId="sketch" {...shell('sketch')} minWidth={520} minHeight={360} title="Sketch" focused={shell('sketch').active} hidden={!shell('sketch').visible} onclose={() => onCloseWindow('sketch')}>{#if sketchContent}{@render sketchContent()}{/if}</Window>{/if}
{#if mountedWindows.params}<Window windowId="params" {...shell('params')} minWidth={280} minHeight={250} title="Parameters" focused={shell('params').active} hidden={!shell('params').visible} highlighted={highlightTarget === 'params'} onclose={() => onCloseWindow('params')}>{#if paramsContent}{@render paramsContent()}{/if}</Window>{/if}
{#if mountedWindows.capture}<Window windowId="capture" {...shell('capture')} minWidth={720} minHeight={520} title="External Shapes" focused={shell('capture').active} hidden={!shell('capture').visible} highlighted={highlightTarget === 'capture'} onclose={() => onCloseWindow('capture')}>{#if captureContent}{@render captureContent()}{/if}</Window>{/if}
{#if mountedWindows.analysis}<Window windowId="analysis" {...shell('analysis')} minWidth={340} minHeight={360} title="Structural Analysis" focused={shell('analysis').active} hidden={!shell('analysis').visible} onclose={() => onCloseWindow('analysis')}>{#if analysisContent}{@render analysisContent()}{/if}</Window>{/if}
{#if shell('settings').visible}<Window windowId="settings" {...shell('settings')} minWidth={400} minHeight={350} title="Settings" focused={shell('settings').active} hidden={!shell('settings').visible} highlighted={false} onclose={() => onCloseWindow('settings')}>{#if settingsContent}{@render settingsContent()}{/if}</Window>{/if}
{#if mountedWindows.activity}<Window windowId="activity" {...shell('activity')} minWidth={440} minHeight={320} title="Session Activity" focused={shell('activity').active} hidden={!shell('activity').visible} highlighted={false} onclose={() => onCloseWindow('activity')}>{#if activityContent}{@render activityContent()}{/if}</Window>{/if}
{#if mountedWindows.dialogue}<Window windowId="dialogue" {...shell('dialogue')} minWidth={350} minHeight={260} title="Dialogue" focused={shell('dialogue').active} hidden={!shell('dialogue').visible} highlighted={highlightTarget === 'dialogue'} onclose={() => onCloseWindow('dialogue')}>{#if dialogueContent}{@render dialogueContent()}{/if}</Window>{/if}
{#if mountedWindows.docs}<Window windowId="docs" {...shell('docs')} minWidth={760} minHeight={480} title="Ecky IR Docs" focused={shell('docs').active} hidden={!shell('docs').visible} highlighted={false} onclose={() => onCloseWindow('docs')}>{#if docsContent}{@render docsContent()}{/if}</Window>{/if}
{#if mountedWindows.terminal && terminalDock}<Window windowId="terminal" {...shell('terminal')} minWidth={400} minHeight={300} title={`${terminalDock.agentLabel} Terminal`} focused={shell('terminal').active} hidden={!shell('terminal').visible} highlighted={false} onclose={() => onCloseWindow('terminal')}>{#if terminalContent}{@render terminalContent()}{/if}</Window>{/if}

<style>
  .app-overlay-actions { position: absolute; top: 10px; right: 10px; z-index: 5001; display: flex; gap: 12px; align-items: flex-start; }
  .app-overlay-actions--dock { top: auto; right: auto; left: 50%; bottom: 16px; transform: translateX(-50%); max-width: calc(100vw - 24px); align-items: center; justify-content: center; gap: 6px; padding: 6px; border: 1px solid var(--bg-300); background: color-mix(in srgb, var(--bg-100) 94%, transparent); box-shadow: 0 10px 22px color-mix(in srgb, #000 42%, transparent); backdrop-filter: blur(10px); overflow: hidden; }
  .dock-group { display: flex; gap: 4px; min-width: 0; overflow: hidden; }
  .dock-separator { align-self: stretch; width: 1px; flex: 0 0 1px; margin: 5px 2px; background: linear-gradient(180deg, transparent, var(--secondary), transparent); }
  .dock-btn { position: relative; width: 44px; height: 44px; padding: 2px 1px 1px; background: color-mix(in srgb, var(--bg-200) 86%, transparent); border: 1px solid var(--bg-300); color: var(--text-dim); font-family: var(--font-mono); font-size: 0.72rem; font-weight: bold; letter-spacing: 0.1em; text-transform: uppercase; cursor: pointer; display: flex; flex-direction: column; gap: 1px; align-items: center; justify-content: center; backdrop-filter: blur(6px); box-shadow: none; overflow: hidden; }
  .dock-btn:hover, .dock-btn:focus-visible, .dock-btn--active { border-color: var(--primary); color: var(--primary); }
  .dock-btn--active { background: color-mix(in srgb, var(--primary) 16%, var(--bg-100)); }
  .dock-btn--disabled { opacity: 0.4; cursor: not-allowed; }
  .dock-btn--disabled:hover, .dock-btn--disabled:focus-visible { border-color: var(--bg-300); color: var(--text-dim); box-shadow: none; }
  .dock-label { display: block; max-width: 100%; overflow: hidden; color: currentColor; font-size: 0.45rem; line-height: 0.55rem; letter-spacing: 0.02em; text-overflow: clip; white-space: nowrap; }
  .dock-description { position: absolute; width: 1px; height: 1px; margin: -1px; padding: 0; overflow: hidden; clip: rect(0 0 0 0); border: 0; white-space: nowrap; }
  .dock-tooltip-layer { position: fixed; inset: 0; z-index: 6000; overflow: hidden; pointer-events: none; }
  .dock-tooltip { position: absolute; transform: translate(-50%, -100%); max-width: min(320px, calc(100vw - 16px)); padding: 5px 8px; overflow: hidden; border: 1px solid var(--secondary); border-radius: 0; background: color-mix(in srgb, var(--bg-100) 96%, transparent); color: var(--text); box-shadow: 0 6px 18px color-mix(in srgb, #000 48%, transparent); font-family: var(--font-mono); font-size: 0.6rem; letter-spacing: 0.04em; text-align: center; }
  .settings-overlay-btn { width: 34px; height: 34px; background: color-mix(in srgb, var(--bg-100) 90%, transparent); border: 1px solid var(--bg-300); color: var(--text); cursor: pointer; display: flex; align-items: center; justify-content: center; box-shadow: var(--shadow); }
  .settings-overlay-btn:hover { border-color: var(--primary); color: var(--primary); }
  .dock-btn[data-state='activeMode'] { border-color: var(--primary); background: color-mix(in srgb, var(--primary) 25%, var(--bg-100)); }
  .terminal-overlay-btn-attention { border-color: var(--secondary); color: var(--secondary); box-shadow: 0 0 10px color-mix(in srgb, var(--secondary) 50%, transparent); }
  :global(.onboarding-highlight) { position: relative !important; z-index: 1000 !important; box-shadow: 0 0 0 2px var(--primary), 0 0 40px rgba(74, 140, 92, 0.5) !important; pointer-events: none; background: var(--bg-100); }

  @media (max-width: 600px) {
    .app-overlay-actions--dock { gap: 4px; padding: 5px; }
    .dock-group { gap: 2px; }
    .dock-btn { width: 34px; height: 38px; flex: 0 1 34px; }
    .dock-label { display: none; }
  }
</style>
