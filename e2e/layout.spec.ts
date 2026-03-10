import { test, expect } from '@playwright/test';

test.describe('Layout', () => {
  test('workbench layout has all major panels', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    await expect(page.locator('.sidebar')).toBeVisible();
    await expect(page.locator('.viewport-area')).toBeVisible();
    await expect(page.locator('.dialogue-area')).toBeVisible();
  });

  test('sidebar contains parameter and history panels', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    await expect(page.locator('text=TUNABLE PARAMETERS')).toBeVisible();
    await expect(page.locator('text=THREAD HISTORY')).toBeVisible();
  });

  test('vertical and horizontal resizers exist', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const resizers = page.locator('.resizer-w, .resizer-v');
    await expect(resizers).toHaveCount(3);
  });
});
