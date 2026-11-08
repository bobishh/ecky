import { expect, test } from '@playwright/test';
import type { Locator } from '@playwright/test';
import { installProfileMocks, openSketch } from './profile-authoring-harness.js';

async function connectCamera(workspace: Locator) {
  await workspace.getByRole('button', { name: 'Camera', exact: true }).click();
  const dialog = workspace.getByRole('dialog', { name: 'Phone camera' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Start live paper' }).click();
  return dialog;
}

const cameraImage = 'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="640" height="480"%3E%3Crect width="640" height="480" fill="%23646b6a"/%3E%3Cpath d="M100 90h400v300H100z" fill="white"/%3E%3C/svg%3E';
const paperImage = 'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="160" height="220"%3E%3Crect width="160" height="220" fill="white"/%3E%3C/svg%3E';

test('camera starts without a CAD project while profile application remains disabled', async ({ page }) => {
  await installProfileMocks(page, { livePaper: true, missingProject: true });
  const workspace = await openSketch(page, false);
  await workspace.getByRole('button', { name: 'Camera', exact: true }).click();
  const dialog = workspace.getByRole('dialog', { name: 'Phone camera' });
  const start = dialog.getByRole('button', { name: 'Start live paper' });
  const stop = dialog.getByRole('button', { name: 'Stop live paper' });
  await expect(start).toBeEnabled();
  await expect(stop).toBeDisabled();
  await start.click();
  await expect(dialog.getByAltText('Scan to open the phone camera')).toBeVisible();
  await expect(start).toBeDisabled();
  await expect(stop).toBeEnabled();
  await stop.click();
  await expect(start).toBeEnabled();
  await expect(stop).toBeDisabled();
  await expect(dialog.getByAltText('Scan to open the phone camera')).toHaveCount(0);
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
});

test('camera binding failure explains why Start is disabled inside the camera dialog', async ({ page }) => {
  await installProfileMocks(page, { livePaper: true });
  await page.addInitScript(() => {
    Object.defineProperty(window.crypto.subtle, 'digest', { value: () => Promise.reject(new Error('Camera scope digest unavailable')) });
  });
  const workspace = await openSketch(page, false);
  await workspace.getByRole('button', { name: 'Camera', exact: true }).click();
  const dialog = workspace.getByRole('dialog', { name: 'Phone camera' });
  await expect(dialog.getByRole('button', { name: 'Start live paper' })).toBeDisabled();
  await expect(dialog.getByRole('status')).toContainText('Camera scope digest unavailable');
  await expect(dialog.getByRole('button', { name: 'Stop live paper' })).toBeDisabled();
});

// Given a paired stream, when Rust publishes corrected paper and accepted primitives,
// then Sketch shows metric geometry, supports correction and uses normal profile Apply.
test('live paper projects backend geometry and transfers an analytic profile to normal Apply', async ({ page }, testInfo) => {
  await installProfileMocks(page, { livePaper: true });
  const workspace = await openSketch(page, false);
  const dialog = await connectCamera(workspace);
  const start = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.livePaperStarts[0]);
  expect(start.scopeKey).toMatch(/^sha256:[a-f0-9]{64}$/);
  expect(start.view).toBe('front');
  await expect(dialog.getByRole('link', { name: 'Open phone camera' })).toHaveAttribute('href', /session=live-test/);
  await expect(dialog.getByRole('link', { name: 'Print FRONT template' })).toHaveAttribute('href', /^data:application\/pdf;base64,/);
  await expect(dialog.getByAltText('Scan to open the phone camera')).toHaveAttribute('src', /^data:image\/svg\+xml/);
  const dialogBounds = await dialog.boundingBox();
  expect(dialogBounds).not.toBeNull();
  expect(Math.abs(dialogBounds!.x + dialogBounds!.width / 2 - 640)).toBeLessThan(2);
  expect(Math.abs(dialogBounds!.y + dialogBounds!.height / 2 - 360)).toBeLessThan(2);
  await page.screenshot({ path: testInfo.outputPath('camera-pairing.png') });
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    previewKind: 'paper', state: 'streaming', detail: 'Paper tracked', sequence: 3,
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
  const dialog = await connectCamera(workspace);
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    previewKind: 'paper', state: 'streaming', detail: 'Paper tracked', sequence: 4,
    accepted: [{ kind: 'line', id: 'stable-line', startMm: [10, 10], endMm: [80, 10], residualMm: 0.1 }],
  }));
  const pane = workspace.getByTestId('profile-view-front');
  const before = await pane.boundingBox();
  const viewBox = await pane.locator('svg').getAttribute('viewBox');
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'markerLost', detail: 'Show all four paper markers. Geometry updates paused.', sequence: 5,
  }));
  await expect(workspace.getByTestId('live-paper-state').getByText('Show all four paper markers. Geometry updates paused.', { exact: true })).toBeVisible();
  await expect(workspace.getByTestId('live-primitive-stable-line')).toHaveCount(1);
  expect(await pane.boundingBox()).toEqual(before);
  await expect(pane.locator('svg')).toHaveAttribute('viewBox', viewBox!);
  await workspace.getByRole('button', { name: 'Camera', exact: true }).click();
  await dialog.getByRole('button', { name: 'Stop live paper' }).click();
  await expect(workspace.getByTestId('live-primitive-stable-line')).toHaveCount(1);
  await expect(workspace.getByAltText('Scan to open the phone camera')).toHaveCount(0);
  await expect(workspace.getByRole('link', { name: 'Open phone camera' })).toHaveCount(0);
  await expect(dialog.getByRole('link', { name: 'Print FRONT template' })).toBeVisible();
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await workspace.getByLabel('Live paper primitive').selectOption('stable-line');
  await workspace.getByLabel('Live vertex 1 X (mm)').fill('12');
  await workspace.getByLabel('Live vertex 1 X (mm)').press('Tab');
  await expect(workspace.getByTestId('live-primitive-stable-line')).toHaveAttribute('x1', '12');
});


test('restoring another model capture never binds its image or geometry to this sketch', async ({ page }) => {
  await installProfileMocks(page, { livePaper: true, restoreForeignLivePaper: true });
  const workspace = await openSketch(page, false);
  await workspace.getByRole('button', { name: 'Camera', exact: true }).click();
  const dialog = workspace.getByRole('dialog', { name: 'Phone camera' });
  await expect(dialog.getByText('Capture belongs to another model. Stop capture, then start live paper here.', { exact: true })).toBeVisible();
  await expect(workspace.getByTestId('live-primitive-foreign-line')).toHaveCount(0);
  await expect(workspace.getByLabel('Live paper primitive')).toHaveCount(0);
  await expect(workspace.getByRole('button', { name: 'Use accepted profile' })).toHaveCount(0);
  await dialog.getByRole('button', { name: 'Stop live paper' }).click();
  await dialog.getByRole('button', { name: 'Start live paper' }).click();
  await expect(workspace.getByTestId('live-primitive-foreign-line')).toHaveCount(0);
  await expect(workspace.getByText('Capture belongs to another model. Stop capture, then start live paper here.', { exact: true })).toHaveCount(0);
});


// Given a markerless incoming stream, when paper calibration is unavailable or lost,
// then camera pixels remain visible while metric overlays wait for calibrated paper.
test('camera source shows markerless video immediately and reacquires paper without layout shifts', async ({ page }, testInfo) => {
  await installProfileMocks(page, { livePaper: true });
  const workspace = await openSketch(page, false);
  const dialog = await connectCamera(workspace);
  await expect(dialog.getByAltText('Scan to open the phone camera')).toBeVisible();
  await expect(workspace.getByLabel('Sketch source')).toHaveValue('camera');
  await expect(workspace.getByTestId('camera-inspector')).toBeVisible();
  const pane = workspace.getByTestId('profile-view-front');
  const before = await pane.boundingBox();
  await page.evaluate((cameraPreviewDataUrl) => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'markerLost', previewKind: 'camera', cameraPreviewDataUrl, sequence: 1,
    detail: 'Align all four markers. Geometry updates paused.',
  }), cameraImage);
  await expect(dialog).toBeHidden();
  await expect(pane.getByTestId('live-camera-image')).toBeVisible();
  await expect(pane.getByText('Align all four markers', { exact: true })).toBeVisible();
  await expect(pane.locator('svg')).toBeHidden();
  expect(await pane.boundingBox()).toEqual(before);
  await page.screenshot({ path: testInfo.outputPath('camera-before-markers.png') });
  await workspace.getByRole('button', { name: 'Camera', exact: true }).click();
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({ sequence: 2 }));
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await page.evaluate((previewDataUrl) => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'streaming', previewKind: 'paper', previewDataUrl, sequence: 3,
    detail: 'Paper tracked', accepted: [{ kind: 'line', id: 'retained', startMm: [10, 10], endMm: [50, 10], residualMm: 0.1 }],
  }), paperImage);
  await expect(pane.getByTestId('live-paper-image')).toBeVisible();
  await expect(pane.getByTestId('live-primitive-retained')).toHaveCSS('visibility', 'visible');
  await expect(pane.getByTestId('live-camera-image')).toHaveCount(0);
  expect(await pane.boundingBox()).toEqual(before);
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({ state: 'markerLost', previewKind: 'camera', sequence: 4 }));
  await expect(pane.getByTestId('live-camera-image')).toBeVisible();
  await expect(pane.getByTestId('live-primitive-retained')).toBeHidden();
  await expect(workspace.getByText('1 accepted · 0 need review', { exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({ state: 'streaming', previewKind: 'paper', sequence: 5 }));
  await expect(pane.getByTestId('live-primitive-retained')).toHaveCSS('visibility', 'visible');
  expect(await pane.boundingBox()).toEqual(before);
});

// Given one camera connection, when Sketch selects another view, Rust switches the
// template and its geometry independently without restarting the camera session.
test('Sketch view tabs switch camera templates and restore each view geometry', async ({ page }) => {
  await installProfileMocks(page, { livePaper: true });
  const workspace = await openSketch(page, false);
  const dialog = await connectCamera(workspace);
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await page.evaluate(({ cameraPreviewDataUrl, previewDataUrl }) => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'streaming', previewKind: 'paper', cameraPreviewDataUrl, previewDataUrl,
    accepted: [{ kind: 'line', id: 'front-line', startMm: [10, 10], endMm: [30, 10], residualMm: 0.1 }],
  }), { cameraPreviewDataUrl: cameraImage, previewDataUrl: paperImage });
  await workspace.getByRole('button', { name: 'Show top view', exact: true }).click();
  const top = workspace.getByTestId('profile-view-top');
  await expect(top.getByTestId('live-camera-image')).toBeVisible();
  await expect(top.getByTestId('live-primitive-front-line')).toHaveCount(0);
  await expect(workspace.getByTestId('live-paper-state').getByText('Show the TOP template. Geometry updates paused.', { exact: true })).toBeVisible();
  const state = await page.evaluate(() => ({ starts: (window as any).__PROFILE_AUTHORING__.livePaperStarts, views: (window as any).__PROFILE_AUTHORING__.livePaperViewChanges }));
  expect(state.starts).toHaveLength(1);
  expect(state.views).toEqual([{ sessionId: 'live-test', view: 'top' }]);
  await workspace.getByRole('button', { name: 'Camera', exact: true }).click();
  await expect(dialog.getByRole('link', { name: 'Print TOP template' })).toHaveAttribute('download', 'ecky-live-paper-top.pdf');
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await page.evaluate((previewDataUrl) => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'streaming', previewKind: 'paper', previewDataUrl,
    accepted: [{ kind: 'circle', id: 'top-circle', centerMm: [30, 30], radiusMm: 10, residualMm: 0.1 }],
  }), paperImage);
  await expect(top.getByTestId('live-primitive-top-circle')).toBeVisible();
  await workspace.getByRole('button', { name: 'Show front view', exact: true }).click();
  await expect(workspace.getByText('1 accepted · 0 need review', { exact: true })).toBeVisible();
  await page.evaluate((previewDataUrl) => (window as any).__PROFILE_AUTHORING__.emitLivePaper({ state: 'streaming', previewKind: 'paper', previewDataUrl }), paperImage);
  await expect(workspace.getByTestId('live-primitive-front-line')).toHaveCSS('visibility', 'visible');
  await expect(workspace.getByTestId('live-primitive-top-circle')).toHaveCount(0);
  const cameraStyle = await workspace.getByRole('button', { name: 'Camera', exact: true }).evaluate((element) => ({ radius: getComputedStyle(element).borderRadius, color: getComputedStyle(element).color }));
  const tabColor = await workspace.getByRole('button', { name: 'Show front view', exact: true }).evaluate((element) => getComputedStyle(element).color);
  expect(cameraStyle).toEqual({ radius: '0px', color: tabColor });
});


// Given FRONT geometry, when profile preparation is pending, view/source controls
// stay fixed; a response for another view cannot enter this view's normal profile.
test('camera profile transfer serializes view controls and rejects mismatched backend view', async ({ page }) => {
  await installProfileMocks(page, { livePaper: true, livePaperProfileView: 'top', holdLivePaperProfile: true });
  const workspace = await openSketch(page, false);
  const dialog = await connectCamera(workspace);
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.emitLivePaper({
    state: 'streaming', previewKind: 'paper', detail: 'Paper tracked',
    accepted: [{ kind: 'circle', id: 'front-circle', centerMm: [30, 30], radiusMm: 10, residualMm: 0.1 }],
  }));
  await workspace.getByRole('button', { name: 'Use accepted profile' }).click();
  await expect(workspace.getByRole('button', { name: 'Show top view', exact: true })).toBeDisabled();
  await expect(workspace.getByLabel('Sketch source')).toBeDisabled();
  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.resolveLivePaperProfile());
  await expect(workspace.getByTestId('profile-status')).toContainText('Camera view changed');
  await expect(workspace.getByLabel('Sketch source')).toHaveValue('camera');
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
  await expect(workspace.getByRole('button', { name: 'Show top view', exact: true })).toBeEnabled();
  expect(await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls)).toHaveLength(0);
});
