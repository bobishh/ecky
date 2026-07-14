import { expect, test } from '@playwright/test';

/**
 * Landing page e2e — BDD Given/When/Then, user-visible behavior.
 * The landing is a separate static Vite project from the Tauri app, but it
 * reuses the canonical genome from src/lib/genie. These tests gate the two
 * things that can silently break: (a) the landing shell renders with the right
 * structure + CTAs, and (b) the mascot mounts without console errors (genome
 * import / Three.js scene build failures would surface as errors on load).
 */

test.describe('Ecky landing', () => {
  test('Given first visit When page opens Then brand, hero, and CTAs render', async ({ page }) => {
    const errors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') errors.push(msg.text());
    });

    await page.goto('/');

    await expect(page.locator('.nav')).toContainText('Ecky CAD');
    await expect(page.getByRole('heading', { name: 'Prompt-driven CAD' })).toBeVisible();
    await expect(page.getByText('Describe a part in words')).toBeVisible();

    // Happy path: primary CTAs present with canonical destinations.
    await expect(page.getByRole('link', { name: /Download/ }).first()).toHaveAttribute('href', /github\.com\/bobishh\/ecky\/releases/);
    await expect(page.getByRole('link', { name: /Source/ }).first()).toHaveAttribute('href', 'https://github.com/bobishh/ecky');
    await expect(page.getByRole('link', { name: 'Docs' }).first()).toHaveAttribute('href', '/docs');

    expect(errors, 'page opened with no console errors').toEqual([]);
  });

  test('Given mascot zone When page loads Then WebGL canvas mounts without errors', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(`pageerror: ${err.message}`));
    page.on('console', (msg) => {
      if (msg.type() === 'error') errors.push(msg.text());
    });

    await page.goto('/');
    const canvas = page.locator('.mascot canvas');
    await expect(canvas).toBeVisible();

    // Genome + Three.js scene build happens in $effect; let it settle.
    // Don't hardcode pixel size — devicePixelRatio differs across runners.
    // The real gate is: canvas mounted with nonzero backing dimensions.
    await expect.poll(async () => {
      const ok = await canvas.evaluate((el: HTMLCanvasElement) => el.width > 0 && el.height > 0);
      return ok;
    }, { message: 'canvas has nonzero backing size', timeout: 10_000 }).toBe(true);

    expect(errors, 'mascot mounted without genome/renderer errors').toEqual([]);
  });

  test('Given landing When scrolled Then three layers and feature cards render', async ({ page }) => {
    await page.goto('/');

    await expect(page.locator('.layer-tag', { hasText: 'SURFACE' })).toBeVisible();
    await expect(page.locator('.layer-tag', { hasText: 'CORE IR' })).toBeVisible();
    await expect(page.locator('.layer-tag', { hasText: 'BACKEND' })).toBeVisible();

    const features = page.locator('.feature-card');
    await expect(features).toHaveCount(6);

    // The .ecky code sample renders.
    await expect(page.locator('.code code')).toContainText('(model');
  });

  test('Given gallery with no real screenshots When page opens Then reserved placeholder slots show', async ({ page }) => {
    // Pending state: screenshots aren't built yet, so the gallery shows
    // reserved placeholders rather than broken images.
    await page.goto('/');

    const slots = page.locator('.gallery-placeholder');
    await expect(slots).toHaveCount(3);
    await expect(slots.first()).toContainText(/stamp.*TPU.*PLA/);
    await expect(slots.nth(1)).toContainText(/reserved/);
    await expect(slots.nth(2)).toContainText(/reserved/);

    // No img elements inside placeholders — they must be text-only until
    // real screenshots are dropped in.
    await expect(page.locator('.gallery-slot img')).toHaveCount(0);
  });
});
