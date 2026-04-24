import { test, expect } from '@playwright/test';

test.describe('Audio controls', () => {
  test('Given idle workbench, audio mute toggle remains visible', async ({ page }) => {
    await page.goto('/');

    await expect(page.locator('.microwave-unit')).toHaveCount(0);
    await expect(page.getByRole('button', { name: /mute ecky audio/i })).toBeVisible();
  });

  test('Given audio toggle, muting keeps control available for unmute', async ({ page }) => {
    await page.goto('/');

    const toggle = page.getByRole('button', { name: /mute ecky audio/i });
    await expect(toggle).toBeVisible();
    await toggle.click();

    await expect(page.getByRole('button', { name: /unmute ecky audio/i })).toBeVisible();
  });
});
