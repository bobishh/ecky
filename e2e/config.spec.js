import { test, expect } from '@playwright/test';

test.describe('Configuration Panel', () => {
  test('can toggle between workbench and config views', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);

    // Initially on workbench
    await expect(page.locator('text=TUNABLE PARAMETERS')).toBeVisible();

    // Click settings button to go to config
    await page.click('.settings-overlay-btn');
    await expect(page.locator('text=ENGINES')).toBeVisible();
    await expect(page.locator('text=TUNABLE PARAMETERS')).not.toBeVisible();

    // Click back to workbench
    await page.click('.settings-overlay-btn');
    await expect(page.locator('text=TUNABLE PARAMETERS')).toBeVisible();
  });

  test('config panel shows engine section by default', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    await page.click('.settings-overlay-btn');
    await expect(page.locator('text=ENGINES')).toBeVisible();
  });

  test('settings button shows gear emoji on workbench and hammer on config', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);

    // On workbench, should show gear
    await expect(page.locator('.settings-overlay-btn')).toContainText('⚙️');

    // Go to config
    await page.click('.settings-overlay-btn');
    // On config, should show hammer
    await expect(page.locator('.settings-overlay-btn')).toContainText('⚒️');
  });
});
