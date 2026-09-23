import { expect, test } from '@playwright/test';

test('Given a visitor When reading below the models Then code, setup and project limits replace repeated feature cards', async ({ page }, info) => {
  await page.goto('/');
  const source = page.locator('#source');
  await expect(source.getByRole('heading', { name: 'A model is a text file.' })).toBeVisible();
  await expect(source.locator('pre')).toContainText('(model');
  await expect(page.getByRole('link', { name: /Build instructions/ })).toHaveAttribute('href', /#running-from-source/);
  await expect(page.locator('#learn')).toContainText('Start with a bracket');
  await expect(page.locator('.feature-grid')).toHaveCount(0);
  await source.scrollIntoViewIfNeeded();
  await page.screenshot({ path: info.outputPath('landing-bottom.png'), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: info.outputPath('landing-mobile.png'), fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});
