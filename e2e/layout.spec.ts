import { test, expect } from '@playwright/test';

test.describe('Layout', () => {
  test('workbench layout has all major panels', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('.viewport-area')).toBeVisible();
    await expect(page.getByRole('button', { name: 'PROJECTS' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'PARAMS' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'DIALOGUE' })).toBeVisible();
  });

  test('dock opens parameter and project windows', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('[data-window-id="params"]')).toBeVisible();
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await expect(page.locator('[data-window-id="projects"]')).toBeVisible();
  });

  test('vertical and horizontal resizers exist', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('[data-window-id="params"] .window-resize-handle')).toBeVisible();
  });
});
