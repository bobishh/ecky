import { expect, test } from '@playwright/test';

test('Given overflowing docs When using the themed scrollbar Then content scrolls and fitting panes stay clear', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 600 });
  await page.goto('/docs/ecky-ir');
  const article = page.locator('.docs-article');
  await expect(article).toBeVisible();
  await expect.poll(() => article.evaluate(element => element.scrollHeight > element.clientHeight)).toBe(true);

  const appearance = await article.evaluate(element => ({
    width: getComputedStyle(element, '::-webkit-scrollbar').width,
    grip: getComputedStyle(element, '::-webkit-scrollbar-thumb').backgroundImage,
  }));
  expect(appearance.width).toBe('14px');
  expect(appearance.grip).toContain('repeating-linear-gradient');
  await article.hover();
  await page.mouse.wheel(0, 400);
  await expect.poll(() => article.evaluate(element => element.scrollTop)).toBeGreaterThan(0);

  const darkFace = await article.evaluate(element => getComputedStyle(element, '::-webkit-scrollbar-thumb').backgroundColor);
  await page.evaluate(() => document.documentElement.dataset.theme = 'victorian');
  const lightFace = await article.evaluate(element => getComputedStyle(element, '::-webkit-scrollbar-thumb').backgroundColor);
  expect(lightFace).not.toBe(darkFace);
  await page.evaluate(() => delete document.documentElement.dataset.theme);

  // A header that fits must not gain a scrollbar just because styling is global.
  const header = page.locator('.docs-header');
  expect(await header.evaluate(element => element.clientHeight === element.scrollHeight)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});
