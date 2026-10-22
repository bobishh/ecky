import assert from 'node:assert/strict';
import test from 'node:test';
import {
  dockControls,
  moveDockFocus,
  reduceDockState,
  resolveLauncherAction,
} from './dock';

test('dock metadata preserves work sequence, groups, stable names, short labels, and icon ids', () => {
  const controls = dockControls(false);

  assert.deepEqual(
    controls.map(({ id }) => id),
    ['projects', 'params', 'dialogue', 'code', 'docs', 'library', 'capture', 'draw', 'settings'],
  );
  assert.deepEqual(
    controls.map(({ group }) => group),
    [
      'persistent',
      'persistent',
      'persistent',
      'persistent',
      'persistent',
      'persistent',
      'persistent',
      'utility',
      'utility',
    ],
  );
  assert.deepEqual(
    controls.map(({ accessibleName }) => accessibleName),
    [
      'Projects',
      'Parameters',
      'Dialogue',
      'Code inspector',
      'Ecky IR docs',
      'Reusable component library',
      'Work with external shapes',
      'Draw annotations',
      'Settings',
    ],
  );
  assert.ok(controls.some(({ id }) => id === 'docs'), 'documentation entry remains available');
  assert.ok(!controls.some(({ id }) => id === 'analysis'), 'FEM entry stays hidden from the main panel');
  assert.ok(controls.every(({ shortLabel, iconId }) => shortLabel.length > 0 && iconId.length > 0));
});

test('conditional terminal joins utility order without invalidating roving focus', () => {
  const withoutTerminal = dockControls(false).map(({ id }) => id);
  const withTerminal = dockControls(true).map(({ id }) => id);

  assert.deepEqual(withTerminal.slice(-3), ['draw', 'terminal', 'settings']);
  assert.equal(moveDockFocus(withoutTerminal, 'settings', 'ArrowRight'), 'projects');
  assert.equal(moveDockFocus(withTerminal, 'settings', 'ArrowLeft'), 'terminal');
  assert.equal(moveDockFocus(withTerminal, 'terminal', 'Home'), 'projects');
  assert.equal(moveDockFocus(withTerminal, 'projects', 'End'), 'settings');
});

test('roving focus recovers when current control disappeared', () => {
  const rendered = dockControls(false).map(({ id }) => id);

  assert.equal(moveDockFocus(rendered, 'terminal', 'ArrowRight'), 'projects');
  assert.equal(moveDockFocus(rendered, 'terminal', 'ArrowLeft'), 'settings');
});

test('dock state reduction covers closed, open, focused, mode, disabled, busy, and attention precedence', () => {
  assert.equal(reduceDockState({}), 'closed');
  assert.equal(reduceDockState({ visible: true }), 'open');
  assert.equal(reduceDockState({ visible: true, focused: true }), 'focused');
  assert.equal(reduceDockState({ visible: true, focused: true, activeMode: true }), 'activeMode');
  assert.equal(reduceDockState({ visible: true, activeMode: true, attention: true }), 'attention');
  assert.equal(reduceDockState({ visible: true, attention: true, busy: true }), 'busy');
  assert.equal(reduceDockState({ visible: true, busy: true, disabled: true }), 'disabled');
});

test('launcher action opens hidden, focuses background, and closes focused windows', () => {
  assert.equal(resolveLauncherAction({ visible: false, focused: false }), 'open');
  assert.equal(resolveLauncherAction({ visible: true, focused: false }), 'focus');
  assert.equal(resolveLauncherAction({ visible: true, focused: true }), 'close');
});
