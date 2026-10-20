import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const panelSource = readFileSync(new URL('./LibraryPanel.svelte', import.meta.url), 'utf8');
const bindingsSource = readFileSync(
  new URL('../../src-tauri/src/bindings.rs', import.meta.url),
  'utf8',
);
const clientSource = readFileSync(new URL('./tauri/client.ts', import.meta.url), 'utf8');

test('LibraryPanel submits tagged identity/path intents and projects backend-owned catalogs', () => {
  assert.match(panelSource, /\{#if freecadEnabled\}/);
  assert.match(panelSource, /libraryPanelIntent\(\{[\s\S]{0,80}kind: 'loadFreecad'/);
  assert.match(panelSource, /libraryPanelIntent\(\{[\s\S]{0,80}kind: 'setFreecadRoot'/);
  assert.match(panelSource, /libraryPanelIntent\(\{[\s\S]{0,80}kind: 'installPackage'/);
  assert.doesNotMatch(panelSource, /\bgetConfig\s*\(/);
  assert.doesNotMatch(panelSource, /\bsaveConfig\s*\(/);
  assert.doesNotMatch(panelSource, /\bsearchFreecadLibrary\s*\(/);
  assert.doesNotMatch(panelSource, /\binstallComponentPackageArchive\s*\(/);
  assert.doesNotMatch(panelSource, /\blistInstalledComponentPackageHeaders\s*\(/);
  assert.doesNotMatch(panelSource, /FREECAD_PAGE_SIZE/);
  assert.match(bindingsSource, /library_panel_intent/);
  assert.doesNotMatch(bindingsSource, /install_component_package_archive/);
  assert.doesNotMatch(bindingsSource, /list_installed_component_package_headers/);
  assert.doesNotMatch(bindingsSource, /search_freecad_library/);
  assert.doesNotMatch(bindingsSource, /read_fem_result/);
  assert.doesNotMatch(clientSource, /export async function readFemResult/);
});
