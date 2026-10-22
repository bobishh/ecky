export type DockControlId =
  | 'projects'
  | 'params'
  | 'dialogue'
  | 'sketch'
  | 'code'
  | 'docs'
  | 'library'
  | 'capture'
  | 'analysis'
  | 'draw'
  | 'terminal'
  | 'settings';

export type DockIconId = DockControlId;
export type DockGroup = 'persistent' | 'utility';
export type DockNavigationKey = 'ArrowLeft' | 'ArrowRight' | 'Home' | 'End';
export type DockState = 'closed' | 'open' | 'focused' | 'activeMode' | 'disabled' | 'busy' | 'attention';
export type DockLauncherAction = 'open' | 'focus' | 'close';

export type DockStateInput = {
  visible?: boolean;
  focused?: boolean;
  activeMode?: boolean;
  disabled?: boolean;
  busy?: boolean;
  attention?: boolean;
};

export type DockControl = {
  id: DockControlId;
  group: DockGroup;
  accessibleName: string;
  shortLabel: string;
  iconId: DockIconId;
};

const BASE_CONTROLS: readonly DockControl[] = [
  { id: 'projects', group: 'persistent', accessibleName: 'Projects', shortLabel: 'PROJ', iconId: 'projects' },
  { id: 'params', group: 'persistent', accessibleName: 'Parameters', shortLabel: 'PARAMS', iconId: 'params' },
  { id: 'dialogue', group: 'persistent', accessibleName: 'Dialogue', shortLabel: 'TALK', iconId: 'dialogue' },
  { id: 'code', group: 'persistent', accessibleName: 'Code inspector', shortLabel: 'CODE', iconId: 'code' },
  { id: 'docs', group: 'persistent', accessibleName: 'Ecky IR docs', shortLabel: 'DOCS', iconId: 'docs' },
  { id: 'library', group: 'persistent', accessibleName: 'Reusable component library', shortLabel: 'LIB', iconId: 'library' },
  { id: 'capture', group: 'persistent', accessibleName: 'Work with external shapes', shortLabel: 'EXT', iconId: 'capture' },
  { id: 'draw', group: 'utility', accessibleName: 'Draw annotations', shortLabel: 'DRAW', iconId: 'draw' },
  { id: 'settings', group: 'utility', accessibleName: 'Settings', shortLabel: 'SET', iconId: 'settings' },
];

const TERMINAL_CONTROL: DockControl = {
  id: 'terminal',
  group: 'utility',
  accessibleName: 'Agent terminal',
  shortLabel: 'TERM',
  iconId: 'terminal',
};

export function dockControls(includeTerminal: boolean): DockControl[] {
  const controls = BASE_CONTROLS.map((control) => ({ ...control }));
  if (!includeTerminal) return controls;
  controls.splice(controls.length - 1, 0, { ...TERMINAL_CONTROL });
  return controls;
}

export function moveDockFocus<T extends string>(
  renderedIds: readonly T[],
  currentId: T,
  key: DockNavigationKey,
): T | null {
  if (renderedIds.length === 0) return null;
  if (key === 'Home') return renderedIds[0];
  if (key === 'End') return renderedIds[renderedIds.length - 1];

  const currentIndex = renderedIds.indexOf(currentId);
  if (currentIndex < 0) {
    return key === 'ArrowLeft' ? renderedIds[renderedIds.length - 1] : renderedIds[0];
  }
  const delta = key === 'ArrowLeft' ? -1 : 1;
  return renderedIds[(currentIndex + delta + renderedIds.length) % renderedIds.length];
}

export function reduceDockState(input: DockStateInput): DockState {
  if (input.disabled) return 'disabled';
  if (input.busy) return 'busy';
  if (input.attention) return 'attention';
  if (input.activeMode) return 'activeMode';
  if (input.focused) return 'focused';
  if (input.visible) return 'open';
  return 'closed';
}

export function resolveLauncherAction(input: Pick<DockStateInput, 'visible' | 'focused'>): DockLauncherAction {
  if (!input.visible) return 'open';
  return input.focused ? 'close' : 'focus';
}
