import { test, expect } from '@playwright/test';

test.describe('History Panel', () => {
  test('shows search input', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const searchInput = page.locator('.history-panel .search-input');
    await expect(searchInput).toBeVisible();
    await expect(searchInput).toHaveAttribute('placeholder', 'Search threads...');
  });

  test('new thread button exists', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const newBtn = page.locator('.new-thread-btn');
    await expect(newBtn).toBeVisible();
  });

  test('shows empty state when no history', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    await expect(page.locator('.empty-state')).toBeVisible();
  });
});
