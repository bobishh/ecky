import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { expect, test } from '@playwright/test';
const require = createRequire(import.meta.url);
const { PNG } = require(join(dirname(require.resolve('playwright-core/package.json')), 'lib/utilsBundle.js'));

for (const label of ['FILAMENT DRYER', 'BOTTLE HOLDER']) {
  test(`Given ${label} When the gallery opens Then it shows an upright assembly with a separate print download`, async ({ page }, testInfo) => {
    await page.goto('/');
    const workbench = page.getByTestId('model-workbench');
    await workbench.getByRole('button', { name: new RegExp(label) }).click();
    await expect(workbench.locator('.viewer')).toHaveAttribute('aria-label', /assembled.*drag to rotate/i);
    await expect(workbench.locator('.viewer-load')).toHaveCount(0);
    await expect(workbench.getByRole('alert')).toHaveCount(0);
    await expect(workbench.getByRole('link', { name: 'DOWNLOAD ZIP' })).toBeVisible();
    await expect(workbench.locator('.viewport-hint')).toContainText('ASSEMBLED VIEW');
    await page.setViewportSize({ width: 1280, height: 1000 });
    const canvas = workbench.locator('.viewer canvas');
    await canvas.evaluate(el => window.scrollTo({ top: el.getBoundingClientRect().top + window.scrollY - 90, behavior: 'instant' }));
    const frame = await canvas.screenshot({ path: testInfo.outputPath(`${label}.png`) });
    const png = PNG.sync.read(frame);
    let top = png.height, bottom = 0;
    for (let y = 0; y < png.height; y++) for (let x = 0; x < png.width; x++) {
      const i = (y * png.width + x) * 4;
      if (png.data[i] > png.data[i + 2] + 10 || png.data[i + 1] > png.data[i + 2] + 10) {
        top = Math.min(top, y); bottom = Math.max(bottom, y);
      }
    }
    expect(bottom - top, 'assembly fills enough of the frame to inspect').toBeGreaterThan(png.height * 0.65);
    expect(top).toBeGreaterThan(2);
    expect(bottom).toBeLessThan(png.height - 3);
  });
}
