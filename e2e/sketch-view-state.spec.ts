import { expect, test } from '@playwright/test';
import { installProfileMocks, openSketch, drawRectangle } from './profile-authoring-harness.js';

test('manual profiles stay in their own projection when selection changes and drafts resume', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page, false);
  const sketchWindow = page.locator('[data-window-id="sketch"]');
  const sketchBounds = await sketchWindow.boundingBox();
  const dockBounds = await page.getByTestId('workbench-bottom-dock').boundingBox();
  if (!sketchBounds || !dockBounds) throw new Error('Sketch work area or dock is not visible.');
  expect(sketchBounds.x).toBe(0);
  expect(sketchBounds.y).toBe(0);
  expect(sketchBounds.width).toBe(1280);
  expect(sketchBounds.y + sketchBounds.height).toBeLessThan(dockBounds.y);
  await expect(sketchWindow.locator('.window-resize-handle')).toBeHidden();
  await expect(workspace.getByTestId('profile-view-front')).toBeVisible();
  await expect(workspace.getByTestId('profile-view-top')).toHaveCount(0);
  await drawRectangle(workspace, 'front');
  if (process.env.SKETCH_SINGLE_SCREENSHOT_PATH) {
    await page.screenshot({ path: process.env.SKETCH_SINGLE_SCREENSHOT_PATH, fullPage: true });
  }

  await workspace.getByRole('button', { name: 'Show top view' }).click();
  await expect(workspace.getByTestId('profile-view-top')).toBeVisible();
  await expect(workspace.getByTestId('profile-view-front')).toHaveCount(0);
  await workspace.getByLabel('Drawing tool').selectOption('polyline');
  await workspace.getByRole('button', { name: 'Draw profile' }).click();
  const top = workspace.getByTestId('profile-view-top');
  const topBounds = await top.boundingBox();
  if (!topBounds) throw new Error('Top profile pane is not visible.');
  await top.click({ position: { x: topBounds.width * 0.25, y: topBounds.height * 0.25 } });
  await top.click({ position: { x: topBounds.width * 0.75, y: topBounds.height * 0.25 } });
  await top.click({ position: { x: topBounds.width * 0.75, y: topBounds.height * 0.75 } });
  await workspace.getByRole('button', { name: 'Close loop' }).click();
  await workspace.getByRole('button', { name: 'All three views', exact: true }).click();
  await expect(workspace.getByTestId('profile-view-front')).toBeVisible();
  await expect(workspace.getByTestId('profile-view-top')).toBeVisible();
  await expect(workspace.getByTestId('profile-view-side')).toBeVisible();
  await expect(workspace.getByTestId('profile-view-front').locator('polygon.selected-loop')).toHaveCount(1);
  await expect(top.locator('polygon.selected-loop')).toHaveCount(1);

  const frontPane = workspace.getByTestId('profile-view-front');
  await workspace.getByRole('button', { name: 'Select front projection' }).click();
  const frontGridSpacing = async () => {
    const paneBox = await frontPane.boundingBox();
    const viewBox = (await frontPane.locator('svg.sketch-pane__drawing').getAttribute('viewBox'))?.split(/\s+/).map(Number);
    if (!paneBox || !viewBox?.[2]) throw new Error('Front camera is not ready.');
    return paneBox.width * 10 / viewBox[2];
  };
  const gridCoversVisibleView = async () => {
    const viewBox = (await frontPane.locator('svg.sketch-pane__drawing').getAttribute('viewBox'))?.split(/\s+/).map(Number);
    const grid = frontPane.locator('svg.sketch-pane__drawing rect').first();
    if (!viewBox?.length) throw new Error('Front view box is not ready.');
    const rect = await grid.evaluate((element) => ['x', 'y', 'width', 'height'].map((name) => Number(element.getAttribute(name))));
    expect(rect).toEqual(viewBox);
  };
  const frontVertexDistance = async () => {
    const first = await frontPane.locator('circle.vertex-handle').nth(0).boundingBox();
    const second = await frontPane.locator('circle.vertex-handle').nth(1).boundingBox();
    if (!first || !second) throw new Error('Front sketch vertices are not visible.');
    return Math.hypot(first.x - second.x, first.y - second.y);
  };
  const threePaneGridSpacing = await frontGridSpacing();
  const threePaneVertexDistance = await frontVertexDistance();
  await gridCoversVisibleView();
  await workspace.getByRole('button', { name: 'Show front view' }).click();
  await expect.poll(frontGridSpacing).toBeCloseTo(threePaneGridSpacing, 0);
  await expect.poll(frontVertexDistance).toBeCloseTo(threePaneVertexDistance, 0);
  await gridCoversVisibleView();
  await page.setViewportSize({ width: 1024, height: 720 });
  await expect.poll(frontGridSpacing).toBeCloseTo(threePaneGridSpacing, 0);
  await expect.poll(frontVertexDistance).toBeCloseTo(threePaneVertexDistance, 0);
  await gridCoversVisibleView();

  expect(await frontPane.evaluate((pane) => getComputedStyle(pane).backgroundImage)).toBe('none');
  const svg = frontPane.locator('svg.sketch-pane__drawing');
  const initialViewBox = await svg.getAttribute('viewBox');
  const frontBoundsForPan = await frontPane.boundingBox();
  if (!frontBoundsForPan) throw new Error('Front profile pane is not visible for camera check.');
  await frontPane.hover();
  await page.mouse.wheel(0, -300);
  await expect.poll(() => svg.getAttribute('viewBox')).not.toBe(initialViewBox);
  const zoomedViewBox = await svg.getAttribute('viewBox');
  await page.keyboard.down('Shift');
  await page.mouse.move(frontBoundsForPan.x + frontBoundsForPan.width * 0.35, frontBoundsForPan.y + frontBoundsForPan.height * 0.45);
  await page.mouse.down();
  await page.mouse.move(frontBoundsForPan.x + frontBoundsForPan.width * 0.45, frontBoundsForPan.y + frontBoundsForPan.height * 0.55);
  await page.mouse.up();
  await page.keyboard.up('Shift');
  await expect.poll(() => svg.getAttribute('viewBox')).not.toBe(zoomedViewBox);
  const beforeZoomDistance = await frontVertexDistance();
  const beforeZoomGridSpacing = await frontGridSpacing();
  await frontPane.hover();
  await page.mouse.wheel(0, -200);
  await expect.poll(frontGridSpacing).toBeGreaterThan(beforeZoomGridSpacing * 1.1);
  await expect.poll(frontVertexDistance).toBeGreaterThan(beforeZoomDistance * 1.1);

  await workspace.getByRole('button', { name: 'All three views', exact: true }).click();
  await workspace.getByTestId('profile-view-front').getByRole('button', { name: 'FIT VIEW' }).click();
  await workspace.getByTestId('profile-view-top').getByRole('button', { name: 'FIT VIEW' }).click();
  if (process.env.SKETCH_ALL_THREE_SCREENSHOT_PATH) {
    await page.screenshot({ path: process.env.SKETCH_ALL_THREE_SCREENSHOT_PATH, fullPage: true });
  }
  await workspace.getByRole('button', { name: 'Select side projection' }).click();
  await expect(workspace.getByTestId('profile-view-side').locator('polygon')).toHaveCount(0);
  await expect(workspace.getByTestId('profile-view-front').locator('polygon.selected-loop')).toHaveCount(1);
  await expect(top.locator('polygon.selected-loop')).toHaveCount(1);

  await workspace.getByRole('button', { name: 'Select front projection' }).click();
  await workspace.getByRole('button', { name: 'Undo' }).click();
  await expect(top.locator('polygon.selected-loop')).toHaveCount(1);

  await workspace.getByRole('button', { name: 'Draw profile' }).click();
  const front = workspace.getByTestId('profile-view-front');
  const frontBounds = await front.boundingBox();
  if (!frontBounds) throw new Error('Front profile pane is not visible.');
  await front.click({ position: { x: frontBounds.width * 0.3, y: frontBounds.height * 0.35 } });
  await workspace.getByRole('button', { name: 'Select top projection' }).click();
  await workspace.getByRole('button', { name: 'Select front projection' }).click();
  await expect(front.locator('polygon.open-draft')).toHaveCount(1);
  await expect(front.locator('polygon.open-draft')).toHaveAttribute('points', /.+/);
  await front.click({ position: { x: frontBounds.width * 0.7, y: frontBounds.height * 0.35 } });
  await front.click({ position: { x: frontBounds.width * 0.7, y: frontBounds.height * 0.7 } });
  await workspace.getByRole('button', { name: 'Close loop' }).click();
  await expect(top.locator('polygon.selected-loop')).toHaveCount(1);
});

test('deleting the final converted projection loop stays deleted after switching views', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await workspace.getByRole('button', { name: 'Select top projection' }).click();
  await workspace.getByRole('button', { name: 'Convert to editable approximate polygon' }).click();
  await expect(workspace.getByTestId('profile-view-top').locator('polygon')).toHaveCount(2);
  await workspace.getByRole('button', { name: 'Remove selected loop' }).click();
  await workspace.getByRole('button', { name: 'Show side view' }).click();
  await workspace.getByRole('button', { name: 'Show top view' }).click();
  await expect(workspace.getByTestId('profile-view-top').locator('polygon')).toHaveCount(0);
});
