import { expect, test } from '@playwright/test';

test('Given a new reader When following chapter one Then a complete editable example accompanies the explanation', async ({ page, request }, info) => {
  await page.goto('/docs/chapters/');
  await page.getByRole('link', { name: /Chapter 01/ }).click();
  const example = page.getByRole('link', { name: 'Download connected bracket' });
  await expect(example).toBeVisible();
  const source = await request.get((await example.getAttribute('href'))!);
  expect(source.ok()).toBe(true);
  expect(await source.text()).toContain('(result (union foot flange))');
  await expect(page.locator('.docs-main pre').first()).toContainText('(model');
  await expect(page.locator('.docs-main')).toContainText('Change span from 64 to 80');
  await expect(page.locator('.docs-main')).not.toContainText('{#');
  await expect(page.getByRole('navigation', { name: 'On this page' })).toBeVisible();
  await page.screenshot({ path: info.outputPath('chapter-desktop.png'), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: info.outputPath('chapter-mobile.png'), fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('Given the reference When reading box Then coordinates and a runnable example are explained', async ({ page }, info) => {
  await page.goto('/docs/primitive-signatures/#box');
  await expect(page.locator('.docs-main')).toContainText('X and Y are centered');
  await expect(page.locator('.docs-main pre').first()).toContainText('(model');
  await page.screenshot({ path: info.outputPath('reference-desktop.png') });
});

test('Given mobile contents When dismissed with Escape Then reading resumes on the same section', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/docs/primitive-signatures/#box');
  await page.getByRole('button', { name: '☰ Contents', exact: true }).click();
  await expect(page.locator('#docs-toc')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#docs-toc')).toBeHidden();
  await expect(page.getByRole('heading', { name: 'box', exact: true })).toBeVisible();
});

test('Given the rewritten chapters When the EPUB is downloaded Then it contains the same lessons and source links', async ({ request }, info) => {
  const { writeFileSync } = await import('node:fs');
  const { execFileSync } = await import('node:child_process');
  const response = await request.get('/docs/ecky-ir-field-guide.epub');
  expect(response.ok()).toBe(true);
  const file = info.outputPath('guide.epub');
  writeFileSync(file, await response.body());
  const content = execFileSync('unzip', ['-p', file, 'OEBPS/content.xhtml'], { encoding: 'utf8' });
  expect(content).toContain('Build a bracket, then an enclosure');
  expect(content).toContain('Read the film scanner assembly');
  expect(content).toContain('https://ecky-cad.com/docs/examples/');
});
