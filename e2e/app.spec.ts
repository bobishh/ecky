import { test, expect } from '@playwright/test';

async function numericZIndex(locator) {
  return locator.evaluate((element) => Number.parseInt(window.getComputedStyle(element).zIndex || '0', 10));
}

test('Given app opens When workbench loads Then bottom icon dock controls are available', async ({ page }) => {
  await page.goto('/');

  const dock = page.getByTestId('workbench-bottom-dock');
  await expect(dock).toBeVisible();
  const dockBox = await dock.boundingBox();
  const viewport = page.viewportSize();
  expect(dockBox).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(dockBox!.y).toBeGreaterThan(viewport!.height / 2);
  expect(dockBox!.y + dockBox!.height).toBeLessThanOrEqual(viewport!.height);

  await expect(dock.getByRole('button', { name: 'PROJECTS' })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'PARAMS' })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'DIALOGUE' })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'DOCS' })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'CODE' })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'SKETCH' })).toBeVisible();
  await expect(dock.getByRole('button', { name: /AUDIO ON|AUDIO OFF/i })).toHaveCount(0);
  await expect(dock.getByRole('button', { name: /Draw Annotations|Exit Draw Mode/ })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'Settings' })).toBeVisible();
  await expect(dock.getByRole('button', { name: '+' })).toHaveCount(0);
  await expect(dock.getByRole('button', { name: 'New project' })).toHaveCount(0);
});

test('Given workbench dock When layout and audio settings are checked Then audio mute lives in settings and projects/docs sit after the separator', async ({
  page,
}) => {
  await page.goto('/');

  const dock = page.getByTestId('workbench-bottom-dock');
  const primary = dock.locator('.dock-group--primary');
  const utility = dock.locator('.dock-group--utility');

  await expect(primary.getByRole('button', { name: /Draw Annotations|Exit Draw Mode/ })).toBeVisible();
  await expect(primary.getByRole('button', { name: 'PROJECTS' })).toHaveCount(0);
  await expect(primary.getByRole('button', { name: 'DOCS' })).toHaveCount(0);
  await expect(primary.getByRole('button', { name: /AUDIO ON|AUDIO OFF/i })).toHaveCount(0);
  await expect(utility.getByRole('button', { name: 'PROJECTS' })).toBeVisible();
  await expect(utility.getByRole('button', { name: 'DOCS' })).toBeVisible();

  await dock.getByRole('button', { name: 'Settings' }).click();
  const settingsWindow = page.locator('[data-window-id="settings"]');
  await expect(settingsWindow).toBeVisible();
  await settingsWindow.getByRole('button', { name: 'APP' }).click();
  const audioBtn = settingsWindow.getByRole('button', { name: /AUDIO ON|AUDIO OFF/i });
  await expect(audioBtn).toBeVisible();
  await audioBtn.click();
  await expect(audioBtn).toHaveText('AUDIO OFF');
});

test('Given workbench dock When settings opens and closes Then workbench controls remain available', async ({ page }) => {
  await page.goto('/');
  const dock = page.getByTestId('workbench-bottom-dock');

  await dock.getByRole('button', { name: 'Settings' }).click();
  const settingsWindow = page.locator('[data-window-id="settings"]');
  await expect(settingsWindow).toBeVisible();
  await expect(settingsWindow.getByText('CONNECTION TYPE')).toBeVisible();
  await expect(dock.getByRole('button', { name: 'PARAMS' })).toBeVisible();

  await settingsWindow.locator('.window-close').click();
  await expect(settingsWindow).toBeHidden();
  await expect(dock.getByRole('button', { name: 'PARAMS' })).toBeVisible();
});

test('Given workbench dock When code button clicked twice Then inspector toggles like other dock windows', async ({ page }) => {
  await page.goto('/');
  const dock = page.getByTestId('workbench-bottom-dock');
  await dock.getByRole('button', { name: 'DOCS' }).click();
  const docsWindow = page.locator('[data-window-id="docs"]');
  await docsWindow.getByRole('button', { name: 'Forms and Structure' }).click();
  await docsWindow.getByRole('button', { name: 'OPEN IN CODE' }).click();

  await expect(page.getByText(/MACRO INSPECTOR:/i)).toBeVisible();

  const codeButton = dock.getByRole('button', { name: 'CODE' });
  await codeButton.click();
  const codeWindow = page.locator('[data-window-id="code"]');
  await expect(codeWindow).toBeHidden();

  await codeButton.click();
  await expect(codeWindow).toBeVisible();
});

test('Given workbench dock When sketch clicked twice Then sketch workspace behaves like a toggle window without close sketch action', async ({
  page,
}) => {
  await page.goto('/');
  const dock = page.getByTestId('workbench-bottom-dock');
  const sketchButton = dock.getByRole('button', { name: 'SKETCH' });

  await sketchButton.click();
  const sketchWindow = page.locator('[data-window-id="sketch"]');
  await expect(sketchWindow).toBeVisible();
  await expect(sketchWindow.getByRole('heading', { name: 'SKETCH WORKSPACE' })).toBeVisible();
  await expect(sketchWindow.getByRole('button', { name: /^CLOSE SKETCH$/i })).toHaveCount(0);

  await sketchButton.click();
  await expect(sketchWindow).toBeHidden();
});

test('Given workbench dock When docs opens Then floating docs window renders lessons and closes cleanly', async ({ page }) => {
  await page.goto('/');

  await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'DOCS' }).click();
  await expect(page.locator('.export-actions').getByRole('button', { name: /CODE/i })).toHaveCount(0);

  const docsWindow = page.locator('[data-window-id="docs"]');
  await expect(docsWindow).toBeVisible();
  await expect(docsWindow.getByRole('heading', { name: 'Ecky IR Field Guide' })).toBeVisible();
  await expect(docsWindow.getByRole('button', { name: 'First Solid: Ball on a Base' })).toBeVisible();
  await expect(docsWindow.getByRole('button', { name: 'Final Model: Integrated Film Adapter Open Helicoid v9' })).toBeVisible();
  await expect(docsWindow.getByRole('button', { name: 'Verify Clauses' })).toBeVisible();

  await docsWindow.getByRole('button', { name: 'First Solid: Ball on a Base' }).click();
  await expect(docsWindow.getByRole('heading', { name: 'First Solid: Ball on a Base' })).toBeVisible();
  await expect(docsWindow.locator('pre').first()).toContainText('(model');
  await expect(docsWindow.locator('img[alt*="First Solid"]').first()).toBeVisible();
  await docsWindow.getByRole('button', { name: 'OPEN IN CODE' }).click();
  const codeModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR: First Solid: Ball on a Base' });
  await expect(codeModal).toBeVisible();
  await expect(codeModal).toHaveClass(/window--focused/);
  await expect(docsWindow).not.toHaveClass(/window--focused/);
  expect(await numericZIndex(codeModal)).toBeGreaterThan(await numericZIndex(docsWindow));
  await expect(codeModal.locator('.cm-content')).toContainText('(sphere 10)');
  await expect(codeModal.getByRole('button', { name: 'APPLY' })).toBeVisible();
  await expect(codeModal.getByRole('button', { name: 'FORK TO NEW THREAD' })).toBeVisible();
  await expect(codeModal.getByRole('button', { name: 'COMMIT VERSION' })).toBeVisible();
  await expect(codeModal.getByRole('button', { name: 'INSERT VERIFY' })).toBeVisible();
  await expect(codeModal.getByText('ECKY SOURCE')).toHaveCount(0);
  await expect(codeModal.getByText('SCRATCH SNIPPET ONLY')).toHaveCount(0);
  await codeModal.locator('.window-close').click();
  await expect(codeModal).toBeHidden();
  await expect(docsWindow).toHaveClass(/window--focused/);

  await docsWindow.getByRole('button', { name: 'Paths and Surfaces: Revolve and Sweep' }).click();
  await expect(docsWindow.getByRole('heading', { name: 'Paths and Surfaces: Revolve and Sweep' })).toBeVisible();
  await expect(docsWindow.locator('pre').first()).toContainText('(revolve');
  await docsWindow.getByRole('button', { name: 'OPEN IN CODE' }).click();
  const tutorialModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR: Paths and Surfaces: Revolve and Sweep' });
  await expect(tutorialModal).toBeVisible();
  await expect(tutorialModal.getByRole('button', { name: 'COPY CODE' })).toBeVisible();
  await expect(tutorialModal.getByRole('button', { name: 'APPLY' })).toBeVisible();
  await expect(tutorialModal.getByRole('button', { name: 'COMMIT VERSION' })).toBeVisible();
  await expect(tutorialModal.getByRole('button', { name: 'INSERT VERIFY' })).toBeVisible();
  await expect(tutorialModal.getByText('ECKY SOURCE')).toHaveCount(0);
  await expect(tutorialModal.getByText('SCRATCH SNIPPET ONLY')).toHaveCount(0);
  await expect(tutorialModal.locator('.cm-ecky-keyword').filter({ hasText: 'model' }).first()).toHaveCSS('color', 'rgb(212, 160, 79)');
  await expect(tutorialModal.locator('.cm-ecky-number').filter({ hasText: '360' }).first()).toHaveCSS('color', 'rgb(125, 178, 215)');
  await tutorialModal.locator('.window-close').click();
  await expect(tutorialModal).toBeHidden();

  await docsWindow.getByRole('button', { name: 'Verify Clauses' }).click();
  await expect(docsWindow.getByRole('heading', { name: 'Verify Clauses' })).toBeVisible();
  await expect(docsWindow.locator('pre').first()).toContainText('(verify');

  await docsWindow.getByRole('button', { name: 'OPEN IN CODE' }).click();
  const verifyModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR: Verify Clauses' });
  await expect(verifyModal).toBeVisible();
  await expect(verifyModal.locator('.cm-content')).toContainText('clearance min-distance');
  await expect(verifyModal.getByRole('button', { name: 'APPLY' })).toBeVisible();
  await expect(verifyModal.getByRole('button', { name: 'FORK TO NEW THREAD' })).toBeVisible();
  await expect(verifyModal.getByRole('button', { name: 'COMMIT VERSION' })).toBeVisible();
  await expect(verifyModal.getByRole('button', { name: 'VERIFY EXISTS' })).toBeDisabled();
  await expect(verifyModal.getByText('ECKY SOURCE')).toHaveCount(0);
  await expect(verifyModal.getByText('SCRATCH SNIPPET ONLY')).toHaveCount(0);
  await expect(verifyModal.locator('.cm-ecky-keyword').filter({ hasText: 'verify' }).first()).toHaveCSS('color', 'rgb(212, 160, 79)');
  await verifyModal.locator('.window-close').click();
  await expect(verifyModal).toBeHidden();

  await docsWindow.getByRole('button', { name: /Constraint dojo/i }).click();
  await expect(docsWindow.getByRole('heading', { name: 'Constraint Dojo' })).toBeVisible();
  await expect(docsWindow.locator('.docs-status--pending')).toContainText('Pending');

  await docsWindow.locator('.window-close').click();
  await expect(docsWindow).toBeHidden();
});

test('Given fresh thread When docs snippet opens in code Then modal can apply as live code', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'DOCS' }).click();
  const docsWindow = page.locator('[data-window-id="docs"]');
  await docsWindow.getByRole('button', { name: 'First Solid: Ball on a Base' }).click();
  await docsWindow.getByRole('button', { name: 'OPEN IN CODE' }).click();

  const codeModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR: First Solid: Ball on a Base' });
  await expect(codeModal).toBeVisible();
  await expect(codeModal.getByRole('button', { name: 'APPLY' })).toBeVisible();
  await expect(codeModal.getByRole('button', { name: 'COMMIT VERSION' })).toBeVisible();
});

test('Given projects window When plus new opens chooser Then chooser is global not nested in projects window', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'PROJECTS' }).click();
  const projectsWindow = page.locator('[data-window-id="projects"]');
  await expect(projectsWindow).toBeVisible();
  await expect(projectsWindow.getByText(/NO PREVIEW|Loading|LOAD ERROR|IN WORK/)).toBeVisible();
  await projectsWindow.getByRole('button', { name: '+ NEW' }).click();

  const chooser = page.locator('.modal-backdrop').filter({ hasText: 'Start New Project' });
  await expect(chooser).toBeVisible();
  await expect(projectsWindow.getByText('Start New Project')).toHaveCount(0);
});
