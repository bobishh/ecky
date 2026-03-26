import { test, expect } from '@playwright/test';

test.describe('Configuration Panel', () => {
  test('can toggle between workbench and config views', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);

    // Initially on workbench
    await expect(page.locator('text=TUNABLE PARAMETERS')).toBeVisible();

    // Click settings button to go to config
    await page.click('button[title="Configuration"]');
    await expect(page.locator('text=CONNECTION TYPE')).toBeVisible();
    await expect(page.locator('text=TUNABLE PARAMETERS')).not.toBeVisible();

    // Click back to workbench
    await page.click('button[title="Close"]');
    await expect(page.locator('text=TUNABLE PARAMETERS')).toBeVisible();
  });

  test('config panel shows connection type section by default', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    await page.click('button[title="Configuration"]');
    await expect(page.locator('text=CONNECTION TYPE')).toBeVisible();
  });

  test('settings button shows gear emoji on workbench and close icon on config', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);

    // On workbench, should show gear
    await expect(page.locator('button[title="Configuration"]')).toContainText('⚙️');

    // Go to config
    await page.click('button[title="Configuration"]');
    // On config, should show close affordance
    await expect(page.locator('button[title="Close"]')).toContainText('×');
  });
});
