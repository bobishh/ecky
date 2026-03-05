import { test, expect } from '@playwright/test';

test('app should load and show workbench by default', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('text=TUNABLE PARAMETERS')).toBeVisible();
  await expect(page.locator('text=THREAD HISTORY')).toBeVisible();
});

test('switching between workbench and config should work', async ({ page }) => {
  await page.goto('/');
  
  // Click settings button
  await page.click('.settings-overlay-btn');
  await expect(page.locator('text=ENGINES')).toBeVisible();
  await expect(page.locator('text=TUNABLE PARAMETERS')).not.toBeVisible();
  
  // Click settings button again to return to workbench
  await page.click('.settings-overlay-btn');
  await expect(page.locator('text=TUNABLE PARAMETERS')).toBeVisible();
  await expect(page.locator('text=ENGINES')).not.toBeVisible();
});
