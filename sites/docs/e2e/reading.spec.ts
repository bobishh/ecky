import { expect, test } from '@playwright/test';

test('Given a new reader When following chapter one Then a complete editable example accompanies the explanation', async ({ page, request }, info) => {
  await page.goto('/docs/chapters/');
  await page.getByRole('link', { name: /Chapter 01/ }).click();
  const example = page.getByRole('link', { name: 'Download the complete worm' });
  await expect(example).toBeVisible();
  const source = await request.get((await example.getAttribute('href'))!);
  expect(source.ok()).toBe(true);
  expect(await source.text()).toContain('(body (sweep (circle 6mm) spine))');
  await expect(page.locator('.docs-main pre').first()).toContainText('(model');
  await expect(page.locator('.docs-main')).toContainText('Try');
  await expect(page.locator('.docs-main figure img')).toBeVisible();
  expect(await page.locator('.docs-main figure img').evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
  await expect(page.locator('.docs-main')).not.toContainText('{#');
  await expect(page.getByRole('navigation', { name: 'On this page' })).toBeVisible();
  await page.screenshot({ path: info.outputPath('chapter-desktop.png'), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: info.outputPath('chapter-mobile.png'), fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('Given the reference When reading box Then coordinates and a runnable example are explained', async ({ page }) => {
  await page.goto('/docs/primitive-signatures/#box');
  await expect(page.locator('.docs-main')).toContainText('X and Y are centered');
  await expect(page.locator('.docs-main pre').first()).toContainText('(model');
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
  expect(content).toContain('A worm with a wiggle');
  expect(content).toContain('Open the gates, check the farm');
  expect(content).toContain('https://ecky-cad.com/docs/examples/');
});


test('Given a missing model When requesting its download Then server returns a real 404', async ({ request }) => {
  const response = await request.get('/docs/examples/missing-worm.ecky');
  expect(response.status()).toBe(404);
  expect(await response.text()).not.toContain('<html');
});

test('Given all six chapters When following their assets Then illustrations and exact sources load', async ({ page, request }) => {
  await page.goto('/docs/chapters/');
  const urls = await page.locator('nav[aria-label="Chapters"] a').evaluateAll(nodes => nodes.map(n => n.getAttribute('href')!));
  expect(urls.length).toBe(6);
  for (const url of urls) {
    await page.goto(url);
    const images = page.locator('.docs-main figure img');
    expect(await images.count()).toBeGreaterThan(0);
    for (const image of await images.all()) {
      const response = await request.get((await image.getAttribute('src'))!);
      expect(response.ok()).toBe(true);
      expect(response.headers()['content-type']).toContain('image/png');
    }
    for (const link of await page.locator('.docs-main a[href$=".ecky"]').all()) {
      const response = await request.get((await link.getAttribute('href'))!);
      expect(response.ok()).toBe(true);
      expect(await response.text()).toContain('(model');
    }
  }
});
