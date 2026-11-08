import { expect, test } from '@playwright/test';
import { installProfileMocks, openSketch } from './profile-authoring-harness.js';

// Given a paired stream, when Rust publishes corrected paper and accepted primitives,
// then Sketch shows metric geometry, supports correction and uses normal profile Apply.
test('live paper projects backend geometry and transfers an analytic profile to normal Apply', async ({ page }, testInfo) => {
  await installProfileMocks(page, { livePaper: true });
  const workspace = await openSketch(page, false);
  await workspace.getByRole('button', { name: 'Start live paper' }).click();
  const start = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.livePaperStarts[0]);
  expect(start.scopeKey).toMatch(/^sha256:[a-f0-9]{64}$/);
  expect(start.view).toBe('front');
  await expect(workspace.getByRole('link', { name: 'Open phone camera' })).toHaveAttribute('href', /session=live-test/);
  await expect(workspace.getByRole('link', { name: 'Print paper template' })).toHaveAttribute('href', /^data:application\/pdf;base64,/);
  await expect(workspace.getByAltText('Scan to open the phone camera')).toHaveAttribute('src', /^data:image\/svg\+xml/);
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'streaming', detail: 'Paper tracked', sequence: 3,
    previewDataUrl: 'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="160" height="220"%3E%3Crect width="160" height="220" fill="white"/%3E%3C/svg%3E',
    accepted: [
      { kind: 'polyline', id: 'outline', pointsMm: [[10, 10], [90, 10], [90, 60], [10, 60]], closed: true, residualMm: 0.1 },
      { kind: 'circle', id: 'hole', centerMm: [50, 35], radiusMm: 5, residualMm: 0.1 },
    ], pending: [{ kind: 'line', id: 'join', startMm: [20, 20], endMm: [40, 20], residualMm: 0.2 }],
  }));
  await expect(workspace.getByTestId('live-paper-image')).toHaveCount(1);
  await expect(workspace.getByTestId('live-primitive-hole')).toHaveAttribute('r', '5');
  await workspace.getByLabel('Live paper primitive').selectOption('hole');
  await workspace.getByLabel('Live center X (mm)').fill('51');
  await workspace.getByLabel('Live center X (mm)').press('Tab');
  await expect(workspace.getByTestId('live-primitive-hole')).toHaveAttribute('cx', '51');
  await workspace.getByLabel('Live radius (mm)').fill('6');
  await workspace.getByLabel('Live radius (mm)').press('Tab');
  await expect(workspace.getByTestId('live-primitive-hole')).toHaveAttribute('r', '6');
  await workspace.getByLabel('Live paper primitive').selectOption('join');
  await workspace.getByRole('button', { name: 'Confirm geometry' }).click();
  await expect(workspace.getByText('3 accepted · 0 need review', { exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('live-paper-sketch.png') });
  await workspace.getByRole('button', { name: 'Use accepted profile' }).click();
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeEnabled();
  await workspace.getByRole('button', { name: 'Apply profile' }).click();
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__.applyCalls.length === 1);
  const request = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(request.sketchLoops).toEqual(expect.arrayContaining([
    expect.objectContaining({ role: 'hole', geometry: expect.objectContaining({ kind: 'circle', centerMm: [51, 35], radiusMm: 6 }) }),
  ]));
});

// Given accepted geometry, when markers disappear or transport pauses, accepted IDs
// remain visible and the pane keeps the same position and dimensions.
test('marker loss pauses updates while retaining accepted geometry and stable layout', async ({ page }) => {
  await installProfileMocks(page, { livePaper: true });
  const workspace = await openSketch(page, false);
  await workspace.getByRole('button', { name: 'Start live paper' }).click();
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'streaming', detail: 'Paper tracked', sequence: 4,
    accepted: [{ kind: 'line', id: 'stable-line', startMm: [10, 10], endMm: [80, 10], residualMm: 0.1 }],
  }));
  const pane = workspace.getByTestId('profile-view-front');
  const before = await pane.boundingBox();
  const viewBox = await pane.locator('svg').getAttribute('viewBox');
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'markerLost', detail: 'Show all four paper markers. Geometry updates paused.', sequence: 5,
  }));
  await expect(workspace.getByText('Show all four paper markers. Geometry updates paused.', { exact: true })).toBeVisible();
  await expect(workspace.getByTestId('live-primitive-stable-line')).toHaveCount(1);
  expect(await pane.boundingBox()).toEqual(before);
  await expect(pane.locator('svg')).toHaveAttribute('viewBox', viewBox!);
  await workspace.getByRole('button', { name: 'Stop live paper' }).click();
  await expect(workspace.getByTestId('live-primitive-stable-line')).toHaveCount(1);
  await expect(workspace.getByAltText('Scan to open the phone camera')).toHaveCount(0);
  await expect(workspace.getByRole('link', { name: 'Open phone camera' })).toHaveCount(0);
  await expect(workspace.getByRole('link', { name: 'Print paper template' })).toBeVisible();
  await workspace.getByLabel('Live paper primitive').selectOption('stable-line');
  await workspace.getByLabel('Live vertex 1 X (mm)').fill('12');
  await workspace.getByLabel('Live vertex 1 X (mm)').press('Tab');
  await expect(workspace.getByTestId('live-primitive-stable-line')).toHaveAttribute('x1', '12');
});


test('restoring another model capture never binds its image or geometry to this sketch', async ({ page }) => {
  await installProfileMocks(page, { livePaper: true, restoreForeignLivePaper: true });
  const workspace = await openSketch(page, false);
  await expect(workspace.getByText('Capture belongs to another model. Stop capture, then start live paper here.', { exact: true })).toBeVisible();
  await expect(workspace.getByTestId('live-primitive-foreign-line')).toHaveCount(0);
  await expect(workspace.getByLabel('Live paper primitive')).toBeDisabled();
  await expect(workspace.getByRole('button', { name: 'Use accepted profile' })).toBeDisabled();
  await workspace.getByRole('button', { name: 'Stop live paper' }).click();
  await workspace.getByRole('button', { name: 'Start live paper' }).click();
  await expect(workspace.getByTestId('live-primitive-foreign-line')).toHaveCount(0);
  await expect(workspace.getByText('Capture belongs to another model. Stop capture, then start live paper here.', { exact: true })).toHaveCount(0);
});
