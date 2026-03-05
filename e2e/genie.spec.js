import { test, expect } from '@playwright/test';

test.describe('VertexGenie', () => {
  test('genie canvas renders in the viewport', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const canvas = page.locator('.genie-canvas');
    await expect(canvas).toBeVisible();
  });

  test('genie is present in the genie layer', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const genieLayer = page.locator('.genie-layer');
    await expect(genieLayer).toBeVisible();
  });
});
