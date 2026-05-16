import { test, expect } from '@playwright/test';

test('Given app opens When workbench loads Then dock controls are available', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'PROJECTS' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'PARAMS' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'DIALOGUE' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'SKETCH' })).toBeVisible();
});

test('Given workbench dock When settings opens and closes Then workbench controls remain available', async ({ page }) => {
  await page.goto('/');

  await page.getByRole('button', { name: '⚙️' }).click();
  const settingsWindow = page.locator('[data-window-id="settings"]');
  await expect(settingsWindow).toBeVisible();
  await expect(settingsWindow.getByText('CONNECTION TYPE')).toBeVisible();
  await expect(page.getByRole('button', { name: 'PARAMS' })).toBeVisible();

  await settingsWindow.locator('.window-close').click();
  await expect(settingsWindow).toBeHidden();
  await expect(page.getByRole('button', { name: 'PARAMS' })).toBeVisible();
});
