import { expect, test } from '@playwright/test';
import { drawRectangle, installProfileMocks, openSketch } from './profile-authoring-harness.js';
test('manual rectangle applies as a normal model version without section reference', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await expect(workspace.getByRole('button', { name: 'Project all three views' })).toBeEnabled();
  await drawRectangle(workspace, 'top');
  await workspace.getByLabel('Profile name').fill('mounting-tab');
  await workspace.getByLabel('Depth (mm)').fill('8');
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCalls = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls);
  expect(applyCalls[0]).toMatchObject({
    threadId: 'profile-thread',
    baseMessageId: 'profile-version-1',
    partName: 'mounting-tab',
    depthMm: 8,
    plane: 'xy',
    reference: null,
  });
  expect(applyCalls[0].loops).toHaveLength(1);
  expect(applyCalls[0].loops[0].points).toHaveLength(4);
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionRequests.some((request: any) => request.preferredMessageId === 'profile-version-2'));
});

test('exact section applies profile asset while sampled display loops stay out of the request', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByLabel('Derivation mode').selectOption('exactSection');
  await workspace.getByLabel('Section plane').selectOption('xy');
  await workspace.getByLabel('Section offset (mm)').fill('3');
  await workspace.getByRole('button', { name: 'Extract exact section' }).click();
  await expect(workspace.getByText('EXACT CAD PROFILE', { exact: true })).toBeVisible();
  await expect(workspace.getByText(/XY section · offset 3.00 mm/i)).toBeVisible();
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeEnabled();
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.loops).toBeUndefined();
  expect(applyCall.reference).toEqual({
    sourceArtifactPath: '/mock/profile-model.step',
    sourceArtifactDigest: 'sha256:profile-source',
    operation: { kind: 'section', plane: 'xy', offsetMm: 3 },
    toleranceMm: 0.5,
    accuracy: 'exactBrep',
    profileAsset: { artifactPath: '/mock/profile-assets/section.step', artifactDigest: 'sha256:profile-section' },
  });
  const deriveCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.deriveCalls[0]);
  expect(deriveCall.artifactPath).toBe('/mock/profile-model.step');
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionRequests.some((request: any) => request.preferredMessageId === 'profile-version-2'));
});

test('approximate mesh section uses STL and submits editable loops with approximate provenance', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByLabel('Derivation mode').selectOption('meshSection');
  await workspace.getByRole('button', { name: 'Extract approximate section' }).click();
  await expect(workspace.getByText('APPROXIMATE PROFILE', { exact: true })).toBeVisible();
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const state = await page.evaluate(() => ({
    derive: (window as any).__PROFILE_AUTHORING__.deriveCalls[0],
    apply: (window as any).__PROFILE_AUTHORING__.applyCalls[0],
  }));
  expect(state.derive.artifactPath).toBe('/mock/model.stl');
  expect(state.apply.loops).toHaveLength(2);
  expect(state.apply.reference).toMatchObject({
    sourceArtifactPath: '/mock/model.stl',
    accuracy: 'approximateMesh',
    profileAsset: null,
  });
});

test('exact projected edges stay open until a closed profile is built', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByLabel('Derivation mode').selectOption('exactProjection');
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  const front = workspace.getByTestId('profile-view-front');
  await expect(front.locator('polyline.projected-edge')).toHaveCount(4);
  await expect(front.locator('polygon.outer')).toHaveCount(0);
  await expect(workspace.getByText('EXACT CAD PROJECTION', { exact: true })).toBeVisible();
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
  await expect(workspace.getByRole('button', { name: 'Build closed profile' })).toBeEnabled();

  await workspace.getByRole('button', { name: 'Build closed profile' }).click();
  await expect(front.locator('polygon.outer')).toBeVisible();
  await expect(workspace.getByText('EXACT CAD PROFILE', { exact: true })).toBeVisible();
  await expect(workspace.getByRole('button', { name: 'Convert to editable approximate polygon' })).toBeVisible();
  await expect(workspace.locator('circle.vertex-handle')).toHaveCount(0);

  await workspace.getByRole('button', { name: 'Convert to editable approximate polygon' }).click();
  await expect(workspace.getByText('APPROXIMATE PROFILE', { exact: true })).toBeVisible();
  await expect(workspace.locator('circle.vertex-handle')).toHaveCount(8);
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.loops).toHaveLength(2);
  expect(applyCall.reference).toMatchObject({
    sourceArtifactPath: '/mock/profile-model.step',
    operation: { kind: 'projection', view: 'front' },
    accuracy: 'approximatePolygon',
    profileAsset: null,
  });
});

test('projection visibility defaults to all edges and existing drawings keep their recorded mode', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);
  await workspace.getByLabel('Derivation mode').selectOption('exactProjection');

  const visibility = workspace.getByLabel('Projection edges');
  await expect(visibility).toHaveValue('allEdges');
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionDrawingCalls.length === 3);
  let calls = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.projectionDrawingCalls);
  expect(calls.map((call: any) => call.visibility)).toEqual(['allEdges', 'allEdges', 'allEdges']);
  await expect(workspace.getByTestId('profile-view-front').getByText('ALL EDGES · PROJECTED', { exact: true })).toBeVisible();

  await visibility.selectOption('visibleEdges');
  await expect(workspace.getByTestId('profile-view-front').getByText('ALL EDGES · PROJECTED', { exact: true })).toBeVisible();
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionDrawingCalls.length === 6);
  calls = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.projectionDrawingCalls);
  expect(calls.slice(3).map((call: any) => call.visibility)).toEqual(['visibleEdges', 'visibleEdges', 'visibleEdges']);
  await expect(workspace.getByTestId('profile-view-front').getByText('VISIBLE EDGES · PROJECTED', { exact: true })).toBeVisible();
});

test('one exact projection action fills all views; selected top builds and applies a closed profile', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByRole('button', { name: 'All three views', exact: true }).click();
  await expect(workspace.getByTestId('profile-projection-views').getByTestId('profile-view-front')).toBeVisible();
  await expect(workspace.getByTestId('profile-projection-views').getByTestId('profile-view-top')).toBeVisible();
  await expect(workspace.getByTestId('profile-projection-views').getByTestId('profile-view-side')).toBeVisible();
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  for (const view of ['front', 'top', 'side']) {
    await expect(workspace.getByTestId(`profile-view-${view}`).locator('polyline.projected-edge')).toHaveCount(4);
    await expect(workspace.getByTestId(`profile-view-${view}`).locator('polygon.outer')).toHaveCount(0);
  }
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionDrawingCalls.length === 3);
  const drawingCalls = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.projectionDrawingCalls);
  expect(drawingCalls.map((call: any) => call.view).sort()).toEqual(['front', 'side', 'top']);
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
  await workspace.getByRole('button', { name: 'Select top projection' }).click();
  await workspace.getByRole('button', { name: 'Build closed profile' }).click();
  await expect(workspace.getByTestId('profile-view-top').locator('polygon.hole')).toHaveCount(1);
  await expect(workspace.getByTestId('profile-view-top').locator('polyline.projected-edge')).toHaveCount(4);
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.deriveCalls.length === 1);
  const deriveCalls = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.deriveCalls);
  expect(deriveCalls[0].operation).toEqual({ kind: 'projection', view: 'top' });
  await expect(workspace.getByRole('button', { name: 'Select top projection' })).toHaveAttribute('aria-pressed', 'true');
  if (process.env.PROFILE_AUTHORING_SCREENSHOT_PATH) {
    const dismiss = page.getByRole('button', { name: 'DISMISS' });
    if (await dismiss.isVisible()) await dismiss.click();
    await page.screenshot({ path: process.env.PROFILE_AUTHORING_SCREENSHOT_PATH, fullPage: true });
  }
  await workspace.getByLabel('Clearance (mm)').fill('1.2');
  await workspace.getByLabel('Depth (mm)').fill('3.5');
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.threadId).toBe('profile-thread');
  expect(applyCall.baseMessageId).toBe('profile-version-1');
  expect(applyCall.expectedSource).toContain('(part flange');
  expect(applyCall.clearanceMm).toBe(1.2);
  expect(applyCall.depthMm).toBe(3.5);
  expect(applyCall.loops).toBeUndefined();
  expect(applyCall.reference).toMatchObject({
    sourceArtifactPath: '/mock/profile-model.step',
    operation: { kind: 'projection', view: 'top' },
    accuracy: 'exactBrep',
    profileAsset: { artifactPath: '/mock/profile-assets/top.step', artifactDigest: 'sha256:profile-top' },
  });
  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.projectionRequests.some((request: any) => request.preferredMessageId === 'profile-version-2'));
});

test('native STEP projection output renders independently in front, top, and side', async ({ page }) => {
  const nativeProjectionDirectory = process.env.PROFILE_AUTHORING_NATIVE_OUTPUTS_DIR;
  test.skip(!nativeProjectionDirectory, 'Set PROFILE_AUTHORING_NATIVE_OUTPUTS_DIR to inspect saved native STEP projection output.');
  await installProfileMocks(page, { nativeProjectionDirectory });
  const workspace = await openSketch(page);
  await workspace.getByRole('button', { name: 'All three views', exact: true }).click();
  await workspace.getByRole('button', { name: 'Project all three views' }).click();

  for (const view of ['front', 'top', 'side']) {
    await expect(workspace.getByTestId(`profile-view-${view}`).locator('polyline.projected-edge').first()).toHaveCount(1);
    await expect(workspace.getByTestId(`profile-view-${view}`).getByText('ALL EDGES · PROJECTED', { exact: true })).toBeVisible();
    await expect(workspace.getByTestId(`profile-view-${view}`).locator('polygon.outer')).toHaveCount(0);
  }
  if (process.env.PROFILE_AUTHORING_PROJECTION_SCREENSHOT_PATH) {
    await page.screenshot({ path: process.env.PROFILE_AUTHORING_PROJECTION_SCREENSHOT_PATH, fullPage: true });
  }
});

test('selecting SIDE applies side projection on YZ plane', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);

  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await expect(workspace.getByTestId('profile-view-side').locator('polyline.projected-edge')).toHaveCount(4);
  await workspace.getByRole('button', { name: 'Select side projection' }).click();
  await expect(workspace.getByLabel('Profile plane', { exact: true })).toHaveValue('yz');
  await workspace.getByRole('button', { name: 'Build closed profile' }).click();
  await expect(workspace.getByTestId('profile-view-side').locator('polygon.outer')).toBeVisible();
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const applyCall = await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls[0]);
  expect(applyCall.plane).toBe('yz');
  expect(applyCall.reference.operation).toEqual({ kind: 'projection', view: 'side' });
  expect(applyCall.reference.profileAsset.artifactPath).toBe('/mock/profile-assets/side.step');
});

test('one failed drawing projection does not discard successful view drawings', async ({ page }) => {
  await installProfileMocks(page, { failedProjectionView: 'side' });
  const workspace = await openSketch(page);

  await workspace.getByRole('button', { name: 'All three views', exact: true }).click();
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await expect(workspace.getByTestId('profile-view-front').locator('polyline.projected-edge')).toHaveCount(4);
  await expect(workspace.getByTestId('profile-view-top').locator('polyline.projected-edge')).toHaveCount(4);
  await expect(workspace.getByTestId('profile-view-side')).toContainText('Mock side drawing projection failed.');
  await expect(workspace.getByTestId('profile-view-side').locator('polyline.projected-edge')).toHaveCount(0);
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
});

test('closed-profile failure keeps selected view drawing and does not enable Apply', async ({ page }) => {
  await installProfileMocks(page, { failedProfileView: 'top' });
  const workspace = await openSketch(page);
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  await workspace.getByRole('button', { name: 'Select top projection' }).click();
  const top = workspace.getByTestId('profile-view-top');
  await expect(top.locator('polyline.projected-edge')).toHaveCount(4);

  await workspace.getByRole('button', { name: 'Build closed profile' }).click();
  await expect(top.locator('polyline.projected-edge')).toHaveCount(4);
  await expect(top).toContainText('Mock top closed profile failed.');
  await expect(top.locator('polygon.outer')).toHaveCount(0);
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
});

test('structured projection errors show backend details in the pane and workspace status', async ({ page }) => {
  await installProfileMocks(page, { failedProjectionView: 'side', structuredProjectionError: true });
  const workspace = await openSketch(page);

  await workspace.getByRole('button', { name: 'All three views', exact: true }).click();
  await workspace.getByRole('button', { name: 'Project all three views' }).click();
  const pane = workspace.getByTestId('profile-view-side');
  await expect(pane).toContainText('Could not create exact projection.');
  await expect(pane).toContainText('No visible projection edges were returned.');
  await expect(pane).toContainText('view=side');
  await expect(workspace.getByTestId('profile-status')).toContainText('Could not create exact projection.');
  await expect(workspace.getByTestId('profile-status')).toContainText('No visible projection edges were returned.');
  await expect(pane).not.toContainText('[object Object]');
});

test('pending derivation failure keeps the manual profile and fixed controls visible', async ({ page }) => {
  await installProfileMocks(page, { deriveMode: 'pending-error' });
  const workspace = await openSketch(page);
  await drawRectangle(workspace);
  const status = workspace.getByTestId('profile-status');

  await workspace.getByLabel('Derivation mode').selectOption('exactSection');
  const statusBox = await status.boundingBox();
  await workspace.getByRole('button', { name: 'Extract exact section' }).click();
  const extracting = workspace.getByRole('button', { name: 'Deriving…' });
  await expect(extracting).toBeVisible();
  await expect(extracting).toBeDisabled();
  await expect(workspace.getByLabel('Profile name')).toBeDisabled();
  expect(await status.boundingBox()).toEqual(statusBox);

  await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.rejectPendingDerivation());
  await expect(status).toContainText('Mock profile derivation failed.');
  await expect(workspace.locator('circle.vertex-handle')).toHaveCount(4);
  await expect(workspace.getByRole('button', { name: 'Draw profile' })).toBeEnabled();
  await expect(workspace.getByLabel('Profile name')).toBeEnabled();
  expect(await status.boundingBox()).toEqual(statusBox);
});

test('render failure refreshes history but preserves current preview and editable profile', async ({ page }) => {
  await installProfileMocks(page, { applyMode: 'render-error' });
  const workspace = await openSketch(page);
  await drawRectangle(workspace);
  await workspace.getByLabel('Profile name').fill('failed-tab');
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await expect(workspace.getByTestId('profile-status')).toContainText('Native render rejected profile.');
  await expect(workspace.getByTestId('profile-status')).toContainText('choose it from history');
  await expect(workspace.locator('polygon.selected-loop')).toHaveCount(1);
  await expect(workspace.getByLabel('Profile name')).toHaveValue('failed-tab');
  const refreshed = await page.evaluate(() => ({
    history: (window as any).__PROFILE_AUTHORING__.historyRefreshes,
    projectedFailedVersion: (window as any).__PROFILE_AUTHORING__.projectionRequests
      .some((request: any) => request.preferredMessageId === 'profile-version-2'),
  }));
  expect(refreshed.history.some((request: any) => request.kind === 'messages')).toBe(true);
  expect(refreshed.projectedFailedVersion).toBe(false);
});

test('pre-submit validation failure keeps the current source and does not refresh version history', async ({ page }) => {
  await installProfileMocks(page, { applyMode: 'validation-error' });
  const workspace = await openSketch(page);
  await drawRectangle(workspace);
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await expect(workspace.getByTestId('profile-status')).toContainText('Profile clearance or offset failed validation.');
  await expect(workspace.locator('polygon.selected-loop')).toHaveCount(1);
  const state = await page.evaluate(() => ({
    historyRefreshes: (window as any).__PROFILE_AUTHORING__.historyRefreshes,
    messages: (window as any).__PROFILE_AUTHORING__.applyCalls.length,
    projectedFailedVersion: (window as any).__PROFILE_AUTHORING__.projectionRequests
      .some((request: any) => request.preferredMessageId === 'profile-version-2'),
  }));
  expect(state.historyRefreshes.some((request: any) => request.kind === 'messages')).toBe(false);
  expect(state.messages).toBe(1);
  expect(state.projectedFailedVersion).toBe(false);
});

test('without a model artifact Sketch remains available but derivation stays disabled', async ({ page }) => {
  await installProfileMocks(page, { missingArtifact: true });
  const workspace = await openSketch(page);

  await expect(workspace.getByLabel('Derivation mode').locator('option[value="exactProjection"]')).toHaveAttribute('disabled', '');
  await expect(workspace.getByLabel('Derivation mode').locator('option[value="exactSection"]')).toHaveAttribute('disabled', '');
  await expect(workspace.getByLabel('Derivation mode').locator('option[value="meshSection"]')).toHaveAttribute('disabled', '');
  await expect(workspace.getByRole('button', { name: 'Project all three views' })).toBeDisabled();
  await expect(workspace.getByText(/supported STEP model is needed/i)).toBeVisible();
  await expect(workspace.getByRole('button', { name: 'Draw profile' })).toBeEnabled();
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
});

test('drawing photo import recognizes a calibrated plate and preserves analytic circle through apply', async ({ page }) => {
  await installProfileMocks(page);
  const workspace = await openSketch(page);
  await workspace.getByRole('button', { name: 'Select top projection' }).click();
  await workspace.getByRole('button', { name: 'Drawing photo' }).click();
  await expect(workspace.getByTestId('drawing-photo-source')).toBeVisible();
  const recognizeButton = workspace.getByRole('button', { name: 'Recognize drawing' });
  await expect(recognizeButton).toBeDisabled();
  await expect(workspace.getByTestId('drawing-recognition-readiness')).toContainText('Pick 2 scale points');
  await expect(workspace.getByText(/Crop and perspective correction are optional/i)).toBeVisible();

  const source = workspace.getByTestId('drawing-photo-source');
  const sourceBox = await source.boundingBox();
  if (!sourceBox) throw new Error('Drawing photo preview has no visible bounds.');
  const pickSourcePoint = async (x: number, y: number) => page.mouse.click(sourceBox.x + sourceBox.width * x, sourceBox.y + sourceBox.height * y);
  await workspace.getByRole('button', { name: 'Select image region' }).click();
  await page.mouse.move(sourceBox.x + sourceBox.width * 0.05, sourceBox.y + sourceBox.height * 0.1);
  await page.mouse.down();
  await page.mouse.move(sourceBox.x + sourceBox.width * 0.95, sourceBox.y + sourceBox.height * 0.9);
  await page.mouse.up();
  await expect(workspace.getByText(/Crop: 576 × 384 px/)).toBeVisible();

  await workspace.getByRole('button', { name: 'Pick four corners' }).click();
  await pickSourcePoint(0.1, 0.15);
  await expect(workspace.getByTestId('drawing-recognition-readiness')).toContainText('Pick rectangle corner 2 of 4');
  await workspace.getByRole('button', { name: 'Show front view' }).click();
  await workspace.getByRole('button', { name: 'Show top view' }).click();
  await expect(workspace.getByText('0/4 corners picked. Supply the known ratio to rectify a photographed rectangle.')).toBeVisible();
  await expect(workspace.getByText(/Crop: 576 × 384 px/)).toBeVisible();

  await workspace.getByRole('button', { name: 'Pick four corners' }).click();
  for (const point of [[0.1, 0.15], [0.9, 0.15], [0.9, 0.85], [0.1, 0.85]]) {
    await pickSourcePoint(point[0]!, point[1]!);
  }
  await workspace.getByLabel('Known rectangle aspect ratio').fill('1.6');
  await workspace.getByRole('button', { name: 'Pick four corners' }).click();
  await pickSourcePoint(0.2, 0.2);
  await workspace.getByRole('button', { name: 'Cancel photo picking' }).click();
  await expect(workspace.getByText('4/4 corners picked. Supply the known ratio to rectify a photographed rectangle.')).toBeVisible();

  await workspace.getByRole('button', { name: 'Pick scale points' }).click();
  await pickSourcePoint(0.1, 0.25);
  await expect(workspace.getByTestId('drawing-recognition-readiness')).toContainText('Pick scale point 2 of 2');
  await workspace.getByRole('button', { name: 'Cancel photo picking' }).click();
  await expect(workspace.getByText('0/2 scale points picked. Calibration uses one uniform scale.')).toBeVisible();

  await workspace.getByRole('button', { name: 'Pick scale points' }).click();
  await pickSourcePoint(0.1, 0.25);
  await pickSourcePoint(0.9, 0.25);
  await expect(recognizeButton).toBeDisabled();
  await expect(workspace.getByTestId('drawing-recognition-readiness')).toContainText('Enter a positive known length');
  await workspace.getByLabel('Known length (mm)').fill('0');
  await expect(recognizeButton).toBeDisabled();
  await workspace.getByLabel('Known length (mm)').fill('80');
  await expect(recognizeButton).toBeEnabled();
  await expect(workspace.getByText('Reference distance: 80 mm')).toBeVisible();
  await workspace.getByRole('button', { name: 'Pick scale points' }).click();
  await pickSourcePoint(0.2, 0.25);
  await workspace.getByRole('button', { name: 'Cancel photo picking' }).click();
  await expect(workspace.getByText('2/2 scale points picked. Calibration uses one uniform scale.')).toBeVisible();
  await expect(workspace.getByLabel('Known length (mm)')).toHaveValue('80');
  await expect(recognizeButton).toBeEnabled();

  await workspace.getByRole('button', { name: 'Recognize drawing' }).click();
  await expect(workspace.getByTestId('drawing-profile-candidates')).toContainText('plate-outline');
  await expect(workspace.getByLabel('Calibrated distance (mm)')).toContainText('Recovered distance: 80.00 mm · image-derived, check dimensions.');
  await expect(workspace.getByTestId('profile-view-top').locator('circle.drawing-circle')).toHaveCount(1);
  await workspace.getByLabel('Recognized primitive').selectOption('hole-1');
  await workspace.getByLabel('Circle radius (mm)').fill('4.9');
  await expect(workspace.getByTestId('profile-view-top').locator('circle.drawing-circle')).toHaveAttribute('r', '4.9');
  await workspace.getByRole('button', { name: 'Review corrections' }).click();
  await workspace.getByRole('button', { name: 'Accept selected profile' }).click();
  await expect(workspace.getByLabel('Profile plane', { exact: true })).toBeDisabled();
  await expect(workspace.getByLabel('Profile plane offset (mm)')).toBeEnabled();
  await workspace.getByLabel('Profile name').fill('photo-plate');
  await workspace.getByLabel('Depth (mm)').fill('3');

  if (process.env.DRAWING_PHOTO_SCREENSHOT_PATH) {
    await workspace.getByRole('button', { name: 'Show top view' }).click();
    await workspace.getByTestId('profile-view-top').getByRole('button', { name: 'FIT VIEW' }).click();
    await workspace.getByLabel('Profile controls', { exact: true }).evaluate((sidebar: HTMLElement) => {
      const opacity = sidebar.querySelector<HTMLInputElement>('[aria-label="Source image opacity"]');
      if (opacity) {
        opacity.value = '0.9';
        opacity.dispatchEvent(new Event('input', { bubbles: true }));
      }
      const radius = sidebar.querySelector<HTMLElement>('[aria-label="Circle radius (mm)"]');
      if (radius) {
        const top = radius.getBoundingClientRect().top - sidebar.getBoundingClientRect().top;
        sidebar.scrollTop += top - 160;
      }
    });
    await page.screenshot({ path: process.env.DRAWING_PHOTO_SCREENSHOT_PATH });
  }

  await workspace.getByLabel('Recognized primitive').selectOption('plate-outline');
  const outlineVertex = workspace.getByTestId('drawing-vertex-0');
  const vertexBox = await outlineVertex.boundingBox();
  if (!vertexBox) throw new Error('Recognized plate outline has no editable vertex handle.');
  await page.mouse.move(vertexBox.x + vertexBox.width / 2, vertexBox.y + vertexBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(vertexBox.x + vertexBox.width / 2 + 18, vertexBox.y + vertexBox.height / 2 + 10, { steps: 2 });
  await page.mouse.up();
  await expect(workspace.getByLabel('Polyline vertex 1 X (mm)')).not.toHaveValue('0');
  await workspace.getByRole('button', { name: 'Review corrections' }).click();

  await workspace.getByLabel('Photo correction tool').selectOption('line');
  await workspace.getByRole('button', { name: 'Draw correction' }).click();
  const profilePane = workspace.getByTestId('profile-view-top');
  const profileBox = await profilePane.boundingBox();
  if (!profileBox) throw new Error('Calibrated profile canvas has no visible bounds.');
  await page.mouse.click(profileBox.x + profileBox.width * 0.2, profileBox.y + profileBox.height * 0.2);
  await page.mouse.click(profileBox.x + profileBox.width * 0.3, profileBox.y + profileBox.height * 0.3);
  await workspace.getByRole('button', { name: 'Review corrections' }).click();
  await workspace.getByRole('button', { name: 'Accept selected profile' }).click();
  await workspace.getByRole('button', { name: 'Apply profile' }).click();

  await page.waitForFunction(() => (window as any).__PROFILE_AUTHORING__?.applyCalls.length === 1);
  const result = await page.evaluate(() => ({
    prepared: (window as any).__PROFILE_AUTHORING__.preparedDrawingPath,
    recognition: (window as any).__PROFILE_AUTHORING__.recognitionCalls[0],
    apply: (window as any).__PROFILE_AUTHORING__.applyCalls[0],
  }));
  expect(result.prepared).toBe('/mock/plate-photo.png');
  expect(result.recognition.view).toBe('top');
  expect(result.recognition.region.widthPx).toBeCloseTo(576, 2);
  expect(result.recognition.region.heightPx).toBeCloseTo(384, 2);
  expect(result.recognition.region.rotationDegrees).toBe(0);
  expect(result.recognition.rectification).toMatchObject({ aspectRatio: 1.6 });
  expect(result.recognition.calibration.distanceMm).toBe(80);
  expect(result.apply.drawingProfile.evidenceId).toBe('drawing-evidence-1');
  expect(result.apply.drawingProfile.primitives).toHaveLength(3);
  expect(result.apply.drawingProfile.primitives.find((primitive: any) => primitive.kind === 'polyline').pointsMm[0][0]).not.toBe(0);
  expect(result.apply.drawingProfile.primitives.some((primitive: any) => primitive.id === 'user-line-1')).toBe(true);
  expect(result.apply.drawingProfile.primitives.find((primitive: any) => primitive.kind === 'circle')).toMatchObject({
    centerMm: [40, 25],
    radiusMm: 4.9,
  });
  expect(result.apply.loops).toBeUndefined();
  expect(result.apply.reference).toBeNull();
  expect(await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.reviewCalls.length)).toBe(3);
});

test('drawing recognition failure keeps the source photo and blocks profile apply', async ({ page }) => {
  await installProfileMocks(page, { drawingMode: 'recognition-error' });
  const workspace = await openSketch(page);
  await workspace.getByRole('button', { name: 'Select top projection' }).click();
  await workspace.getByRole('button', { name: 'Drawing photo' }).click();
  const source = workspace.getByTestId('drawing-photo-source');
  const sourceBox = await source.boundingBox();
  if (!sourceBox) throw new Error('Drawing photo preview has no visible bounds.');

  await workspace.getByRole('button', { name: 'Pick scale points' }).click();
  await page.mouse.click(sourceBox.x + sourceBox.width * 0.15, sourceBox.y + sourceBox.height * 0.25);
  await page.mouse.click(sourceBox.x + sourceBox.width * 0.85, sourceBox.y + sourceBox.height * 0.25);
  await workspace.getByLabel('Known length (mm)').fill('80');
  await workspace.getByRole('button', { name: 'Recognize drawing' }).click();

  await expect(workspace.getByRole('alert')).toContainText('Mock drawing recognition failed.');
  await expect(source).toBeVisible();
  await expect(workspace.getByRole('button', { name: 'Apply profile' })).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__PROFILE_AUTHORING__.applyCalls.length)).toBe(0);
});
