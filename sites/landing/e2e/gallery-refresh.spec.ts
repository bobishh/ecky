import { expect, test } from '@playwright/test';

const models = [
  ['PHONE CASE', 'iphone-case'],
  ['FILAMENT DRYER', 'filament-dryer'],
  ['SARA’S BOARD', 'sara-brushing-board'],
  ['FOLD', 'fold-spoon-rest'],
  ['BOTTLE HOLDER', 'bicycle-bottle-holder'],
];

test('Given recent saved models When each is selected Then its geometry, source and single ZIP are available', async ({ page, request }, testInfo) => {
  await page.goto('/');
  const workbench = page.getByTestId('model-workbench');
  await expect(workbench.getByRole('group', { name: 'Working models' }).getByRole('button')).toHaveCount(5);
  for (const [label, id] of models) {
    await workbench.getByRole('button', { name: new RegExp(label) }).click();
    await expect(workbench).toHaveAttribute('data-selected-variant', id);
    await expect(workbench.locator('.viewer-load')).toHaveCount(0);
    await expect(workbench.getByRole('alert')).toHaveCount(0);
    await workbench.screenshot({ path: testInfo.outputPath(`${id}.png`) });
    const downloads = workbench.locator('.model-downloads');
    await expect(downloads.getByRole('link', { name: 'DOWNLOAD ZIP', exact: true })).toHaveCount(1);
    await expect(downloads.locator('a[download$=".stl"]')).toHaveCount(0);
    const archive = downloads.getByRole('link', { name: 'DOWNLOAD ZIP', exact: true });
    await expect(archive).toHaveAttribute('download', `${id}.zip`);
    const response = await request.get((await archive.getAttribute('href'))!);
    expect(response.ok()).toBe(true);
    expect((await response.body()).subarray(0, 4).toString('hex')).toBe('504b0304');
    await workbench.getByRole('button', { name: 'SEE CODE' }).click();
    await expect(page.getByTestId('case-source')).toContainText('(model');
    await page.keyboard.press('Escape');
  }
});

test('Given an off-origin bottle assembly When rotated Then the whole model stays centered inside the viewport', async ({ page }, testInfo) => {
  const { createRequire } = await import('node:module');
  const { dirname, join } = await import('node:path');
  const require = createRequire(import.meta.url);
  const { PNG } = require(join(dirname(require.resolve('playwright-core/package.json')), 'lib/utilsBundle.js'));
  // Model origins are arbitrary. Translate the real assembly together to
  // exercise the failure without changing any shape or relative placement.
  await page.route('**/models/previews/bottle-*.stl', async route => {
    const response = await route.fetch();
    const body = Buffer.from(await response.body());
    const count = body.readUInt32LE(80);
    for (let triangle = 0; triangle < count; triangle++) {
      for (let vertex = 0; vertex < 3; vertex++) {
        const offset = 84 + triangle * 50 + 12 + vertex * 12;
        body.writeFloatLE(body.readFloatLE(offset) + 1000, offset);
        body.writeFloatLE(body.readFloatLE(offset + 4) + 500, offset + 4);
      }
    }
    await route.fulfill({ response, body });
  });
  await page.goto('/');
  await page.getByRole('button', { name: /BOTTLE HOLDER/ }).click();
  const viewer = page.locator('.viewer');
  await expect(viewer.locator('.viewer-load')).toHaveCount(0);
  await expect(viewer.getByRole('alert')).toHaveCount(0);
  const canvas = viewer.locator('canvas');
  await page.setViewportSize({ width: 1280, height: 1000 });
  for (let index = 0; index < 5; index += 1) {
    await canvas.evaluate(el => window.scrollTo({ top: el.getBoundingClientRect().top + window.scrollY - 90, behavior: 'instant' }));
    const frame = await canvas.screenshot({ path: testInfo.outputPath(`bottle-orbit-${index}.png`) });
    await testInfo.attach(`bottle-orbit-${index}`, { body: frame, contentType: 'image/png' });
    const png = PNG.sync.read(frame);
    let left = png.width, right = 0, top = png.height, bottom = 0, pixels = 0;
    for (let y = 0; y < png.height; y++) for (let x = 0; x < png.width; x++) {
      const offset = (y * png.width + x) * 4;
      // Lit model colors are brighter than the dark viewport and its grid.
      if ((png.data[offset] > png.data[offset + 2] + 8 || png.data[offset + 1] > png.data[offset + 2] + 8) && png.data[offset] + png.data[offset + 1] + png.data[offset + 2] > 190) {
        left = Math.min(left, x); right = Math.max(right, x);
        top = Math.min(top, y); bottom = Math.max(bottom, y); pixels++;
      }
    }
    expect(pixels).toBeGreaterThan(1000);
    expect(left).toBeGreaterThan(2); expect(top).toBeGreaterThan(2);
    expect(right).toBeLessThan(png.width - 3); expect(bottom).toBeLessThan(png.height - 3);
    expect(Math.abs((left + right) / 2 - png.width / 2)).toBeLessThan(png.width * 0.15);
    expect(Math.abs((top + bottom) / 2 - png.height / 2)).toBeLessThan(png.height * 0.15);
    const bounds = await canvas.boundingBox();
    await page.mouse.move(bounds!.x + bounds!.width / 2, bounds!.y + bounds!.height / 2);
    await page.mouse.down();
    await page.mouse.move(bounds!.x + bounds!.width / 2 + 100, bounds!.y + bounds!.height / 2 + 140, { steps: 6 });
    await page.mouse.up();
  }
});

test('Given a narrow viewport When each new model is opened Then controls and downloads remain inside the page', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  const workbench = page.getByTestId('model-workbench');
  for (const [label, id] of models) {
    await workbench.getByRole('button', { name: new RegExp(label) }).click();
    await expect(workbench.locator('.viewer-load')).toHaveCount(0);
    await expect(workbench.getByRole('alert')).toHaveCount(0);
    await expect(workbench.getByRole('link', { name: 'DOWNLOAD ZIP' })).toBeVisible();
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    expect(overflow).toBeLessThanOrEqual(0);
    const box = await workbench.locator('.viewer').boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(390);
    await workbench.screenshot({ path: testInfo.outputPath(`${id}-mobile.png`) });
  }
  await page.screenshot({ path: testInfo.outputPath('landing-mobile.png'), fullPage: true });
});
