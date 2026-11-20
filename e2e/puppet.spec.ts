import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';
const puppetGlb = readFileSync(new URL('./fixtures/puppet.glb', import.meta.url));
import { installProfileMocks } from './profile-authoring-harness.js';

test('Given External Shapes is open When I build a puppet Then I can switch workflows and export from the same window', async ({ page }) => {
  await installProfileMocks(page);
  await page.route('**/mock/puppet.glb', route => route.fulfill({ contentType: 'model/gltf-binary', body: puppetGlb }));
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.evaluate(() => {
    const internals = (window as any).__TAURI_INTERNALS__;
    const previous = internals.invoke;
    let count = 0;
    (window as any).__PUPPET_CALLS__ = [];
    internals.invoke = async (cmd: string, args: any) => {
      (window as any).__PUPPET_CALLS__.push({ cmd, args });
      if (cmd === 'plugin:dialog|open') return '/mock/part.stl';
      if (cmd === 'safe_save_dialog') return `/mock/${args.defaultPath}`;
      if (cmd === 'puppet_import_part') {
        count += 1;
        return { id: `part${count}`, name: count === 1 ? 'Body' : 'Head', assetId: 'a'.repeat(64),
          parentId: null, positionMm: [0, 0, 0], pivotMm: [0, 0, 0], rotationDeg: [0, 0, 0],
          scale: 1, axis: 'y', amplitudeDeg: 0, phaseDeg: 0, triangleCount: 1, textured: false };
      }
      if (cmd === 'puppet_preview') return '/mock/puppet.glb';
      if (cmd === 'puppet_save' || cmd === 'puppet_export') return args.path;
      return previous(cmd, args);
    };
  });
  await page.getByRole('toolbar', { name: 'Workbench tools' }).getByRole('button', { name: 'Work with external shapes', exact: true }).click();
  const externalShapes = page.locator('[data-window-id="capture"]');
  const puppetTab = externalShapes.getByRole('tab', { name: 'PUPPET', exact: true });
  await puppetTab.click();
  await expect(puppetTab).toHaveAttribute('aria-selected', 'true');
  const builder = externalShapes.getByRole('tabpanel', { name: 'Puppet' });
  await expect(page.locator('[aria-modal="true"]')).toHaveCount(0);
  await builder.getByRole('button', { name: 'Add part file', exact: true }).click();
  await builder.getByRole('button', { name: 'Add part file', exact: true }).click();
  await expect(builder.getByTestId('puppet-viewport')).toHaveAttribute('data-status', 'Preview ready');
  await builder.getByLabel('Part name').fill('Unicorn head');
  await builder.getByLabel('Parent part').selectOption('part1');
  await builder.getByLabel('Swing angle').fill('20');
  const smoothing = builder.getByLabel('Smoothing limit (mm)');
  await expect(smoothing).toHaveValue('0');
  for (let step = 0; step < 4; step += 1) await smoothing.press('ArrowRight');
  await expect(smoothing).toHaveValue('0.2');
  await builder.getByRole('button', { name: 'Update preview', exact: true }).click();
  await builder.getByRole('button', { name: 'Play motion', exact: true }).click();
  await expect(builder.getByRole('button', { name: 'Stop motion', exact: true })).toBeVisible();
  await externalShapes.getByRole('tab', { name: 'CAPTURE', exact: true }).click();
  await expect(builder).toBeHidden();
  await expect(externalShapes.getByRole('heading', { name: 'Scan a real object' })).toBeVisible();
  await puppetTab.click();
  await expect(builder.getByLabel('Part name')).toHaveValue('Unicorn head');
  await expect(smoothing).toHaveValue('0.2');
  await expect(builder.getByRole('button', { name: 'Play motion', exact: true })).toBeEnabled();
  await builder.getByRole('button', { name: 'Save puppet', exact: true }).click();
  await expect(builder.getByRole('status')).toContainText('Saved');
  await builder.getByRole('button', { name: 'Export GLB', exact: true }).click();
  await expect(builder.getByRole('status')).toContainText('Exported');
  const calls = await page.evaluate(() => (window as any).__PUPPET_CALLS__);
  const previewed = calls.findLast((call: any) => call.cmd === 'puppet_preview').args.document;
  const exported = calls.findLast((call: any) => call.cmd === 'puppet_export').args.document;
  expect(previewed.parts[1].smoothingMm).toBe(0.2);
  expect(exported.parts[1]).toMatchObject({ name: 'Unicorn head', parentId: 'part1', amplitudeDeg: 20, smoothingMm: 0.2 });
});

test('failed import preserves the assembly and displays the backend reason', async ({ page }) => {
  await installProfileMocks(page);
  await page.route('**/mock/puppet.glb', route => route.fulfill({ contentType: 'model/gltf-binary', body: puppetGlb }));
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.evaluate(() => {
    const internals = (window as any).__TAURI_INTERNALS__;
    const previous = internals.invoke;
    internals.invoke = async (cmd: string, args: any) => {
      if (cmd === 'plugin:dialog|open') return '/mock/head.obj';
      if (cmd === 'puppet_import_part') throw { code: 'validation', message: 'Missing texture: head.png. Keep the OBJ, MTL and images together.' };
      return previous(cmd, args);
    };
  });
  await page.getByRole('toolbar', { name: 'Workbench tools' }).getByRole('button', { name: 'Work with external shapes', exact: true }).click();
  const externalShapes = page.locator('[data-window-id="capture"]');
  const puppetTab = externalShapes.getByRole('tab', { name: 'PUPPET', exact: true });
  await puppetTab.click();
  await expect(puppetTab).toHaveAttribute('aria-selected', 'true');
  const builder = externalShapes.getByRole('tabpanel', { name: 'Puppet' });
  await expect(page.locator('[aria-modal="true"]')).toHaveCount(0);
  await builder.getByRole('button', { name: 'Add part file', exact: true }).click();
  await expect(builder.getByRole('alert')).toContainText('Missing texture: head.png');
  await expect(builder.getByRole('button', { name: 'Export GLB', exact: true })).toBeDisabled();
  await expect(builder.getByRole('button', { name: 'Add part file', exact: true })).toBeEnabled();
});
