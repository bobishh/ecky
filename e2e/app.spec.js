import { test, expect } from '@playwright/test';

test('app should load and show the designer tab', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.app-project-tab--active')).toContainText('DESIGNER');
});

test('switching tabs should work', async ({ page }) => {
  await page.goto('/');
  await page.click('text=MACRO INSPECTOR');
  await expect(page.locator('.app-project-tab--active')).toContainText('MACRO INSPECTOR');
  
  await page.click('text=CONFIG');
  await expect(page.locator('.app-project-tab--active')).toContainText('CONFIG');
  await expect(page.locator('text=ENGINES')).toBeVisible();
});
