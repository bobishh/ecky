import { test, expect, type Locator } from '@playwright/test';

async function numericZIndex(locator: Locator) {
  return locator.evaluate((element: Element) =>
    Number.parseInt(window.getComputedStyle(element).zIndex || '0', 10),
  );
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

  await expect(dock.getByRole('button', { name: 'Projects', exact: true })).toBeVisible();
  await expect(dock.getByRole('button', { name: /(PARAMS|Parameters)/i, exact: true })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'Dialogue', exact: true })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'Ecky IR docs', exact: true })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'Code inspector', exact: true })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'Sketch Workspace', exact: true })).toHaveCount(0);
  await expect(dock.getByRole('button', { name: /AUDIO ON|AUDIO OFF/i })).toHaveCount(0);
  await expect(dock.getByRole('button', { name: 'Draw annotations', exact: true })).toBeVisible();
  await expect(dock.getByRole('button', { name: 'Settings' })).toBeVisible();
  await expect(dock.getByRole('button', { name: '+' })).toHaveCount(0);
  await expect(dock.getByRole('button', { name: 'New project' })).toHaveCount(0);
});

test('Given workbench dock When groups are checked Then workspace windows precede modes and audio stays in settings', async ({
  page,
}) => {
  await page.goto('/');

  const dock = page.getByTestId('workbench-bottom-dock');
  const primary = dock.locator('.dock-group--primary');
  const utility = dock.locator('.dock-group--utility');

  await expect(primary.getByRole('button', { name: 'Projects', exact: true })).toBeVisible();
  await expect(primary.getByRole('button', { name: /(PARAMS|Parameters)/i, exact: true })).toBeVisible();
  await expect(primary.getByRole('button', { name: 'Ecky IR docs', exact: true })).toBeVisible();
  await expect(primary.getByRole('button', { name: /AUDIO ON|AUDIO OFF/i })).toHaveCount(0);
  await expect(utility.getByRole('button', { name: /Draw annotations/i })).toBeVisible();
  await expect(utility.getByRole('button', { name: 'Settings', exact: true })).toBeVisible();

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
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  const dock = page.getByTestId('workbench-bottom-dock');

  await dock.getByRole('button', { name: 'Settings' }).click();
  const settingsWindow = page.locator('[data-window-id="settings"]');
  await expect(settingsWindow).toBeVisible();
  await expect(settingsWindow.getByText('CONNECTION TYPE')).toBeVisible();
  await expect(dock.getByRole('button', { name: /(PARAMS|Parameters)/i, exact: true })).toBeVisible();

  await settingsWindow.locator('.window-close').click();
  await expect(settingsWindow).toBeHidden();
  await expect(dock.getByRole('button', { name: /(PARAMS|Parameters)/i, exact: true })).toBeVisible();
});

test('Given workbench When Ecky is clicked once Then Projects opens for model work', async ({ page }) => {
  await page.goto('/');
  const projectsWindow = page.locator('[data-window-id="projects"]');
  if (await projectsWindow.isVisible()) {
    await projectsWindow.locator('.window-close').click();
  }

  await page.getByRole('button', { name: 'Poke Ecky' }).click();

  await expect(projectsWindow).toBeVisible();
});

test('Given workbench dock When focused Code repeats Then inspector closes and reopens', async ({ page }) => {
  await page.goto('/');
  const dock = page.getByTestId('workbench-bottom-dock');
  const codeButton = dock.getByRole('button', { name: 'Code inspector', exact: true });
  const codeWindow = page.locator('[data-window-id="code"]');

  await codeButton.click();
  await expect(codeWindow).toBeVisible();
  await codeButton.click();
  await expect(codeWindow).toBeHidden();

  await codeButton.click();
  await expect(codeWindow).toBeVisible();
});

test('Given workbench dock When docs opens Then floating docs window renders lessons and closes cleanly', async ({ page }) => {
  await page.goto('/');

  await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'Ecky IR docs', exact: true }).click();
  await expect(page.locator('.export-actions').getByRole('button', { name: /CODE/i })).toHaveCount(0);

  const docsWindow = page.locator('[data-window-id="docs"]');
  await expect(docsWindow).toBeVisible();
  await expect(docsWindow.getByRole('heading', { name: 'Worm Farm' })).toBeVisible();
  await expect(docsWindow.getByRole('button', { name: 'Chapter 1: A rake with teeth pointing down' })).toBeVisible();
  await expect(docsWindow.getByRole('button', { name: 'Chapter 6: Open the gates, check the farm' })).toBeVisible();

  await docsWindow.getByRole('button', { name: 'Chapter 1: A rake with teeth pointing down' }).click();
  await expect(docsWindow.getByRole('heading', { name: 'Chapter 1: A rake with teeth pointing down' })).toBeVisible();
  await expect(docsWindow.locator('pre').first()).toContainText('(model');
  const illustration = docsWindow.locator('figure img').first();
  await expect(illustration).toBeVisible();
  await expect.poll(() => illustration.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
  await expect(docsWindow.locator('pre').filter({ hasText: '(union' }).first()).toBeVisible();
  await docsWindow.getByRole('button', { name: 'OPEN ATTEMPT IN CODE' }).click();
  const codeModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR: A rake with teeth pointing down.ecky' });
  await expect(codeModal).toBeVisible();
  await expect(codeModal).toHaveClass(/window--focused/);
  await expect(docsWindow).not.toHaveClass(/window--focused/);
  expect(await numericZIndex(codeModal)).toBeGreaterThan(await numericZIndex(docsWindow));
  await expect(codeModal.locator('.cm-content')).toContainText('(tag connected-rake)');
  await expect(codeModal.getByRole('button', { name: 'APPLY' })).toBeVisible();
  await expect(codeModal.getByRole('button', { name: 'FORK TO NEW THREAD' })).toHaveCount(0);
  await expect(codeModal.getByRole('button', { name: 'VERIFY EXISTS' })).toBeDisabled();
  await expect(codeModal.getByText('ECKY SOURCE')).toHaveCount(0);
  await expect(codeModal.getByText('SCRATCH SNIPPET ONLY')).toHaveCount(0);
  await codeModal.locator('.window-close').click();
  await expect(codeModal).toBeHidden();
  await expect(docsWindow).toHaveClass(/window--focused/);

  await docsWindow.getByRole('button', { name: 'Chapter 3: A fence, a carrot, and a reusable recipe' }).click();
  await expect(docsWindow.getByRole('heading', { name: 'Chapter 3: A fence, a carrot, and a reusable recipe' })).toBeVisible();
  await expect(docsWindow.locator('pre').first()).toContainText('(model');
  await docsWindow.getByRole('button', { name: 'OPEN ATTEMPT IN CODE' }).click();
  const tutorialModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR: A fence, a carrot, and a reusable recipe.ecky' });
  await expect(tutorialModal).toBeVisible();
  await expect(tutorialModal.getByRole('button', { name: 'COPY CODE' })).toBeVisible();
  await expect(tutorialModal.getByRole('button', { name: 'APPLY' })).toBeVisible();
  await expect(tutorialModal.getByRole('button', { name: 'INSERT VERIFY' })).toBeVisible();
  await expect(tutorialModal.getByText('ECKY SOURCE')).toHaveCount(0);
  await expect(tutorialModal.getByText('SCRATCH SNIPPET ONLY')).toHaveCount(0);
  await expect(tutorialModal.locator('.cm-ecky-keyword').filter({ hasText: 'model' }).first()).toHaveCSS('color', 'rgb(212, 160, 79)');
  await expect(tutorialModal.locator('.cm-ecky-number').first()).toHaveCSS('color', 'rgb(125, 178, 215)');
  await tutorialModal.locator('.window-close').click();
  await expect(tutorialModal).toBeHidden();

  await docsWindow.locator('.window-close').click();
  await expect(docsWindow).toBeHidden();
});

test('Given fresh thread When docs snippet opens in code Then modal can apply as live code', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'Ecky IR docs', exact: true }).click();
  const docsWindow = page.locator('[data-window-id="docs"]');
  await docsWindow.getByRole('button', { name: 'Chapter 1: A rake with teeth pointing down' }).click();
  await docsWindow.getByRole('button', { name: 'OPEN ATTEMPT IN CODE' }).click();

  const codeModal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR: A rake with teeth pointing down.ecky' });
  await expect(codeModal).toBeVisible();
  await expect(codeModal.getByRole('button', { name: 'APPLY' })).toBeVisible();
});

test('Given projects window When plus new opens chooser Then chooser is global not nested in projects window', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'PROJECTS' }).click();
  const projectsWindow = page.locator('[data-window-id="projects"]');
  await expect(projectsWindow).toBeVisible();
  await expect(projectsWindow.getByText(/NO ACTIVE PROJECTS|NO PREVIEW|Loading|LOAD ERROR/)).toBeVisible();
  await projectsWindow.getByRole('button', { name: '+ NEW' }).click();

  const chooser = page.locator('.modal-backdrop').filter({ hasText: 'Start New Project' });
  await expect(chooser).toBeVisible();
  await expect(projectsWindow.getByText('Start New Project')).toHaveCount(0);
});

test('Given reference index When following different operation links Then each mounts its own section', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'Ecky IR docs', exact: true }).click();
  const docs = page.locator('[data-window-id="docs"]');
  await docs.getByRole('button', { name: 'Function Reference', exact: true }).click();
  for (const [name, section] of [['box', 'Primitive Signatures'], ['translate', 'Boolean and Transform Signatures']]) {
    await docs.getByRole('button', { name: 'Operation Index', exact: true }).click();
    await docs.locator(`.docs-article a[href="#${name}"]`).first().click();
    await expect(docs.locator('.docs-article h2')).toHaveText(section);
    await expect(docs.locator(`h3#${name}`)).toBeInViewport();
  }
});
