<script lang="ts">
  import { onMount } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { Unicode11Addon } from '@xterm/addon-unicode11';
  import '@xterm/xterm/css/xterm.css';

  import {
    agentTerminalSessionKey,
    resolveAgentTerminalReplayText,
    resolveTerminalStreamWrite,
    shouldReplayTerminalOnVisibilityRestore,
  } from './agents/terminal';
  import type { AgentTerminalSnapshot } from './types/domain';

  let {
    snapshot,
    visible = true,
    onRawInput = (_data: string) => {},
    onResize = (_size: { cols: number; rows: number }) => {},
  }: {
    snapshot: AgentTerminalSnapshot;
    visible?: boolean;
    onRawInput?: (data: string) => void;
    onResize?: (size: { cols: number; rows: number }) => void;
  } = $props();

  let hostEl = $state<HTMLDivElement | null>(null);

  let term: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let unicode11Addon: Unicode11Addon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let lastSessionKey = '';
  let lastRenderedStream = '';
  let lastReportedSizeKey = '';
  let lastVisible = false;

  function activeStream(nextSnapshot: AgentTerminalSnapshot): string {
    return resolveAgentTerminalReplayText(nextSnapshot);
  }

  function fitTerminalAndNotify(nextSnapshot: AgentTerminalSnapshot) {
    if (!term || !fitAddon || !visible) return;

    const dims = fitAddon.proposeDimensions();
    if (!dims) return;
    fitAddon.fit();

    if (!nextSnapshot.active) return;
    const sizeKey = `${agentTerminalSessionKey(nextSnapshot)}:${dims.cols}x${dims.rows}`;
    if (sizeKey === lastReportedSizeKey) return;
    lastReportedSizeKey = sizeKey;
    onResize({ cols: dims.cols, rows: dims.rows });
  }

  function renderSnapshot(nextSnapshot: AgentTerminalSnapshot) {
    if (!term) return;

    const nextStream = activeStream(nextSnapshot);
    const nextSessionKey = agentTerminalSessionKey(nextSnapshot);
    if (nextSessionKey !== lastSessionKey) {
      term.reset();
      term.clear();
      lastSessionKey = nextSessionKey;
      lastRenderedStream = '';
      lastReportedSizeKey = '';
    }

    const update = resolveTerminalStreamWrite(lastRenderedStream, nextStream);
    if (update.mode === 'noop') return;

    if (update.mode === 'append') {
      if (update.data.length > 0) {
        term.write(update.data);
      }
    } else {
      term.reset();
      term.clear();
      if (update.data.length > 0) {
        term.write(update.data);
      }
    }

    lastRenderedStream = nextStream;
  }

  function replaySnapshot(nextSnapshot: AgentTerminalSnapshot) {
    if (!term) return;
    const nextStream = activeStream(nextSnapshot);
    term.reset();
    term.clear();
    if (nextStream.length > 0) {
      term.write(nextStream);
    }
    lastSessionKey = agentTerminalSessionKey(nextSnapshot);
    lastRenderedStream = nextStream;
    lastReportedSizeKey = '';
  }

  export function focusTerminal() {
    term?.focus();
  }

  onMount(() => {
    if (!hostEl) return;

    fitAddon = new FitAddon();
    term = new Terminal({
      allowProposedApi: true,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: 'block',
      fontFamily: 'IBM Plex Mono, Menlo, Monaco, Consolas, monospace',
      fontSize: 14,
      lineHeight: 1.24,
      scrollback: 5000,
      theme: {
        background: '#071019',
        foreground: '#d6e8cf',
        cursor: '#cfdba2',
        cursorAccent: '#071019',
        selectionBackground: 'rgba(186, 157, 81, 0.28)',
        black: '#071019',
        red: '#d96c6c',
        green: '#7eb07a',
        yellow: '#ba9d51',
        blue: '#6a8fbf',
        magenta: '#9b88c9',
        cyan: '#83b6be',
        white: '#d6e8cf',
        brightBlack: '#37506d',
        brightRed: '#ef8a8a',
        brightGreen: '#9fcea1',
        brightYellow: '#e0c673',
        brightBlue: '#8fb4df',
        brightMagenta: '#bca8e4',
        brightCyan: '#a5d8d9',
        brightWhite: '#f1f8eb',
      },
    });
    term.loadAddon(fitAddon);
    unicode11Addon = new Unicode11Addon();
    term.loadAddon(unicode11Addon);
    term.unicode.activeVersion = '11';
    term.open(hostEl);
    if (visible) {
      replaySnapshot(snapshot);
      fitTerminalAndNotify(snapshot);
    } else {
      lastSessionKey = agentTerminalSessionKey(snapshot);
      lastRenderedStream = activeStream(snapshot);
    }
    lastVisible = visible;

    term.onData((data) => {
      if (!snapshot.active) return;
      onRawInput(data);
    });

    resizeObserver = new ResizeObserver(() => {
      fitTerminalAndNotify(snapshot);
    });
    resizeObserver.observe(hostEl);

    return () => {
      resizeObserver?.disconnect();
      unicode11Addon?.dispose();
      fitAddon?.dispose();
      term?.dispose();
      resizeObserver = null;
      unicode11Addon = null;
      fitAddon = null;
      term = null;
      lastSessionKey = '';
      lastRenderedStream = '';
      lastReportedSizeKey = '';
    };
  });

  $effect(() => {
    if (!term) return;
    const nextSessionKey = agentTerminalSessionKey(snapshot);
    if (
      shouldReplayTerminalOnVisibilityRestore({
        previousSessionKey: lastSessionKey,
        nextSessionKey,
        wasVisible: lastVisible,
        isVisible: visible,
      })
    ) {
      replaySnapshot(snapshot);
      requestAnimationFrame(() => {
        fitTerminalAndNotify(snapshot);
      });
    } else if (visible) {
      renderSnapshot(snapshot);
      fitTerminalAndNotify(snapshot);
    } else {
      lastSessionKey = nextSessionKey;
      lastRenderedStream = activeStream(snapshot);
    }
    lastVisible = visible;
    if (visible && snapshot.active) {
      term.focus();
    }
  });
</script>

<div class="agent-terminal-surface" bind:this={hostEl}></div>

<style>
  .agent-terminal-surface {
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
    padding: 12px;
    background:
      radial-gradient(circle at top, color-mix(in srgb, var(--primary) 10%, transparent), transparent 42%),
      linear-gradient(180deg, rgba(6, 11, 17, 0.96), rgba(3, 8, 14, 0.98));
  }

  .agent-terminal-surface :global(.xterm) {
    height: 100%;
  }

  .agent-terminal-surface :global(.xterm-viewport) {
    overflow-y: auto !important;
  }

  .agent-terminal-surface :global(.xterm-screen),
  .agent-terminal-surface :global(.xterm-rows) {
    height: 100%;
  }
</style>
